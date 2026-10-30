//! VK_EXT_present_timing on our own swapchain (Linux, Vulkan; `RR_PACING=timing`).
//!
//! wgpu creates its swapchains without `VK_SWAPCHAIN_CREATE_PRESENT_TIMING_BIT_EXT`, so this module
//! makes one itself on wgpu's `VkSurfaceKHR` and lets wgpu render into the swapchain images:
//!
//! ```text
//!   instance: + VK_KHR_get_surface_capabilities2          (hal instance callback)
//!   device:   + VK_KHR_present_id2, VK_EXT_present_timing, VK_KHR_calibrated_timestamps,
//!             presentId2 / presentTiming / presentAtAbsoluteTime features (hal device callback)
//!   swapchain: PRESENT_TIMING | PRESENT_ID_2, FIFO, >= 3 images, images wrapped as wgpu textures
//!   frame:    acquire (semaphore) -> raw barrier UNDEFINED -> COLOR_ATTACHMENT (waits acquire)
//!             -> wgpu render pass + submit -> raw barrier -> PRESENT_SRC (signals ready, fence)
//!             -> vkQueuePresentKHR + VkPresentId2KHR + VkPresentTimingsInfoEXT (target time on
//!                CLOCK_MONOTONIC, nearest refresh) -> vkGetPastPresentationTimingEXT feedback
//! ```
//!
//! wgpu tracks the wrapped images as colour targets; the two raw barriers keep the real layout
//! equal to that at every wgpu submission. Anything missing (extension, feature, the
//! CLOCK_MONOTONIC time domain, the gamescope layer refusing the flags) returns an error and the
//! Gpu falls back to wgpu's own surface with the estimated grid.

use ash::vk;
use std::ffi::{c_char, c_void, CStr};
use wgpu::hal::api::Vulkan;

const EXT_PRESENT_TIMING: &CStr = c"VK_EXT_present_timing";
const KHR_PRESENT_ID2: &CStr = c"VK_KHR_present_id2";
const KHR_CALIBRATED: &CStr = c"VK_KHR_calibrated_timestamps";
const KHR_SURFACE_CAPS2: &CStr = c"VK_KHR_get_surface_capabilities2";

const ST_PHYS_PRESENT_TIMING_FEATURES: i32 = 1000208000;
const ST_SWAPCHAIN_TIMING_PROPERTIES: i32 = 1000208001;
const ST_SWAPCHAIN_TIME_DOMAIN_PROPERTIES: i32 = 1000208002;
const ST_PRESENT_TIMINGS_INFO: i32 = 1000208003;
const ST_PRESENT_TIMING_INFO: i32 = 1000208004;
const ST_PAST_PRESENTATION_TIMING_INFO: i32 = 1000208005;
const ST_PAST_PRESENTATION_TIMING_PROPERTIES: i32 = 1000208006;
const ST_PAST_PRESENTATION_TIMING: i32 = 1000208007;
const ST_PRESENT_TIMING_SURFACE_CAPS: i32 = 1000208008;
const ST_SURFACE_CAPS_PRESENT_ID2: i32 = 1000479000;
const ST_PRESENT_ID2: i32 = 1000479001;
const ST_PHYS_PRESENT_ID2_FEATURES: i32 = 1000479002;

const SWAPCHAIN_PRESENT_ID2_BIT: u32 = 0x40;
const SWAPCHAIN_PRESENT_TIMING_BIT: u32 = 0x200;

const STAGE_QUEUE_OPERATIONS_END: u32 = 0x1;
const STAGE_REQUEST_DEQUEUED: u32 = 0x2;
const STAGE_FIRST_PIXEL_OUT: u32 = 0x4;
const STAGE_FIRST_PIXEL_VISIBLE: u32 = 0x8;

const TIMING_INFO_NEAREST_REFRESH_CYCLE: u32 = 0x2;
const PAST_TIMING_ALLOW_OUT_OF_ORDER: u32 = 0x2;

const TIME_DOMAIN_CLOCK_MONOTONIC: i32 = 1;
const ERROR_PRESENT_TIMING_QUEUE_FULL: i32 = -1000208000;

#[repr(C)]
struct PhysPresentTimingFeatures {
    s_type: i32,
    p_next: *mut c_void,
    present_timing: u32,
    present_at_absolute_time: u32,
    present_at_relative_time: u32,
}

#[repr(C)]
struct PhysPresentId2Features {
    s_type: i32,
    p_next: *mut c_void,
    present_id2: u32,
}

#[repr(C)]
struct SurfaceCapsPresentId2 {
    s_type: i32,
    p_next: *mut c_void,
    supported: u32,
}

