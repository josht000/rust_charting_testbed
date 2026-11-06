/**
 * Scripted benchmark driver (`?bench=1` on web, `--bench` in the Tauri shell).
 * Produces results matching results/schema.md and saves them via
 * `saveResults` (web: POST /__save on the serve-web server; Tauri: the
 * `save_results` command writes <workspace>/results/tauri-desktop.json).
 */

import { chartRegistry, firstChartReadyAt, type ChartHandle } from './ChartPanel';
import { computeIndicators, isTauri, saveResults } from './data';
import type { Bar } from './types';

const BARS = 23400;
const INDICATOR_RUNS = 20;

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

function median(xs: number[]): number {
  if (xs.length === 0) return 0;
  const s = [...xs].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
}

function percentile(xs: number[], p: number): number {
  if (xs.length === 0) return 0;
  const s = [...xs].sort((a, b) => a - b);
  const i = Math.min(s.length - 1, Math.max(0, Math.ceil((p / 100) * s.length) - 1));
  return s[i];
}

function frameStats(deltas: number[]) {
  // Drop the first frame (phase transition hiccup).
  const d = deltas.slice(1);
  const avg = d.length ? d.reduce((a, b) => a + b, 0) / d.length : 0;
  return {
    avg_ms: +avg.toFixed(3),
    p95_ms: +percentile(d, 95).toFixed(3),
    p99_ms: +percentile(d, 99).toFixed(3),
    fps: avg > 0 ? +(1000 / avg).toFixed(1) : 0,
  };
}

/** Collect per-frame deltas (rAF) for `durationMs`. */
function collectFrames(durationMs: number): Promise<number[]> {
  return new Promise((resolve) => {
    const deltas: number[] = [];
    let last = performance.now();
    const end = last + durationMs;
    const tick = (t: number) => {
      deltas.push(t - last);
      last = t;
      if (t < end) requestAnimationFrame(tick);
      else resolve(deltas);
    };
    requestAnimationFrame(tick);
  });
}

/** Collect per-frame deltas until `stop` resolves. */
function collectFramesUntil(stop: Promise<unknown>): Promise<number[]> {
  return new Promise((resolve) => {
    const deltas: number[] = [];
    let last = performance.now();
    let stopped = false;
    stop.then(() => {
      stopped = true;
    });
    const tick = (t: number) => {
      deltas.push(t - last);
      last = t;
      if (!stopped) requestAnimationFrame(tick);
      else resolve(deltas);
    };
    requestAnimationFrame(tick);
  });
}

