# GPU chart rendering notes

## Goal and status

Keep Slint for the window and controls while moving **chart plot
rasterization** to wgpu. Retain the current CPU renderer as the comparison
baseline. Native and browser GPU rendering are prototypes. The experimental
`wgpu-transform` mode performs chart view transforms in the vertex shader;
selection and dense aggregation of bars still run on the CPU.

| Path | Current implementation |
|---|---|
| CPU (default) | [`chart.rs`](src/chart.rs) draws into a `SharedPixelBuffer` using [`raster.rs`](src/raster.rs); Slint displays the image. Native full updates use Rayon; pan can reuse a previous frame. |
| Native hybrid | `SLINT_TB_RENDER=wgpu` uses Slint's wgpu device to draw the chart into a GPU texture, then displays that texture as a Slint image. Geometry and coordinate transforms are still computed on the CPU. |
| GPU transforms | `SLINT_TB_RENDER=wgpu-transform` or `?render=wgpu-transform` uses a separate GPU pipeline that transforms chart data coordinates in WGSL. |
| Paths | `SLINT_TB_RENDER=paths` on native or `?render=paths` on web emits Slint paths; this is separate from the wgpu chart prototype. |
| Web hybrid | `?render=wgpu` uses Slint's wgpu renderer and the shared chart texture pipeline. `&gpu=webgpu` or `&gpu=webgl` selects a specific browser backend; `&gpu=auto` uses Slint/wgpu selection. |
| Web CPU (default) | Slint winit/FemtoVG over WebGL displays the CPU-rendered chart image. |

The `render-cpu`, `render-gpu`, and `render-both` Cargo features currently do
**not** select or gate these renderers. Only the environment variable above
selects the native chart path. Unknown native and web values are rejected; web
accepts `render=pixels`, `render=paths`, `render=wgpu`, and
`render=wgpu-transform` and rejects unknown
renderer or GPU backend values.

## Experimental Slint / femtovg 0.28.0 patch

Slint and slint-build are pinned to 1.18.1 in [`Cargo.toml`](Cargo.toml).
Slint's `i-slint-renderer-femtovg` 1.18.1 depends on femtovg `0.27`, so adding
femtovg 0.28 as a separate direct dependency will **not** make Slint use it.
`cargo tree -i femtovg` shows the resolved dependency.

For the 0.28 comparison, a copy of the `i-slint-renderer-femtovg` 1.18.1
crate was patched outside the repository. This is an experiment, not an
upstream-supported upgrade or a change to the checked-in dependency graph:

1. In a separate copy of the project, copy the exact 1.18.1 renderer crate
   from the Cargo registry to a local directory, for example
   `vendor/i-slint-renderer-femtovg`.
2. In that copy's renderer `Cargo.toml`, change the
   `[dependencies.femtovg]` version from `"0.27"` to `"0.28"`, preserving its
   existing features and default-feature settings.
3. In the renderer's `itemrenderer.rs`, adapt the changed blur API while
   retaining the old isotropic blur value:

   ```diff
   -    sigma: shadow_options.blur_sigma(),
   +    sigma_x: shadow_options.blur_sigma(),
   +    sigma_y: shadow_options.blur_sigma(),
   ```

4. In the copied application's `Cargo.toml`, add a Cargo patch pointing to
   the copied renderer. For example, with the vendor directory immediately
   inside the copied `3_slint` directory:

   ```toml
   [patch.crates-io]
   i-slint-renderer-femtovg = { path = "vendor/i-slint-renderer-femtovg" }
   ```

5. Run `cargo tree -i femtovg` and confirm Slint resolves **0.28.0**, then
   build and test. Keep the original 0.27 lockfile/build for comparison.
   The copied project also needs its relative `../shared` dependencies and
   a `../results` directory. Do not edit the registry cache in place.

This patch was sufficient for the native release build tested here; it does not
establish web compatibility or overall Slint API compatibility. Prefer a
future upstream-supported Slint release over maintaining a private patch.

The recorded comparison used `SLINT_BACKEND=femtovg` (OpenGL) and CPU chart
rendering, not the wgpu chart path. Single-run average action-to-
`AfterRendering` latencies in milliseconds:

| Workload | femtovg 0.27 | femtovg 0.28 |
|---|---:|---:|
| Pan/zoom | 7.56 | 7.66 |
| Live 1 Hz | 20.04 | 20.28 |
| Live 100 Hz | 13.28 | 13.28 |
| Streaming | 20.48 | 20.37 |