#[repr(C)]
struct PresentTimingSurfaceCaps {
    s_type: i32,
    p_next: *mut c_void,
    present_timing_supported: u32,
    present_at_absolute_time_supported: u32,
    present_at_relative_time_supported: u32,
    present_stage_queries: u32,
}

#[repr(C)]
struct SwapchainTimingProperties {
    s_type: i32,
    p_next: *mut c_void,
    refresh_duration: u64,
    refresh_interval: u64,
}

#[repr(C)]
struct SwapchainTimeDomainProperties {
    s_type: i32,
    p_next: *mut c_void,
    time_domain_count: u32,
    p_time_domains: *mut i32,
    p_time_domain_ids: *mut u64,
}

#[repr(C)]
struct PastPresentationTimingInfo {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
    swapchain: vk::SwapchainKHR,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PresentStageTime {
    stage: u32,
    time: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PastPresentationTiming {
    s_type: i32,
    p_next: *mut c_void,
    present_id: u64,
    target_time: u64,
    present_stage_count: u32,
    p_present_stages: *mut PresentStageTime,
    time_domain: i32,
    time_domain_id: u64,
    report_complete: u32,
}

#[repr(C)]
struct PastPresentationTimingProperties {
    s_type: i32,
    p_next: *mut c_void,
    timing_properties_counter: u64,
    time_domains_counter: u64,
    presentation_timing_count: u32,
    p_presentation_timings: *mut PastPresentationTiming,
}

#[repr(C)]
struct PresentTimingInfo {
    s_type: i32,
    p_next: *const c_void,
    flags: u32,
    target_time: u64,
    time_domain_id: u64,
    present_stage_queries: u32,
    target_time_domain_present_stage: u32,
}

#[repr(C)]
struct PresentTimingsInfo {
    s_type: i32,
    p_next: *const c_void,
    swapchain_count: u32,
    p_timing_infos: *const PresentTimingInfo,
}

#[repr(C)]
struct PresentId2 {
    s_type: i32,
    p_next: *const c_void,
    swapchain_count: u32,
    p_present_ids: *const u64,
}

type PfnSetQueueSize = unsafe extern "system" fn(vk::Device, vk::SwapchainKHR, u32) -> vk::Result;
type PfnTimingProps = unsafe extern "system" fn(vk::Device, vk::SwapchainKHR, *mut SwapchainTimingProperties, *mut u64) -> vk::Result;
type PfnTimeDomains = unsafe extern "system" fn(vk::Device, vk::SwapchainKHR, *mut SwapchainTimeDomainProperties, *mut u64) -> vk::Result;
type PfnPastTiming = unsafe extern "system" fn(vk::Device, *const PastPresentationTimingInfo, *mut PastPresentationTimingProperties) -> vk::Result;

struct Fns {
    set_queue_size: PfnSetQueueSize,
    timing_props: PfnTimingProps,
    time_domains: PfnTimeDomains,
    past_timing: PfnPastTiming,
}

unsafe fn load<T>(instance: &ash::Instance, device: vk::Device, name: &CStr) -> Option<T> {
    let f = unsafe { (instance.fp_v1_0().get_device_proc_addr)(device, name.as_ptr() as *const c_char) }?;
    // SAFETY: the caller names a function whose signature is T.
    Some(unsafe { std::mem::transmute_copy::<unsafe extern "system" fn(), T>(&f) })
}

/// A wgpu instance whose Vulkan instance also has VK_KHR_get_surface_capabilities2 (present
/// timing needs it). None: not Vulkan, or the extension is missing.
pub(crate) fn instance() -> Option<wgpu::Instance> {
    let desc = wgpu::hal::InstanceDescriptor {
        name: "rare-render",
        flags: wgpu::InstanceFlags::default(),
        memory_budget_thresholds: Default::default(),
        backend_options: Default::default(),
        telemetry: None,
        display: None,
    };
    let cb: Box<wgpu::hal::vulkan::CreateInstanceCallback<'static>> = Box::new(|args| {
        let has = unsafe { args.entry.enumerate_instance_extension_properties(None) }
            .map(|v| v.iter().any(|e| e.extension_name_as_c_str() == Ok(KHR_SURFACE_CAPS2)))
            .unwrap_or(false);
        if has && !args.extensions.contains(&KHR_SURFACE_CAPS2) {
            args.extensions.push(KHR_SURFACE_CAPS2);
        }
    });
    let hal = unsafe { wgpu::hal::vulkan::Instance::init_with_callback(&desc, Some(cb)) }.ok()?;
    if !hal.shared_instance().extensions().contains(&KHR_SURFACE_CAPS2) {
        log::info!("pacing: VK_KHR_get_surface_capabilities2 missing");
        return None;
    }
    Some(unsafe { wgpu::Instance::from_hal::<Vulkan>(hal) })
}

