//! Asset loading shared by the wgpu crates.
//!
//! [`model`] turns a binary glTF into flat, GPU-ready buffers and [`environment`]
//! decodes the two skybox images with their mip chains. Neither touches the GPU
//! or the DOM, so both build for the host target and are unit tested there; the
//! crates that render (`wasm-viewer`, `wasm-kitchen`) only upload the results.

pub mod environment;
pub mod model;