These runs do not establish a performance difference. See
[`BENCHMARK_BASELINE.md`](../BENCHMARK_BASELINE.md) and the raw
[0.27](../results/slint-femtovg-0.27-baseline.json) and
[0.28](../results/slint-femtovg-0.28-experimental.json) results.

## Local Slint winit fractional-DPI patch

The project also vendors `i-slint-backend-winit` 1.18.1, with its original
license files, under [`vendor/i-slint-backend-winit`](vendor/i-slint-backend-winit).
The `[patch.crates-io]` entry in `Cargo.toml` selects that copy for all builds.
The only source change is in `winitwindowadapter.rs`'s `resize_event`: on
wasm, it sets the canvas backing dimensions to the physical size obtained
from the same logical-size/scale-factor conversion passed to Slint's renderer.

Without this patch, Slint configured its wgpu surface using the round-tripped
logical size, then overwrote the canvas backing size with the original
physical size. At fractional scale the conversion can truncate a pixel:
an observed 869x632 canvas ended up paired with an 868x632 surface/stencil
buffer, causing WebGPU render validation errors. The patched result is
868x632 for both. CSS still fills the viewport, so a one-pixel difference in
backing resolution can be stretched by the browser.

The previously failing 1.5625-scale resize and subsequent zoom/pan were
verified in the integrated Chromium browser with WebGPU; the old GPU path,
WebGL transform path and CPU path were also checked at that size. Other
browser engines and device-loss recovery remain unverified. Reassess and
remove the patch when an upstream Slint fix is available.
The vendored crate's normalized registry manifest omitted its upstream Slint
dev-dependency; it is restored so the focused regression test can run:

```powershell
cargo test --manifest-path vendor\i-slint-backend-winit\Cargo.toml --features renderer-software --lib fractional_scale_matches_renderer_roundtrip
```

## Native and web prototype: how it draws pixels

1. [`app.rs`](src/app.rs) selects Slint's wgpu 30 renderer and captures its
   device and queue from `GraphicsAPI::WGPU30` during `RenderingSetup`.
2. [`geom.rs`](src/geom.rs) emits ordered chart primitives: grid, candles,
   indicator segments and volume-profile rectangles. Dense candles are
   aggregated per column; sparse candles are emitted per bar. These primitives
   are currently **screen-space**, generated on the CPU.
3. [`gpu.rs`](src/gpu.rs) turns rectangles and one-pixel-wide line segments
   into colored triangle vertices. `Emit::px` converts physical pixel
   coordinates to normalized device coordinates on the CPU; the vertex
   shader passes these coordinates through. The vertices are uploaded to a
   per-chart buffer, which grows when needed.
4. A single render pass clears an `Rgba8Unorm` chart texture and draws the
   batched triangles. Wgpu rasterizes fragments to texture pixels. The texture
   is imported using `slint::Image::try_from`; Slint composites it with the
   rest of the UI. No CPU chart pixel buffer is uploaded on this path.
5. Profile rectangles carry alpha `128/255`; the pipeline's
   `ALPHA_BLENDING` uses source-over blending. The resulting color over an
   opaque chart is `src * alpha + dst * (1 - alpha)`. The ordinary CPU path
   already blends the profile; the separate primitive interpreter in
   `raster.rs` also honors alpha.

The CPU renderer still calls its original `Chart::draw`, not the new
primitive interpreter. There are also older direct geometry emitters in
`gpu.rs` and `Chart::layers`. Do not assume these implementations or their
rounding and clipping are pixel-identical. The GPU readback test covers a
small blended rectangle over a candle-colored pixel, not full-chart parity.

## Experimental GPU view transforms

[`gpu_transform.rs`](src/gpu_transform.rs) keeps price and index coordinates
relative to per-view origins chosen in f64 before conversion to f32. Its
per-chart uniform carries the rebased view and physical viewport; WGSL maps
candles and indicator endpoints to physical pixels and then to clip space.
Candle widths, snapping, line normals, and minimum body/profile heights are
computed in pixel space. Grid lines and dense column X coordinates are already
in pixel space; profile X is right-aligned in pixel space, while profile Y is
in price space. Primitive order and 128/255 profile alpha are preserved.

The CPU still selects visible bars, computes view bounds, builds per-column
dense aggregates and chooses indicator stride. Geometry and vertices are
rebuilt and uploaded for **every** chart update, including pan/zoom; this
mode does not yet support uniform-only pan or geometry caching. Both GPU
modes are explicitly selectable for A/B comparison and use the same Slint
device and texture-import path.