/// Opens the device with present timing enabled. Err: the adapter cannot (the caller requests a
/// plain device).
pub(crate) fn open_device(adapter: &wgpu::Adapter, desc: &wgpu::DeviceDescriptor) -> Result<(wgpu::Device, wgpu::Queue, bool), String> {
    let hal = unsafe { adapter.as_hal::<Vulkan>() }.ok_or("not a Vulkan adapter")?;
    let caps = hal.physical_device_capabilities();
    for e in [EXT_PRESENT_TIMING, KHR_PRESENT_ID2, KHR_CALIBRATED] {
        if !caps.supports_extension(e) {
            return Err(format!("{} not supported", e.to_string_lossy()));
        }
    }
    let inst = hal.shared_instance().raw_instance();
    let pd = hal.raw_physical_device();
    // Query the features.
    let mut pt = PhysPresentTimingFeatures { s_type: ST_PHYS_PRESENT_TIMING_FEATURES, p_next: std::ptr::null_mut(), present_timing: 0, present_at_absolute_time: 0, present_at_relative_time: 0 };
    let mut id2 = PhysPresentId2Features { s_type: ST_PHYS_PRESENT_ID2_FEATURES, p_next: &mut pt as *mut _ as *mut c_void, present_id2: 0 };
    let mut f2 = vk::PhysicalDeviceFeatures2::default();
    f2.p_next = &mut id2 as *mut _ as *mut c_void;
    unsafe { inst.get_physical_device_features2(pd, &mut f2) };
    if pt.present_timing == 0 || id2.present_id2 == 0 {
        return Err("presentTiming / presentId2 features off".into());
    }
    let absolute = pt.present_at_absolute_time != 0;
    // Enable them: our structs go in front of wgpu's pNext chain.
    let mut en_pt = PhysPresentTimingFeatures { s_type: ST_PHYS_PRESENT_TIMING_FEATURES, p_next: std::ptr::null_mut(), present_timing: 1, present_at_absolute_time: absolute as u32, present_at_relative_time: 0 };
    let mut en_id2 = PhysPresentId2Features { s_type: ST_PHYS_PRESENT_ID2_FEATURES, p_next: &mut en_pt as *mut _ as *mut c_void, present_id2: 1 };
    let en_id2_ptr: *mut PhysPresentId2Features = &mut en_id2;
    let en_pt_ptr: *mut PhysPresentTimingFeatures = &mut en_pt;
    let cb: Box<wgpu::hal::vulkan::CreateDeviceCallback<'_>> = Box::new(move |args| {
        for e in [EXT_PRESENT_TIMING, KHR_PRESENT_ID2, KHR_CALIBRATED] {
            if !args.extensions.contains(&e) {
                args.extensions.push(e);
            }
        }
        // SAFETY: both structs outlive open_with_callback (they live in the enclosing frame).
        unsafe {
            (*en_pt_ptr).p_next = args.create_info.p_next as *mut c_void;
            args.create_info.p_next = en_id2_ptr as *const c_void;
        }
    });
    let open = unsafe { hal.open_with_callback(desc.required_features, &desc.required_limits, &desc.memory_hints, Some(cb)) }.map_err(|e| format!("open device: {e:?}"))?;
    drop(hal);
    let (device, queue) = unsafe { adapter.create_device_from_hal::<Vulkan>(open, desc) }.map_err(|e| format!("device: {e}"))?;
    let _ = (&en_id2, &en_pt);
    Ok((device, queue, absolute))
}

struct Slot {
    acquire: vk::Semaphore,
    fence: vk::Fence,
    pre: vk::CommandBuffer,
    post: vk::CommandBuffer,
}

