//! wgpu (Vulkan on the device) renderer: one pipeline, one instance buffer, a glyph atlas, an
//! image atlas for small images (icons, avatars, thumbnails) and one texture per large image. A
//! frame is a handful of instanced draw calls (one per run of primitives that share a texture).
//!
//! `Gpu::new` presents to a window; `Gpu::headless` renders offscreen only (golden images in CI,
//! unit tests), with the same pipeline and the same `capture`.

mod pacing;
#[cfg(target_os = "linux")]
mod vk_timing;

use crate::color::Color;
pub use pacing::{Pacer, Pacing};
use std::sync::Arc;
use winit::window::Window;

pub type TexId = u32;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Prim {
    pub rect: [f32; 4],
    pub clip: [f32; 4],
    pub c0: [f32; 4],
    pub c1: [f32; 4],
    pub c2: [f32; 4],
    /// Texture rectangle (images, glyphs), or the casting box [x, y, w, h] of a shadow.
    pub uv: [f32; 4],
    /// radius, border / ring gap / shadow sigma, kind, opacity
    pub p: [f32; 4],
    /// gradient direction x/y, gradient mid or ring band, gradient offset; shadow: casting radius
    pub q: [f32; 4],
    /// Rounded clip: the clipping box [x0, y0, x1, y1] of the nearest rounded `overflow: hidden`
    /// ancestor (applied on top of `clip`)...
    pub rclip: [f32; 4],
    /// ...and its corner radii [tl, tr, br, bl] (all 0: no rounded clip; only `cr[0]` set: that
    /// radius on all four corners).
    pub cr: [f32; 4],
    /// Per-corner radii [tl, tr, br, bl] of the shape (all 0: `p[0]` on every corner).
    pub radii: [f32; 4],
    /// Per-side border widths [top, right, bottom, left] of a rect (all 0: `p[1]` on every side);
    /// for a shadow, the casting box's corner radii.
    pub bw: [f32; 4],
}

pub const KIND_RECT: f32 = 0.0;
pub const KIND_GRAD: f32 = 1.0;
pub const KIND_IMAGE: f32 = 2.0;
pub const KIND_GLYPH: f32 = 3.0;
pub const KIND_RING: f32 = 4.0;
pub const KIND_MASK: f32 = 5.0;
/// Gaussian-blurred rounded rect, cut out under its casting box (CSS box-shadow).
pub const KIND_SHADOW: f32 = 6.0;
/// One segment of a linear gradient at any angle, sRGB or Oklab (`scene::LinearGradient`).
pub const KIND_GRADN: f32 = 7.0;

#[derive(Clone, Copy, Debug)]
pub struct Batch {
    pub tex: TexId,
    pub start: u32,
    pub end: u32,
}

/// Primitives in paint order, split into batches where the bound image texture changes.
#[derive(Default)]
pub struct DrawList {
    pub prims: Vec<Prim>,
    pub batches: Vec<Batch>,
}

impl DrawList {
    pub fn clear(&mut self) {
        self.prims.clear();
        self.batches.clear();
    }

    /// `tex` is the image texture the primitive samples, or None (0 = the white texture).
    pub fn push(&mut self, p: Prim, tex: Option<TexId>) {
        let n = self.prims.len() as u32;
        self.prims.push(p);
        match (self.batches.last_mut(), tex) {
            (Some(b), None) => b.end = n + 1,
            (Some(b), Some(t)) if b.tex == t => b.end = n + 1,
            (Some(b), Some(t)) if b.tex == 0 => {
                b.tex = t;
                b.end = n + 1
            }
            _ => self.batches.push(Batch { tex: tex.unwrap_or(0), start: n, end: n + 1 }),
        }
    }
}

/// An uploaded image: the texture to bind and where the pixels sit in it (normalised).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuImage {
    pub tex: TexId,
    pub uv: [f32; 4],
    pub w: u32,
    pub h: u32,
    slot: Option<(u32, etagere::AllocId)>,
}

struct GpuTex {
    tex: wgpu::Texture,
    bind: wgpu::BindGroup,
    bytes: u64,
    size: (u32, u32),
}

struct AtlasPage {
    tex: TexId,
    texture: wgpu::Texture,
    alloc: etagere::AtlasAllocator,
}