function heapMb(): number {
  const mem = (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory;
  return mem ? +(mem.usedJSHeapSize / 1048576).toFixed(1) : 0;
}

async function waitForCharts(timeoutMs = 120_000): Promise<ChartHandle[]> {
  const t0 = performance.now();
  for (;;) {
    const handles = chartRegistry.filter((h): h is ChartHandle => !!h);
    if (handles.length === 8) return handles;
    if (performance.now() - t0 > timeoutMs) throw new Error('charts did not load in time');
    await sleep(100);
  }
}

/** Deterministic synthetic live bar mutating the chart's last bar. */
function makeLiveBar(h: ChartHandle, i: number): Bar {
  const last = h.bars[h.bars.length - 1];
  const c = last.close * (1 + Math.sin(i / 7) * 1e-4);
  return {
    ts: last.ts,
    open: last.open,
    high: Math.max(last.high, c),
    low: Math.min(last.low, c),
    close: c,
    volume: last.volume + 10,
  };
}

async function benchIndicatorCompute(handles: ChartHandle[]) {
  const runTotals: number[] = [];
  const perChart: number[] = [];
  for (let r = 0; r < INDICATOR_RUNS; r++) {
    let total = 0;
    for (const h of handles) {
      const t = performance.now();
      await computeIndicators(h.chartIdx, h.bars);
      const dt = performance.now() - t;
      total += dt;
      perChart.push(dt);
    }
    runTotals.push(total);
  }
  return {
    all_charts_median: +median(runTotals).toFixed(2),
    per_chart_median: +median(perChart).toFixed(2),
    runs: INDICATOR_RUNS,
  };
}

async function benchPanZoom(handles: ChartHandle[], durationMs = 10_000) {
  let step = 0;
  const iv = setInterval(() => {
    step++;
    const dir = Math.floor(step / 40) % 2 === 0 ? 1 : -1;
    for (const h of handles) {
      const ts = h.chart.timeScale();
      const r = ts.getVisibleLogicalRange();
      if (!r) continue;
      let span = r.to - r.from;
      if (step % 20 === 0) span = Math.max(50, span * (span > 4000 ? 0.5 : 2)); // zoom toggles
      let from = r.from + dir * 25;
      let to = from + span;
      if (from < -50) {
        from = -50;
        to = from + span;
      }
      if (to > BARS + 50) {
        to = BARS + 50;
        from = to - span;
      }
      ts.setVisibleLogicalRange({ from, to });
    }
  }, 50);
  const frames = await collectFrames(durationMs);
  clearInterval(iv);
  return frameStats(frames);
}

async function benchLiveUpdates(handles: ChartHandle[], updatesPerSec: number, durationMs = 10_000) {
  const periodMs = Math.max(1, Math.round(1000 / updatesPerSec));
  let tick = 0;
  const iv = setInterval(() => {
    tick++;
    for (const h of handles) h.pushLive(makeLiveBar(h, tick));
  }, periodMs);
  const frames = await collectFrames(durationMs);
  clearInterval(iv);
  return frameStats(frames);
}

async function fetchArtifactStats() {
  const empty = { exe: 0, wasm: 0, js_bundle: 0, total_web_dist: 0 };
  if (isTauri) return empty; // exe size is injected by the save_results command
  try {
    const res = await fetch('/__stats');
    if (!res.ok) return empty;
    const s = await res.json();
    return {
      exe: 0,
      wasm: +(s.wasm_mb ?? 0),
      js_bundle: +(s.js_bundle_mb ?? 0),
      total_web_dist: +(s.total_web_dist_mb ?? 0),
    };
  } catch {
    return empty;
  }
}

export async function runBench(): Promise<void> {
  const t0 = (window as unknown as { __t0?: number }).__t0 ?? 0;
  console.log('[bench] waiting for charts...');
  const handles = await waitForCharts();
  const startup = firstChartReadyAt > 0 ? firstChartReadyAt - t0 : 0;
  const idleHeap = heapMb();
  console.log(`[bench] charts ready; startup=${startup.toFixed(0)}ms`);

  // Stream catch-up: all 8 charts finish their post-preload stream while we
  // record frame times during catch-up.
  console.log('[bench] awaiting stream catch-up...');
  const streamStart = Math.min(...handles.map((h) => h.streamStartAt));
  const allDone = Promise.all(handles.map((h) => h.streamDone));
  const streamFrames = await collectFramesUntil(allDone);
  await allDone;
  const streamCatchupMs = performance.now() - streamStart;
  const streamedBars = handles.reduce((n, h) => n + (h.bars.length - 4000), 0);
  const streamRate = streamCatchupMs > 0 ? streamedBars / (streamCatchupMs / 1000) : 0;
  console.log(
    `[bench] stream done: ${streamCatchupMs.toFixed(0)}ms, ${streamRate.toFixed(0)} bars/s`,
  );

  console.log('[bench] indicator compute x20...');
  const indicator = await benchIndicatorCompute(handles);

  console.log('[bench] pan/zoom 10s...');
  const panZoom = await benchPanZoom(handles);

  console.log('[bench] live updates 1/s 10s...');
  const live1 = await benchLiveUpdates(handles, 1);

  console.log('[bench] live updates 100/s 10s...');
  const live100 = await benchLiveUpdates(handles, 100);

  const afterHeap = heapMb();
  const artifacts = await fetchArtifactStats();

  const results = {
    config: 'tauri',
    target: isTauri ? 'desktop' : 'web',
    timestamp: new Date().toISOString(),
    environment: {
      os: navigator.platform || 'unknown',
      cpu: `${navigator.hardwareConcurrency ?? '?'} logical cores`,
      browser: navigator.userAgent,
      version: `react19/lwc5.2.1/dockview8${isTauri ? ' tauri2' : ' web'}`,
    },
    charts: 8,
    bars_per_chart: BARS,
    startup_ms_to_first_chart: +startup.toFixed(1),
    stream_catchup_ms: +streamCatchupMs.toFixed(1),
    stream_rate_bars_per_sec: +streamRate.toFixed(0),
    indicator_compute_ms: indicator,
    frame_time: {
      stream_catchup: frameStats(streamFrames),
      pan_zoom_10s: panZoom,
      live_update_1s_per_chart_10s: live1,
      live_update_100hz_per_chart_10s: live100,
    },
    memory_mb: {
      idle_after_load: idleHeap,
      after_30s: afterHeap,
      method: heapMb() > 0 ? 'performance.memory.usedJSHeapSize' : 'unavailable (non-Chromium)',
    },
    artifact_size_mb: artifacts,
    notes: isTauri
      ? 'Desktop shell, streaming model: 4000-bar preload + 19400 bars streamed via Tauri events (bar-batch/stream-done, ~50ms batches); indicators via Tauri commands (preload) + incremental JS updates (stream).'
      : 'Web, streaming model: 4000-bar preload (/data/chart{i}.init.json) + 19400 bars replayed from chart{i}.rest.json in 50ms batches; preload indicators via talib-wasm + pure JS, incremental updates during stream.',
  };

  const filename = isTauri ? 'tauri-desktop.json' : 'tauri-web.json';
  const json = JSON.stringify(results, null, 2);
  console.log('[bench] results:\n' + json);
  const where = await saveResults(filename, json).catch((e) => `save failed: ${e}`);
  console.log(`[bench] saved -> ${where}`);
  (window as unknown as { __benchDone?: unknown }).__benchDone = { path: where };
}
