//! Pure-Rust reference implementations of the 6 chart indicators.
//! Used as the correctness baseline; TA-Lib outputs are cross-checked
//! against these in each config's tests (tolerance 1e-6 after warmup).

use datagen::Bar;

/// Standard TA-Lib-style EMA: seeds with SMA of first `period` values,
/// then k = 2/(period+1). First `period-1` outputs are NaN.
pub fn ema(data: &[f64], period: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; data.len()];
    if data.len() < period || period == 0 {
        return out;
    }
    let k = 2.0 / (period as f64 + 1.0);
    let mut prev: f64 = data[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = prev;
    for (i, &x) in data.iter().enumerate().skip(period) {
        prev = x * k + prev * (1.0 - k);
        out[i] = prev;
    }
    out
}

/// Simple moving average. First `period-1` outputs are NaN.
pub fn sma(data: &[f64], period: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; data.len()];
    if data.len() < period || period == 0 {
        return out;
    }
    let mut sum: f64 = data[..period].iter().sum();
    out[period - 1] = sum / period as f64;
    for i in period..data.len() {
        sum += data[i] - data[i - period];
        out[i] = sum / period as f64;
    }
    out
}

/// VWAP resetting at each new calendar day (ts/86400 changes).
/// Typical price = (h+l+c)/3, weight = volume. No NaNs.
pub fn vwap_daily(bars: &[Bar]) -> Vec<f64> {
    let mut out = Vec::with_capacity(bars.len());
    let (mut day, mut pv, mut vv) = (-1i64, 0.0, 0.0);
    for b in bars {
        let d = b.ts / 86_400;
        if d != day {
            day = d;
            pv = 0.0;
            vv = 0.0;
        }
        let tp = (b.high + b.low + b.close) / 3.0;
        pv += tp * b.volume;
        vv += b.volume;
        out.push(if vv > 0.0 { pv / vv } else { tp });
    }
    out
}

/// One bin of the horizontal volume profile.
#[derive(Clone, Copy, Debug)]
pub struct ProfileBin {
    pub price_low: f64,
    pub price_high: f64,
    pub volume: f64,
}

/// Horizontal volume profile over the last `lookback` bars, `bins` price bins
/// spanning [min low, max high]. Volume of each bar is assigned to the bin
/// containing its close.
pub fn volume_profile(bars: &[Bar], lookback: usize, bins: usize) -> Vec<ProfileBin> {
    let start = bars.len().saturating_sub(lookback);
    let slice = &bars[start..];
    if slice.is_empty() || bins == 0 {
        return Vec::new();
    }
    let lo = slice.iter().map(|b| b.low).fold(f64::INFINITY, f64::min);
    let hi = slice.iter().map(|b| b.high).fold(f64::NEG_INFINITY, f64::max);
    let step = if hi > lo { (hi - lo) / bins as f64 } else { 1.0 };
    let mut out = Vec::with_capacity(bins);
    for i in 0..bins {
        out.push(ProfileBin {
            price_low: lo + i as f64 * step,
            price_high: lo + (i + 1) as f64 * step,
            volume: 0.0,
        });
    }
    for b in slice {
        let idx = (((b.close - lo) / step) as usize).min(bins - 1);
        out[idx].volume += b.volume;
    }
    out
}

pub fn closes(bars: &[Bar]) -> Vec<f64> {
    bars.iter().map(|b| b.close).collect()
}

/// All six indicators for one chart, matching the spec.
pub struct ChartIndicators {
    pub ema10: Vec<f64>,
    pub ema20: Vec<f64>,
    pub ema50: Vec<f64>,
    pub sma200: Vec<f64>,
    pub vwap: Vec<f64>,
    pub profile: Vec<ProfileBin>,
}

pub fn compute_all(bars: &[Bar]) -> ChartIndicators {
    let c = closes(bars);
    ChartIndicators {
        ema10: ema(&c, 10),
        ema20: ema(&c, 20),
        ema50: ema(&c, 50),
        sma200: sma(&c, 200),
        vwap: vwap_daily(bars),
        profile: volume_profile(bars, 2000, 40),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ema_seeds_with_sma() {
        let d: Vec<f64> = (1..=30).map(|x| x as f64).collect();
        let e = ema(&d, 10);
        assert!(e[..9].iter().all(|v| v.is_nan()));
        assert!((e[9] - 5.5).abs() < 1e-9);
        // Well-known check: EMA converges; last value between SMA and close.
        assert!(e[29] > e[9] && e[29] < 30.0);
    }

    #[test]
    fn sma_basic() {
        let d = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let s = sma(&d, 3);
        assert!((s[2] - 2.0).abs() < 1e-9);
        assert!((s[4] - 4.0).abs() < 1e-9);
    }

    #[test]
    fn vwap_resets_daily() {
        let mk = |ts: i64, c: f64, v: f64| Bar {
            ts, open: c, high: c, low: c, close: c, volume: v,
        };
        let bars = vec![mk(0, 10.0, 1.0), mk(1, 20.0, 1.0), mk(86_400, 30.0, 1.0)];
        let v = vwap_daily(&bars);
        assert!((v[1] - 15.0).abs() < 1e-9);
        assert!((v[2] - 30.0).abs() < 1e-9); // reset on new day
    }

    #[test]
    fn profile_sums_volume() {
        let bars = datagen::generate(0, 1);
        let p = volume_profile(&bars, 2000, 40);
        assert_eq!(p.len(), 40);
        let total: f64 = p.iter().map(|b| b.volume).sum();
        let expect: f64 = bars[bars.len() - 2000..].iter().map(|b| b.volume).sum();
        assert!((total - expect).abs() < 1e-6);
    }
}
