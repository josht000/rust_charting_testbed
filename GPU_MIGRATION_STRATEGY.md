# GPU Chart Rasterization Migration Strategy

> Correction: This is an early proposal, not a record of completed work. Web GPU
> integration is a core requirement, not lower priority. The performance targets,
> compatibility claims, time estimates and proposed web APIs below are unverified;
> GPU transforms are part of the requested prototype, not an optional deferral.
> This historical strategy calls benchmarking “Phase 3”; the later project
> status calls GPU transforms “Phase 3”. Use the latter definition and
> [GPU.md](3_slint/GPU.md) for current implementation status.

## Current State
- **3_Slint**: 8 charts, each 23,400 bars, rendered via SharedPixelBuffer (CPU rasterization)
- **Baseline**: ~13.2ms avg frame time (74.5 fps at 100Hz), ~23.6ms at 1Hz streaming
- **Architecture**: 
  - `chart.rs`: Bar data + indicators, `render_frame()` into SharedPixelBuffer
  - `raster.rs`: Software rasterizer (pixel buffer operations)
  - `gpu.rs`: Existing GPU path (wgpu triangle mesh, but secondary)
  - `app.rs`: Render mode selection (Pixels/Paths/Wgpu via env var)
  - Slint femtovg-wgpu renderer handles UI composition

## Goal
Replace chart rasterization bottleneck with GPU while keeping baseline CPU path as reference.

---

## Phase 1: Architecture Separation (No GPU changes yet)

### Step 1.1: Extract Chart Geometry
**File**: `src/geom.rs` (new)
- Define `Prim` enum: `Rect`, `Seg`, `HLine`, `VLine`
- Create `GeomCtx` struct: view parameters (w, h, lo, hi, start, end, i0, i1)
- Implement coordinate transform helpers: `py(price)`, `px_of(index)`
- Function `emit_primitives(chart) -> Vec<Prim>`
  - Same aggregation logic as current CPU rasterizer
  - Returns command list, not pixels
- **Goal**: Decouple geometry generation from rendering target

### Step 1.2: Refactor CPU Rasterizer to Use Geometry
**File**: `src/raster.rs` (modify)
- Keep existing `Canvas` struct and pixel ops intact
- Add `pub fn render_primitives(canvas, prims: &[Prim])`
  - Interprets `Prim` commands into canvas calls
- Update `chart.rs::render_frame()` to call `emit_primitives()` then `render_primitives()`
- **Validation**: Frame times should be identical (within 1-2%)

### Step 1.3: Add Render Mode Feature Flags
**File**: `Cargo.toml` (modify)
```toml
[features]
default = ["slint/default", "render-cpu"]
render-cpu = []      # CPU SharedPixelBuffer (baseline)
render-gpu = []      # GPU mesh rendering
render-both = []     # Hybrid: GPU plot + CPU UI via texture import
```
- Add `src/lib.rs` top-level feature gating
- Update `app.rs::render_mode()` to respect `--features`
- Support both compile-time and runtime selection

---

## Phase 2: GPU Rendering Path (Prototype)

### Step 2.1: Enhanced GPU Module
**File**: `src/gpu.rs` (refactor existing)
- **Keep** existing: `Vert` struct, shader, NDC transform
- **Add** new geometry interpreter:
  ```rust
  impl Gpu {
      pub fn emit_from_primitives(verts: &mut Vec<Vert>, prims: &[Prim], 
                                   w: f32, h: f32) 
      // Convert Prim -> triangle mesh in NDC space
      // Reuses current rect() and seg() logic from Emit
  }
  ```
