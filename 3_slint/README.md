# Config 3 — Slint + Rust

8 candlestick charts × 23,400 1-second bars, 6 indicators each, one window.
Everything except the shell HTML is Rust — UI markup is `.slint`, all chart
drawing is a hand-rolled software rasterizer in Rust.

## Stack

- **UI**: [`slint`](https://slint.dev) 1.18.1 (`slint` + `slint-build`), dark theme, fixed 4×2 grid
- **Chart rendering**: custom software rasterizer → `slint::SharedPixelBuffer<Rgba8Pixel>` → `slint::Image`
- **Indicators**: `ta` 0.5.0 (pure-Rust "rust talib") for EMA10/20/50 + SMA200;
  `shared/indicators` for daily-reset VWAP + 40-bin volume profile
- **Data**: `shared/datagen` (`datagen::generate(chart, 1)`)
- **Web**: `wasm32-unknown-unknown` via wasm-pack, Slint winit+FemtoVG (WebGL)

## Layout

```
ui/app.slint        Slint UI: AppWindow (4x2 grid) + ChartPanel (title bar, Image, TouchArea)
src/lib.rs          crate root, wasm_bindgen(start) entry
src/main.rs         native binary (slint-desktop)
src/app.rs          window wiring, pan/zoom callbacks, live-update timer
src/chart.rs        Chart state, ta-based indicators, per-chart renderer
src/raster.rs       tiny rasterizer (fill/rect/line/alpha-blend on RGBA8)
src/bench.rs        scripted benchmark state machine -> results/*.json
src/plat.rs         time/memory/env/emit shims for native vs wasm
src/bin/serve.rs    std-only static server (:5173) + POST /save for web results
tests/indicators.rs ta vs shared-reference cross-check (1e-9, no warmup needed)
index.html          web host page (<canvas id="canvas">)
build-web.{sh,ps1}  wasm-pack build + sizes.json manifest
serve.{sh,ps1}      start the :5173 server
```

## Streaming data model

Each chart preloads the first `datagen::INITIAL_BARS` (4000) bars statically,
then the remaining 19,400 generated bars are **streamed** through the same
O(1)-per-bar incremental indicator path as live updates: a `slint::Timer`
pushes `ceil(rate × 50 ms)` real `datagen` bars into all 8 charts every 50 ms
(`src/app.rs::start_stream`, `src/chart.rs::stream_batch`). The volume profile
is re-binned once per batch. Rate is configurable: `--rate=N` / `?rate=N`
bars/sec, default **1000** interactive, **2000** in bench mode. After catch-up,
synthetic live updates continue (interactive 1 Hz; bench phases unchanged).

## Run

```bash
# desktop
cargo run --release --bin slint-desktop                       # interactive (streams at 1000 bars/s)
cargo run --release --bin slint-desktop -- --rate=5000        # faster stream
cargo run --release --bin slint-desktop -- --bench            # scripted bench -> results/slint-desktop.json

# web
./build-web.sh       # or build-web.ps1 — produces pkg/ + sizes.json
./serve.sh           # or serve.ps1 — http://localhost:5173
# browser: http://localhost:5173                 interactive (1000 bars/s)
#          http://localhost:5173/?rate=3000      interactive, faster stream
#          http://localhost:5173/?bench=1        scripted bench -> POSTs results/slint-web.json
#                                                (also logs JSON + triggers a download)
```

Any static file server works instead of `serve` (`npx serve -l 5173 .`,
`python -m http.server 5173`), but only `serve` persists the bench JSON via
`POST /save`.

## Interaction

- **Pan**: left-drag horizontally on a chart
- **Zoom**: mouse wheel over a chart (zooms around the cursor position)
- Charts append a live 1s bar every second in interactive mode (view follows
  the right edge).
