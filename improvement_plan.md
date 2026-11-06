# Improvement plan

## Baseline (idle machine, 2026-11-06 — results/*.json)

| Metric | tauri-desktop | tauri-web | egui-desktop | egui-web | slint-desktop | slint-web |
|---|---|---|---|---|---|---|
| Startup → first chart (ms) | 395 | 516 | 1509 | 523 | 292 | 26 |
| Stream catch-up (s) | 78.8 | 75.4 | 9.73 | 9.74 | 10.0 | 10.9 |
| Achieved stream rate (bars/s agg) | ~1970 | ~2059 | ~15957 | ~15941 | ~15520 | ~14256 |
| Indicator compute all-8 (ms) | 237.5 † | 14.8 | 1.6 | 2.2 | 2.5 | 3.0 |
| Stream frames avg (fps) | 172.8 (5.8) | 151.6 (6.6) | 13.5 (74.2) | 13.4 (74.6) | 20.0 (19.3) | 24.3 (17.7) |
| Pan/zoom avg (fps) | 13.4 (74.6) | 20.0 (50) | 13.4 (74.6) | 13.6 (73.6) | 8.1 (61.2) | 7.5 (45.9) |
| Live 1/s (fps) | 14.2 (70.5) | 18.0 (55.6) | 13.4 (74.6) | 13.4 (74.6) | 18.7 (0.9) | 30.5 (0.9) |
| Live 100/s (fps) | 98.2 (10.2) | 111.4 (9.0) | 13.4 (74.6) | 13.4 (74.6) | 10.7 (65.4) | 17.3 (29.6) |
| Memory idle→after (MB) | 88→478 | 90→920 | 161→191 | 1.9→2.7 ‡ | 132→149 | 2.2→2.2 ‡ |
| Artifact (MB) | exe 25.4 | js 4.2/dist 57 | exe 15.0 | wasm 7.7 | exe 10.7 | wasm 3.6 |

† invoke JSON round-trip, not real compute. ‡ JS heap only; wasm linear memory invisible.

## Tauri + Lightweight Charts
The bottleneck isn't rasterization — it's LWC's per-bar series.update() API (internal bookkeeping per call, ~0.3-0.4 ms). Options, cheapest first:

1. **Coalesce display updates** — accumulate streamed bars, flush to `candle.setData()` at a throttled cadence (instead of `update()` per bar). Line series update once per flush. Data state stays per-bar; display updates at a coarser rate, exact reconcile at stream end.
2. **Custom series primitive for candles** — bypass update() entirely (the volume profile already proves the pattern). Renders visible candles/lines directly on the pane canvas each frame. Keeps LWC axes/time scale/crosshair.
3. **wgpu in the web shell** — compile a Rust wgpu renderer to wasm targeting a canvas; React shell stays. Reimplements what egui already is.
4. (Non-starter per spec: uPlot swap.)