/// Images up to this size in both dimensions share atlas pages, so a row of icons or avatars is
/// one batch instead of one draw each.
pub const ATLAS_MAX: u32 = 256;
pub const ATLAS_PAGE: u32 = 1024;

/// A frame to draw into: from `Gpu::acquire`, consumed by `Gpu::submit`.
pub struct Frame {
    kind: FrameKind,
}

enum FrameKind {
    Surface(wgpu::SurfaceTexture),
    #[cfg(target_os = "linux")]
    Own(u32),
}

pub struct Gpu {
    /// Our own present-timing swapchain (dropped before the device).
    #[cfg(target_os = "linux")]
    own: Option<vk_timing::Swapchain>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    surface: Option<wgpu::Surface<'static>>,
    pub config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    img_layout: wgpu::BindGroupLayout,
    globals: wgpu::Buffer,
    bind0: wgpu::BindGroup,
    atlas: wgpu::Texture,
    pub atlas_size: u32,
    textures: Vec<Option<GpuTex>>,
    free: Vec<TexId>,
    pages: Vec<AtlasPage>,
    instances: wgpu::Buffer,
    capacity: usize,
    pub adapter_info: String,
    pub texture_bytes: u64,
    pub pacer: Pacer,
}

#[cfg(target_os = "linux")]
impl Drop for Gpu {
    fn drop(&mut self) {
        if let Some(mut own) = self.own.take() {
            own.destroy(&self.device);
        }
    }
}

fn device_desc(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    wgpu::DeviceDescriptor {
        label: Some("rare-render"),
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..Default::default()
    }
}

pub fn present_mode_from_env() -> wgpu::PresentMode {
    match std::env::var("RR_PRESENT").unwrap_or_default().to_ascii_lowercase().as_str() {
        "mailbox" => wgpu::PresentMode::Mailbox,
        "immediate" => wgpu::PresentMode::Immediate,
        "relaxed" | "fiforelaxed" => wgpu::PresentMode::FifoRelaxed,
        _ => wgpu::PresentMode::Fifo,
    }
}

