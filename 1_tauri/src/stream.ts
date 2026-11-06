/**
 * Incremental indicator state for the streaming data model.
 *
 * Feed bars one at a time via `pushBar`; each returns the indicator values for
 * that bar. State is seeded by replaying the 4000-bar preload through the same
 * per-bar update path, so it is bit-identical to batch `computeAllJs`
 * (same FP op order). The volume profile is recomputed per batch from the
 * trailing 2000-bar window via the existing `volumeProfile()` helper.
 */

import type { Bar, ProfileBin } from './types';
import { volumeProfile } from './indicators';

export const INITIAL_BARS = 4000;
const PROFILE_LOOKBACK = 2000;
const PROFILE_BINS = 40;
const EMA_PERIODS = [10, 20, 50] as const;
const SMA_PERIOD = 200;

interface EmaState {
  k: number;
  count: number;
  sum: number; // SMA seed accumulator during warmup
  prev: number;
  seeded: boolean;
}

export interface BarIndicators {
  ema10: number | null;
  ema20: number | null;
  ema50: number | null;
  sma200: number | null;
  vwap: number | null;
}

export class IncrementalIndicators {
  private _emas: EmaState[];
  private _smaWindow: number[] = [];
  private _smaSum = 0;
  private _vwapDay = -1;
  private _vwapPv = 0;
  private _vwapVv = 0;
  private _profileWindow: Bar[] = [];
  // Snapshot of accumulator state taken before the last pushBar, used by
  // updateLast() to recompute the forming bar's outputs without double-feeding.
  private _emaBefore: { prev: number; sum: number; seeded: boolean }[] = [];
  private _smaSumBefore = 0;
  private _vwapDayBefore = -1;
  private _vwapPvBefore = 0;
  private _vwapVvBefore = 0;
  private _haveLast = false;

  constructor(seedBars: Bar[] = []) {
    this._emas = EMA_PERIODS.map((p) => ({
      k: 2 / (p + 1),
      count: 0,
      sum: 0,
      prev: 0,
      seeded: false,
    }));
    for (const b of seedBars) this.pushBar(b);
  }

  /** Feed one bar; returns its indicator values (null during warmup). */
  pushBar(bar: Bar): BarIndicators {
    this._snapshot();
    return this._apply(bar);
  }

  /**
   * Recompute the LAST fed bar after it mutated (a forming live bar whose
   * close/high/low/volume changed). Restores pre-bar state then reapplies —
   * bar count is unchanged. Cheap: O(1), no history replay.
   */
  updateLast(bar: Bar): BarIndicators {
    if (!this._haveLast) return this.pushBar(bar);
    const c = bar.close;
    const emaVals = this._emas.map((e, i) => {
      const b4 = this._emaBefore[i];
      if (!e.seeded) {
        e.sum = b4.sum + c;
        const idx = e.count - 1;
        if (idx < EMA_PERIODS[i] - 1) return null;
        e.prev = e.sum / EMA_PERIODS[i];
        e.seeded = true;
        return e.prev;
      }
      e.prev = c * e.k + b4.prev * (1 - e.k);
      return e.prev;
    });
    this._smaSum = this._smaSumBefore + c;
    this._smaWindow[this._smaWindow.length - 1] = c;
    const sma = this._smaWindow.length === SMA_PERIOD ? this._smaSum / SMA_PERIOD : null;
    this._vwapDay = this._vwapDayBefore;
    this._vwapPv = this._vwapPvBefore;
    this._vwapVv = this._vwapVvBefore;
    const vwap = this._pushVwap(bar);
    this._profileWindow[this._profileWindow.length - 1] = bar;
    return { ema10: emaVals[0], ema20: emaVals[1], ema50: emaVals[2], sma200: sma, vwap };
  }

  private _snapshot(): void {
    this._emaBefore = this._emas.map((e) => ({
      prev: e.prev,
      sum: e.sum,
      seeded: e.seeded,
    }));
    this._smaSumBefore = this._smaSum;
    this._vwapDayBefore = this._vwapDay;
    this._vwapPvBefore = this._vwapPv;
    this._vwapVvBefore = this._vwapVv;
    this._haveLast = false;
  }

  private _apply(bar: Bar): BarIndicators {
    const c = bar.close;
    this._haveLast = true;
    const emaVals = this._emas.map((e, i) => this._pushEma(e, c, EMA_PERIODS[i]));
    const sma = this._pushSma(c);
    const vwap = this._pushVwap(bar);
    this._profileWindow.push(bar);
    if (this._profileWindow.length > PROFILE_LOOKBACK) {
      this._profileWindow.splice(0, this._profileWindow.length - PROFILE_LOOKBACK);
    }
    return {
      ema10: emaVals[0],
      ema20: emaVals[1],
      ema50: emaVals[2],
      sma200: sma,
      vwap,
    };
  }

  private _pushEma(e: EmaState, x: number, period: number): number | null {
    const i = e.count++;
    if (!e.seeded) {
      e.sum += x;
      if (i < period - 1) return null;
      if (i === period - 1) {
        e.prev = e.sum / period;
        e.seeded = true;
      }
      return e.prev;
    }
    e.prev = x * e.k + e.prev * (1 - e.k);
    return e.prev;
  }

  private _pushSma(x: number): number | null {
    this._smaWindow.push(x);
    this._smaSum += x;
    if (this._smaWindow.length > SMA_PERIOD) {
      this._smaSum -= this._smaWindow.shift() as number;
    }
    return this._smaWindow.length === SMA_PERIOD ? this._smaSum / SMA_PERIOD : null;
  }

  private _pushVwap(bar: Bar): number {
    const d = Math.floor(bar.ts / 86400);
    if (d !== this._vwapDay) {
      this._vwapDay = d;
      this._vwapPv = 0;
      this._vwapVv = 0;
    }
    const tp = (bar.high + bar.low + bar.close) / 3;
    this._vwapPv += tp * bar.volume;
    this._vwapVv += bar.volume;
    return this._vwapVv > 0 ? this._vwapPv / this._vwapVv : tp;
  }

  /** Recompute the 40-bin volume profile over the trailing 2000 bars. Call once per batch. */
  updateProfile(): ProfileBin[] {
    return volumeProfile(this._profileWindow, PROFILE_LOOKBACK, PROFILE_BINS);
  }
}