/// Our swapchain with present timing.
pub(crate) struct Swapchain {
    device: ash::Device,
    pd: vk::PhysicalDevice,
    queue: vk::Queue,
    surface: vk::SurfaceKHR,
    swapchain_fns: ash::khr::swapchain::Device,
    surface_fns: ash::khr::surface::Instance,
    fns: Fns,
    raw: vk::SwapchainKHR,
    images: Vec<vk::Image>,
    textures: Vec<wgpu::Texture>,
    views: Vec<wgpu::TextureView>,
    ready: Vec<vk::Semaphore>,
    pool: vk::CommandPool,
    slots: Vec<Slot>,
    frame: usize,
    /// An image acquired (and moved to COLOR_ATTACHMENT) but not presented yet: a frame the app
    /// dropped. The next acquire hands it out again instead of leaking it.
    held: Option<u32>,
    pub format: wgpu::TextureFormat,
    vk_format: vk::Format,
    extent: vk::Extent2D,
    time_domain_id: u64,
    stages: u32,
    absolute: bool,
    next_id: u64,
    /// (present id, the refresh it aimed at)
    sent: Vec<(u64, u64)>,
    pub refresh_ns: Option<u64>,
    timing_counter: u64,
    pub presented: u64,
}

pub(crate) enum Acquired {
    Image(u32),
    Recreate,
}

fn barrier(img: vk::Image, from: vk::ImageLayout, to: vk::ImageLayout, src: vk::AccessFlags, dst: vk::AccessFlags) -> vk::ImageMemoryBarrier<'static> {
    vk::ImageMemoryBarrier::default()
        .src_access_mask(src)
        .dst_access_mask(dst)
        .old_layout(from)
        .new_layout(to)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(img)
        .subresource_range(vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 })
}

