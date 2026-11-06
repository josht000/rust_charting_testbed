//! Chart model + rendering (egui::Painter path; wgpu path lives in `gpu`).

use datagen::Bar;
use egui::{Color32, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui};
use indicators::ChartIndicators;
use ta::indicators::{ExponentialMovingAverage, SimpleMovingAverage};
use ta::Next;

pub const COLOR_UP: Color32 = Color32::from_rgb(38, 166, 154);
pub const COLOR_DOWN: Color32 = Color32::from_rgb(239, 83, 80);
pub const COLOR_EMA10: Color32 = Color32::WHITE;
pub const COLOR_EMA20: Color32 = Color32::from_rgb(255, 152, 0); // orange
pub const COLOR_EMA50: Color32 = Color32::from_rgb(0, 188, 212); // cyan
pub const COLOR_SMA200: Color32 = Color32::YELLOW;
pub const COLOR_VWAP: Color32 = Color32::from_rgb(156, 39, 176); // purple
pub const COLOR_PROFILE: Color32 = Color32::from_rgba_unmultiplied_const(30, 100, 255, 128);
pub const COLOR_GRID: Color32 = Color32::from_rgb(45, 45, 55);
pub const COLOR_TEXT: Color32 = Color32::from_rgb(160, 160, 170);
pub const COLOR_BG: Color32 = Color32::from_rgb(18, 18, 24);

pub const LINE_SERIES: &[(usize, Color32)] = &[
    (0, COLOR_EMA10),
    (1, COLOR_EMA20),
    (2, COLOR_EMA50),
    (3, COLOR_SMA200),
    (4, COLOR_VWAP),
];

/// Compute all 6 indicators. EMA/SMA via the `ta` crate ("rust talib"),
/// VWAP + volume profile via shared/indicators (not in TA-Lib).
pub fn compute_indicators(bars: &[Bar]) -> ChartIndicators {
    let n = bars.len();
    let mut e10 = ExponentialMovingAverage::new(10).unwrap();
    let mut e20 = ExponentialMovingAverage::new(20).unwrap();
    let mut e50 = ExponentialMovingAverage::new(50).unwrap();
    let mut s200 = SimpleMovingAverage::new(200).unwrap();
    let mut ema10 = Vec::with_capacity(n);
    let mut ema20 = Vec::with_capacity(n);
    let mut ema50 = Vec::with_capacity(n);
    let mut sma200 = Vec::with_capacity(n);
    for b in bars {
        ema10.push(e10.next(b.close));
        ema20.push(e20.next(b.close));
        ema50.push(e50.next(b.close));
        sma200.push(s200.next(b.close));
    }
    // `ta` EMA seeds with the first value; TA-Lib convention leaves the first
    // period-1 outputs undefined. NaN-pad so rendering/tests line up with the
    // shared/indicators reference.
    for v in ema10.iter_mut().take(9) {
        *v = f64::NAN;
    }
    for v in ema20.iter_mut().take(19) {
        *v = f64::NAN;
    }
    for v in ema50.iter_mut().take(49) {
        *v = f64::NAN;
    }
    for v in sma200.iter_mut().take(199) {
        *v = f64::NAN;
    }
    ChartIndicators {
        ema10,
        ema20,
        ema50,
        sma200,
        vwap: indicators::vwap_daily(bars),
        profile: indicators::volume_profile(bars, 2000, 40),
    }
}

/// O(1) per-bar incremental indicator update for live ticks.
pub struct LiveInd {
    sma_sum: f64,
    vwap_day: i64,
    vwap_pv: f64,
    vwap_vv: f64,
}

impl LiveInd {
    pub fn new(bars: &[Bar]) -> Self {
        let n = bars.len();
        let sma_sum: f64 = bars[n.saturating_sub(200)..].iter().map(|b| b.close).sum();
        let last_day = bars[n - 1].ts / 86_400;
        let mut pv = 0.0;
        let mut vv = 0.0;
        for b in bars.iter().rev() {
            if b.ts / 86_400 != last_day {
                break;
            }
            pv += (b.high + b.low + b.close) / 3.0 * b.volume;
            vv += b.volume;
        }
        Self {
            sma_sum,
            vwap_day: last_day,
            vwap_pv: pv,
            vwap_vv: vv,
        }
    }

