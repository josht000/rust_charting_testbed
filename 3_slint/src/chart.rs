//! Chart state + software rendering into `slint::Image`.
//!
//! Indicators: EMA/SMA come from the pure-Rust `ta` crate (the "rust talib").
//! `ta`'s EMA seeds with the first fed value, so we feed it the SMA of the
//! first `period` closes as the seed value — this reproduces the TA-Lib /
//! shared-reference EMA *exactly* (identical recursion afterwards, no warmup
//! tolerance needed). VWAP and volume profile use `shared/indicators`.

use datagen::Bar;
use indicators::ChartIndicators;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use ta::indicators::{ExponentialMovingAverage, SimpleMovingAverage};
use ta::Next;

use crate::raster::{Canvas, Rgb, BG, DOWN, EMA10, EMA20, EMA50, GRID, PROFILE, SMA200, UP, VWAP};

/// TA-Lib-compatible EMA built on `ta::ExponentialMovingAverage`:
/// seed = SMA(first `period` values), then feed values from index `period`.
/// First `period - 1` outputs are NaN, exactly like `indicators::ema`.
fn ema_ta(data: &[f64], period: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; data.len()];
    if data.len() < period || period == 0 {
        return out;
    }
    let seed: f64 = data[..period].iter().sum::<f64>() / period as f64;
    let mut e = ExponentialMovingAverage::new(period).expect("period > 0");
    out[period - 1] = e.next(seed); // is_new -> seed
    for (i, &x) in data.iter().enumerate().skip(period) {
        out[i] = e.next(x);
    }
    out
}

/// SMA via `ta::SimpleMovingAverage`; first `period - 1` outputs forced to NaN
/// (`ta` emits partial averages during warmup instead of NaN).
fn sma_ta(data: &[f64], period: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; data.len()];
    if data.len() < period || period == 0 {
        return out;
    }
    let mut s = SimpleMovingAverage::new(period).expect("period > 0");
    for (i, &x) in data.iter().enumerate() {
        let v = s.next(x);
        if i >= period - 1 {
            out[i] = v;
        }
    }
    out
}

/// All six indicators using `ta` for EMA/SMA and shared/indicators for the rest.
pub fn compute_all_ta(bars: &[Bar]) -> ChartIndicators {
    let c = indicators::closes(bars);
    ChartIndicators {
        ema10: ema_ta(&c, 10),
        ema20: ema_ta(&c, 20),
        ema50: ema_ta(&c, 50),
        sma200: sma_ta(&c, 200),
        vwap: indicators::vwap_daily(bars),
        profile: indicators::volume_profile(bars, 2000, 40),
    }
}

/// Incremental indicator state for live updates (O(1) per new bar).
/// EMAs/SMA are `ta` indicators pre-fed with all existing closes, so their
/// state has converged to the reference series (< 1e-9 after ~500 bars).
pub struct LiveInd {
    ema10: ExponentialMovingAverage,
    ema20: ExponentialMovingAverage,
    ema50: ExponentialMovingAverage,
    sma200: SimpleMovingAverage,
    vwap_pv: f64,
    vwap_vv: f64,
    vwap_day: i64,
}

impl LiveInd {
    pub fn new(bars: &[Bar]) -> Self {
        let mut s = Self {
            ema10: ExponentialMovingAverage::new(10).unwrap(),
            ema20: ExponentialMovingAverage::new(20).unwrap(),
            ema50: ExponentialMovingAverage::new(50).unwrap(),
            sma200: SimpleMovingAverage::new(200).unwrap(),
            vwap_pv: 0.0,
            vwap_vv: 0.0,
            vwap_day: -1,
        };
        for b in bars {
            s.ema10.next(b.close);
            s.ema20.next(b.close);
            s.ema50.next(b.close);
            s.sma200.next(b.close);
            s.vwap_acc(b);
        }
        s
    }

    fn vwap_acc(&mut self, b: &Bar) -> f64 {
        let d = b.ts / 86_400;
        if d != self.vwap_day {
            self.vwap_day = d;
            self.vwap_pv = 0.0;
            self.vwap_vv = 0.0;
        }
        let tp = (b.high + b.low + b.close) / 3.0;
        self.vwap_pv += tp * b.volume;
        self.vwap_vv += b.volume;
        if self.vwap_vv > 0.0 {
            self.vwap_pv / self.vwap_vv
        } else {
            tp
        }
    }

    /// Feed one new bar, appending to each indicator series in `ind`.
    pub fn push(&mut self, b: &Bar, ind: &mut ChartIndicators) {
        ind.ema10.push(self.ema10.next(b.close));
        ind.ema20.push(self.ema20.next(b.close));
        ind.ema50.push(self.ema50.next(b.close));
        ind.sma200.push(self.sma200.next(b.close));
        ind.vwap.push(self.vwap_acc(b));
    }
}

pub struct Chart {
    pub idx: usize,
    /// Consumed bars: INITIAL_BARS preload + streamed + live-appended.
    pub bars: Vec<Bar>,
    /// Not-yet-streamed remainder of the generated day (replay source).
    rest: Vec<Bar>,
    pub ind: ChartIndicators,
    pub live: LiveInd,
    /// Visible bar-index range [start, end).
    pub start: f64,
    pub end: f64,
    pub w: u32,
    pub h: u32,
    pub appended: u64,
    rng_state: u64,
}

