# NOTES — slint config

## Rendering approach

Slint has no canvas/immediate-mode widget, so each chart is software-rasterized
in Rust directly into a `slint::SharedPixelBuffer<Rgba8Pixel>` (hand-rolled
rasterizer in `src/raster.rs`, ~140 lines — no tiny-skia needed) and displayed
in a plain `Image` element (`image-fit: fill`).

- Buffer size = panel pixel size (from `TouchArea` `changed width/height`), not per-bar.
- Candles: when >2 bars/px the visible range is aggregated per pixel column
  (first-open/last-close/max-high/min-low); zoomed in, real per-bar candles.
- Indicator polylines are stride-sampled to ≈3 points/pixel column.
- Volume profile: last 2000 bars, 40 bins, right edge, independent horizontal
  scale, blue at 50% alpha (per-pixel blend).
- Pan/zoom: `TouchArea` `pointer-event`/`moved`/`scroll-event` → Rust view
  transform → re-render that chart only.

## Indicators / "rust talib" choice

Chose **pure-Rust `ta` 0.5.0** for EMA10/20/50 and SMA200 (the C `ta-lib`
wrappers need a native TA-Lib install — a non-starter on Windows).
`ta::ExponentialMovingAverage` seeds with the first fed value, so we feed it
`SMA(first period closes)` as the seed: the recursion then matches the
TA-Lib-style reference in `shared/indicators` *exactly* (< 1e-9 everywhere,
verified in `tests/indicators.rs` — no warmup tolerance required).

- VWAP (daily reset via `ts/86400`) and the 40-bin volume profile reuse
  `shared/indicators` verbatim.
- Live updates keep incremental `ta` indicator state per chart (O(1)/bar);
  EMA state converges to reference < 1e-9 after ~4×period (warmup note: `ta`
  incremental seeding differs from SMA-seeded during the first ~40–200 bars
  after a cold start — irrelevant here since state is warmed over 4,000+ bars).

## Streaming data model

- `Chart::new` generates the full day (generation excluded from timings) but
  preloads only the first `INITIAL_BARS` = 4000 bars: indicators + `LiveInd`
  incremental state are built on that prefix; the remaining 19,400 bars sit in
  a `rest` replay queue.
- `Chart::stream_batch(n)` moves up to `n` **real** generated bars through the
  same O(1) incremental indicator path used by live updates
  (`ta` EMA/SMA `next()`, running VWAP accumulators); the volume profile is
  re-binned (last 2000 bars × 40 bins) once per batch, not per bar.
- `App::start_stream` drives all 8 charts with one `slint::Timer` every 50 ms;
  batch = `ceil(rate × 0.05)`. View follows the right edge while catching up.
- After all 8 streams are exhausted: interactive mode starts the 1 Hz
  synthetic live timer; bench mode folds catch-up frame stats and proceeds to
  the indicator/pan-zoom/live phases.
- Bench records `stream_catchup_ms`, `stream_rate_bars_per_sec` (configured)
  and `frame_time.stream_catchup`. **Bench default rate = 2000 bars/s**
  (interactive default 1000; override with `--rate` / `?rate`).
- Measured: desktop catch-up 10.0 s @ rate 2000 (~1940 bars/s achieved —
  50 ms ticks plus ~22 ms/tick of rasterize work ≈ on target); web headless
  SwiftShader 11.2 s (~1730/s). Frame avg during catch-up ~22 ms desktop /
  ~32 ms web — the 8-chart re-render is the bottleneck, not the indicator
  updates (those are <1 ms/batch).

## Workspace limitation

Slint ships no docking/split-layout library, so the workspace is a **fixed
4×2 `GridLayout`** of `ChartPanel`s. Panels resize proportionally with the
window; there are no draggable splitters or tear-out tabs. This is the main
feature gap vs. config 1 (Dockview) / config 2 (egui_dock).

## Benchmark methodology

- Frame time = latency from a scripted UI action (pan/zoom/live-update tick)
  to `RenderingState::AfterRendering` (via `window.set_rendering_notifier`).
  fps = rendered frames / phase duration.
- Pan/zoom phase: 16 ms timer alternates panning/zooming a rotating chart, 10 s.
- Live phases append a synthetic bar to *all 8 charts* each tick: 1 Hz and
  100 Hz (10 ms timer) respectively, 10 s each. At 100 Hz the workload
  saturates (~22 ms desktop / ~13 ms wasm per 8-chart update) — the measured
  fps is the honest sustained update rate.
- Memory: Windows `GetProcessMemoryInfo` WorkingSetSize (desktop);
  `performance.memory.usedJSHeapSize` (web — JS heap only; wasm linear memory
  is not included, so web memory is under-reported).
- `startup_ms_to_first_chart` = app-start (post-datagen) → first real chart
  rasterization.
- `--bench` (desktop) writes + prints `results/slint-desktop.json` then exits.
  `?bench=1` (web) console.logs the JSON, POSTs it to `/save` (the bundled
  `serve` binary persists `results/slint-web.json`), and offers a download.

## Web notes / caveats

- Built with `wasm-pack build --release --target web --no-default-features
  --features slint/backend-winit,slint/renderer-femtovg` — winit backend +
  FemtoVG over WebGL. Slint embeds the Inter font, so text works offline.
- Requires a `<canvas id="canvas">` element (Slint convention); canvas fills
  the window and Slint observes its size.
- rand 0.8 needs `getrandom/js` on `wasm32-unknown-unknown` (in Cargo.toml).
- The checked-in `results/slint-web.json` was produced under
  **headless Chrome 156 + SwiftShader** — the harness ran end-to-end and the
  numbers are plausible, but for canonical web numbers re-run `?bench=1` in a
  normal (GPU-backed) browser; the JSON POSTs itself automatically.
- `artifact_size_mb` for web comes from `sizes.json` generated by
  `build-web.sh` (wasm 3.63 MB, js 0.09 MB, total dist 3.73 MB).

## Verification status

- `cargo test --release`: 2/2 pass (indicator parity incl. post-stream state,
  incremental-live parity)
- `cargo clippy --release --all-targets`: clean
- `cargo build --release`: OK (`slint-desktop.exe` ~10.7 MB)
- wasm build: OK; `http://localhost:5173` serves index/js/wasm (curl-verified)
- `results/slint-desktop.json` + `results/slint-web.json` written, schema-conformant