    pub fn push(&mut self, bars: &[Bar], ind: &mut ChartIndicators, bar: Bar) {
        let n = bars.len();
        let c = bar.close;
        let k = |p: usize| 2.0 / (p as f64 + 1.0);
        let step_ema = |v: &mut Vec<f64>, p: usize| {
            let prev = v[n - 1];
            v.push(if prev.is_nan() { c } else { c * k(p) + prev * (1.0 - k(p)) });
        };
        step_ema(&mut ind.ema10, 10);
        step_ema(&mut ind.ema20, 20);
        step_ema(&mut ind.ema50, 50);

        if n >= 200 {
            self.sma_sum += c - bars[n - 200].close;
        } else {
            self.sma_sum += c;
        }
        ind.sma200.push(if n + 1 >= 200 {
            self.sma_sum / 200.0
        } else {
            f64::NAN
        });

        let day = bar.ts / 86_400;
        if day != self.vwap_day {
            self.vwap_day = day;
            self.vwap_pv = 0.0;
            self.vwap_vv = 0.0;
        }
        let tp = (bar.high + bar.low + bar.close) / 3.0;
        self.vwap_pv += tp * bar.volume;
        self.vwap_vv += bar.volume;
        ind.vwap.push(if self.vwap_vv > 0.0 {
            self.vwap_pv / self.vwap_vv
        } else {
            tp
        });
    }
}

#[derive(Clone, Copy)]
pub struct View {
    pub start: f64,
    pub end: f64,
}

impl View {
    pub fn all(len: usize) -> Self {
        Self {
            start: 0.0,
            end: len as f64,
        }
    }
    pub fn span(&self) -> f64 {
        self.end - self.start
    }
    pub fn clamp(&mut self, len: usize) {
        let min_span = 16.0;
        let len = len as f64;
        if self.span() < min_span {
            let mid = 0.5 * (self.start + self.end);
            self.start = mid - min_span / 2.0;
            self.end = mid + min_span / 2.0;
        }
        if self.span() > len * 1.2 {
            self.start = 0.0;
            self.end = len;
        }
        if self.start < 0.0 {
            self.end -= self.start;
            self.start = 0.0;
        }
        if self.end > len {
            let d = self.end - len;
            self.end = len;
            self.start = (self.start - d).max(0.0);
        }
    }
    pub fn pan_bars(&mut self, dx_bars: f64, len: usize) {
        self.start += dx_bars;
        self.end += dx_bars;
        self.clamp(len);
    }
    pub fn zoom(&mut self, factor: f64, at_frac: f64, len: usize) {
        let at = self.start + at_frac.clamp(0.0, 1.0) * self.span();
        self.start = at - (at - self.start) * factor;
        self.end = at + (self.end - at) * factor;
        self.clamp(len);
    }
}

/// One pixel column of the aggregated candle view.
#[derive(Clone, Copy)]
pub struct Col {
    pub x0: f32,
    pub x1: f32,
    pub i_last: usize, // index of last bar in this column
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

pub struct Chart {
    pub idx: usize,
    /// Full generated day; only `..streamed` has "arrived" yet (streaming
    /// data model: INITIAL_BARS preloaded, the rest replays in batches).
    pub bars: Vec<Bar>,
    pub streamed: usize,
    pub ind: ChartIndicators,
    pub live: LiveInd,
    pub view: View,
}

impl Chart {
    pub fn new(idx: usize) -> Self {
        let bars = datagen::generate(idx, 1);
        let streamed = datagen::INITIAL_BARS.min(bars.len());
        let ind = compute_indicators(&bars[..streamed]);
        let live = LiveInd::new(&bars[..streamed]);
        let view = View::all(streamed);
        Self {
            idx,
            bars,
            streamed,
            ind,
            live,
            view,
        }
    }

