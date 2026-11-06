//! Deterministic 1-second OHLCV bar generation shared by all configs.
//! Same seed => identical data across every config.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Bar {
    /// Unix seconds.
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

pub const BARS_PER_DAY: usize = 6 * 60 * 65; // 23_400 bars: 09:30..16:00
/// Fixed session open (09:30) used for generation; benchmark data is a single day.
pub const SESSION_OPEN_TS: i64 = 1_700_000_000 + 9 * 3600 + 30 * 60; // arbitrary fixed date

pub const NUM_CHARTS: usize = 8;
pub const BASE_SEED: u64 = 0x5EED_CAFE;

/// Seed for a given chart index so all configs generate identical data.
pub fn chart_seed(chart: usize) -> u64 {
    BASE_SEED.wrapping_add(chart as u64)
}

/// Generate `days` trading days of 1s bars for `chart` (0..8).
/// Starts at `SESSION_OPEN_TS`, skips nights/weekends (16:00->09:30 next weekday).
/// Random walk with mild intraday volatility regime shifts.
pub fn generate(chart: usize, days: usize) -> Vec<Bar> {
    let mut rng = StdRng::seed_from_u64(chart_seed(chart));
    let mut bars = Vec::with_capacity(BARS_PER_DAY * days);

    // Give each chart a distinct personality.
    let mut price = 20.0 + chart as f64 * 37.0 + rng.gen::<f64>() * 10.0;
    let base_vol = 0.0004 + chart as f64 * 0.00015;
    let mut day = 0usize;
    let mut ts = SESSION_OPEN_TS;

    while day < days {
        for i in 0..BARS_PER_DAY {
            // Intraday U-shaped volatility and volume.
            let t = i as f64 / BARS_PER_DAY as f64;
            let intraday = 1.0 + 1.5 * (t - 0.5).powi(2) * 4.0;
            let vol = base_vol * intraday * (1.0 + rng.gen::<f64>() * 0.5);
            let drift = (rng.gen::<f64>() - 0.5) * 0.0002;
            let ret = drift + vol * (rng.gen::<f64>() * 2.0 - 1.0);

            let open = price;
            let close = open * (1.0 + ret);
            let wick_hi = rng.gen::<f64>() * vol * open;
            let wick_lo = rng.gen::<f64>() * vol * open;
            let high = open.max(close) + wick_hi;
            let low = (open.min(close) - wick_lo).max(0.01);
            let volume = (500.0 + rng.gen::<f64>() * 4500.0) * intraday;

            bars.push(Bar { ts, open, high, low, close, volume });
            price = close;
            ts += 1;
        }
        day += 1;
        // Jump to next session open (skip nights; days are contiguous sessions).
        ts = SESSION_OPEN_TS + day as i64 * 86_400;
        price *= 1.0 + (rng.gen::<f64>() - 0.5) * 0.01; // overnight gap
    }
    bars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let a = generate(0, 1);
        let b = generate(0, 1);
        assert_eq!(a.len(), BARS_PER_DAY);
        assert!((a[0].close - b[0].close).abs() < f64::EPSILON);
        assert!((a[100].volume - b[100].volume).abs() < f64::EPSILON);
    }

    #[test]
    fn charts_differ() {
        let a = generate(0, 1);
        let b = generate(1, 1);
        assert!((a[500].close - b[500].close).abs() > 1e-9);
    }

    #[test]
    fn ohlc_valid() {
        for b in generate(3, 1).iter().step_by(97) {
            assert!(b.high >= b.open.max(b.close));
            assert!(b.low <= b.open.min(b.close));
            assert!(b.low > 0.0 && b.volume > 0.0);
        }
    }

    #[test]
    fn daily_reset_ts() {
        let bars = generate(0, 2);
        assert_eq!(bars.len(), BARS_PER_DAY * 2);
        let d1 = bars[BARS_PER_DAY - 1].ts / 86_400;
        let d2 = bars[BARS_PER_DAY].ts / 86_400;
        assert_eq!(d2, d1 + 1);
    }
}

/// Number of bars delivered in the initial static load. The remainder of the
/// day (BARS_PER_DAY - INITIAL_BARS) is delivered via the streaming path.
pub const INITIAL_BARS: usize = 4_000;
