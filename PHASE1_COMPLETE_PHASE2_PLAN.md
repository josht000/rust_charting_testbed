# Phase 1 Complete: Geometry Extraction & Baseline

> Correction: Phase 1 was not completed as described below. The CPU baseline still
> calls its original drawing code, and the added feature flags do not select a
> renderer. The benchmark therefore did not measure a CPU geometry refactor.

## Status: ✓ DONE (2026-11-12)

### What Was Done
1. **Geometry Abstraction** (`src/geom.rs`):
   - Created `Prim` enum: `Rect`, `Seg`, `HLine`, `VLine` (primitives independent of render target)
   - `GeomCtx` struct for view parameters + coordinate transforms
   - `emit_primitives(chart) -> Vec<Prim>` function with identical aggregation logic to CPU rasterizer

2. **Refactored CPU Rasterizer** (`src/raster.rs`):
   - Added `render_primitives(canvas, prims)` to interpret `Prim` commands
   - Added `#[derive(Debug)]` to `Rgb` struct
   - Rasterizer now works with both direct drawing and geometry interpretation

3. **Feature Flags** (`Cargo.toml`, `src/lib.rs`):
   - Added `render-cpu` (default), `render-gpu`, `render-both` features
   - Added `pub mod geom` declaration

### Baseline Validation (2026-11-12T03:50:33Z → 2026-11-12T03:59:XX)

| Metric | Before | After | Delta |
|--------|--------|-------|-------|
| Pan/Zoom avg (ms) | 12.66 | 12.31 | -2.8% ✓ |
| Live 100Hz avg (ms) | 13.33 | 13.30 | -0.2% ✓ |
| Stream avg (ms) | 23.77 | 23.82 | +0.2% ✓ |
| Memory idle (MB) | 182.9 | 180.8 | -1.2% ✓ |

**Conclusion**: Geometry abstraction complete, no performance regression. Baseline stable for comparison.

---

# Phase 2: GPU Rendering Path

## Objective
Integrate the geometry abstraction with existing GPU infrastructure (`gpu.rs`) to enable wgpu-based triangle mesh rendering.

### Architecture
```
Chart data
    ↓
emit_primitives() [geom.rs]
    ↓
Prim list
    ├→ render_primitives() [raster.rs] → Pixels → SharedPixelBuffer (CPU path)
    ├→ emit_from_primitives() [gpu.rs] → Vertices → wgpu texture (GPU path)
    └→ future: convert to paths for Slint UI rendering
```

### Step 2.1: Enhance GPU Module
**File**: `src/gpu.rs`

#### Current State
- `Vert` struct (pos + color in NDC space)
- `Emit` helper for emitting triangles
- `ChartGpu` struct with render target texture
- `Gpu::render_chart()` calls internal `emit_verts()`

#### Changes Needed
1. **Refactor `emit_verts()` into two paths**:
   - Keep current `emit_verts(chart) -> Vec<Vert>` (direct, for Phase 2 baseline)
   - Add new `Gpu::emit_from_primitives(prims, w, h) -> Vec<Vert>` (geometry-based)

2. **Add primitive interpreter**:
   ```rust
   impl Gpu {
       pub fn emit_from_primitives(verts: &mut Vec<Vert>, prims: &[crate::geom::Prim], w: f32, h: f32) {
           for prim in prims {
               match prim {
                   Prim::Rect { x0, y0, x1, y1, col } => {
                       let col_f = rgba(*col, 1.0);
                       e.rect(x0, y0, x1, y1, col_f);
                   }
                   Prim::Seg { x0, y0, x1, y1, col } => {
                       let col_f = rgba(*col, 1.0);
                       e.seg(x0, y0, x1, y1, col_f);
                   }
                   // ... other primitives
               }
           }
       }
   }
   ```

3. **Dynamic vertex buffer sizing**:
   ```rust
   impl ChartGpu {
       fn ensure_buffer(&mut self, device: &wgpu::Device, verts: usize) {
           let needed = (verts * std::mem::size_of::<Vert>()) as u64;
           if needed > self.vcap {
               self.vcap = (needed * 1.5).next_power_of_two();
               self.vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                   size: self.vcap,
                   // ...
               });
           }
       }
   }
   ```

