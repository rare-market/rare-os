//! Rare Render: the launcher's native GPU UI engine.
//!
//! One retained scene (Taffy layout) painted into a flat list of instanced primitives (SDF rounded
//! rects, gradients, rings, images, glyph masks) drawn by one wgpu pipeline. Text is shaped by
//! cosmic-text (HarfRust) and rasterised by swash into a glyph atlas. Images decode and resize off
//! the main thread into GPU textures sized for where they are drawn, with a disk cache of the
//! resized pixels. Animations are evaluated at frame time on the render thread, so nothing that
//! moves waits on app logic. Focus is an engine feature (spatial navigation with group memory),
//! and controller input is read from evdev on its own thread and wakes the loop at once.
//!
//! The app owns the frame loop (winit) and calls: input -> app state -> `Scene` edits ->
//! `Scene::layout` -> `Scene::paint` -> `Gpu::render`. Nothing redraws while nothing moves.

pub mod a11y;
pub mod anim;
pub mod chart;
pub mod clock;
pub mod color;
pub mod feedback;
pub mod focus;
pub mod gpu;
pub mod images;
pub mod input;
pub mod platform;
pub mod pointer;
pub mod scene;
pub mod scroll;
pub mod stats;
pub mod text;
pub mod theme;
pub mod view;

pub use anim::{Anim, CAnim, Ease};
pub use color::Color;
pub use gpu::{Batch, DrawList, Gpu, Prim, TexId};
pub use images::{Fit, ImageLoader, ImgReq, ImgState, Src};
pub use scene::{layer, Id, LinearGradient, Node, Scene, Shadow, SlotFn, SlotPaint};
pub use view::{group_corners, key, El, Look, Store, View};
pub use text::{TextStyle, TextSys};

pub use cosmic_text;
pub use taffy;
pub use winit;