    /// Bars that have actually arrived (preload + streamed so far).
    pub fn arrived(&self) -> &[Bar] {
        &self.bars[..self.streamed]
    }

    pub fn is_streaming(&self) -> bool {
        self.streamed < self.bars.len()
    }

    /// Stream `n` more real bars from the generated replay. Returns the
    /// number actually appended. Indicator updates are O(1) per bar;
    /// the volume profile refreshes once per batch.
    pub fn push_streamed(&mut self, n: usize) -> usize {
        let k = n.min(self.bars.len() - self.streamed);
        if k == 0 {
            return 0;
        }
        let following = self.view.end >= self.streamed as f64 - 1.0;
        for _ in 0..k {
            let bar = self.bars[self.streamed];
            self.live
                .push(&self.bars[..self.streamed], &mut self.ind, bar);
            self.streamed += 1;
        }
        // While catching up, keep the view pinned to all arrived data so the
        // chart visibly fills out.
        if following {
            self.view.end += k as f64;
        }
        self.view.clamp(self.streamed);
        self.ind.profile = indicators::volume_profile(self.arrived(), 2000, 40);
        k
    }

    /// Append one synthetic live bar (post-catch-up 1Hz / bench phases).
    /// Only meaningful once the replay stream is exhausted.
    pub fn push_bar(&mut self, bar: Bar) {
        debug_assert!(!self.is_streaming());
        self.live
            .push(&self.bars[..self.streamed], &mut self.ind, bar);
        self.bars.push(bar);
        self.streamed += 1;
        // Slide the window to follow the new bar if the user hasn't panned.
        let len = self.streamed as f64;
        if (self.view.end - (len - 1.0)).abs() < 2.0 {
            self.view.start += 1.0;
            self.view.end += 1.0;
        }
        self.view.clamp(self.streamed);
        // Profile over last 2000 bars — recompute per push (a ~2k-bar scan is
        // trivial even at 800 pushes/s across all charts).
        self.ind.profile = indicators::volume_profile(self.arrived(), 2000, 40);
    }

    /// Visible price range over the current view.
    pub fn price_range(&self) -> (f64, f64) {
        let len = self.streamed;
        let i0 = self.view.start.max(0.0) as usize;
        let i1 = (self.view.end.ceil() as usize).min(len);
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for b in &self.arrived()[i0.min(i1)..i1] {
            lo = lo.min(b.low);
            hi = hi.max(b.high);
        }
        if !lo.is_finite() || !hi.is_finite() {
            (0.0, 1.0)
        } else if hi - lo < 1e-9 {
            (lo - 1.0, hi + 1.0)
        } else {
            let pad = (hi - lo) * 0.05;
            (lo - pad, hi + pad)
        }
    }

    /// Visible bar index range `(i0, i1)` and bars-per-pixel for the view.
    /// `bpp < 1.0` means zoomed in past one bar per pixel — render per bar,
    /// not per pixel column.
    pub fn visible_range(&self, plot: Rect) -> (usize, usize, f64) {
        let len = self.streamed;
        let bpp = self.view.span() / plot.width().max(1.0) as f64;
        let i0 = (self.view.start.floor().max(0.0) as usize).min(len);
        let i1 = ((self.view.end.ceil() as usize).min(len)).max(i0);
        (i0, i1, bpp)
    }

    /// Screen x of bar `i`'s center given bars-per-pixel `bpp`.
    pub fn bar_x(&self, i: usize, plot: Rect, bpp: f64) -> f32 {
        plot.left() + ((i as f64 + 0.5 - self.view.start) / bpp) as f32
    }

