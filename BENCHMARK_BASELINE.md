# GPU Migration - Baseline Benchmark Results

> Correction: The processor model below was not verified. Slint 1.18.1 resolves
> femtovg 0.27.0; the earlier report did not test 0.28.0. The proposed feature
> flags and benchmark output paths below are not wired up.

## Controlled femtovg comparison (2026-11-12 UTC)

The CPU chart baseline was run sequentially with `SLINT_TB_RENDER=pixels` and
`SLINT_BACKEND=femtovg` (winit/femtovg, OpenGL). Both runs used the same
isolated application source and release settings. Slint 1.18.1 pins femtovg
0.27, so the 0.28 run used an isolated copy of its renderer with the dependency
changed to 0.28 and its Gaussian blur `sigma` mapped to both `sigma_x` and
`sigma_y`. This is an experimental, unsupported Slint patch, not a dependency
upgrade to this repository. The original CPU chart renderer was unchanged.

| Action-to-AfterRendering latency | femtovg 0.27.0 avg | femtovg 0.28.0 avg |
|---|---:|---:|
| Pan/zoom | 7.56 ms | 7.66 ms |
| Live 1 Hz | 20.04 ms | 20.28 ms |
| Live 100 Hz | 13.28 ms | 13.28 ms |
| Streaming | 20.48 ms | 20.37 ms |

These are single runs; the differences are small and should not be interpreted
as a demonstrated speedup or regression. `AfterRendering` is not a GPU
completion or presentation fence. The benchmark's startup metric is recorded
when the first chart is prepared, not when it is presented.

Raw results: [0.27 baseline](results/slint-femtovg-0.27-baseline.json) and
[0.28 experiment](results/slint-femtovg-0.28-experimental.json). Their embedded
environment field identifies Slint, not the femtovg version; the version is
identified by the file name and the experiment above.

## Baseline (2026-11-12T03:50:33Z)
**Configuration**: Slint 1.18.1 + femtovg 0.27.x (via Slint dependency)
**CPU**: Intel Core i5-8400 (6 cores)
**OS**: Windows

### Frame Time Performance (10s averaging)

| Benchmark | Avg (ms) | P95 (ms) | P99 (ms) | FPS |
|-----------|----------|----------|----------|-----|
| **Pan/Zoom** | 12.66 | 20.75 | 22.20 | 54.3 |
| **Live 100Hz/chart** | 13.33 | 15.48 | 17.45 | 73.9 |
| **Live 1Hz/chart** | 24.78 | 26.50 | 26.50 | 0.9 |
| **Stream Catchup** | 23.77 | 24.75 | 25.69 | 18.6 |

### Memory
- **Idle (post-load)**: 182.9 MB
- **After 30s**: 207.7 MB
- **Startup to first chart**: 564 ms

### Indicator Compute
- **All charts (median)**: 2.59 ms
- **Per chart (median)**: 0.30 ms

### Artifact
- **Exe size**: 17.43 MB

---

## Improvement Targets

### Phase 1-3 (Native GPU/Hybrid)
Expected improvement **15-40%** on graphics-bound phases:
- Pan/Zoom target: **10.7 ms** (20% improvement)
- Live 100Hz target: **10.7 ms** (20% improvement)
- Streaming target: **20.1 ms** (15% improvement)

### Phase 4 (Web GPU)
Expected parity with desktop GPU (within 5-10%):
- WebGPU frame times: target **≤11.5 ms** (pan/zoom)
- Canvas2D fallback: target **≤15 ms** (10% faster than CPU baseline)

---

## femtovg 0.28.0 Check
Slint 1.18.1 includes femtovg 0.27.x.
- Latest slint available: 1.18.1 (check for updates periodically)
- femtovg 0.28.0 (if released) would require slint 1.19+ upgrade
- **Decision**: Proceed with current stack for Phase 1-3; monitor for 0.28.0 improvements

---

## Benchmark Run Commands

**Baseline (as-is):**
```bash
cd 3_slint
cargo build --release
./target/release/slint-desktop --bench
# Results written to ../results/slint-desktop.json
```

**Post-Phase-1 (geometry extraction, CPU only):**
```bash
SLINT_TB_RENDER=cpu cargo build --release --features render-cpu
./target/release/slint-desktop --bench
# Results: results/slint-geom-cpu.json
```

**Post-Phase-2 (GPU rendering):**
```bash
SLINT_TB_RENDER=gpu cargo build --release --features render-gpu
./target/release/slint-desktop --bench
# Results: results/slint-geom-gpu.json
```

**Post-Phase-3 (Hybrid):**
```bash
SLINT_TB_RENDER=both cargo build --release --features render-both
./target/release/slint-desktop --bench
# Results: results/slint-hybrid.json
```

**Web (Phase 4):**
```bash
cd 3_slint
wasm-pack build --release --target web
# Benchmark via browser devtools
# Results: results/slint-wasm-webgpu.json
```

---

## Notes
- Frame time = UI action → rendered frame latency (measured via AfterRendering hook)
- All benchmarks exclude data generation time
- GPU measurements include queue.submit() fence time
- Streaming: 4000 bar preload + 2000 bars/sec catch-up + live updates
