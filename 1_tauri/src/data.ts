/**
 * Data access layer: identical API in the Tauri desktop shell (Rust via
 * `invoke` + events) and in the standalone web build (fetch pregenerated JSON
 * + JS-side indicators + timer-replay streaming).
 *
 * Streaming model: each chart loads INITIAL_BARS (4000) statically, then the
 * remaining 19,400 bars arrive in ~50ms batches at a configurable rate
 * (`?rate=N` bars/sec, default 1000).
 */

import type { Bar, Indicators } from './types';
import { computeAllTalib } from './indicators';
import { INITIAL_BARS } from './stream';

export { INITIAL_BARS };

export const isTauri =
  typeof window !== 'undefined' &&
  (('__TAURI__' in window) || ('__TAURI_INTERNALS__' in window));

/** Batch period for streamed bars, ms. */
export const STREAM_BATCH_MS = 50;
export const DEFAULT_STREAM_RATE = 1000;
export const BENCH_STREAM_RATE = 2000;

type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

let invokeFn: InvokeFn | null = null;
async function getInvoke(): Promise<InvokeFn> {
  if (!invokeFn) {
    const mod = await import('@tauri-apps/api/core');
    invokeFn = mod.invoke as InvokeFn;
  }
  return invokeFn;
}

/** Static preload: first INITIAL_BARS bars of the day. */
export async function getInitialBars(chart: number): Promise<Bar[]> {
  if (isTauri) {
    const invoke = await getInvoke();
    return invoke<Bar[]>('get_initial_bars', { chart });
  }
  const res = await fetch(`/data/chart${chart}.init.json`);
  if (!res.ok) throw new Error(`fetch chart${chart}.init: ${res.status}`);
  return res.json();
}

/** Full-day bars (kept for bench indicator_compute phase). */
export async function getBars(chart: number): Promise<Bar[]> {
  if (isTauri) {
    const invoke = await getInvoke();
    return invoke<Bar[]>('get_bars', { chart });
  }
  const res = await fetch(`/data/chart${chart}.json`);
  if (!res.ok) throw new Error(`fetch chart${chart}: ${res.status}`);
  return res.json();
}

/**
 * Indicators for a chart. `count` limits computation to the first N bars
 * (used for the preload; Rust side slices, JS side slices the given array).
 */
export async function computeIndicators(
  chart: number,
  bars: Bar[],
  count?: number,
): Promise<Indicators> {
  if (isTauri) {
    const invoke = await getInvoke();
    return invoke<Indicators>('compute_indicators', { chart, count: count ?? null });
  }
  return computeAllTalib(count ? bars.slice(0, count) : bars);
}

/**
 * Stream the post-preload bars for `chart` in ~STREAM_BATCH_MS batches at
 * `rate` bars/sec (<=0 falls back to DEFAULT_STREAM_RATE).
 * Resolves when the stream completes.
 */
export async function streamBars(
  chart: number,
  rate: number,
  onBatch: (batch: Bar[]) => void,
): Promise<void> {
  const r = rate > 0 ? rate : DEFAULT_STREAM_RATE;
  const perBatch = Math.max(1, Math.ceil((r * STREAM_BATCH_MS) / 1000));

  if (isTauri) {
    const invoke = await getInvoke();
    const { listen } = await import('@tauri-apps/api/event');
    let resolveDone!: () => void;
    const done = new Promise<void>((res) => (resolveDone = res));
    // Register listeners before starting the stream so no batch is missed.
    const unBatch = await listen<Bar[]>(`bar-batch-${chart}`, (e) => onBatch(e.payload));
    const unDone = await listen(`stream-done-${chart}`, () => resolveDone());
    try {
      await invoke('start_stream', { chart, rate: r });
      await done;
    } finally {
      unBatch();
      unDone();
    }
    return;
  }

  // Web: fetch the remainder once, then replay in timed batches.
  const res = await fetch(`/data/chart${chart}.rest.json`);
  if (!res.ok) throw new Error(`fetch chart${chart}.rest: ${res.status}`);
  const rest: Bar[] = await res.json();
  let idx = 0;
  await new Promise<void>((resolve) => {
    const iv = setInterval(() => {
      const slice = rest.slice(idx, idx + perBatch);
      idx += slice.length;
      if (slice.length > 0) onBatch(slice);
      if (idx >= rest.length) {
        clearInterval(iv);
        resolve();
      }
    }, STREAM_BATCH_MS);
  });
}

/** Persist benchmark JSON (see previous version for details). */
export async function saveResults(filename: string, json: string): Promise<string> {
  if (isTauri) {
    const invoke = await getInvoke();
    return invoke<string>('save_results', { filename, contents: json });
  }
  try {
    const res = await fetch(`/__save?name=${encodeURIComponent(filename)}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: json,
    });
    if (res.ok) return await res.text();
    throw new Error(`/__save ${res.status}`);
  } catch (e) {
    const blob = new Blob([json], { type: 'application/json' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = filename;
    a.click();
    return `download fallback (${String(e)})`;
  }
}

/** In Tauri, true when launched with `--bench`. On web, false (uses ?bench=1). */
export async function benchRequested(): Promise<boolean> {
  if (!isTauri) return false;
  const invoke = await getInvoke();
  return invoke<boolean>('bench_requested');
}

/**
 * Effective stream rate for a panel: `?rate=N` wins; bench mode (`?bench=1`
 * or --bench) defaults to BENCH_STREAM_RATE; otherwise DEFAULT_STREAM_RATE.
 */
export async function getStreamRate(): Promise<number> {
  const params = new URLSearchParams(window.location.search);
  const r = Number(params.get('rate'));
  if (r > 0) return r;
  if (params.get('bench') === '1' || (await benchRequested())) return BENCH_STREAM_RATE;
  return DEFAULT_STREAM_RATE;
}