    /// Aggregate visible bars into one candle per pixel column.
    pub fn columns(&self, plot: Rect) -> Vec<Col> {
        let len = self.streamed;
        let w = plot.width().max(1.0) as usize;
        let span = self.view.span();
        let bpp = span / w as f64;
        let mut cols = Vec::with_capacity(w);
        for j in 0..w {
            let f0 = self.view.start + j as f64 * bpp;
            let f1 = self.view.start + (j + 1) as f64 * bpp;
            let i0 = (f0.floor() as usize).min(len);
            let mut i1 = (f1.ceil() as usize).min(len);
            if i1 <= i0 {
                i1 = (i0 + 1).min(len);
            }
            if i0 >= i1 {
                continue;
            }
            let slice = &self.arrived()[i0..i1];
            let mut high = f64::NEG_INFINITY;
            let mut low = f64::INFINITY;
            for b in slice {
                high = high.max(b.high);
                low = low.min(b.low);
            }
            let x0 = plot.left() + j as f32;
            cols.push(Col {
                x0,
                x1: plot.left() + (j + 1) as f32,
                i_last: i1 - 1,
                open: slice[0].open,
                high,
                low,
                close: slice[slice.len() - 1].close,
            });
        }
        cols
    }

    /// Render the chart. Returns true once something was painted.
    /// `gpu` = use the wgpu paint callback instead of Painter shapes.
    pub fn render(&mut self, ui: &mut Ui, gpu: bool) -> bool {
        let avail = ui.available_rect_before_wrap();
        let (resp, painter) =
            ui.allocate_painter(avail.size(), Sense::click_and_drag());
        let rect = resp.rect;
        // Reserve right (price labels) and bottom (time labels) margins.
        let plot = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.max.x - 62.0, rect.max.y - 18.0),
        );
        if plot.width() < 20.0 || plot.height() < 20.0 {
            return false;
        }

        // --- interactions: drag to pan, wheel to zoom ---
        if resp.dragged() {
            let dx = resp.drag_delta().x;
            let span = self.view.span();
            self.view
                .pan_bars(-dx as f64 * span / plot.width() as f64, self.streamed);
        }
        if let Some(hover) = resp.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.01 {
                let factor = (-scroll as f64 / 300.0).exp();
                let frac = (hover.x - plot.left()) / plot.width();
                self.view.zoom(factor, frac as f64, self.streamed);
            }
        }

        let (lo, hi) = self.price_range();
        painter.rect_filled(rect, 0.0, COLOR_BG);

        // --- grid + price labels ---
        let font = FontId::monospace(10.0);
        for k in 0..=5 {
            let y = plot.top() + plot.height() * k as f32 / 5.0;
            painter.hline(plot.left()..=plot.right(), y, Stroke::new(0.5, COLOR_GRID));
            let price = hi - (hi - lo) * k as f64 / 5.0;
            painter.text(
                Pos2::new(rect.max.x - 60.0, y - 6.0),
                egui::Align2::LEFT_TOP,
                format!("{price:.2}"),
                font.clone(),
                COLOR_TEXT,
            );
        }
        for k in 0..=4 {
            let x = plot.left() + plot.width() * k as f32 / 4.0;
            painter.vline(x, plot.top()..=plot.bottom(), Stroke::new(0.5, COLOR_GRID));
            let i = (self.view.start + self.view.span() * k as f64 / 4.0) as usize;
            if let Some(b) = self.arrived().get(i.min(self.streamed.saturating_sub(1))) {
                let secs = b.ts % 86_400;
                painter.text(
                    Pos2::new(x, plot.bottom() + 2.0),
                    egui::Align2::CENTER_TOP,
                    format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60),
                    font.clone(),
                    COLOR_TEXT,
                );
            }
        }

        let cols = self.columns(plot);
        if cols.is_empty() {
            return false;
        }

        if gpu {
            let verts = crate::gpu::build_verts(self, plot, &cols, lo, hi);
            painter.add(crate::gpu::callback(self.idx, verts, plot));
        } else {
            paint_shapes(&painter, self, plot, &cols, lo, hi);
        }
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, Color32::from_rgb(60, 60, 70)),
            egui::StrokeKind::Inside,
        );
        true
    }
}

fn y_of(plot: Rect, lo: f64, hi: f64, v: f64) -> f32 {
    plot.bottom() - ((v - lo) / (hi - lo)) as f32 * plot.height()
}

