# GPU Migration Project: Status

The original CPU chart renderer remains the default baseline. Two optional
wgpu paths draw chart plots into textures composited by Slint:

- `wgpu` generates screen-space chart geometry and clip coordinates on the
  CPU before drawing the triangles on the GPU.
- `wgpu-transform` generates rebased data-space candle and indicator vertices
  on the CPU and maps them to pixels and clip space in WGSL. Profile X and
  grid coordinates remain in pixel space, while profile Y is transformed from
  price space. Both paths use the same native and browser device integration.

## Phase status

| Phase | Status |
|---|---|
| Baseline and geometry | CPU baseline retained; geometry extraction exists for the GPU paths. The original CPU renderer still uses `Chart::draw`, not the primitive interpreter. |
| GPU chart rasterization | Native and browser prototypes are selectable. Volume profiles use source-over alpha blending. |
| GPU view transforms | Experimental `wgpu-transform` mode is implemented. Visible selection, dense aggregation and geometry uploads remain CPU-side on each update; uniform-only pan and geometry caching are not implemented. |
| Web integration | WebGPU and WebGL modes are implemented. A local Slint winit patch fixes the observed fractional-DPI canvas/surface size mismatch; broader browser, driver and device-loss validation remains necessary. |

This report uses **Phase 3** for GPU view transforms. The earlier
[strategy](GPU_MIGRATION_STRATEGY.md) used the same label for benchmarking;
its estimates and proposed phase names are historical.

## Evidence and outstanding work

Native GPU readback tests cover blending, buffer growth and transformed
fixtures. Dense, sparse, pan, zoom and streaming views are compared with the
previous GPU path under a documented tolerance. These checks do not prove
pixel-perfect equivalence with the CPU path, nor do callback timings establish
GPU presentation latency or a speedup.

Next work includes broader precision and CPU/GPU parity tests, cross-device
and browser coverage, robust device-loss/resize handling, and geometry
caching with explicit invalidation. Do not rely on the earlier Phase 2
performance claims: the old benchmark could silently drop oversized frames.

For implementation details, test commands, renderer options, benchmark
caveats and the isolated femtovg 0.28.0 experiment, see
[GPU.md](3_slint/GPU.md).