Native GPU readback tests cover blending, buffer growth and transformed
fixtures. Dense, sparse, pan, zoom and streaming views are compared with the
old GPU path using a tolerance of at most 5% of pixels differing by more than
2 color levels at 64x32. The same fixtures allow up to 35% differing pixels
against the CPU baseline because its software line rasterization and rounding
differ; this is a coarse regression guard, not a full CPU/GPU parity proof.

## Remaining work

1. **Establish reproducible baselines.** Record CPU and hybrid runs with
   renderer, adapter, dimensions, DPI, versions, mode, workload and sample
   counts. Avoid overwriting results. `AfterRendering` measures action-to-Slint
   render callback latency, not GPU completion or display presentation;
   startup currently records chart preparation rather than presentation.
2. **Make geometry a shared contract.** Extract the CPU draw semantics,
   including grid snapping, dense and sparse candles, lines, clipping, paint
   order and profile alpha. Preserve the CPU path and its partial-pan repaint
   optimization; the primitive interpreter is not yet a replacement. Add
   deterministic tests for empty charts, dense/sparse ranges, pan, zoom,
   resize, live updates and blending.
3. **Optimize transforms safely.** Add geometry caching only with explicit
   invalidation for data, visible selection, aggregation/stride, viewport,
   rebasing and device changes. Verify eligibility before making a pan
   uniform-only. Expand precision, clipping and cross-backend parity tests.
4. **Harden Slint integration.** Initial charts are redrawn after device
   setup; notifier registration is checked, and GPU state is discarded on
   teardown. Verify device-loss recovery, resize and texture replacement
   across supported browsers and drivers. Never silently drop frames or
   fall back to a different renderer in a benchmark.
5. **Add real renderer controls and metadata.** Wire CLI and/or Cargo
   controls to actual CPU/hybrid behavior; validate unsupported options and
   record the chosen mode and backend in each result. Keep the default CPU
   baseline available.
6. **Harden web GPU rendering.** The current implementation shares Slint's
   wgpu device, imported textures, geometry and WGSL with native. Test more
   browsers and hardware, especially resize, device loss and unsupported
   adapters. Explicit `gpu=webgpu` and `gpu=webgl` require that backend;
   `gpu=auto` relies on Slint/wgpu selection and is not a guaranteed recovery
   from every adapter or device failure. There is no silent CPU fallback.
7. **Validate and measure both platforms.** Compare representative CPU/GPU
   outputs (using documented tolerances, not assumed pixel equality), including
   overlapping translucent profiles and indicators. Exercise streaming, live,
   pan/zoom, empty data, resize and high DPI. Run repeated native and
   GPU-backed browser benchmarks; report variance and explain any renderer or
   timing differences before claiming a speedup.

## Running and testing the current native paths

From `3_slint` in PowerShell:

```powershell
cargo build --release --bin slint-desktop

# CPU baseline; --bench writes ../results/slint-desktop.json.
$env:SLINT_TB_RENDER = "pixels"
.\target\release\slint-desktop.exe --bench

# Native screen-space hybrid prototype.
$env:SLINT_TB_RENDER = "wgpu"
.\target\release\slint-desktop.exe --bench

# Experimental GPU transforms, separate from the screen-space GPU path.
$env:SLINT_TB_RENDER = "wgpu-transform"
$env:SLINT_TB_BENCH_OUTPUT = "C:\temp\transform-run-1.json"
.\target\release\slint-desktop.exe --bench
Remove-Item Env:\SLINT_TB_BENCH_OUTPUT
Remove-Item Env:\SLINT_TB_RENDER

cargo test --lib
# Requires a working native GPU adapter:
cargo test --lib gpu_blends_profile_and_grows_buffer -- --ignored
cargo test --lib transformed_primitives_render_and_blend_on_gpu -- --ignored
cargo test --test indicators
```

Native defaults are `slint-desktop.json`, `slint-desktop-wgpu.json`, and
`slint-desktop-wgpu-transform.json`; repeated runs of a mode overwrite its
file. Set `SLINT_TB_BENCH_OUTPUT` to an existing-directory path to preserve
each run. The benchmark JSON records mode, GPU backend and adapter, per-chart
physical sizes, scale factor and sample counts. The earlier GPU benchmark in
[`PHASE2_GPU_RESULTS.md`](../PHASE2_GPU_RESULTS.md) is not valid evidence of a
speedup: the old renderer could silently drop oversized frames and visual
parity was not verified.

## Running and testing web

Build with `.\build-web.ps1` or `./build-web.sh`, then run `.\serve.ps1`
or `./serve.sh` from this directory. The bundle includes both Slint's
WebGL renderer for the CPU baseline and its wgpu renderer:

