//! Chart geometry definitions: independent of rendering backend.
//!
//! Separates chart data layout and aggregation from rasterization.
//! Emits command batches that can be rendered by CPU (pixel buffer) or
//! GPU (triangle mesh) without architectural changes to the geometry.

use crate::chart::Chart;
use crate::raster::Rgb;

/// A single rendered primitive with a color.
#[derive(Clone, Debug)]
pub enum Prim {
    /// Filled axis-aligned rectangle [x0,x1) x [y0,y1)
    Rect { x0: f64, y0: f64, x1: f64, y1: f64, col: Rgb, alpha: u8 },
    /// Thin line segment (1 px) via DDA, or as a thin quad on GPU
    Seg { x0: f64, y0: f64, x1: f64, y1: f64, col: Rgb },
    /// Thin horizontal line
    HLine { x0: f64, x1: f64, y: i32, col: Rgb },
    /// Thin vertical line
    VLine { x: i32, y0: i32, y1: i32, col: Rgb },
}

impl Prim {
    pub fn col(&self) -> Rgb {
        match self {
            Prim::Rect { col, .. } | Prim::Seg { col, .. } | Prim::HLine { col, .. } | Prim::VLine { col, .. } => *col,
        }
    }
}

/// Immutable geometry emit context: view parameters + emitter for primitives.
pub struct GeomCtx {
    pub w: f64,
    pub h: f64,
    pub lo: f64,
    pub hi: f64,
    pub start: f64,
    pub end: f64,
    pub i0: usize,
    pub i1: usize,
}

impl GeomCtx {
    /// Pixel Y-coordinate for a price (top-left origin, inverted Y)
    pub fn py(&self, price: f64) -> f64 {
        (self.hi - price) / (self.hi - self.lo) * (self.h - 1.0)
    }

    /// Pixel X-coordinate for a bar index
    pub fn px_of(&self, i: usize) -> f64 {
        let span = self.end - self.start;
        (i as f64 - self.start) / span * self.w
    }

    /// Extract from Chart view state (internal use)
    pub fn from_chart(chart: &Chart) -> Option<Self> {
        let Some(m) = chart.view_meta() else { return None };
        Some(Self {
            w: chart.w.max(8) as f64,
            h: chart.h.max(8) as f64,
            lo: m.lo,
            hi: m.hi,
            start: m.start,
            end: m.end,
            i0: m.i0,
            i1: m.i1,
        })
    }
}