impl Swapchain {
    /// Builds the swapchain on `surface`. `want` is the format wgpu would have used.
    pub(crate) fn new(device: &wgpu::Device, surface: &wgpu::Surface<'static>, w: u32, h: u32, absolute: bool) -> Result<Swapchain, String> {
        let hal_dev = unsafe { device.as_hal::<Vulkan>() }.ok_or("not a Vulkan device")?;
        let raw_dev = hal_dev.raw_device().clone();
        let inst = hal_dev.shared_instance().raw_instance().clone();
        let entry = hal_dev.shared_instance().entry().clone();
        let pd = hal_dev.raw_physical_device();
        let family = hal_dev.queue_family_index();
        let raw_queue = hal_dev.raw_queue();
        drop(hal_dev);
        let vk_surface = {
            let s = unsafe { surface.as_hal::<Vulkan>() }.ok_or("not a Vulkan surface")?;
            unsafe { s.raw_native_handle() }.ok_or("no native surface")?
        };
        let fns = unsafe {
            Fns {
                set_queue_size: load(&inst, raw_dev.handle(), c"vkSetSwapchainPresentTimingQueueSizeEXT").ok_or("vkSetSwapchainPresentTimingQueueSizeEXT")?,
                timing_props: load(&inst, raw_dev.handle(), c"vkGetSwapchainTimingPropertiesEXT").ok_or("vkGetSwapchainTimingPropertiesEXT")?,
                time_domains: load(&inst, raw_dev.handle(), c"vkGetSwapchainTimeDomainPropertiesEXT").ok_or("vkGetSwapchainTimeDomainPropertiesEXT")?,
                past_timing: load(&inst, raw_dev.handle(), c"vkGetPastPresentationTimingEXT").ok_or("vkGetPastPresentationTimingEXT")?,
            }
        };
        // The surface must take present ids and timing, and report a usable stage.
        let caps2_fns = ash::khr::get_surface_capabilities2::Instance::new(&entry, &inst);
        let mut ptc = PresentTimingSurfaceCaps { s_type: ST_PRESENT_TIMING_SURFACE_CAPS, p_next: std::ptr::null_mut(), present_timing_supported: 0, present_at_absolute_time_supported: 0, present_at_relative_time_supported: 0, present_stage_queries: 0 };
        let mut idc = SurfaceCapsPresentId2 { s_type: ST_SURFACE_CAPS_PRESENT_ID2, p_next: &mut ptc as *mut _ as *mut c_void, supported: 0 };
        let mut caps2 = vk::SurfaceCapabilities2KHR::default();
        caps2.p_next = &mut idc as *mut _ as *mut c_void;
        let info = vk::PhysicalDeviceSurfaceInfo2KHR::default().surface(vk_surface);
        unsafe { caps2_fns.get_physical_device_surface_capabilities2(pd, &info, &mut caps2) }.map_err(|e| format!("surface caps2: {e}"))?;
        if idc.supported == 0 || ptc.present_timing_supported == 0 {
            return Err(format!("surface: presentId2 {} presentTiming {}", idc.supported, ptc.present_timing_supported));
        }
        let stages = ptc.present_stage_queries & (STAGE_FIRST_PIXEL_OUT | STAGE_FIRST_PIXEL_VISIBLE | STAGE_REQUEST_DEQUEUED | STAGE_QUEUE_OPERATIONS_END);
        if stages == 0 {
            return Err("surface reports no present stage".into());
        }
        let absolute = absolute && ptc.present_at_absolute_time_supported != 0;
        let surface_fns = ash::khr::surface::Instance::new(&entry, &inst);
        let formats = unsafe { surface_fns.get_physical_device_surface_formats(pd, vk_surface) }.map_err(|e| format!("formats: {e}"))?;
        let (vk_format, format) = formats
            .iter()
            .find_map(|f| match f.format {
                vk::Format::B8G8R8A8_UNORM => Some((f.format, wgpu::TextureFormat::Bgra8Unorm)),
                vk::Format::R8G8B8A8_UNORM => Some((f.format, wgpu::TextureFormat::Rgba8Unorm)),
                _ => None,
            })
            .ok_or("no 8-bit UNORM surface format")?;
        let swapchain_fns = ash::khr::swapchain::Device::new(&inst, &raw_dev);
        let pool = unsafe {
            raw_dev.create_command_pool(&vk::CommandPoolCreateInfo::default().queue_family_index(family).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER), None)
        }
        .map_err(|e| format!("command pool: {e}"))?;
        let mut sc = Swapchain {
            device: raw_dev,
            pd,
            queue: raw_queue,
            surface: vk_surface,
            swapchain_fns,
            surface_fns,
            fns,
            raw: vk::SwapchainKHR::null(),
            images: Vec::new(),
            textures: Vec::new(),
            views: Vec::new(),
            ready: Vec::new(),
            pool,
            slots: Vec::new(),
            frame: 0,
            held: None,
            format,
            vk_format,
            extent: vk::Extent2D { width: w, height: h },
            time_domain_id: 0,
            stages,
            absolute,
            next_id: 1,
            sent: Vec::new(),
            refresh_ns: None,
            timing_counter: u64::MAX,
            presented: 0,
        };
        if let Err(e) = sc.build(device, w, h) {
            sc.destroy(device);
            return Err(e);
        }
        Ok(sc)
    }

    fn build(&mut self, device: &wgpu::Device, w: u32, h: u32) -> Result<(), String> {
        let caps = unsafe { self.surface_fns.get_physical_device_surface_capabilities(self.pd, self.surface) }.map_err(|e| format!("caps: {e}"))?;
        let extent = if caps.current_extent.width != u32::MAX { caps.current_extent } else { vk::Extent2D { width: w.max(1), height: h.max(1) } };
        let mut count = caps.min_image_count.max(3);
        if caps.max_image_count > 0 {
            count = count.min(caps.max_image_count);
        }
        let alpha = if caps.supported_composite_alpha.contains(vk::CompositeAlphaFlagsKHR::OPAQUE) { vk::CompositeAlphaFlagsKHR::OPAQUE } else { vk::CompositeAlphaFlagsKHR::INHERIT };
        let old = self.raw;
        let info = vk::SwapchainCreateInfoKHR::default()
            .flags(vk::SwapchainCreateFlagsKHR::from_raw(SWAPCHAIN_PRESENT_TIMING_BIT | SWAPCHAIN_PRESENT_ID2_BIT))
            .surface(self.surface)
            .min_image_count(count)
            .image_format(self.vk_format)
            .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(caps.current_transform)
            .composite_alpha(alpha)
            .present_mode(vk::PresentModeKHR::FIFO)
            .clipped(true)
            .old_swapchain(old);
        let raw = unsafe { self.swapchain_fns.create_swapchain(&info, None) }.map_err(|e| format!("create swapchain: {e}"))?;
        self.release(device);
        if old != vk::SwapchainKHR::null() {
            unsafe { self.swapchain_fns.destroy_swapchain(old, None) };
        }
        self.raw = raw;
        self.extent = extent;
        self.images = unsafe { self.swapchain_fns.get_swapchain_images(raw) }.map_err(|e| format!("images: {e}"))?;
        let hal_dev = unsafe { device.as_hal::<Vulkan>() }.ok_or("device")?;
        for (i, img) in self.images.iter().enumerate() {
            let size = wgpu::Extent3d { width: extent.width, height: extent.height, depth_or_array_layers: 1 };
            let hal_desc = wgpu::hal::TextureDescriptor {
                label: Some("rr-swapchain"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUses::COLOR_TARGET,
                memory_flags: wgpu::hal::MemoryFlags::empty(),
                view_formats: vec![],
            };
            // The swapchain owns the image: wgpu gets a no-op drop callback.
            let ht = unsafe { hal_dev.texture_from_raw(*img, &hal_desc, Some(Box::new(|| {})), wgpu::hal::vulkan::TextureMemory::External) };
            let desc = wgpu::TextureDescriptor {
                label: Some("rr-swapchain"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            };
            let tex = unsafe { device.create_texture_from_hal::<Vulkan>(ht, &desc, wgpu::TextureUses::UNINITIALIZED) };
            self.views.push(tex.create_view(&Default::default()));
            self.textures.push(tex);
            let _ = i;
            self.ready.push(unsafe { self.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }.map_err(|e| format!("semaphore: {e}"))?);
        }
        drop(hal_dev);
        let n = self.images.len();
        while self.slots.len() < n {
            let cbs = unsafe { self.device.allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(self.pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(2)) }
                .map_err(|e| format!("command buffers: {e}"))?;
            let acquire = unsafe { self.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }.map_err(|e| format!("semaphore: {e}"))?;
            let fence = unsafe { self.device.create_fence(&vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED), None) }.map_err(|e| format!("fence: {e}"))?;
            self.slots.push(Slot { acquire, fence, pre: cbs[0], post: cbs[1] });
        }
        unsafe {
            let _ = (self.fns.set_queue_size)(self.device.handle(), raw, 16);
        }
        // CLOCK_MONOTONIC must be one of the swapchain's time domains: it is the clock the pacer,
        // the input stamps and the journeys capture all use.
        let mut props = SwapchainTimeDomainProperties { s_type: ST_SWAPCHAIN_TIME_DOMAIN_PROPERTIES, p_next: std::ptr::null_mut(), time_domain_count: 0, p_time_domains: std::ptr::null_mut(), p_time_domain_ids: std::ptr::null_mut() };
        let mut counter = 0u64;
        let r = unsafe { (self.fns.time_domains)(self.device.handle(), raw, &mut props, &mut counter) };
        if r != vk::Result::SUCCESS && r != vk::Result::INCOMPLETE {
            return Err(format!("time domains: {r}"));
        }
        let n = props.time_domain_count as usize;
        let mut domains = vec![0i32; n];
        let mut ids = vec![0u64; n];
        props.p_time_domains = domains.as_mut_ptr();
        props.p_time_domain_ids = ids.as_mut_ptr();
        let r = unsafe { (self.fns.time_domains)(self.device.handle(), raw, &mut props, &mut counter) };
        if r != vk::Result::SUCCESS && r != vk::Result::INCOMPLETE {
            return Err(format!("time domains: {r}"));
        }
        let k = (0..(props.time_domain_count as usize).min(n)).find(|i| domains[*i] == TIME_DOMAIN_CLOCK_MONOTONIC).ok_or_else(|| format!("no CLOCK_MONOTONIC time domain ({domains:?})"))?;
        self.time_domain_id = ids[k];
        self.timing_counter = u64::MAX;
        self.refresh();
        log::info!(
            "pacing: own swapchain {}x{} {} images, {:?}, stages {:#x}, absolute {}, refresh {:?}",
            extent.width,
            extent.height,
            self.images.len(),
            self.format,
            self.stages,
            self.absolute,
            self.refresh_ns
        );
        Ok(())
    }

    /// Reads the refresh duration when the driver says it changed.
    fn refresh(&mut self) {
        let mut p = SwapchainTimingProperties { s_type: ST_SWAPCHAIN_TIMING_PROPERTIES, p_next: std::ptr::null_mut(), refresh_duration: 0, refresh_interval: 0 };
        let mut counter = 0u64;
        let r = unsafe { (self.fns.timing_props)(self.device.handle(), self.raw, &mut p, &mut counter) };
        if r == vk::Result::SUCCESS && p.refresh_duration > 0 {
            self.refresh_ns = Some(p.refresh_duration);
            self.timing_counter = counter;
        }
    }

    /// Frees what refers to the current images (before a rebuild or on drop).
    fn release(&mut self, device: &wgpu::Device) {
        unsafe {
            let _ = self.device.queue_wait_idle(self.queue);
        }
        self.views.clear();
        self.textures.clear();
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        for s in self.ready.drain(..) {
            unsafe { self.device.destroy_semaphore(s, None) };
        }
        self.images.clear();
        self.sent.clear();
        self.held = None;
    }

    pub(crate) fn destroy(&mut self, device: &wgpu::Device) {
        self.release(device);
        unsafe {
            for s in self.slots.drain(..) {
                self.device.destroy_semaphore(s.acquire, None);
                self.device.destroy_fence(s.fence, None);
            }
            self.device.destroy_command_pool(self.pool, None);
            if self.raw != vk::SwapchainKHR::null() {
                self.swapchain_fns.destroy_swapchain(self.raw, None);
                self.raw = vk::SwapchainKHR::null();
            }
        }
    }

    pub(crate) fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) -> Result<(), String> {
        self.build(device, w, h)
    }

    pub(crate) fn view(&self, i: u32) -> &wgpu::TextureView {
        &self.views[i as usize]
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        (self.extent.width, self.extent.height)
    }

    /// Acquires the next image and moves it to COLOR_ATTACHMENT (waiting for the acquire).
    pub(crate) fn acquire(&mut self) -> Result<Acquired, String> {
        if let Some(i) = self.held {
            return Ok(Acquired::Image(i));
        }
        let slot = &self.slots[self.frame % self.slots.len()];
        // The slot's command buffers are free once its last present's barrier ran.
        unsafe {
            self.device.wait_for_fences(&[slot.fence], true, u64::MAX).map_err(|e| format!("fence: {e}"))?;
        }
        let idx = match unsafe { self.swapchain_fns.acquire_next_image(self.raw, u64::MAX, slot.acquire, vk::Fence::null()) } {
            Ok((i, _suboptimal)) => i,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => return Ok(Acquired::Recreate),
            Err(e) => return Err(format!("acquire: {e}")),
        };
        let img = self.images[idx as usize];
        unsafe {
            let cb = slot.pre;
            self.device.reset_command_buffer(cb, vk::CommandBufferResetFlags::empty()).map_err(|e| format!("cb: {e}"))?;
            self.device.begin_command_buffer(cb, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)).map_err(|e| format!("cb: {e}"))?;
            let b = barrier(img, vk::ImageLayout::UNDEFINED, vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL, vk::AccessFlags::empty(), vk::AccessFlags::COLOR_ATTACHMENT_WRITE | vk::AccessFlags::COLOR_ATTACHMENT_READ);
            self.device.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::DependencyFlags::empty(), &[], &[], &[b]);
            self.device.end_command_buffer(cb).map_err(|e| format!("cb: {e}"))?;
            let waits = [slot.acquire];
            let stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            let cbs = [cb];
            let submit = vk::SubmitInfo::default().wait_semaphores(&waits).wait_dst_stage_mask(&stages).command_buffers(&cbs);
            self.device.queue_submit(self.queue, &[submit], vk::Fence::null()).map_err(|e| format!("submit: {e}"))?;
        }
        self.held = Some(idx);
        Ok(Acquired::Image(idx))
    }

    /// After wgpu's submit: moves the image to PRESENT_SRC and presents it aimed at `target_ns`
    /// (CLOCK_MONOTONIC, 0: as soon as possible). Returns false when the swapchain is out of date.
    pub(crate) fn present(&mut self, idx: u32, target_ns: u64) -> Result<bool, String> {
        let si = self.frame % self.slots.len();
        self.frame += 1;
        self.held = None;
        let slot = &self.slots[si];
        let img = self.images[idx as usize];
        let ready = self.ready[idx as usize];
        unsafe {
            // Reset only now: a frame dropped before this point leaves the fence signalled.
            self.device.reset_fences(&[slot.fence]).map_err(|e| format!("fence: {e}"))?;
            let cb = slot.post;
            self.device.reset_command_buffer(cb, vk::CommandBufferResetFlags::empty()).map_err(|e| format!("cb: {e}"))?;
            self.device.begin_command_buffer(cb, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)).map_err(|e| format!("cb: {e}"))?;
            let b = barrier(img, vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL, vk::ImageLayout::PRESENT_SRC_KHR, vk::AccessFlags::COLOR_ATTACHMENT_WRITE, vk::AccessFlags::empty());
            self.device.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::PipelineStageFlags::BOTTOM_OF_PIPE, vk::DependencyFlags::empty(), &[], &[], &[b]);
            self.device.end_command_buffer(cb).map_err(|e| format!("cb: {e}"))?;
            let cbs = [cb];
            let sig = [ready];
            let submit = vk::SubmitInfo::default().command_buffers(&cbs).signal_semaphores(&sig);
            self.device.queue_submit(self.queue, &[submit], slot.fence).map_err(|e| format!("submit: {e}"))?;
        }
        let id = self.next_id;
        self.next_id += 1;
        let ids = [id];
        let query = if self.stages & STAGE_FIRST_PIXEL_OUT != 0 {
            STAGE_FIRST_PIXEL_OUT
        } else if self.stages & STAGE_FIRST_PIXEL_VISIBLE != 0 {
            STAGE_FIRST_PIXEL_VISIBLE
        } else if self.stages & STAGE_REQUEST_DEQUEUED != 0 {
            STAGE_REQUEST_DEQUEUED
        } else {
            STAGE_QUEUE_OPERATIONS_END
        };
        let target = if self.absolute { target_ns } else { 0 };
        let timing = [PresentTimingInfo {
            s_type: ST_PRESENT_TIMING_INFO,
            p_next: std::ptr::null(),
            flags: if target > 0 { TIMING_INFO_NEAREST_REFRESH_CYCLE } else { 0 },
            target_time: target,
            time_domain_id: self.time_domain_id,
            present_stage_queries: query,
            target_time_domain_present_stage: if target > 0 { query } else { 0 },
        }];
        let timings = PresentTimingsInfo { s_type: ST_PRESENT_TIMINGS_INFO, p_next: std::ptr::null(), swapchain_count: 1, p_timing_infos: timing.as_ptr() };
        let id2 = PresentId2 { s_type: ST_PRESENT_ID2, p_next: &timings as *const _ as *const c_void, swapchain_count: 1, p_present_ids: ids.as_ptr() };
        let waits = [ready];
        let scs = [self.raw];
        let idxs = [idx];
        let mut info = vk::PresentInfoKHR::default().wait_semaphores(&waits).swapchains(&scs).image_indices(&idxs);
        info.p_next = &id2 as *const _ as *const c_void;
        let r = unsafe { self.swapchain_fns.queue_present(self.queue, &info) };
        self.sent.push((id, target_ns));
        if self.sent.len() > 64 {
            self.sent.drain(..32);
        }
        self.presented += 1;
        match r {
            Ok(_) => Ok(true),
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Ok(false),
            // VK_ERROR_PRESENT_TIMING_QUEUE_FULL_EXT: presented, but this one carries no report.
            Err(e) if e.as_raw() == ERROR_PRESENT_TIMING_QUEUE_FULL => Ok(true),
            Err(e) => Err(format!("present: {e}")),
        }
    }

    /// Finished presentation reports: (the refresh the frame aimed at, when it reached the
    /// screen), on CLOCK_MONOTONIC.
    pub(crate) fn poll(&mut self) -> Vec<(u64, u64)> {
        const N: usize = 8;
        let mut stages = [[PresentStageTime::default(); 4]; N];
        let mut timings: [PastPresentationTiming; N] = std::array::from_fn(|i| PastPresentationTiming {
            s_type: ST_PAST_PRESENTATION_TIMING,
            p_next: std::ptr::null_mut(),
            present_id: 0,
            target_time: 0,
            present_stage_count: 4,
            p_present_stages: std::ptr::null_mut(),
            time_domain: 0,
            time_domain_id: 0,
            report_complete: 0,
        }
        .with_stages(stages[i].as_mut_ptr()));
        let info = PastPresentationTimingInfo { s_type: ST_PAST_PRESENTATION_TIMING_INFO, p_next: std::ptr::null(), flags: PAST_TIMING_ALLOW_OUT_OF_ORDER, swapchain: self.raw };
        let mut props = PastPresentationTimingProperties {
            s_type: ST_PAST_PRESENTATION_TIMING_PROPERTIES,
            p_next: std::ptr::null_mut(),
            timing_properties_counter: 0,
            time_domains_counter: 0,
            presentation_timing_count: N as u32,
            p_presentation_timings: timings.as_mut_ptr(),
        };
        let r = unsafe { (self.fns.past_timing)(self.device.handle(), &info, &mut props) };
        let _ = &mut stages;
        if r != vk::Result::SUCCESS && r != vk::Result::INCOMPLETE {
            return Vec::new();
        }
        if props.timing_properties_counter != self.timing_counter {
            self.refresh();
        }
        let mut out = Vec::new();
        for t in timings.iter().take((props.presentation_timing_count as usize).min(N)) {
            if t.report_complete == 0 || (t.time_domain != TIME_DOMAIN_CLOCK_MONOTONIC && t.time_domain_id != self.time_domain_id) {
                continue;
            }
            let st = unsafe { std::slice::from_raw_parts(t.p_present_stages, (t.present_stage_count as usize).min(4)) };
            let Some(seen) = st.iter().filter(|s| s.time > 0).map(|s| s.time).max() else { continue };
            let aimed = self.sent.iter().find(|(id, _)| *id == t.present_id).map(|(_, a)| *a).unwrap_or(0);
            out.push((aimed, seen));
        }
        out
    }
}

impl PastPresentationTiming {
    fn with_stages(mut self, p: *mut PresentStageTime) -> Self {
        self.p_present_stages = p;
        self
    }
}
