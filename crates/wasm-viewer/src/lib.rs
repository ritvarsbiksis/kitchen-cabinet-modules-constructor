//! A glTF viewer that renders into an HTML canvas with `wgpu`.
//!
//! The web app calls [`start_viewer`] with a canvas, the bytes of a `.glb` file
//! and the two skybox images; it returns a handle that owns the GPU resources,
//! the event listeners and the animation frame loop until `destroy()` is called
//! on it.
//!
//! `camera` is target independent and unit tested with a plain `cargo test`, as
//! are `model` and `environment`, which live in the shared `scene-assets` crate
//! and are re-exported here; the modules below them only exist on `wasm32`.

pub mod camera;

pub use scene_assets::{environment, model};

#[cfg(target_arch = "wasm32")]
mod renderer;
#[cfg(target_arch = "wasm32")]
mod viewer;

#[cfg(target_arch = "wasm32")]
pub use viewer::{start_viewer, Viewer};
