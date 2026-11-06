/**
 * Parity test: JS indicator implementations vs the Rust reference
 * (shared/indicators), via golden files produced by
 * `cargo run --release --bin gen_golden` in src-tauri/ (golden JSON lives in
 * tests/golden/). Tolerance: 1e-6 after warmup (NaN region excluded).
 *
 * talib-wasm is loaded through createRequire because vitest's CJS interop
 * cannot unwrap the module's promise-shaped export.
 */
import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { computeAllJs, computeAllTalib } from './indicators';
import { IncrementalIndicators } from './stream';
import type { Bar, Indicators } from './types';

const require = createRequire(import.meta.url);
const HERE = fileURLToPath(new URL('.', import.meta.url));
const GOLDEN_DIR = resolve(HERE, '../tests/golden');
const DATA_DIR = resolve(HERE, '../public/data');
const TOL = 1e-6;
const STREAM_TOL = 1e-9;

type Golden = {
  ema10: (number | null)[];
  ema20: (number | null)[];
  ema50: (number | null)[];
  sma200: (number | null)[];
  vwap: (number | null)[];
  profile: { price_low: number; price_high: number; volume: number }[];
};

const barsCache = new Map<number, Bar[]>();
function loadBars(i: number): Bar[] {
  let b = barsCache.get(i);
  if (!b) {
    b = JSON.parse(readFileSync(resolve(DATA_DIR, `chart${i}.json`), 'utf8'));
    barsCache.set(i, b!);
  }
  return b!;
}

function loadGolden(i: number): Golden {
  return JSON.parse(readFileSync(resolve(GOLDEN_DIR, `chart${i}.json`), 'utf8'));
}

/** Max |a-e| over indices where e is a finite number; also checks warmup nulls. */
function seriesDiff(actual: (number | null)[], expected: (number | null)[]) {
  if (actual.length !== expected.length) {
    return { maxDiff: Infinity, badWarmup: -1, lengthMismatch: true };
  }
  let maxDiff = 0;
  let badWarmup = -1;
  for (let i = 0; i < expected.length; i++) {
    const e = expected[i];
    if (e === null || !Number.isFinite(e)) {
      if (actual[i] !== null && badWarmup < 0) badWarmup = i;
      continue;
    }
    const a = actual[i];
    const d = a === null || !Number.isFinite(a) ? Infinity : Math.abs(a - e);
    if (d > maxDiff) maxDiff = d;
  }
  return { maxDiff, badWarmup, lengthMismatch: false };
}

function assertIndicatorsClose(actual: Indicators, golden: Golden, ctx: string) {
  for (const key of ['ema10', 'ema20', 'ema50', 'sma200', 'vwap'] as const) {
    const d = seriesDiff(actual[key] as (number | null)[], golden[key]);
    expect(d.lengthMismatch, `${ctx}.${key} length`).toBe(false);
    expect(d.badWarmup, `${ctx}.${key} first non-null warmup idx`).toBe(-1);
    expect(d.maxDiff, `${ctx}.${key} maxDiff`).toBeLessThanOrEqual(TOL);
  }
  expect(actual.profile.length, `${ctx}.profile bins`).toBe(golden.profile.length);
  let pMax = 0;
  for (let i = 0; i < golden.profile.length; i++) {
    const a = actual.profile[i];
    const g = golden.profile[i];
    pMax = Math.max(
      pMax,
      Math.abs(a.price_low - g.price_low),
      Math.abs(a.price_high - g.price_high),
      Math.abs(a.volume - g.volume) / Math.max(1, Math.abs(g.volume)), // rel for big volumes
    );
  }
  expect(pMax, `${ctx}.profile maxDiff`).toBeLessThanOrEqual(TOL);
}

describe('JS indicators vs Rust reference (golden)', () => {
  it('pure-JS computeAllJs matches Rust for all 8 charts', () => {
    for (let i = 0; i < 8; i++) {
      assertIndicatorsClose(computeAllJs(loadBars(i)), loadGolden(i), `chart${i}.js`);
    }
  }, 60_000);

  it('talib-wasm EMA/SMA matches Rust for all 8 charts', async () => {
    const factory = require('talib-wasm/build/talib.wrapped.min.js');
    const talib = await factory();
    for (let i = 0; i < 8; i++) {
      assertIndicatorsClose(
        await computeAllTalib(loadBars(i), talib),
        loadGolden(i),
        `chart${i}.talib`,
      );
    }
  }, 60_000);
});

/**
 * Streaming model: IncrementalIndicators fed bar-by-bar must produce the same
 * series as the batch computeAllJs, both starting empty and seeded with the
 * 4000-bar preload then fed the remaining 19,400 bars.
 */
describe('incremental streaming indicators', () => {
  function streamedSeries(bars: Bar[], seed = 0): Record<string, (number | null)[]> {
    const inc = new IncrementalIndicators(bars.slice(0, seed));
    const out: Record<string, (number | null)[]> = {
      ema10: new Array(bars.length).fill(null),
      ema20: new Array(bars.length).fill(null),
      ema50: new Array(bars.length).fill(null),
      sma200: new Array(bars.length).fill(null),
      vwap: new Array(bars.length).fill(null),
    };
    // Seed outputs equal the batch computation over the preload by construction;
    // only bars after `seed` are produced incrementally here.
    for (let i = seed; i < bars.length; i++) {
      const v = inc.pushBar(bars[i]);
      out.ema10[i] = v.ema10;
      out.ema20[i] = v.ema20;
      out.ema50[i] = v.ema50;
      out.sma200[i] = v.sma200;
      out.vwap[i] = v.vwap;
    }
    return out;
  }

  it('bar-by-bar stream from empty == batch computeAllJs', () => {
    for (let i = 0; i < 8; i++) {
      const bars = loadBars(i);
      const batch = computeAllJs(bars);
      const got = streamedSeries(bars, 0);
      for (const key of ['ema10', 'ema20', 'ema50', 'sma200', 'vwap'] as const) {
        const d = seriesDiff(got[key], batch[key] as (number | null)[]);
        expect(d.maxDiff, `chart${i}.${key} stream-vs-batch`).toBeLessThanOrEqual(STREAM_TOL);
        expect(d.badWarmup).toBe(-1);
      }
    }
  }, 60_000);

  it('4000-bar seed + 19400 streamed == batch computeAllJs on streamed bars', () => {
    for (let i = 0; i < 8; i++) {
      const bars = loadBars(i);
      const batch = computeAllJs(bars);
      const got = streamedSeries(bars, 4000);
      for (const key of ['ema10', 'ema20', 'ema50', 'sma200', 'vwap'] as const) {
        const d = seriesDiff(got[key].slice(4000), (batch[key] as (number | null)[]).slice(4000));
        expect(d.maxDiff, `chart${i}.${key} seeded-stream`).toBeLessThanOrEqual(STREAM_TOL);
        expect(d.badWarmup).toBe(-1);
      }
    }
  }, 60_000);
});