- `http://localhost:5173/?render=pixels` — CPU chart baseline.
- `http://localhost:5173/?render=wgpu&gpu=webgpu` — force WebGPU.
- `http://localhost:5173/?render=wgpu&gpu=webgl` — force wgpu's WebGL backend.
- `http://localhost:5173/?render=wgpu&gpu=auto` — wgpu backend selection.
- `http://localhost:5173/?render=wgpu-transform&gpu=webgpu` — GPU view transforms on WebGPU.
- `http://localhost:5173/?render=wgpu-transform&gpu=webgl` — GPU view transforms on WebGL.

Add `&bench=1` to run the scripted benchmark. The server saves CPU results as
`slint-web.json`, and wgpu results as `slint-web-wgpu-<backend>.json`. Repeated
runs of the same mode overwrite its file. Transform results use
`slint-web-wgpu-transform-<backend>.json`. `wgpu_backend` records the actual
adapter backend, not merely the requested setting. Browser support and secure
context requirements apply; when the requested GPU backend cannot initialize,
check the browser console instead of treating a missing benchmark as success.

The local Slint winit patch above fixes the observed fractional-scale resize
failure in the tested Chromium browser. Broader browser, driver and device-loss
validation is still needed.

## Phase 3 exploratory benchmarks

Two native runs per mode used the same 520x554 physical chart size and 1.25
scale factor, with `SLINT_BACKEND=winit-femtovg-wgpu`; the GPU chart runs
reported Vulkan on an NVIDIA GeForce RTX 3060. These are ranges of average
action-to-`AfterRendering` latency in ms, not GPU completion times. The runs
predate the local winit patch, whose production change is wasm-only:

| Mode | Pan/zoom | Live 1 Hz | Live 100 Hz | Streaming |
|---|---:|---:|---:|---:|
| CPU | 12.02–12.20 | 23.55–23.69 | 13.28–13.29 | 22.82–23.60 |
| Screen-space wgpu | 12.30–12.36 | 19.34–19.57 | 14.99–15.24 | 19.56–19.67 |
| GPU transforms | 10.00–10.09 | 41.95–43.13 | 38.96–39.65 | 32.57–33.18 |

Two browser runs per mode used the integrated Chromium browser, 433x585
physical chart sizes and 1.25 scale. The CPU baseline uses Slint's WebGL
presentation renderer, while GPU modes use Slint's wgpu renderer. These runs
also predate the fractional-DPI patch:

| Mode | Pan/zoom | Live 1 Hz | Live 100 Hz | Streaming |
|---|---:|---:|---:|---:|
| CPU / Slint WebGL | 4.50–4.83 | 27.93–30.24 | 10.79–10.88 | 26.50–26.77 |
| Screen-space wgpu / WebGL | 5.26–5.45 | 27.17–27.37 | 7.67–7.69 | 25.02–25.43 |
| GPU transforms / WebGL | 5.59–5.95 | 24.83–26.82 | 14.82–15.05 | 27.34–27.49 |
| Screen-space wgpu / WebGPU | 5.50–6.09 | 28.70–29.36 | 6.88–7.06 | 24.67–25.04 |
| GPU transforms / WebGPU | 6.75–6.85 | 25.39–25.49 | 12.72–12.84 | 25.43–26.10 |

The browser benchmark POSTs were intercepted to avoid overwriting existing
results; these tables retain their summaries, not the full browser JSON.
The WebGL adapter identified NVIDIA/ANGLE; the browser did not expose a WebGPU
adapter name. These are small, uncontrolled samples and the CPU baseline uses
a different browser presentation renderer. They are not evidence of a
general speedup. In particular, the transform prototype is slower in these
live-update samples; geometry is still rebuilt and uploaded for every update.

One exploratory run on the VS Code integrated Electron/Chromium browser
(2026-11-12) exercised all three paths. Winit's canvas sizing overrode the
requested test viewport during these runs; those measurements predate the
viewport sizing fix. Average
action-to-`AfterRendering` times, in ms:

| Workload | CPU chart / Slint WebGL | wgpu WebGL | wgpu WebGPU |
|---|---:|---:|---:|
| Pan/zoom | 4.83 | 5.49 | 4.32 |
| Live 1 Hz | 44.67 | 28.39 | 27.02 |
| Live 100 Hz | 44.30 | 11.93 | 10.61 |
| Streaming | 40.60 | 25.03 | 26.22 |

These are single runs with different Slint presentation backends, not an
isolated chart-rasterization comparison. CPU indicator timings also varied
substantially between runs; repeat on known hardware before drawing
performance conclusions. Pan and zoom visibly changed both GPU plots; full
chart pixel parity, device-loss recovery and browser portability remain
unverified.
