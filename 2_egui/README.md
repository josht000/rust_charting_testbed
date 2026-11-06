# Config 2 — egui + eframe + egui_dock + wgpu

8 candlestick charts (23,400 one-second bars each) in a dockable workspace,
each with 6 indicators: EMA10 (white), EMA20 (orange), EMA50 (cyan),
SMA200 (yellow), VWAP with per-day reset (purple), and a 40-bin horizontal
volume profile (blue, 50% opacity, right edge, independent x scale).

- Data: `../shared/datagen` (identical seeds/data for all configs)
- **Streaming data model**: each chart preloads `INITIAL_BARS` (4000) bars at
  startup; the remaining 19,400 bars replay in ~75 ms batches at
  `--rate=N` / `?rate=N` bars/sec per chart (default 1000 interactive,
  2000 in bench mode). Indicators update incrementally per streamed bar;
  after catch-up a synthetic 1 bar/sec feed keeps charts live.
- Indicators: `ta` crate for EMA/SMA, `../shared/indicators` for VWAP + profile
- Rendering: custom wgpu paint callback (`egui_wgpu::CallbackTrait`) for
  candles/wicks/lines/profile; egui::Painter for grid + axis labels.
  Falls back to pure Painter shapes if the wgpu render state is missing.
- Docking: `egui_dock` (drag tabs to re-arrange the 2×4 grid)

## Requirements

- Rust (msvc on Windows) + `rustup target add wasm32-unknown-unknown`
- `cargo install trunk wasm-bindgen-cli` (trunk auto-fetches the matching
  wasm-bindgen/wasm-opt versions on first build)
- A WebGL2- or WebGPU-capable browser for the web build (Chrome/Edge/Firefox)

## Desktop

```sh
cargo run --release                  # streams at default 1000 bars/s/chart
cargo run --release -- --rate 4000   # faster catch-up (~5 s)
cargo run --release -- --bench       # scripted benchmark, writes
                                     # ../results/egui-desktop.json
cargo run --release -- --bench --rate 5000
```

The benchmark first waits for the stream catch-up to finish (reported as
`stream_catchup_ms` + `stream_rate_bars_per_sec`, with
`frame_time.stream_catchup` stats), then runs the same phases as before.

Interactions: drag a chart to pan, mouse wheel to zoom around the cursor,
drag tabs to re-dock.

## Web (wasm)

```sh
trunk serve --port 5172      # or ./serve.sh / serve.ps1
# open http://localhost:5172
```

For a production bundle: `trunk build --release` → `dist/`.

### Web benchmark

Open `http://localhost:5172/?bench=1` (optionally `&rate=4000`) and wait
~50 s for "bench: DONE" in the
status bar. The result JSON is:

- logged to the console between `==BENCH_JSON_BEGIN==` / `==BENCH_JSON_END==`,
- stored on `window.__bench_json`,
- auto-downloaded as `egui-web.json` (if the browser allows it).

Copy it to `../results/egui-web.json`. For headless capture see
`save_results.mjs`:

```sh
msedge --headless=new --enable-logging=stderr "http://localhost:5172/?bench=1" 2> bench.log
node save_results.mjs bench.log ../results/egui-web.json
```

## Tests / lint

```sh
cargo test                     # cross-checks `ta` EMA/SMA vs shared/indicators
cargo clippy --all-targets     # clean
```

## Layout

```
src/main.rs    native + wasm entry points (--bench / ?bench=1)
src/app.rs     eframe App + egui_dock workspace (2x4 grid)
src/chart.rs   chart model, indicators (ta), view, Painter renderer
src/gpu.rs     custom wgpu paint callback (candles/lines/profile verts)
src/bench.rs   scripted benchmark + results/schema.md JSON
src/timing.rs  portable Instant / memory / env helpers
index.html     trunk entry
serve.{sh,ps1} dev server on :5172
```