/// Pure-egui fallback renderer: same aggregation, Painter shapes.
fn paint_shapes(p: &Painter, chart: &Chart, plot: Rect, cols: &[Col], lo: f64, hi: f64) {
    let mut shapes: Vec<Shape> = Vec::with_capacity(cols.len() * 3 + 64);

    // volume profile (behind everything else, independent x scale, right edge)
    let max_vol = chart
        .ind
        .profile
        .iter()
        .map(|b| b.volume)
        .fold(0.0f64, f64::max)
        .max(1e-9);
    let prof_w = plot.width() * 0.22;
    for b in &chart.ind.profile {
        let y0 = y_of(plot, lo, hi, b.price_high);
        let y1 = y_of(plot, lo, hi, b.price_low);
        let w = (b.volume / max_vol) as f32 * prof_w;
        if y1 > y0 && y1 > plot.top() && y0 < plot.bottom() {
            shapes.push(Shape::rect_filled(
                Rect::from_min_max(
                    Pos2::new(plot.right() - w, y0.max(plot.top())),
                    Pos2::new(plot.right(), y1.min(plot.bottom())),
                ),
                0.0,
                COLOR_PROFILE,
            ));
        }
    }

    let (bi0, bi1, bpp) = chart.visible_range(plot);

    // candles: one per bar when zoomed in (bpp < 1), else one per column
    if bpp < 1.0 {
        let bw = ((1.0 / bpp) as f32 * 0.7).max(1.0);
        for i in bi0..bi1 {
            let bar = &chart.arrived()[i];
            let col = if bar.close >= bar.open {
                COLOR_UP
            } else {
                COLOR_DOWN
            };
            let xc = chart.bar_x(i, plot, bpp);
            shapes.push(Shape::line_segment(
                [
                    Pos2::new(xc, y_of(plot, lo, hi, bar.high)),
                    Pos2::new(xc, y_of(plot, lo, hi, bar.low)),
                ],
                Stroke::new(1.5, col),
            ));
            let yo = y_of(plot, lo, hi, bar.open);
            let yc = y_of(plot, lo, hi, bar.close);
            let (t, b) = if yo <= yc { (yo, yc) } else { (yc, yo) };
            shapes.push(Shape::rect_filled(
                Rect::from_min_max(
                    Pos2::new(xc - bw / 2.0, t),
                    Pos2::new(xc + bw / 2.0, (b - t).max(1.0) + t),
                ),
                0.0,
                col,
            ));
        }
    } else {
        for c in cols {
            let col = if c.close >= c.open { COLOR_UP } else { COLOR_DOWN };
            let xc = 0.5 * (c.x0 + c.x1);
            let body_w = ((c.x1 - c.x0) * 0.7).max(1.0);
            let yh = y_of(plot, lo, hi, c.high);
            let yl = y_of(plot, lo, hi, c.low);
            let yo = y_of(plot, lo, hi, c.open);
            let yc = y_of(plot, lo, hi, c.close);
            shapes.push(Shape::line_segment(
                [Pos2::new(xc, yh), Pos2::new(xc, yl)],
                Stroke::new(1.0, col),
            ));
            let (t, b) = if yo <= yc { (yo, yc) } else { (yc, yo) };
            shapes.push(Shape::rect_filled(
                Rect::from_min_max(
                    Pos2::new(xc - body_w / 2.0, t),
                    Pos2::new(xc + body_w / 2.0, (b - t).max(1.0) + t),
                ),
                0.0,
                col,
            ));
        }
    }

    // indicator polylines: per-bar points when zoomed in, else sampled per
    // column at its last bar (per-column sampling repeats the same bar's
    // value across many columns when zoomed in → staircase steps)
    let series = [
        &chart.ind.ema10,
        &chart.ind.ema20,
        &chart.ind.ema50,
        &chart.ind.sma200,
        &chart.ind.vwap,
    ];
    for &(si, color) in LINE_SERIES {
        let s = series[si];
        let mut pts: Vec<Pos2> = Vec::with_capacity(cols.len());
        let mut segs: Vec<Shape> = Vec::new();
        let emit = |x: f32, v: f64, pts: &mut Vec<Pos2>, segs: &mut Vec<Shape>| {
            if v.is_nan() {
                if pts.len() > 1 {
                    segs.push(Shape::line(std::mem::take(pts), Stroke::new(1.0, color)));
                } else {
                    pts.clear();
                }
            } else {
                pts.push(Pos2::new(x, y_of(plot, lo, hi, v)));
            }
        };
        if bpp < 1.0 {
            for i in bi0..bi1 {
                emit(chart.bar_x(i, plot, bpp), s[i], &mut pts, &mut segs);
            }
        } else {
            for c in cols {
                emit(0.5 * (c.x0 + c.x1), s[c.i_last], &mut pts, &mut segs);
            }
        }
        if pts.len() > 1 {
            segs.push(Shape::line(pts, Stroke::new(1.0, color)));
        }
        shapes.extend(segs);
    }

    p.extend(shapes);
}