- **Add** batched vertex buffer management:
  - Resize buffer if `prims.len() * vertices_per_prim` exceeds capacity
  - Log oversized frames (don't drop silently)

### Step 2.2: GPU Transform Pipeline (Optional for Phase 2)
**File**: `src/gpu.rs` (future enhancement)
- Currently: pan/zoom transforms happen on CPU before geometry emit
- Future: Move transforms to GPU uniform buffers
  - Add `ViewTransform` struct: `scale_x`, `scale_y`, `tx`, `ty`
  - Pass as uniform to vertex shader
  - Emit geometry in *data space* (not screen space)
- **Benefit**: Zero CPU cost for pan/zoom (just update uniforms + redraw)
- **Status**: **Defer to Phase 3** (optional optimization)

### Step 2.3: Integrate GPU Path into App
**File**: `src/app.rs` (modify)
- Update `render_chart_gpu()` to use geometry approach:
  ```rust
  let prims = geom::emit_primitives(&chart);
  g.render_from_primitives(&mut slots[idx], &prims);
  ```
- Keep device/queue capture in rendering notifier
- **No changes** to pan/zoom/stream logic—geometry adapts automatically

---

## Phase 3: Hybrid Mode & Benchmarking

### Step 3.1: Compile-Time Control
**File**: `build.rs` (modify)
```
SLINT_TB_RENDER=cpu   -> CPU baseline (SharedPixelBuffer)
SLINT_TB_RENDER=gpu   -> GPU mesh
SLINT_TB_RENDER=both  -> Hybrid (GPU plot, Slint UI)
```

### Step 3.2: Native Benchmarking
**File**: `src/bench.rs` (extend)
- **Baseline**: Current CPU path frame times (already logged)
- **GPU path**: Enable `--bench` with `SLINT_TB_RENDER=gpu`
  - Compare: pan/zoom, live1, live100, stream_catchup
  - Measure GPU fence time separately (queue flush overhead)
- **Hybrid**: Enable both, measure composition overhead
- Output to `results/slint-gpu.json` alongside `slint-desktop.json`

### Step 3.3: Results Analysis
- Expected gains:
  - GPU pan/zoom: 20-40% improvement (transform parallelism)
  - GPU streaming: 10-20% improvement (batch vertex updates)
  - Hybrid overhead: ~2-5ms (texture import + composition)
- Document in `RESULTS.md`: frame time breakdown, memory, CPU/GPU utilization

---

## Phase 4: Web Integration (WebGPU/WebGL)

### Step 4.1: WebGPU Path (Wasm + wgpu-core)
**File**: `src/gpu_web.rs` (new)
- Use `wgpu_web::WebGpuContext` (wgpu-wasm integration)
- Adapt GPU module to use web device/queue
- Same `Prim` interpretation, WebGPU API calls

### Step 4.2: Fallback to Canvas2D (Optional)
**File**: `src/raster_web.rs` (new)
- If WebGPU unavailable, use web-canvas drawing API
- CPU rasterizer but runs in JS context
- Slower but ensures broad compatibility

### Step 4.3: Web Build & Benchmarking
- Build with `wasm-pack`
- Measure WebGPU vs Canvas2D frame times
- Compare to desktop GPU/CPU
- Document in web benchmark results

---

## Benchmarking Strategy (CRITICAL FIRST STEP)

### Baseline Measurements (As-Is)
**Run 1: Current CPU path (femtovg in Slint 1.18.1)**
```bash
cd 3_slint
cargo build --release
./target/release/slint-desktop --bench
# Output: results/slint-desktop.json (BASELINE)
```
- Capture native frame times: pan/zoom, live1, live100, stream_catchup
- Baseline for all subsequent comparisons

### femtovg 0.28.0 Upgrade
**Slint 1.18.1 includes femtovg 0.27.x; check for 0.28.0 availability in slint 1.19+**

If slint 1.19.0 or later is available with femtovg 0.28.0:
```bash
# Edit Cargo.toml: slint = "=1.19.0" (if available)
cargo update
cargo build --release
./target/release/slint-desktop --bench
# Output: results/slint-femtovg-0.28.json
```

Document performance delta vs baseline.

---

## Implementation Order (Revised)

**PHASE 0: Benchmarking (FIRST - 1 hour)**
1. Run baseline benchmark (current stack)
2. Upgrade femtovg (if available in latest slint)
3. Compare frame times
4. Document results in `BENCHMARK_BASELINE.md`

**Phase 1.1–1.3**: Geometry abstraction + feature flags (2–3 hours)
   - Low risk, pure refactoring
   - Validates baseline stability post-femtovg
   
**Phase 2.1–2.3**: GPU path enhancement (2–3 hours)
   - Extends existing `gpu.rs` (already working)
   - No changes to pan/zoom/stream logic
   
**Phase 3.1–3.3**: Hybrid mode & native benchmarking (1–2 hours)
   - CLI/env var selection
   - Results collection and comparison
   
**Phase 4**: Web integration (3–4 hours, EQUAL PRIORITY WITH PHASE 3)
   - WebGPU/wasm implementation for parity with desktop GPU
   - Canvas2D fallback for broad compatibility
   - Web benchmarking suite

---

## Success Criteria

✓ GPU frame times **≥15% better** than CPU baseline on native
✓ WebGPU frame times **≥10% better** than CPU on web (if implemented)
✓ Compile-time and runtime selection (env var + feature flags)
✓ Backward compatible: `render-cpu` feature still works, identical frame times
✓ Benchmarking results in `results/` directory comparing all paths
✓ No visual differences between CPU and GPU output
✓ Streaming/pan/zoom/live-update paths unchanged (geometry adapts)

---

## Files Touched

**Phase 1:**
- `src/geom.rs` ← NEW
- `src/lib.rs` ← ADD module declarations + feature gating
- `Cargo.toml` ← ADD features
- `src/raster.rs` ← ADD `render_primitives()`
- `src/chart.rs` ← MODIFY `render_frame()` to use geometry
- `src/app.rs` ← MODIFY `render_mode()` for feature awareness

**Phase 2:**
- `src/gpu.rs` ← REFACTOR `emit_from_primitives()`
- `src/app.rs` ← MODIFY `render_chart_gpu()` call

**Phase 3:**
- `Cargo.toml` ← MODIFY feature defaults
- `src/bench.rs` ← EXTEND result logging
- `build.rs` ← OPTIONAL env var detection

**Phase 4:**
- `src/gpu_web.rs` ← NEW
- `src/raster_web.rs` ← NEW (optional fallback)
- `build.rs` ← ADD wasm target handling
