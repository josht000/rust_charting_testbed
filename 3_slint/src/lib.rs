//! Slint + Rust trading testbed (config 3).
//!
//! 8 candlestick charts x 23,400 1-second bars, 6 indicators each.
//! Charts are software-rasterized in Rust into `slint::SharedPixelBuffer`
//! images (see `chart.rs` / `raster.rs`); the `.slint` UI is a 4x2 grid of
//! `Image` panels that forwards pan/zoom events back to Rust.

slint::include_modules!();

pub mod app;
pub mod bench;
pub mod chart;
#[cfg(not(target_family = "wasm"))]
pub mod gpu;
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
