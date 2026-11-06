/** One 1-second OHLCV bar (mirrors datagen::Bar). */
export interface Bar {
  ts: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
}

export interface ProfileBin {
  price_low: number;
  price_high: number;
  volume: number;
}

/**
 * Six indicator series for a chart.
 * Series entries are `number | null`: `null` = warmup gap (NaN on the Rust side,
 * serialized as JSON null, mapped to LWC whitespace on render).
 */
export interface Indicators {
  ema10: (number | null)[];
  ema20: (number | null)[];
  ema50: (number | null)[];
  sma200: (number | null)[];
  vwap: (number | null)[];
  profile: ProfileBin[];
}

export const NUM_CHARTS = 8;
export const BARS_PER_DAY = 23400;
