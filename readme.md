# Rust Trading Testbed

We're testing Rust as the backend and maybe the frontend engine for a trading system.

# the different configs

You are to build out each of these configs and compare them.
Each config will be test with 8 different charts, each with 6 different indicators ideally using pure Rust and the rust talib library.
* 10 EMA - white
* 20 EMA
* 50 EMA
* 200 SMA - yellow
* VWAP computed fresh for each day. - purple
* horizontal volume profile 40 bars for the profile - 50% opacity, right hand side independent scale, blue

* You will generate random data a the second level for each of the 8 charts and the data generation timing will not be included.
* Streaming data model: load the first 4000 bars statically, then stream the remaining 19,400 bars in batches at a configurable rate (default ~1000 bars/sec). Indicators must update incrementally as bars stream in.
* The framework must be launched from a locally running web server for each of these configs below. 
* I've provided folders for each config in the root directory.


## 1. Tauri + React + TypeScript + Rust

    - React for the frontend.
    - TradingView Lightweight Charts for financial charts.
    - Dockview or Golden Layout for workspace management.
    - Rust for market data processing and order execution.
    - Tauri for desktop packaging.
    - Same frontend deployed as a conventional web application.

## 2. egui + eframe + egui_dock + wgpu

    - Rust for the entire frontend.
    - Rust for market data and trading logic.
    - wgpu for custom chart rendering.
    - WebAssembly for browser deployment.
    - Native executable for desktop.

## 3. Slint + Rust
    - Slint for the UI framework.
    - Rust for market data and trading logic.
    - WebAssembly for browser deployment.
    - Native executable for desktop.

## Running

- Tauri: `cd 1_tauri && npm run serve:web` → http://localhost:5171 (`?bench=1` to bench); desktop via `src-tauri/target/release/app.exe --bench`.
- egui: `cd 2_egui && trunk serve --port 5172 --release` → http://localhost:5172 (`?bench=1`); native `cargo run --release -- --bench`.
- Slint: `cd 3_slint && ./target/release/serve.exe` → http://localhost:5173 (`?bench=1`); native `slint-desktop.exe --bench`.
- Stream rate: `?rate=N` (web) / `--rate=N` (native), bars/sec, default 1000.
- Results land in `results/`; see `results/COMPARISON.md` for the cross-config comparison.