/// Deterministic tiny RNG for scripted live updates.
pub struct Lcg(pub u64);
impl Lcg {
    pub fn f64(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Append `n` synthetic bars continuing the random walk after `prev`.
    pub fn make_bars(&mut self, prev: &Bar, n: usize) -> Vec<Bar> {
        let mut out = Vec::with_capacity(n);
        let mut price = prev.close;
        for i in 0..n {
            let ts = prev.ts + 1 + i as i64;
            let open = price;
            let close = open * (1.0 + (self.f64() - 0.5) * 0.002);
            let high = open.max(close) + self.f64() * open * 0.0004;
            let low = (open.min(close) - self.f64() * open * 0.0004).max(0.01);
            let volume = 500.0 + self.f64() * 4500.0;
            out.push(Bar {
                ts,
                open,
                high,
                low,
                close,
                volume,
            });
            price = close;
        }
        out
    }
}

/// Replay driver: feeds the unstreamed tail of each chart's generated day
/// into the incremental indicator path in ~75 ms batches at `rate` bars/sec
/// per chart.
pub struct Streamer {
    /// Requested bars/sec *per chart*.
    pub rate: f64,
    pending: f64,
    pub active: bool,
    t0: web_time::Instant,
    /// Total bars pushed across all charts.
    pub pushed: usize,
    /// Stream start → all charts done (set when `active` flips false).
    pub catchup_ms: f64,
    /// Frame intervals recorded while the stream is active (bench stats).
    pub times: Vec<f64>,
}

impl Streamer {
    pub fn new(rate: f64) -> Self {
        Self {
            rate: rate.max(1.0),
            pending: 0.0,
            active: true,
            t0: web_time::Instant::now(),
            pushed: 0,
            catchup_ms: 0.0,
            times: Vec::with_capacity(4096),
        }
    }

    /// Call every frame. `dt` is the frame interval in seconds.
    pub fn tick(&mut self, charts: &mut [Chart], dt: f64) {
        if !self.active {
            return;
        }
        self.times.push(dt);
        self.pending += self.rate * dt;
        // Batch deliveries ~75 ms apart (spec: 50-100 ms batches).
        let batch = (self.rate * 0.075).ceil().max(1.0) as usize;
        if (self.pending as usize) < batch {
            return;
        }
        // Bound per-frame work if a frame stalls badly.
        let n = (self.pending as usize).min(batch * 8);
        self.pending -= n as f64;
        let mut done = true;
        for c in charts.iter_mut() {
            self.pushed += c.push_streamed(n);
            if c.is_streaming() {
                done = false;
            }
        }
        if done {
            self.active = false;
            self.catchup_ms = self.t0.elapsed().as_secs_f64() * 1000.0;
            log::info!(
                "stream catch-up done: {} bars in {:.0} ms ({:.0} bars/s)",
                self.pushed,
                self.catchup_ms,
                self.achieved_rate()
            );
        }
    }

    /// Measured achieved rate (bars/sec across all charts).
    pub fn achieved_rate(&self) -> f64 {
        self.pushed as f64 / (self.catchup_ms / 1000.0).max(1e-9)
    }
}