/// Emit chart geometry as a list of primitives.
/// Same aggregation logic as the CPU rasterizer.
pub fn emit_primitives(chart: &Chart) -> Vec<Prim> {
    let Some(ctx) = GeomCtx::from_chart(chart) else { return Vec::new() };

    let mut prims = Vec::new();

    // Grid lines
    for k in 1..6 {
        let y = (ctx.h * k as f64 / 6.0).round();
        prims.push(Prim::HLine { x0: 0.0, x1: ctx.w, y: y as i32, col: crate::raster::GRID });
    }

    // Candles
    let bars_per_px = (ctx.end - ctx.start) / ctx.w;
    let visible = ctx.i1 - ctx.i0 + 1;
    if bars_per_px > 2.0 {
        // Aggregated mode: multiple bars per pixel
        for x in 0..ctx.w as usize {
            let a = ctx.i0 + (x as f64 * visible as f64 / ctx.w).floor() as usize;
            let b = ctx.i0 + (((x + 1) as f64 * visible as f64 / ctx.w).floor() as usize).max(1) - 1;
            let b = b.min(ctx.i1).max(a);
            let slice = &chart.bars[a.min(ctx.i1)..=b];
            let mut clo = f64::INFINITY;
            let mut chi = 0.0f64;
            for bar in slice {
                clo = clo.min(bar.low);
                chi = chi.max(bar.high);
            }
            let o = slice[0].open;
            let c = slice[slice.len() - 1].close;
            let col = if c >= o { crate::raster::UP } else { crate::raster::DOWN };
            let (x0, x1) = (x as f64, x as f64 + 1.0);
            prims.push(Prim::Rect { x0, y0: ctx.py(chi), x1, y1: ctx.py(clo), col, alpha: 255 });
            prims.push(Prim::Rect { x0, y0: ctx.py(o.max(c)), x1, y1: ctx.py(o.min(c)), col, alpha: 255 });
        }
    } else {
        // Detail mode: individual candlesticks
        let cw = ((ctx.w / (ctx.end - ctx.start)) * 0.7).max(1.0);
        let half = (cw / 2.0).max(0.5);
        for i in ctx.i0..=ctx.i1 {
            let b = &chart.bars[i];
            let x = ctx.px_of(i) + (ctx.w / (ctx.end - ctx.start)) * 0.5;
            let col = if b.close >= b.open { crate::raster::UP } else { crate::raster::DOWN };
            let wx = x.round();
            prims.push(Prim::Rect { x0: wx, y0: ctx.py(b.high), x1: wx + 1.0, y1: ctx.py(b.low), col, alpha: 255 });
            let (top, bot) = (ctx.py(b.open.max(b.close)), ctx.py(b.open.min(b.close)));
            prims.push(Prim::Rect { x0: (x - half).floor(), y0: top, x1: (x + half).ceil(), y1: bot.max(top + 1.0), col, alpha: 255 });
        }
    }

    // Indicator polylines
    let stride = (visible / (ctx.w as usize * 3)).max(1).next_power_of_two();

    let emit_series = |prims: &mut Vec<Prim>, series: &[f64], color: Rgb| {
        let pt = |i: usize| -> Option<(f64, f64)> {
            let v = series[i];
            if v.is_finite() { Some((ctx.px_of(i), ctx.py(v))) } else { None }
        };
        let mut i = ctx.i0 / stride * stride;
        let mut prev = pt(i);
        while i < ctx.i1 {
            let j = (i + stride).min(ctx.i1);
            let cur = pt(j);
            if let (Some(a), Some(b)) = (prev, cur) {
                prims.push(Prim::Seg { x0: a.0, y0: a.1, x1: b.0, y1: b.1, col: color });
            }
            prev = cur;
            i = j;
        }
    };

    emit_series(&mut prims, &chart.ind.sma200, crate::raster::SMA200);
    emit_series(&mut prims, &chart.ind.ema50, crate::raster::EMA50);
    emit_series(&mut prims, &chart.ind.ema20, crate::raster::EMA20);
    emit_series(&mut prims, &chart.ind.ema10, crate::raster::EMA10);
    emit_series(&mut prims, &chart.ind.vwap, crate::raster::VWAP);

    // Volume profile
    let prof = &chart.ind.profile;
    if !prof.is_empty() {
        let maxv = prof.iter().map(|b| b.volume).fold(0.0f64, f64::max);
        if maxv > 0.0 {
            let region = (ctx.w * 0.22).min(150.0);
            for bin in prof {
                let y0 = ctx.py(bin.price_high);
                let y1 = ctx.py(bin.price_low);
                let len = bin.volume / maxv * region;
                prims.push(Prim::Rect { x0: ctx.w - len, y0, x1: ctx.w, y1: y1.max(y0 + 1.0), col: crate::raster::PROFILE, alpha: 128 });
            }
        }
    }

    prims
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_rectangles_are_translucent_and_candles_are_opaque() {
        let mut chart = Chart::new(0);
        chart.resize(400, 200);
        let prims = emit_primitives(&chart);
        let mut profile_count = 0;
        let mut candle_count = 0;
        for prim in prims {
            if let Prim::Rect { col, alpha, .. } = prim {
                if matches!(col, crate::raster::Rgb(30, 136, 229)) {
                    profile_count += 1;
                    assert_eq!(alpha, 128);
                } else {
                    candle_count += 1;
                    assert_eq!(alpha, 255);
                }
            }
        }
        assert!(profile_count > 0);
        assert!(candle_count > 0);
    }
}