## Slint
1. **Parallelize the rasterizer** — rayon across 8 chart buffers; embarrassingly parallel, ~8x raster headroom native-side. (Won't help wasm.)
2. **Incremental rasterization** — repaint only the dirty right-edge region during streaming / pan blits.
3. **Emit geometry instead of pixels** — aggregate bars to pixel columns on CPU, emit Path/Rectangle elements for the GPU UI renderer to rasterize.
4. **wgpu texture import** — render charts via wgpu into a texture wrapped as a Slint image. Most plumbing, least wasm-portable.

## Tauri Iteration log

| Step | Change | Files | Web catch-up | Web stream fps | Notes |
|---|---|---|---|---|---|
| baseline | — | — | 75.4 s | 6.6 | see table above |
| 1a | Coalesced flush: `candle.setData` every 200ms (staggered per chart) + line update 1×/flush, replacing per-bar `update()` | ChartPanel.tsx | **13.8 s** (11,225 bars/s) | **35.8** | ~5.5x; live100Hz 9→17.7fps; heap 920→726MB |
| 1b | Tail-slice `setData` (O(2k) vs O(23k) per flush) + time-based visible range | ChartPanel.tsx | **10.8 s** (14,306 bars/s) | **53.3** | 89% of the 16k/s bench ceiling |
| 2 | `CandlePrimitive` draws candles from raw `bars` each frame; invisible "anchor" LineSeries holds time axis; manual `priceScale.setVisibleRange` autoscale; zero candle series calls during stream & live | ChartPanel.tsx, candles.ts | **10.7 s** (14,470 bars/s) | **55.9** | live100Hz 17.7→**49.1 fps**, pan/zoom 55→**59.8**, heap→**416 MB** |
| 2 (desktop) | same, WebView2 | — | **10.6 s** (14,610 bars/s) | **70.1** | **all phases vsync-locked 74.6 fps** — pan/zoom, live 1/s, live 100Hz; startup 303ms |
| 2-fix | Bug: sparse `lineSeries.update()` per flush created time-scale points that broke the index↔bar alignment the primitive needs — bars past preload rendered at wrong/extrapolated positions (empty charts mid-stream). Fix: no line updates during stream; reconcile at end supplies all points. | ChartPanel.tsx | 10.75 s | 56.1 | mid-stream render verified via screenshot |


## Slint Iteration log

Desktop numbers = `slint-desktop.exe --bench` (action→AfterRendering ms / fps). Render modes are switchable: pixels (default) / `SLINT_TB_RENDER=paths` / `=wgpu` (web: `&render=paths`).

| Step | Change | Files | Desktop stream (ms/fps) | pan/zoom | live100 | Notes |
|---|---|---|---|---|---|---|
| baseline | — | — | 19.95 / 19.3 | 8.13 / 61.2 | 10.67 / 65.4 | catch-up 10.0s |
| 1 | rayon `par_iter` across 8 chart buffers (`SharedPixelBuffer` is Send; `Image` wrapped on UI thread) | app.rs, chart.rs, Cargo.toml | **17.21 / 19.5** | 8.11 / 61.2 | **8.48 / 70.8** | raster ~3ms; texture upload+composite now dominates |
| 2 | Incremental raster: retained frame per chart, integer-px pan blit + dirty strips (left edge sliver, profile region + stale pixels); grid-anchored polylines + dense-bucket alignment for pixel-exactness; `fill_rect_alpha` row fast path | raster.rs, chart.rs, app.rs | 16.83 / 19.6 | **7.57 / 61.0** | 8.00 / 70.9 | byte-exact vs full render (test); streaming/live still full repaint — x-mapping changes every tick by design |
| 3 | Emit Path geometry (`commands` SVG strings → femtovg tessellation) instead of `SharedPixelBuffer` | app.slint (ChartLayer + Path repeater), chart.rs `layers()` | 28.13 / 19.6 | 20.76 / 36.6 | 20.30 / 48.3 | **regression kept, mode off by default** — lyon parse+tessellate of ~10k+ segs/chart/frame costs more than raster+upload. Web worse too (36.0ms stream vs 24.3 px) |
| 4 | wgpu texture import: `unstable-wgpu-30` + `renderer-femtovg-wgpu`, `require_wgpu_30`, device/queue from `GraphicsAPI::WGPU30`, instanced-quad pipeline → `Image::try_from(texture)` | gpu.rs, Cargo.toml, app.rs | 19.58 / 18.0 | 12.13 / 55.6 | 14.13 / 62.6 | **regression** — works and renders correctly, but vertex emit + submit + femtovg-wgpu composite loses to the pixel path. Native only, opt-in |

**Result:** steps 1+2 (parallel + incremental software raster) are the keepers — Slint's `SharedPixelBuffer`→texture path is hard to beat; items 3+4 implemented, measured, and documented as slower alternatives left behind `SLINT_TB_RENDER` switches. Blit correctness is test-guarded (`blit_render_matches_full_render` byte-compares blit vs full render).