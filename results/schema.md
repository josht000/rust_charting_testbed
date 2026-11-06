# Benchmark result schema — `results/<config>.json`

Every config writes one JSON file per target (`<config>-desktop.json`,
`<config>-web.json`). Fields:

```json
{
  "config": "tauri|egui|slint",
  "target": "desktop|web",
  "timestamp": "ISO-8601",
  "environment": { "os": "...", "cpu": "...", "browser": "...", "version": "..." },
  "charts": 8,
  "bars_per_chart": 23400,
  "startup_ms_to_first_chart": 0.0,
  "indicator_compute_ms": { "all_charts_median": 0.0, "per_chart_median": 0.0, "runs": 20 },
  "frame_time": {
    "pan_zoom_10s": { "avg_ms": 0.0, "p95_ms": 0.0, "p99_ms": 0.0, "fps": 0.0 },
    "live_update_1s_per_chart_10s": { "avg_ms": 0.0, "p95_ms": 0.0, "p99_ms": 0.0, "fps": 0.0 },
    "live_update_100hz_per_chart_10s": { "avg_ms": 0.0, "p95_ms": 0.0, "p99_ms": 0.0, "fps": 0.0 }
  },
  "memory_mb": { "idle_after_load": 0.0, "after_30s": 0.0, "method": "..." },
  "artifact_size_mb": { "exe": 0.0, "wasm": 0.0, "js_bundle": 0.0, "total_web_dist": 0.0 },
  "notes": "free text: fallbacks used, missing features, anomalies"
}
```

Streaming data model (all configs):
- Initial static preload: first `INITIAL_BARS` (4000) bars per chart, loaded in
  one shot at startup (pregenerated `chart{i}.init.json` or equivalent).
- The remaining `BARS_PER_DAY - INITIAL_BARS` (19,400) bars are then streamed:
  bars arrive in batches at a configurable rate (`?rate=N` bars/sec, default
  1000) via the platform's streaming path (Tauri events / timer replay /
  callback), NOT by bulk-loading the full file.
- Indicators must update incrementally per streamed bar (rolling EMA/SMA/VWAP
  state, profile updates per batch), not recomputed over the whole history.
- Benchmarks measure frame stats during the catch-up stream too (optional
  `"stream_catchup"` entry inside `frame_time`), plus `stream_catchup_ms`
  and `stream_rate_bars_per_sec` at the top level.

Rules:
- Release/optimized builds only. Same machine, no other heavy load.
- Scripted bench (no human input): `?bench=1` for web, `--bench` for native;
  each writes/prints its JSON. 3 runs, keep the median in COMPARISON.md.
- Data generation time is excluded. All 8 charts' data use seeds from
  `shared/datagen` (`chart_seed(i)`, `BARS_PER_DAY = 23400`).