impl Gpu {
    /// Presents to `window` (Vulkan on the device: X11 surface through gamescope's WSI layer).
    pub fn new(window: Arc<Window>, width: u32, height: u32) -> Gpu {
        let mut pacer = Pacer::from_env();
        let backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY);
        let plain = || wgpu::Instance::new(wgpu::InstanceDescriptor { backends, ..wgpu::InstanceDescriptor::new_without_display_handle() });
        #[cfg(target_os = "linux")]
        let instance = if pacer.mode == Pacing::PresentTiming { vk_timing::instance().unwrap_or_else(plain) } else { plain() };
        #[cfg(not(target_os = "linux"))]
        let instance = plain();
        let surface = instance.create_surface(window).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("adapter");
        let caps = surface.get_capabilities(&adapter);
        // Blend in sRGB-encoded space like CSS: a non-sRGB swapchain format.
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) { wgpu::CompositeAlphaMode::Opaque } else { caps.alpha_modes[0] };
        let want = present_mode_from_env();
        let present_mode = if caps.present_modes.contains(&want) { want } else { wgpu::PresentMode::Fifo };
        let latency = std::env::var("RR_LATENCY").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
        #[allow(unused_mut)]
        let mut config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: Default::default(),
            width: width.max(1),
            height: height.max(1),
            present_mode,
            desired_maximum_frame_latency: latency,
            alpha_mode,
            view_formats: vec![],
        };
        let desc = device_desc(&adapter);
        #[allow(unused_mut)]
        let mut opened = None;
        #[cfg(target_os = "linux")]
        if pacer.mode == Pacing::PresentTiming {
            match vk_timing::open_device(&adapter, &desc) {
                Ok(d) => opened = Some(d),
                Err(e) => log::info!("pacing: present timing off ({e})"),
            }
        }
        // `timing`: the device has the present-timing extensions on; `absolute`: it can aim at a time.
        let (device, queue, _timing, _absolute) = match opened {
            Some((d, q, abs)) => (d, q, true, abs),
            None => {
                let (d, q) = pollster::block_on(adapter.request_device(&desc)).expect("device");
                (d, q, false, false)
            }
        };
        #[cfg(target_os = "linux")]
        let own = if _timing {
            match vk_timing::Swapchain::new(&device, &surface, config.width, config.height, _absolute) {
                Ok(sc) => {
                    config.format = sc.format;
                    (config.width, config.height) = sc.size();
                    pacer.refresh_ns = sc.refresh_ns.unwrap_or(pacer.refresh_ns);
                    Some(sc)
                }
                Err(e) => {
                    log::info!("pacing: own swapchain failed ({e}), using wgpu's surface");
                    None
                }
            }
        } else {
            None
        };
        #[cfg(target_os = "linux")]
        let use_surface = own.is_none();
        #[cfg(not(target_os = "linux"))]
        let use_surface = true;
        if use_surface {
            surface.configure(&device, &config);
            if pacer.mode == Pacing::PresentTiming {
                pacer.mode = Pacing::Estimate;
            }
        }
        let info = adapter.get_info();
        let adapter_info = format!("{} ({:?}, {})", info.name, info.backend, info.driver_info);
        #[allow(unused_mut)]
        let mut gpu = Gpu::init(device, queue, adapter_info, Some(surface), config, pacer);
        #[cfg(target_os = "linux")]
        {
            gpu.own = own;
        }
        log::info!("gpu: {}, format {:?}, present {present_mode:?} (caps {:?}), latency {latency}, pacing {}", gpu.adapter_info, gpu.config.format, caps.present_modes, gpu.pacer.describe());
        gpu
    }

    /// Renders offscreen only: `capture` works, `acquire` returns None. `RR_HEADLESS_ADAPTER=cpu`
    /// (or WGPU_ADAPTER_NAME=llvmpipe) picks a software adapter for reproducible pixels.
    pub fn headless(width: u32, height: u32) -> Option<Gpu> {
        let backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends, ..wgpu::InstanceDescriptor::new_without_display_handle() });
        let cpu = std::env::var("RR_HEADLESS_ADAPTER").is_ok_and(|v| v == "cpu");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: cpu,
            compatible_surface: None,
            ..Default::default()
        }))
        .ok()?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Rgba8Unorm,
            color_space: Default::default(),
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 1,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
        };
        let info = adapter.get_info();
        let adapter_info = format!("{} ({:?}, {})", info.name, info.backend, info.driver_info);
        let (device, queue) = pollster::block_on(adapter.request_device(&device_desc(&adapter))).ok()?;
        Some(Gpu::init(device, queue, adapter_info, None, config, Pacer::new(Pacing::Off, 60.0)))
    }

    fn init(device: wgpu::Device, queue: wgpu::Queue, adapter_info: String, surface: Option<wgpu::Surface<'static>>, config: wgpu::SurfaceConfiguration, pacer: Pacer) -> Gpu {
        let format = config.format;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("rare-render"), source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()) });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex_entry(1),
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let img_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("image"), entries: &[tex_entry(0)] });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout0), Some(&img_layout)], immediate_size: 0 });

        let attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("prims"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Prim>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &attrs })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        // 4096 x 4096 R8 (16 MB): every screen's glyphs at every size, weight and luminance bucket fit
        // without a clear in normal use (a clear costs a repaint and re-rasterising).
        let atlas_size = 4096;
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyphs"),
            size: wgpu::Extent3d { width: atlas_size, height: atlas_size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor { label: Some("globals"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let atlas_view = atlas.create_view(&Default::default());
        let bind0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &layout0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&atlas_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let capacity = 4096;
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("prims"),
            size: (capacity * std::mem::size_of::<Prim>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        #[allow(unused_mut)]
        let mut gpu = Gpu {
            #[cfg(target_os = "linux")]
            own: None,
            device,
            queue,
            surface,
            config,
            pipeline,
            img_layout,
            globals,
            bind0,
            atlas,
            atlas_size,
            textures: Vec::new(),
            free: Vec::new(),
            pages: Vec::new(),
            instances,
            capacity,
            adapter_info,
            texture_bytes: 0,
            pacer,
        };
        // Texture 0: opaque white, bound whenever a batch samples no image.
        let (white, _) = gpu.new_texture(1, 1, Some(&[255, 255, 255, 255]));
        debug_assert_eq!(white, 0);
        gpu
    }

    pub fn is_headless(&self) -> bool {
        self.surface.is_none()
    }

    /// True when frames present through our own VK_EXT_present_timing swapchain.
    pub fn present_timing(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.own.is_some()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 || (w == self.config.width && h == self.config.height) {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        #[cfg(target_os = "linux")]
        if let Some(own) = self.own.as_mut() {
            if let Err(e) = own.resize(&self.device, w, h) {
                log::warn!("pacing: swapchain rebuild failed ({e})");
            }
            (self.config.width, self.config.height) = own.size();
            return;
        }
        if let Some(s) = &self.surface {
            s.configure(&self.device, &self.config);
        }
    }

    fn new_texture(&mut self, w: u32, h: u32, rgba: Option<&[u8]>) -> (TexId, wgpu::Texture) {
        let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        if let Some(px) = rgba {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                px,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
                size,
            );
        }
        let view = tex.create_view(&Default::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.img_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
        });
        let bytes = (w as u64) * (h as u64) * 4;
        self.texture_bytes += bytes;
        let entry = GpuTex { tex: tex.clone(), bind, bytes, size: (w, h) };
        let id = if let Some(id) = self.free.pop() {
            self.textures[id as usize] = Some(entry);
            id
        } else {
            self.textures.push(Some(entry));
            (self.textures.len() - 1) as TexId
        };
        (id, tex)
    }

    /// One texture of its own for `rgba` (premultiplied RGBA8, `w * h * 4` bytes).
    pub fn create_texture_rgba(&mut self, w: u32, h: u32, rgba: &[u8]) -> TexId {
        self.new_texture(w, h, Some(rgba)).0
    }

    /// A texture to update in place every frame (video, a camera, a canvas): draw it with
    /// `Node::image_tex`, fill it with `write_texture`.
    pub fn create_stream(&mut self, w: u32, h: u32) -> TexId {
        self.new_texture(w.max(1), h.max(1), None).0
    }

    /// Writes premultiplied RGBA8 pixels (`w * h * 4` bytes) into texture `id` in place: no new
    /// texture and no new bind group, so a 60 fps stream costs one upload per frame. A new size
    /// reallocates it under the same id. False when `id` is not a texture of its own (atlas pages
    /// and the white texture are not writable this way) or the data is short.
    pub fn write_texture(&mut self, id: TexId, w: u32, h: u32, rgba: &[u8]) -> bool {
        if id == 0 || self.pages.iter().any(|p| p.tex == id) || rgba.len() < (w as usize) * (h as usize) * 4 || w == 0 || h == 0 {
            return false;
        }
        let Some(t) = self.textures.get(id as usize).and_then(Option::as_ref) else { return false };
        if t.size != (w, h) {
            // Reallocate under the same id.
            let (nid, _) = self.new_texture(w, h, Some(rgba));
            let entry = self.textures[nid as usize].take();
            self.free.push(nid);
            if let Some(old) = std::mem::replace(&mut self.textures[id as usize], entry) {
                self.texture_bytes -= old.bytes;
            }
            return true;
        }
        self.write_region(id, 0, 0, w, h, rgba)
    }

    /// Writes a `w` x `h` region at (x, y) of texture `id` in place (partial updates).
    pub fn write_region(&mut self, id: TexId, x: u32, y: u32, w: u32, h: u32, rgba: &[u8]) -> bool {
        let Some(t) = self.textures.get(id as usize).and_then(Option::as_ref) else { return false };
        if id == 0 || x + w > t.size.0 || y + h > t.size.1 || rgba.len() < (w as usize) * (h as usize) * 4 {
            return false;
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &t.tex, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        true
    }

    /// The pixel size of texture `id`.
    pub fn texture_size(&self, id: TexId) -> Option<(u32, u32)> {
        self.textures.get(id as usize).and_then(Option::as_ref).map(|t| t.size)
    }

    pub fn free_texture(&mut self, id: TexId) {
        if id == 0 || self.pages.iter().any(|p| p.tex == id) {
            return;
        }
        if let Some(t) = self.textures.get_mut(id as usize).and_then(Option::take) {
            self.texture_bytes -= t.bytes;
            self.free.push(id);
        }
    }

    /// Uploads an image: small ones (both sides <= `ATLAS_MAX`) into a shared atlas page with a
    /// one-pixel edge-extended border (no bleed under linear filtering), large ones as their own
    /// texture. Draw it with `GpuImage::tex` bound and `GpuImage::uv` as the texture rectangle.
    pub fn upload_image(&mut self, w: u32, h: u32, rgba: &[u8]) -> GpuImage {
        let (w, h) = (w.max(1), h.max(1));
        if w <= ATLAS_MAX && h <= ATLAS_MAX && rgba.len() >= (w * h * 4) as usize {
            if let Some(img) = self.atlas_insert(w, h, rgba) {
                return img;
            }
        }
        let tex = self.create_texture_rgba(w, h, rgba);
        GpuImage { tex, uv: [0.0, 0.0, 1.0, 1.0], w, h, slot: None }
    }

    pub fn free_image(&mut self, img: GpuImage) {
        match img.slot {
            Some((page, id)) => {
                if let Some(p) = self.pages.get_mut(page as usize) {
                    p.alloc.deallocate(id);
                }
            }
            None => self.free_texture(img.tex),
        }
    }

    /// Pages in use and their fill (allocated pixels / page pixels), for RR_STATS.
    pub fn atlas_stats(&self) -> (usize, f32) {
        let used: i64 = self.pages.iter().map(|p| p.alloc.allocated_space() as i64).sum();
        let total = (self.pages.len() as i64 * (ATLAS_PAGE * ATLAS_PAGE) as i64).max(1);
        (self.pages.len(), used as f32 / total as f32)
    }

    fn atlas_insert(&mut self, w: u32, h: u32, rgba: &[u8]) -> Option<GpuImage> {
        let need = etagere::size2(w as i32 + 2, h as i32 + 2);
        let mut found = None;
        for (i, p) in self.pages.iter_mut().enumerate() {
            if let Some(a) = p.alloc.allocate(need) {
                found = Some((i, a));
                break;
            }
        }
        if found.is_none() {
            let (tex, texture) = self.new_texture(ATLAS_PAGE, ATLAS_PAGE, None);
            let mut alloc = etagere::AtlasAllocator::new(etagere::size2(ATLAS_PAGE as i32, ATLAS_PAGE as i32));
            let a = alloc.allocate(need)?;
            self.pages.push(AtlasPage { tex, texture, alloc });
            found = Some((self.pages.len() - 1, a));
        }
        let (page, a) = found?;
        let (x0, y0) = (a.rectangle.min.x as u32, a.rectangle.min.y as u32);
        let (pw, ph) = (w + 2, h + 2);
        let mut padded = vec![0u8; (pw * ph * 4) as usize];
        for py in 0..ph {
            let sy = py.saturating_sub(1).min(h - 1);
            for px in 0..pw {
                let sx = px.saturating_sub(1).min(w - 1);
                let s = ((sy * w + sx) * 4) as usize;
                let d = ((py * pw + px) * 4) as usize;
                padded[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
            }
        }
        let p = &self.pages[page];
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &p.texture, mip_level: 0, origin: wgpu::Origin3d { x: x0, y: y0, z: 0 }, aspect: wgpu::TextureAspect::All },
            &padded,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * pw), rows_per_image: Some(ph) },
            wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
        );
        let s = ATLAS_PAGE as f32;
        let uv = [(x0 + 1) as f32 / s, (y0 + 1) as f32 / s, (x0 + 1 + w) as f32 / s, (y0 + 1 + h) as f32 / s];
        Some(GpuImage { tex: p.tex, uv, w, h, slot: Some((page as u32, a.id)) })
    }

    pub fn upload_glyph(&self, x: u32, y: u32, w: u32, h: u32, data: &[u8]) {
        if w == 0 || h == 0 {
            return;
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &self.atlas, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Blocks until the swapchain hands out an image (this is where FIFO paces us to vblank).
    /// Acquire first, then latch input and paint: the frame shows the newest state. With pacing on,
    /// sleep until `pacer.next_start` before this so the acquire lands just before the deadline.
    /// None: no surface (headless) or the swapchain was rebuilt (try again next frame).
    pub fn acquire(&mut self) -> Option<Frame> {
        let wait = self.pacer.cadence_sleep_ns(crate::clock::mono_ns());
        if wait > 0 {
            std::thread::sleep(std::time::Duration::from_nanos(wait));
        }
        let t0 = crate::clock::mono_ns();
        #[cfg(target_os = "linux")]
        if let Some(own) = self.own.as_mut() {
            let r = own.acquire();
            self.pacer.on_acquire(crate::clock::mono_ns(), crate::clock::mono_ns() - t0);
            return match r {
                Ok(vk_timing::Acquired::Image(i)) => Some(Frame { kind: FrameKind::Own(i) }),
                Ok(vk_timing::Acquired::Recreate) => {
                    let (w, h) = (self.config.width, self.config.height);
                    if let Err(e) = own.resize(&self.device, w, h) {
                        log::warn!("pacing: swapchain rebuild failed ({e})");
                    }
                    None
                }
                Err(e) => {
                    log::warn!("pacing: {e}");
                    None
                }
            };
        }
        let surface = self.surface.as_ref()?;
        let r = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => Some(Frame { kind: FrameKind::Surface(f) }),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.configure(&self.device, &self.config);
                None
            }
            _ => None,
        };
        let t1 = crate::clock::mono_ns();
        self.pacer.on_acquire(t1, t1 - t0);
        r
    }

    fn encode(&mut self, view: &wgpu::TextureView, list: &DrawList, clear: Color, w: u32, h: u32) -> wgpu::CommandEncoder {
        let n = list.prims.len();
        if n > self.capacity {
            self.capacity = n.next_power_of_two();
            self.instances = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("prims"),
                size: (self.capacity * std::mem::size_of::<Prim>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if n > 0 {
            self.queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&list.prims));
        }
        let globals = [w as f32, h as f32, self.atlas_size as f32, self.atlas_size as f32];
        self.queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&globals));
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear.to_wgpu()), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind0, &[]);
            pass.set_vertex_buffer(0, self.instances.slice(..));
            for b in &list.batches {
                let bind = self.textures.get(b.tex as usize).and_then(Option::as_ref).or(self.textures[0].as_ref()).map(|t| &t.bind).unwrap();
                pass.set_bind_group(1, bind, &[]);
                pass.draw(0..4, b.start..b.end);
            }
        }
        enc
    }

    /// Encodes, submits and presents. Returns the CPU time it took in microseconds. With present
    /// timing, the frame aims at `pacer.target_ns` (set by `pacer.begin`) and the presentation
    /// reports that came back feed the pacer.
    pub fn submit(&mut self, frame: Frame, list: &DrawList, clear: Color) -> u32 {
        let t = std::time::Instant::now();
        let (w, h) = (self.config.width, self.config.height);
        match frame.kind {
            FrameKind::Surface(st) => {
                let view = st.texture.create_view(&Default::default());
                let enc = self.encode(&view, list, clear, w, h);
                self.queue.submit([enc.finish()]);
                self.queue.present(st);
            }
            #[cfg(target_os = "linux")]
            FrameKind::Own(i) => {
                let view = match self.own.as_ref() {
                    Some(o) => o.view(i).clone(),
                    None => return 0,
                };
                let enc = self.encode(&view, list, clear, w, h);
                self.queue.submit([enc.finish()]);
                let target = self.pacer.target_ns;
                let own = self.own.as_mut().unwrap();
                match own.present(i, target) {
                    Ok(true) => {}
                    Ok(false) => {
                        if let Err(e) = own.resize(&self.device, w, h) {
                            log::warn!("pacing: swapchain rebuild failed ({e})");
                        }
                    }
                    Err(e) => log::warn!("pacing: {e}"),
                }
                let refresh = own.refresh_ns;
                for (aimed, seen) in own.poll() {
                    self.pacer.on_presented(aimed, seen, refresh);
                }
            }
        }
        t.elapsed().as_micros() as u32
    }

    /// Renders the list offscreen and reads it back as RGBA8 (golden images, screenshots).
    pub fn capture(&mut self, list: &DrawList, clear: Color) -> (u32, u32, Vec<u8>) {
        let (w, h) = (self.config.width, self.config.height);
        let format = self.config.format;
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let mut enc = self.encode(&view, list, clear, w, h);
        let row = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range().expect("mapped");
        let bgra = matches!(format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let r = &data[(y * row) as usize..(y * row + w * 4) as usize];
            for p in r.chunks_exact(4) {
                if bgra {
                    out.extend_from_slice(&[p[2], p[1], p[0], 255]);
                } else {
                    out.extend_from_slice(&[p[0], p[1], p[2], 255]);
                }
            }
        }
        drop(data);
        buf.unmap();
        (w, h, out)
    }
}

