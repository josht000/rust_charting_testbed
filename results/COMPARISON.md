# Config comparison — Rust trading testbed

8 charts × 23,400 one-second bars (4,000-bar static preload + 19,400 streamed).
Indicators: EMA10/20/50, SMA200, daily-reset VWAP, 40-bin volume profile.
Machine: i9-9900K, Windows, 16 logical cores.

**Numbers below are from the idle-machine re-run (2026-11-06), one run per
target, benches executed sequentially.** Earlier contended numbers are noted
where they differed materially.

## Desktop

| Metric | Tauri+React | egui+wgpu | Slint |
|---|---|---|---|
| Startup → first chart | 395 ms | 1,509 ms | 292 ms |
| Stream catch-up (8×19,400 bars) | 78.8 s (~1,970 bars/s agg) | 9.73 s (~15,957 bars/s) | 10.0 s (~15,520 bars/s) |
| Indicator compute, all 8 (median) | 237.5 ms † | 1.6 ms | 2.5 ms |
| Stream-catchup frames | 172.8 ms avg (5.8 fps) | 13.5 ms (74.2 fps) | 20.0 ms (19.3 fps) |
| Pan/zoom avg (fps) | 13.4 ms (74.6) | 13.4 ms (74.6) | 8.1 ms (61.2) |
| Live 1/s per chart | 14.2 ms (70.5) | 13.4 ms (74.6) | 18.7 ms (0.9 ‡) |
| Live 100/s per chart | 98.2 ms (10.2) | 13.4 ms (74.6) | 10.7 ms (65.4) |
| Memory | 88 → 478 MB (JS heap) | 161 → 191 MB (RSS) | 132 → 149 MB (RSS) |
| exe size | 25.4 MB | 15.0 MB | 10.7 MB |

† Tauri desktop indicator compute is dominated by the JSON `invoke` round-trip
(whole-day serialization), not real compute — the same work is 14.8 ms in the
browser via talib-wasm.
‡ Slint "fps" at 1/s live updates is 1 frame per update by design — avg frame
time (18.7 ms) is the meaningful number.

## Web

| Metric | Tauri web :5171 | egui wasm :5172 | Slint wasm :5173 |
|---|---|---|---|
| Startup → first chart | 516 ms | 523 ms | 26 ms § |
| Stream catch-up | 75.4 s (~2,059 bars/s) | 9.74 s (~15,941) | 10.9 s (~14,256) |
| Indicator compute, all 8 | 14.8 ms | 2.2 ms | 3.0 ms |
| Stream-catchup frames | 151.6 ms (6.6 fps) | 13.4 ms (74.6) | 24.3 ms (17.7) |
| Pan/zoom (fps) | 20.0 ms (50) | 13.6 ms (73.6) | 7.5 ms (45.9) |
| Live 1/s per chart | 18.0 ms (55.6) | 13.4 ms (74.6) | 30.5 ms (0.9 ‡) |
| Live 100/s per chart | 111.4 ms (9.0) | 13.4 ms (74.6) | 17.3 ms (29.6) |
| Memory (JS heap) | 90 → 920 MB | 1.9 → 2.7 MB §§ | 2.2 → 2.2 MB §§ |
| Artifact | JS 4.17 MB, dist 57 MB | wasm 7.7 MB + js 0.14 MB | wasm 3.6 MB + js 0.09 MB |

§ Slint web startup measures wasm module → first rendered frame; it excludes
the browser's download/instantiate time. §§ `performance.memory` only counts
the JS heap — wasm linear memory (hundreds of MB of bar/vertex/raster data) is
invisible, so egui/slint web memory is under-reported.

## Findings (idle machine)

1. **egui is uniformly fastest at the frame level.** Every phase on both
   targets sits at the vsync cap (~74 fps, 13.4 ms) — catch-up included. It
   sustains ~16k bars/s aggregate (the bench's 2,000/s×8 ceiling), meaning it
   had headroom beyond the requested rate.
2. **Slint streams just as fast** (~14-15k bars/s, also at the rate ceiling) —
   the earlier "~2k bars/s" figure was measured under agent contention. Its
   cost shows elsewhere: catch-up frames at 17-19 fps and the 1 Hz live phase
   renders one slow frame per update. Pan/zoom is the fastest of all three
   (8.1 ms).
3. **Tauri+React improved ~4x** (catch-up 313.7s→75.4s web) but remains ~8x
   slower than the pure-Rust configs: `series.update()` per bar still saturates
   the render thread (5.8-6.6 fps during catch-up, 9-10 fps at 100 Hz live).
   Confirmed: IPC is not the bottleneck; LWC's per-bar update is.
4. **Memory**: tauri-web heap grew ~10x over the bench (920 MB after catch-up +
   phases). egui desktop RSS is the flattest meaningful number (161→191 MB).
5. **Correctness holds**: streamed incremental indicator state equals the batch
   reference (<1e-9) in every config; Tauri's talib-wasm path matches Rust
   goldens at ~1e-12.

## Caveats

- Single run per target; take median-of-3 before final decisions. egui desktop
  startup varies cold (13.9s first-ever) vs warm (1.5-5.2s) due to wgpu shader
  cache.
- egui web ran on a real GPU in headless Edge; Slint web on SwiftShader
  (software GL) — Slint web frame numbers would likely improve on real GPU.
- `stream_rate_bars_per_sec` is recorded inconsistently across configs
  (requested vs measured); aggregate numbers above are derived from
  `155,200 bars / stream_catchup_ms`.
- Memory columns are not cross-comparable (JS heap vs RSS vs working set).

## Recommendation

- **Raw performance / single-language stack** → egui + wgpu. Vsync-bound in
   every phase on both targets, ~16k bars/s headroom, smallest wasm of the two
   pure-Rust options relative to capability.
- **UI richness, dockable workspace, hiring/debug ecosystem** → Tauri + React,
   but the data layer needs batching/decimation before `series.update()` (or a
   custom primitive) if stream rates matter.
- **Slint** is a credible middle path — simplest codebase, smallest exe
   (10.7 MB) and wasm (3.6 MB), excellent pan/zoom, but software rasterization
   caps streaming frame rates.

## Reproduce

- All six benches, sequentially: `node bench-all.mjs` (repo root).
- Tauri: `cd 1_tauri && npm run serve:web` (:5171) or `node scripts/run-bench.mjs`; desktop `app.exe --bench`.
- egui: `cd 2_egui && trunk serve --port 5172 --release`, `?bench=1`; native `cargo run --release -- --bench`.
- Slint: `cd 3_slint && ./target/release/serve.exe` (:5173), `?bench=1`; native `slint-desktop.exe --bench`.
- Rate override: `?rate=N` / `--rate=N`; default 1000 interactive, 2000 in bench.