impl Chart {
    /// Preload the first `INITIAL_BARS` bars statically; the remainder of the
    /// generated day is queued for the streaming replay (`stream_batch`).
    /// Data generation itself is excluded from all benchmark timings.
    pub fn new(idx: usize) -> Self {
        let mut bars = datagen::generate(idx, 1);
        let rest = bars.split_off(datagen::INITIAL_BARS.min(bars.len()));
        let ind = compute_all_ta(&bars);
        let live = LiveInd::new(&bars);
        let n = bars.len() as f64;
        Self {
            idx,
            bars,
            rest,
            ind,
            live,
            start: 0.0,
            end: n,
            w: 0,
            h: 0,
            appended: 0,
            rng_state: 0x9E3779B97F4A7C15u64.wrapping_add(idx as u64),
        }
    }

    pub fn streaming_done(&self) -> bool {
        self.rest.is_empty()
    }

    /// Stream up to `n` REAL generated bars through the incremental indicator
    /// path (called every ~50 ms, so `n = rate * period`). Returns the number
    /// consumed. The volume profile is recomputed once per batch.
    pub fn stream_batch(&mut self, n: usize) -> usize {
        let take = n.min(self.rest.len());
        if take == 0 {
            return 0;
        }
        let prev_len = self.bars.len();
        for i in 0..take {
            let b = self.rest[i];
            self.bars.push(b);
            self.live.push(&b, &mut self.ind);
        }
        self.rest.drain(..take);
        self.ind.profile = indicators::volume_profile(&self.bars, 2000, 40);
        let n_len = self.bars.len() as f64;
        if self.end >= prev_len as f64 - 1.5 {
            self.end = n_len; // follow the right edge while catching up
        }
        take
    }

