# Config 1 — Tauri + React + TypeScript + Rust

8 candlestick charts (23,400 one-second bars each, one trading day) in a
Dockview workspace. Each chart shows EMA10 (white), EMA20 (orange), EMA50
(cyan), SMA200 (yellow), daily-resetting VWAP (purple), and a 40-bin
horizontal volume profile (blue, 50% opacity, right edge, independent scale)
drawn as a Lightweight-Charts `ISeriesPrimitive`.

## Streaming data model

Per chart: the first **4000 bars** (`datagen::INITIAL_BARS`) load statically
(preload + batch indicator computation), then the remaining **19,400 bars**
stream in ~50ms batches. Stream rate is configurable via `?rate=N` bars/sec
(default 1000; `?bench=1` / `--bench` defaults to 2000). Indicators update
incrementally per streamed bar (`src/stream.ts` — EMA state, rolling SMA
window, VWAP accumulators); the volume profile recomputes once per batch over
the trailing 2000 bars. During catch-up the view follows a ~500-bar tail to
keep repaints cheap; `fitContent()` + an exact `setData` reconcile of the line
series run when the stream completes.

- Web: preload `/data/chart{i}.init.json`, then `chart{i}.rest.json` replayed
  in timed batches.
- Tauri: `get_initial_bars` command, then `start_stream` spawns a thread
  emitting `bar-batch-{chart}` events, ending with `stream-done-{chart}`.

Stack: Vite + React 19 + TypeScript, `lightweight-charts` 5.2.1,
`dockview-react` 8.8.3, TA-Lib via `talib-wasm` (official C lib compiled to
WASM — used for EMA/SMA on the web path; see NOTES.md), Rust backend via
Tauri v2 commands using `../shared/datagen` + `../shared/indicators`.

## Setup

```bash
npm install                 # already done; node 22.12+ recommended (22.11 works, warns)
npm run gen:data            # regenerate public/data/chart{0..7}.json from shared/datagen
npm run gen:golden          # regenerate tests/golden/ (Rust indicator reference output)
```

## Run — web (standalone, no Tauri)

```bash
npm run build
npm run serve:web           # serves dist/ at http://localhost:5171
# optional: http://localhost:5171/?rate=500   (slower stream)
```

In the web target `window.__TAURI__` is absent, so each panel fetches
`/data/chart{i}.init.json` (preload) + `/data/chart{i}.rest.json` (streamed
replay) and computes indicators in JS (TA-Lib WASM for EMA/SMA; pure JS for
VWAP + volume profile; incremental updates during the stream).

## Run — desktop (Tauri)

```bash
npm run dev                 # vite dev server on :5173, then in another shell:
npx tauri dev               # dev-mode window
# or the production build:
npx tauri build             # produces src-tauri/target/release/app.exe (+ msi/nsis)
```

In the Tauri shell the same frontend calls `invoke('get_bars'|'compute_indicators')`
— data generation + all 6 indicators run in Rust via the shared crates.
Launching with `--bench` runs the scripted benchmark.

## Benchmark

Web (headless Chrome via puppeteer-core):

```bash
npm run serve:web           # leave running
npm run bench:web           # opens ?bench=1 headless; writes ../results/tauri-web.json
```

Any browser at `http://localhost:5171/?bench=1` runs the same bench; results
are POSTed to the server's `/__save` endpoint (falls back to a file
download). Desktop: `app.exe --bench` writes `../results/tauri-desktop.json`
via the `save_results` command (which also injects the exe size).

## Tests

```bash
npm test                    # vitest: JS + talib-wasm indicators vs Rust golden (tol 1e-6)
                            # + IncrementalIndicators == batch computeAllJs (tol 1e-9)
cd src-tauri && cargo test  # Rust unit tests for the commands
npm run build               # tsc -b + vite build (also type-checks tests)
```

## Layout

```
src/            React app (App dockview layout, ChartPanel, volprofile primitive,
                indicators (pure JS + TA-Lib WASM), stream.ts incremental
                indicators, data layer, bench driver)
src-tauri/      Tauri v2 app: get_bars / get_initial_bars / start_stream /
                compute_indicators / save_results / bench_requested commands;
                src/bin/gen_golden.rs makes golden JSON
public/data/    pregenerated chart{i}.json + .init.json (4000 bars) +
                .rest.json (19400 bars) — shared/datagen, gen time excluded
tests/golden/   Rust reference indicator output for the parity test
scripts/        serve-web.mjs (:5171 static + /__save + /__stats),
                run-bench.mjs (headless Chrome), screenshot.mjs
```
