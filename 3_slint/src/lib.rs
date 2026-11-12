//! Slint + Rust trading testbed (config 3).
//!
//! 8 candlestick charts x 23,400 1-second bars, 6 indicators each.
//!
//! Render paths:
//! - pixels (default): software-rasterized into `slint::SharedPixelBuffer`
//! - wgpu: GPU chart textures composited by Slint
//! - wgpu-transform: experimental GPU view transforms and chart textures
//! - paths: Slint path layers

slint::include_modules!();

pub mod app;
pub mod bench;
pub mod chart;
pub mod geom;
pub mod gpu;
mod gpu_transform;
pub mod plat;
pub mod raster;

pub fn run_app() {
    app::run();
}

/// wasm-pack entry point (crate-type cdylib). Slint's winit backend attaches
/// to the `<canvas id="canvas">` element in index.html.
#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_main() {
    console_error_panic_hook::set_once();
    run_app();
}