    fn rng_f64(&mut self) -> f64 {
        // xorshift64*
        let mut x = self.rng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng_state = x;
        (x.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Append a synthetic live 1s bar continuing from the last close.
    /// Only runs after the streaming replay is exhausted.
    pub fn push_live_bar(&mut self) {
        if !self.rest.is_empty() {
            return;
        }
        let prev = *self.bars.last().unwrap();
        let vol = 0.0004;
        let ret = (self.rng_f64() * 2.0 - 1.0) * vol;
        let open = prev.close;
        let close = open * (1.0 + ret);
        let high = open.max(close) + self.rng_f64() * vol * open;
        let low = (open.min(close) - self.rng_f64() * vol * open).max(0.01);
        let bar = Bar {
            ts: prev.ts + 1,
            open,
            high,
            low,
            close,
            volume: 500.0 + self.rng_f64() * 4500.0,
        };
        self.bars.push(bar);
        self.live.push(&bar, &mut self.ind);
        // Volume profile is lookback-binned; recompute (cheap: 2000 bars x 40 bins).
        self.ind.profile = indicators::volume_profile(&self.bars, 2000, 40);
        self.appended += 1;
        let n = self.bars.len() as f64;
        if self.end >= n - 1.5 {
            self.end = n; // follow the right edge during live updates
        }
    }

    pub fn resize(&mut self, w: u32, h: u32) -> bool {
        if w == self.w && h == self.h || w == 0 || h == 0 {
            return false;
        }
        self.w = w;
        self.h = h;
        true
    }

    pub fn pan_px(&mut self, dx_px: f64) {
        let span = self.end - self.start;
        let shift = dx_px / self.w.max(1) as f64 * span;
        let n = self.bars.len() as f64;
        let (mut s, mut e) = (self.start - shift, self.end - shift);
        if s < 0.0 {
            e -= s;
            s = 0.0;
        }
        if e > n {
            s -= e - n;
            e = n;
        }
        self.start = s.max(0.0);
        self.end = e.min(n).max(self.start + 8.0);
    }

    pub fn zoom_at(&mut self, x_px: f64, delta_y: f64) {
        let span = self.end - self.start;
        let anchor = self.start + span * (x_px / self.w.max(1) as f64).clamp(0.0, 1.0);
        let factor = (-delta_y * 0.003).exp(); // wheel up -> zoom in
        let n = self.bars.len() as f64;
        let new_span = (span * factor).clamp(8.0, n);
        let frac = ((anchor - self.start) / span).clamp(0.0, 1.0);
        let mut s = anchor - new_span * frac;
        let mut e = s + new_span;
        if s < 0.0 {
            e -= s;
            s = 0.0;
        }
        if e > n {
            s -= e - n;
            e = n;
        }
        self.start = s.max(0.0);
        self.end = e.min(n);
    }

    pub fn stats_line(&self) -> String {
        let b = self.bars.last().unwrap();
        format!("O {:.2}  H {:.2}  L {:.2}  C {:.2}  V {:.0}", b.open, b.high, b.low, b.close, b.volume)
    }

    /// Rasterize into a fresh pixel buffer (Send — usable from worker threads).
    pub fn render_pixels(&self) -> SharedPixelBuffer<Rgba8Pixel> {
        let (w, h) = (self.w.max(8), self.h.max(8));
        let mut pb = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
        {
            let mut cv = Canvas::new(pb.make_mut_slice(), w as usize, h as usize);
            self.draw(&mut cv);
        }
        pb
    }

    pub fn render(&self) -> Image {
        Image::from_rgba8(self.render_pixels())
    }

    fn draw(&self, cv: &mut Canvas) {
        cv.fill(BG);
        let (w, h) = (cv.w as f64, cv.h as f64);
        let n = self.bars.len();
        let i0 = self.start.floor().max(0.0) as usize;
        let i1 = (self.end.ceil() as usize).min(n).max(i0 + 1) - 1;
        if i0 >= n || i1 <= i0 {
            return;
        }

        // Price range of visible bars.
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for b in &self.bars[i0..=i1] {
            lo = lo.min(b.low);
            hi = hi.max(b.high);
        }
        if !lo.is_finite() || hi <= lo {
            return;
        }
        let pad = (hi - lo) * 0.04;
        lo -= pad;
        hi += pad;
        let py = |p: f64| -> f64 { (hi - p) / (hi - lo) * (h - 1.0) };
        let span = self.end - self.start;
        let px_of = |i: usize| -> f64 { (i as f64 - self.start) / span * w };

        // Horizontal grid lines.
        for g in 1..6 {
            let y = (h * g as f64 / 6.0) as i32;
            cv.hline(0, w as i32 - 1, y, GRID);
        }

        // Candles: aggregate per pixel column when dense, per-bar when zoomed in.
        let bars_per_px = span / w;
        let visible = i1 - i0 + 1;
        if bars_per_px > 2.0 {
            for x in 0..(w as i32) {
                let a = i0 + (x as f64 * visible as f64 / w).floor() as usize;
                let b = i0 + (((x + 1) as f64 * visible as f64 / w).floor() as usize).max(1) - 1;
                let b = b.min(i1).max(a);
                let slice = &self.bars[a.min(i1)..=b];
                let mut clo = f64::INFINITY;
                let mut chi = 0.0f64;
                for bar in slice {
                    clo = clo.min(bar.low);
                    chi = chi.max(bar.high);
                }
                let o = slice[0].open;
                let c = slice[slice.len() - 1].close;
                let col = if c >= o { UP } else { DOWN };
                cv.vline(x, py(chi) as i32, py(clo) as i32, col);
                cv.vline(x, py(o.max(c)) as i32, py(o.min(c)) as i32, col);
            }
        } else {
            let cw = ((w / span) * 0.7).max(1.0);
            let half = (cw / 2.0).max(0.5);
            for i in i0..=i1 {
                let b = &self.bars[i];
                let x = px_of(i) + w / span * 0.5;
                let col = if b.close >= b.open { UP } else { DOWN };
                cv.vline(x.round() as i32, py(b.high) as i32, py(b.low) as i32, col);
                let (top, bot) = (py(b.open.max(b.close)), py(b.open.min(b.close)));
                cv.fill_rect((x - half) as i32, top as i32, (x + half).ceil() as i32, (bot as i32).max(top as i32 + 1), col);
            }
        }

        // Indicator polylines.
        let stride = (visible / (cv.w * 3)).max(1);
        let draw_series = |cv: &mut Canvas, series: &[f64], color: Rgb| {
            let mut prev: Option<(f64, f64)> = None;
            let mut i = i0;
            while i <= i1 {
                let v = series[i];
                if v.is_finite() {
                    let p = (px_of(i), py(v));
                    if let Some(q) = prev {
                        cv.line(q.0, q.1, p.0, p.1, color);
                    }
                    prev = Some(p);
                } else {
                    prev = None;
                }
                i += stride;
            }
            // Land on the last visible bar for continuity.
            if i1 % stride != i0 % stride {
                if let Some(q) = prev {
                    let v = series[i1];
                    if v.is_finite() {
                        cv.line(q.0, q.1, px_of(i1), py(v), color);
                    }
                }
            }
        };
        draw_series(cv, &self.ind.sma200, SMA200);
        draw_series(cv, &self.ind.ema50, EMA50);
        draw_series(cv, &self.ind.ema20, EMA20);
        draw_series(cv, &self.ind.ema10, EMA10);
        draw_series(cv, &self.ind.vwap, VWAP);

        // Horizontal volume profile: 40 bins, right edge, independent x-scale, 50% alpha.
        let prof = &self.ind.profile;
        if !prof.is_empty() {
            let maxv = prof.iter().map(|b| b.volume).fold(0.0f64, f64::max);
            if maxv > 0.0 {
                let region = (w * 0.22).min(150.0);
                for bin in prof {
                    let y0 = py(bin.price_high);
                    let y1 = py(bin.price_low);
                    let len = bin.volume / maxv * region;
                    cv.fill_rect_alpha((w - len) as i32, y0 as i32, w as i32, (y1 as i32).max(y0 as i32 + 1), PROFILE, 128);
                }
            }
        }
    }
}
