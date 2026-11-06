//! Tiny software rasterizer drawing directly into a `SharedPixelBuffer<Rgba8Pixel>`.
//! Enough for candlesticks, polylines and the alpha-blended volume profile.

use slint::Rgba8Pixel;

#[derive(Clone, Copy)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    const fn px(self) -> Rgba8Pixel {
        Rgba8Pixel { r: self.0, g: self.1, b: self.2, a: 255 }
    }
}

pub const BG: Rgb = Rgb(15, 17, 21);
pub const GRID: Rgb = Rgb(34, 38, 46);
pub const UP: Rgb = Rgb(38, 166, 154);
pub const DOWN: Rgb = Rgb(239, 83, 80);
pub const EMA10: Rgb = Rgb(235, 235, 235);
pub const EMA20: Rgb = Rgb(255, 152, 0);
pub const EMA50: Rgb = Rgb(0, 188, 212);
pub const SMA200: Rgb = Rgb(255, 235, 59);
pub const VWAP: Rgb = Rgb(171, 71, 188);
pub const PROFILE: Rgb = Rgb(30, 136, 229);

pub struct Canvas<'a> {
    pub buf: &'a mut [Rgba8Pixel],
    pub w: usize,
    pub h: usize,
    /// Horizontal clip [cx0, cx1): writes outside this column range are dropped.
    /// Used for incremental strip repaints after a pixel-exact pan blit.
    pub cx0: i32,
    pub cx1: i32,
}

impl<'a> Canvas<'a> {
    pub fn new(buf: &'a mut [Rgba8Pixel], w: usize, h: usize) -> Self {
        Self { buf, w, h, cx0: 0, cx1: w as i32 }
    }

    /// Restrict drawing to columns [cx0, cx1) of the buffer.
    pub fn with_clip(mut self, cx0: i32, cx1: i32) -> Self {
        self.cx0 = cx0.max(0);
        self.cx1 = cx1.min(self.w as i32);
        self
    }

    pub fn fill(&mut self, c: Rgb) {
        if self.cx0 <= 0 && self.cx1 >= self.w as i32 {
            self.buf.fill(c.px());
            return;
        }
        let px = c.px();
        for y in 0..self.h {
            let row = y * self.w;
            self.buf[row + self.cx0 as usize..row + self.cx1 as usize].fill(px);
        }
    }

    #[inline]
    pub fn put(&mut self, x: i32, y: i32, c: Rgb) {
        if x >= self.cx0 && x < self.cx1 && y >= 0 && (y as usize) < self.h {
            self.buf[y as usize * self.w + x as usize] = c.px();
        }
    }

    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, c: Rgb, a: u8) {
        if x >= self.cx0 && x < self.cx1 && y >= 0 && (y as usize) < self.h {
            let p = &mut self.buf[y as usize * self.w + x as usize];
            let a = a as u16;
            let ia = 255 - a;
            p.r = ((c.0 as u16 * a + p.r as u16 * ia) / 255) as u8;
            p.g = ((c.1 as u16 * a + p.g as u16 * ia) / 255) as u8;
            p.b = ((c.2 as u16 * a + p.b as u16 * ia) / 255) as u8;
        }
    }

    /// Horizontal line, x range is clamped.
    pub fn hline(&mut self, x0: i32, x1: i32, y: i32, c: Rgb) {
        if y < 0 || y as usize >= self.h {
            return;
        }
        let (a, b) = (x0.min(x1).max(self.cx0), x0.max(x1).min(self.cx1 - 1));
        if a > b {
            return;
        }
        let row = y as usize * self.w;
        let px = c.px();
        for p in &mut self.buf[row + a as usize..=row + b as usize] {
            *p = px;
        }
    }

    /// Vertical line, y range is clamped.
    pub fn vline(&mut self, x: i32, y0: i32, y1: i32, c: Rgb) {
        if x < self.cx0 || x >= self.cx1 {
            return;
        }
        let (a, b) = (y0.min(y1).max(0), y0.max(y1).min(self.h as i32 - 1));
        if a > b {
            return;
        }
        let px = c.px();
        for y in a..=b {
            self.buf[y as usize * self.w + x as usize] = px;
        }
    }

    /// Filled rect, [x0,x1) x [y0,y1), clipped.
    pub fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb) {
        let (xa, xb) = (x0.min(x1).max(self.cx0), x0.max(x1).min(self.cx1));
        let (ya, yb) = (y0.min(y1).max(0), y0.max(y1).min(self.h as i32));
        if xa >= xb || ya >= yb {
            return;
        }
        let px = c.px();
        for y in ya..yb {
            let row = y as usize * self.w;
            self.buf[row + xa as usize..row + xb as usize].fill(px);
        }
    }

    /// Filled rect with per-pixel alpha blending (0..=255).
    pub fn fill_rect_alpha(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb, a: u8) {
        let (xa, xb) = (x0.min(x1).max(self.cx0), x0.max(x1).min(self.cx1));
        let (ya, yb) = (y0.min(y1).max(0), y0.max(y1).min(self.h as i32));
        if xa >= xb {
            return;
        }
        let (a16, ia16) = (a as u16, 255 - a as u16);
        let (cr, cg, cb) = (c.0 as u16 * a16, c.1 as u16 * a16, c.2 as u16 * a16);
        for y in ya..yb {
            let row = y as usize * self.w;
            for p in &mut self.buf[row + xa as usize..row + xb as usize] {
                p.r = ((cr + p.r as u16 * ia16) / 255) as u8;
                p.g = ((cg + p.g as u16 * ia16) / 255) as u8;
                p.b = ((cb + p.b as u16 * ia16) / 255) as u8;
            }
        }
    }

    /// Straight line via DDA (1 px thick).
    pub fn line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, c: Rgb) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let steps = dx.abs().max(dy.abs());
        if steps < 1.0 {
            self.put(x0.round() as i32, y0.round() as i32, c);
            return;
        }
        let (mut x, mut y) = (x0, y0);
        let (sx, sy) = (dx / steps, dy / steps);
        // Fast reject for fully off-screen segments (small margins for rounding).
        let (w, h) = (self.w as f64, self.h as f64);
        if (x0 < 0.0 && x1 < 0.0) || (x0 >= w && x1 >= w) || (y0 < 0.0 && y1 < 0.0) || (y0 >= h && y1 >= h) {
            return;
        }
        for _ in 0..=(steps as i32) {
            self.put(x.round() as i32, y.round() as i32, c);
            x += sx;
            y += sy;
        }
    }
}