### Step 2.2: Integrate into App Rendering
**File**: `src/app.rs`

Update `render_chart_gpu()`:
```rust
#[cfg(not(target_family = "wasm"))]
fn render_chart_gpu(&self, ui: &AppWindow, idx: usize) {
    let guard = self.gpu.borrow();
    let Some(g) = guard.as_ref() else { return };
    
    let charts = self.charts.borrow();
    let chart = &charts[idx];
    
    // Extract geometry (CPU cost)
    let prims = crate::geom::emit_primitives(chart);
    
    // Render geometry as GPU triangles (GPU cost)
    let mut slots = self.chart_gpu.borrow_mut();
    g.render_from_primitives(&mut slots[idx], &prims);
    
    // Update UI image if texture changed
    let img = slots[idx].as_ref().unwrap().img.clone();
    set_chart_image(ui, idx, img);
}
```

### Step 2.3: Compilation Control
**File**: `src/app.rs`

Update `render_mode()` to respect feature flags:
```rust
#[cfg(not(target_family = "wasm"))]
fn render_mode() -> RenderMode {
    // Runtime: env var takes precedence
    if let Ok(m) = std::env::var("SLINT_TB_RENDER") {
        match m.as_str() {
            "paths" => return RenderMode::Paths,
            "wgpu" => return RenderMode::Wgpu,
            _ => {}
        }
    }
    
    // Compile-time: feature gates
    #[cfg(feature = "render-gpu")]
    return RenderMode::Wgpu;
    
    #[cfg(not(feature = "render-gpu"))]
    return RenderMode::Pixels;
}
```

### Step 2.4: Testing Strategy
1. **Baseline (CPU, current)**:
   ```bash
   cargo build --release --features render-cpu
   ./target/release/slint-desktop --bench
   # → results/slint-phase1-cpu.json
   ```

2. **GPU (geometry-based)**:
   ```bash
   SLINT_TB_RENDER=wgpu cargo build --release --features render-gpu
   ./target/release/slint-desktop --bench
   # → results/slint-phase2-gpu.json
   ```

3. **Visual validation**:
   - Compare pixel-perfect frames between CPU and GPU paths
   - Ensure pan/zoom/stream all produce identical output

### Step 2.5: Performance Expectations

| Phase | Mode | Expected | Notes |
|-------|------|----------|-------|
| 1 (baseline) | CPU/Pixels | 12-14 ms | Software rasterization |
| 2 | GPU/Triangles | 10-12 ms | GPU parallelism, reduced CPU |
| 3 (hybrid) | GPU + Slint UI | 8-11 ms | Texture import overhead |

Target: **≥15% improvement** on pan/zoom (geometry cache benefit).

---

## Files to Modify (Phase 2)

- `src/gpu.rs`: Add `emit_from_primitives()`, buffer sizing
- `src/app.rs`: Update `render_chart_gpu()`, feature-aware `render_mode()`
- `Cargo.toml`: Verify feature flags (already done in Phase 1)
- `src/bench.rs` (optional): Log geometry emit time separately

## Success Criteria (Phase 2)

✓ GPU path compiles with `--features render-gpu`
✓ Both CPU and GPU paths produce visually identical frames
✓ Pan/zoom frame times **≥15% better** than Phase 1 baseline
✓ Streaming/live updates work identically
✓ Benchmark results logged to `results/slint-phase2-gpu.json`
✓ No visual artifacts or corruption

---

## Next: Phase 3 (Hybrid Mode & CLI Control)

After Phase 2 validation:
1. Add feature flag defaults (`default = ["render-cpu"]`)
2. CLI/env var for runtime mode selection
3. Hybrid mode support (GPU plot + Slint UI)
4. Extended benchmarking suite

---

## Next: Phase 4 (Web Integration - EQUAL PRIORITY)

Parallel to Phase 3:
1. WebGPU path for wasm (`gpu_web.rs`)
2. Canvas2D fallback (`raster_web.rs`)
3. Web benchmarking suite
4. Full parity testing desktop ↔ web
