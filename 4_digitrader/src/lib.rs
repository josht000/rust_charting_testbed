slint::include_modules!();

pub mod app;
pub mod broker;
pub mod chart;
pub mod drawings;
pub mod hotkeys;
pub mod indicators;
pub mod model;
mod settings;
pub mod styles;
#[cfg(target_family = "wasm")]
mod web_popout;
pub mod workspace;

pub fn run_app() {
    app::run();
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_main() {
    console_error_panic_hook::set_once();
    run_app();
}
