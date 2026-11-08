//! A small software-rendered OHLC chart.

use datagen::Bar;
use slint::{Rgba8Pixel, SharedPixelBuffer};

use crate::drawings::Anchor;

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct AxisTick {
    pub position: f32,
    pub label: String,
}

const BACKGROUND: Color = Color(17, 17, 17);
const GRID: Color = Color(38, 38, 38);
const LAST_PRICE: Color = Color(230, 138, 0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceColor {
    #[default]
    Orange,
    Teal,
    Red,
    White,
    Black,
}

impl PriceColor {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "orange" => Some(Self::Orange),
            "teal" => Some(Self::Teal),
            "red" => Some(Self::Red),
            "white" => Some(Self::White),
            "black" => Some(Self::Black),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Orange => "orange",
            Self::Teal => "teal",
            Self::Red => "red",
            Self::White => "white",
            Self::Black => "black",
        }
    }
    fn color(self) -> Color {
        match self {
            Self::Orange => LAST_PRICE,
            Self::Teal => Color(38, 166, 154),
            Self::Red => Color(239, 83, 80),
            Self::White => Color(220, 220, 220),
            Self::Black => Color(0, 0, 0),
        }
    }
    #[cfg(test)]
    fn pixel(self) -> Rgba8Pixel {
        self.color().pixel()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricePattern {
    #[default]
    Dashed,
    Solid,
    Dotted,
}
impl PricePattern {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "dashed" => Some(Self::Dashed),
            "solid" => Some(Self::Solid),
            "dotted" => Some(Self::Dotted),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Dashed => "dashed",
            Self::Solid => "solid",
            Self::Dotted => "dotted",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceBorder {
    #[default]
    White,
    Color,
    None,
}

impl PriceBorder {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "white" => Some(Self::White),
            "color" => Some(Self::Color),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Color => "color",
            Self::None => "none",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PriceStyle {
    pub color: PriceColor,
    pub pattern: PricePattern,
    pub visible: bool,
    pub border: PriceBorder,
    pub grid_visible: bool,
    pub labels_visible: bool,
    pub thickness: u8,
    pub glow_color: PriceColor,
    pub glow_intensity: u8,
    pub shadow_color: PriceColor,
    pub shadow_intensity: u8,
}

impl Default for PriceStyle {
    fn default() -> Self {
        Self {
            color: PriceColor::Orange,
            pattern: PricePattern::Dashed,
            visible: true,
            border: PriceBorder::White,
            grid_visible: true,
            labels_visible: true,
            thickness: 1,
            glow_color: PriceColor::Orange,
            glow_intensity: 0,
            shadow_color: PriceColor::Black,
            shadow_intensity: 0,
        }
    }
}
const UP: Color = Color(38, 166, 154);
const DOWN: Color = Color(239, 83, 80);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Color(u8, u8, u8);

impl Color {
    const fn pixel(self) -> Rgba8Pixel {
        Rgba8Pixel {
            r: self.0,
            g: self.1,
            b: self.2,
            a: 255,
        }
    }

    fn from_hex(value: &str) -> Option<Self> {
        let value = value.strip_prefix('#').unwrap_or(value);
        if value.len() != 6 && value.len() != 8 {
            return None;
        }
        Some(Self(
            u8::from_str_radix(&value[0..2], 16).ok()?,
            u8::from_str_radix(&value[2..4], 16).ok()?,
            u8::from_str_radix(&value[4..6], 16).ok()?,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Candle {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}

fn aggregate_ohlc(bars: &[Bar]) -> Option<Candle> {
    let first = bars.first()?;
    let last = bars.last()?;
    let mut high = f64::NEG_INFINITY;
    let mut low = f64::INFINITY;
    for bar in bars {
        high = high.max(bar.high);
        low = low.min(bar.low);
    }
    Some(Candle {
        open: first.open,
        high,
        low,
        close: last.close,
    })
}

fn dense_column_range(i0: usize, past_last: usize, width: usize, x: usize) -> (usize, usize) {
    let visible = past_last - i0;
    let first = i0 + x * visible / width;
    let after = i0 + (((x + 1) * visible / width).max(1)).min(visible);
    (first, after)
}

fn nice_step(raw: f64) -> f64 {
    let magnitude = 10.0_f64.powf(raw.log10().floor());
    let fraction = raw / magnitude;
    let multiple = if fraction <= 1.0 {
        1.0
    } else if fraction <= 2.0 {
        2.0
    } else if fraction <= 2.5 {
        2.5
    } else if fraction <= 5.0 {
        5.0
    } else {
        10.0
    };
    multiple * magnitude
}

fn time_step(raw: f64) -> i64 {
    const STEPS: &[i64] = &[
        1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 10800, 14400, 21600, 43200,
        86400, 172800, 604800, 1209600, 2592000, 7776000, 15552000, 31536000,
    ];
    STEPS
        .iter()
        .copied()
        .find(|&step| step as f64 >= raw)
        .unwrap_or_else(|| nice_step(raw).min(i64::MAX as f64) as i64)
        .max(1)
}

#[derive(Clone, Copy)]
struct Transform {
    start: f64,
    end: f64,
    low: f64,
    high: f64,
    width: f64,
    height: f64,
}

impl Transform {
    fn time_to_x(self, bar_position: f64) -> f64 {
        (bar_position - self.start) / (self.end - self.start) * self.width
    }

    fn x_to_time(self, x: f64) -> f64 {
        self.start + x / self.width * (self.end - self.start)
    }

    fn price_to_y(self, price: f64) -> f64 {
        (self.high - price) / (self.high - self.low) * (self.height - 1.0)
    }

    fn y_to_price(self, y: f64) -> f64 {
        self.high - y / (self.height - 1.0) * (self.high - self.low)
    }
}

struct Canvas<'a> {
    pixels: &'a mut [Rgba8Pixel],
    width: usize,
    height: usize,
}

impl<'a> Canvas<'a> {
    fn new(pixels: &'a mut [Rgba8Pixel], width: usize, height: usize) -> Self {
        Self {
            pixels,
            width,
            height,
        }
    }

    fn fill(&mut self, color: Color) {
        self.pixels.fill(color.pixel());
    }

    fn put(&mut self, x: i32, y: i32, color: Color) {
        if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            self.pixels[y as usize * self.width + x as usize] = color.pixel();
        }
    }

    fn blend(&mut self, x: i32, y: i32, color: Color, alpha: f32) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let pixel = &mut self.pixels[y as usize * self.width + x as usize];
        let mix =
            |old: u8, new: u8| (old as f32 * (1.0 - alpha) + new as f32 * alpha).round() as u8;
        pixel.r = mix(pixel.r, color.0);
        pixel.g = mix(pixel.g, color.1);
        pixel.b = mix(pixel.b, color.2);
    }

    fn hline(&mut self, x0: i32, x1: i32, y: i32, color: Color) {
        if y < 0 || y as usize >= self.height {
            return;
        }
        let left = x0.min(x1).max(0);
        let right = x0.max(x1).min(self.width as i32 - 1);
        if left <= right {
            let row = y as usize * self.width;
            self.pixels[row + left as usize..=row + right as usize].fill(color.pixel());
        }
    }

    fn line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, color: Color) {
        let (x0, y0, x1, y1) = match Self::clip_line(
            x0,
            y0,
            x1,
            y1,
            self.width as f64 - 1.0,
            self.height as f64 - 1.0,
        ) {
            Some(line) => line,
            None => return,
        };
        let dx = x1 - x0;
        let dy = y1 - y0;
        let steps = dx.abs().max(dy.abs()).ceil() as usize;
        if steps == 0 {
            self.put(x0.round() as i32, y0.round() as i32, color);
            return;
        }
        for step in 0..=steps {
            let fraction = step as f64 / steps as f64;
            self.put(
                (x0 + dx * fraction).round() as i32,
                (y0 + dy * fraction).round() as i32,
                color,
            );
        }
    }

    fn styled_line(
        &mut self,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        color: Color,
        style: &crate::drawings::DrawingStyle,
    ) {
        let Some((x0, y0, x1, y1)) = Self::clip_line(
            x0,
            y0,
            x1,
            y1,
            self.width as f64 - 1.0,
            self.height as f64 - 1.0,
        ) else {
            return;
        };
        let (dx, dy) = (x1 - x0, y1 - y0);
        let steps = dx.abs().max(dy.abs()).ceil() as usize;
        let len = dx.hypot(dy).max(1.0);
        let (nx, ny) = (-dy / len, dx / len);
        let half = (style.width.saturating_sub(1) as f64) / 2.0;
        for step in 0..=steps {
            let visible = match style.pattern {
                crate::drawings::LinePattern::Solid => true,
                crate::drawings::LinePattern::Dashed => step % 10 < 6,
                crate::drawings::LinePattern::Dotted => step % 4 == 0,
            };
            if !visible {
                continue;
            }
            let fraction = if steps == 0 {
                0.0
            } else {
                step as f64 / steps as f64
            };
            let (x, y) = (x0 + dx * fraction, y0 + dy * fraction);
            for offset in 0..style.width.max(1) {
                let perpendicular = offset as f64 - half;
                self.put(
                    (x + nx * perpendicular).round() as i32,
                    (y + ny * perpendicular).round() as i32,
                    color,
                );
            }
        }
    }

    fn clip_line(
        mut x0: f64,
        mut y0: f64,
        mut x1: f64,
        mut y1: f64,
        max_x: f64,
        max_y: f64,
    ) -> Option<(f64, f64, f64, f64)> {
        if ![x0, y0, x1, y1, max_x, max_y]
            .iter()
            .all(|value| value.is_finite())
            || max_x < 0.0
            || max_y < 0.0
        {
            return None;
        }
        fn out_code(x: f64, y: f64, max_x: f64, max_y: f64) -> u8 {
            let mut code = 0;
            if x < 0.0 {
                code |= 1;
            } else if x > max_x {
                code |= 2;
            }
            if y < 0.0 {
                code |= 4;
            } else if y > max_y {
                code |= 8;
            }
            code
        }

        for _ in 0..8 {
            let code0 = out_code(x0, y0, max_x, max_y);
            let code1 = out_code(x1, y1, max_x, max_y);
            if code0 | code1 == 0 {
                return Some((x0, y0, x1, y1));
            }
            if code0 & code1 != 0 {
                return None;
            }

            let code = if code0 != 0 { code0 } else { code1 };
            let (x, y) = if code & 8 != 0 {
                let x = x0 + ((max_y - y0) / (y1 - y0)) * (x1 - x0);
                (x, max_y)
            } else if code & 4 != 0 {
                let x = x0 + ((0.0 - y0) / (y1 - y0)) * (x1 - x0);
                (x, 0.0)
            } else if code & 2 != 0 {
                let y = y0 + ((max_x - x0) / (x1 - x0)) * (y1 - y0);
                (max_x, y)
            } else {
                let y = y0 + ((0.0 - x0) / (x1 - x0)) * (y1 - y0);
                (0.0, y)
            };
            if !x.is_finite() || !y.is_finite() {
                return None;
            }
            if code == code0 {
                (x0, y0) = (x, y);
            } else {
                (x1, y1) = (x, y);
            }
        }
        None
    }

    fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let left = x0.min(x1).max(0);
        let right = x0.max(x1).min(self.width as i32);
        let top = y0.min(y1).max(0);
        let bottom = y0.max(y1).min(self.height as i32);
        if left >= right || top >= bottom {
            return;
        }
        let px = color.pixel();
        for y in top..bottom {
            let row = y as usize * self.width;
            self.pixels[row + left as usize..row + right as usize].fill(px);
        }
    }
}

/// Chart data, view state, and a Slint-compatible software raster output.
#[derive(Clone)]
pub struct Chart {
    bars: std::sync::Arc<Vec<Bar>>,
    start: f64,
    end: f64,
    width: u32,
    height: u32,
    price_style: PriceStyle,
}

impl Chart {
    /// Create a chart showing all supplied bars.
    pub fn new(bars: Vec<Bar>) -> Self {
        let end = bars.len() as f64;
        Self {
            bars: std::sync::Arc::new(bars),
            start: 0.0,
            end,
            width: 0,
            height: 0,
            price_style: PriceStyle::default(),
        }
    }

    /// Set the output dimensions. Zero-sized dimensions render as one pixel.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    pub fn screen_size(&self) -> (u32, u32) {
        (self.width.max(1), self.height.max(1))
    }

    pub fn price_style(&self) -> PriceStyle {
        self.price_style
    }

    pub fn set_price_style(&mut self, style: PriceStyle) {
        self.price_style = style;
    }

    /// The newest close in normalized plot coordinates, including off-screen prices.
    pub fn current_price_marker(&self) -> Option<AxisTick> {
        if self.width == 0 {
            return None;
        }
        let close = self.bars.last()?.close;
        if !close.is_finite() {
            return None;
        }
        let (_, _, transform) = self.view_transform(self.width, self.height)?;
        let y = transform.price_to_y(close);
        let position = (y / transform.height) as f32;
        if !position.is_finite() {
            return None;
        }
        Some(AxisTick {
            position,
            label: format!("{close:.2}"),
        })
    }

    /// Axis positions are normalized to the same plot transform used by candles and drawings.
    pub fn axis_ticks(&self, scale: f32) -> (Vec<AxisTick>, Vec<AxisTick>) {
        let Some((_, _, transform)) = self.view_transform(self.width, self.height) else {
            return (Vec::new(), Vec::new());
        };
        if self.width == 0 {
            return (Vec::new(), Vec::new());
        }
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let logical_height = self.height as f64 / scale as f64;
        let price_step =
            nice_step((transform.high - transform.low) / (logical_height / 55.0).max(1.0));
        let decimals = ((-price_step.log10()).floor() as i32 + 2).clamp(2, 12) as usize;
        let first_price = (transform.low / price_step).ceil();
        let last_price = (transform.high / price_step).floor();
        let count = ((last_price - first_price + 1.0).max(0.0) as usize).min(100);
        let price = (0..count)
            .filter_map(|i| {
                let value = (first_price + i as f64) * price_step;
                let position = transform.price_to_y(value) / transform.height;
                (0.0..1.0).contains(&position).then(|| AxisTick {
                    position: position as f32,
                    label: format!("{:.*}", decimals, if value == 0.0 { 0.0 } else { value }),
                })
            })
            .collect();
        let first_index = self.start.floor() as usize;
        let last_index = (self.end.ceil() as usize).min(self.bars.len()) - 1;
        let first = self.bars[first_index].ts;
        let last = self.bars[last_index].ts;
        let span = (last as f64 - first as f64).max(0.0);
        let logical_width = self.width as f64 / scale as f64;
        let step = time_step(span / (logical_width / 110.0).max(1.0));
        let multi_day = first.div_euclid(86_400) != last.div_euclid(86_400);
        let multi_year = span >= 31_536_000.0;
        let mut time = Vec::new();
        let mut timestamp = first.div_euclid(step).saturating_mul(step);
        if timestamp < first {
            timestamp = timestamp.saturating_add(step);
        }
        for _ in 0..100 {
            if timestamp > last {
                break;
            }
            let right = self.bars.partition_point(|bar| bar.ts < timestamp);
            if right < self.bars.len() {
                let index = if self.bars[right].ts == timestamp || right == 0 {
                    right as f64
                } else {
                    let left = right - 1;
                    let gap = self.bars[right].ts as f64 - self.bars[left].ts as f64;
                    if gap <= 0.0 || gap > step as f64 * 2.0 {
                        match timestamp.checked_add(step) {
                            Some(next) => {
                                timestamp = next;
                                continue;
                            }
                            None => break,
                        }
                    }
                    left as f64 + (timestamp as f64 - self.bars[left].ts as f64) / gap
                };
                let position = transform.time_to_x(index + 0.5) / transform.width;
                if (0.0..1.0).contains(&position) {
                    if let Some(date) = chrono::DateTime::from_timestamp(timestamp, 0) {
                        let format = if multi_year && step < 60 {
                            "%Y-%m-%d %H:%M:%S"
                        } else if multi_year && step < 86400 {
                            "%Y-%m-%d %H:%M"
                        } else if multi_year {
                            "%Y-%m-%d"
                        } else if multi_day && step < 60 {
                            "%m-%d %H:%M:%S"
                        } else if multi_day {
                            "%m-%d %H:%M"
                        } else if step < 60 {
                            "%H:%M:%S"
                        } else {
                            "%H:%M"
                        };
                        time.push(AxisTick {
                            position: position as f32,
                            label: date.format(format).to_string(),
                        });
                    }
                }
                if time.is_empty() {
                    let index = ((self.start + self.end) / 2.0).floor() as usize;
                    let index = index.min(self.bars.len() - 1);
                    let position = transform.time_to_x(index as f64 + 0.5) / transform.width;
                    if (0.0..1.0).contains(&position) {
                        if let Some(date) = chrono::DateTime::from_timestamp(self.bars[index].ts, 0)
                        {
                            time.push(AxisTick {
                                position: position as f32,
                                label: date
                                    .format(if multi_day {
                                        "%Y-%m-%d %H:%M:%S"
                                    } else {
                                        "%H:%M:%S"
                                    })
                                    .to_string(),
                            });
                        }
                    }
                }
            }
            match timestamp.checked_add(step) {
                Some(next) => timestamp = next,
                None => break,
            }
        }
        (time, price)
    }

    /// Pan by a screen-space pixel delta; positive values move toward older bars.
    pub fn pan_px(&mut self, dx: f64) {
        if !dx.is_finite() || self.bars.is_empty() || self.width == 0 {
            return;
        }
        let span = self.end - self.start;
        let shift = dx / self.width as f64 * span;
        self.set_range(self.start - shift, span);
    }

    /// Zoom around a horizontal pixel anchor; positive deltas zoom in.
    pub fn zoom_at(&mut self, x: f64, delta: f64) {
        if !x.is_finite() || !delta.is_finite() || self.bars.is_empty() || self.width == 0 {
            return;
        }
        let span = self.end - self.start;
        if span <= 0.0 {
            return;
        }
        let anchor = self.start + span * (x / self.width as f64).clamp(0.0, 1.0);
        let new_span = (span * (-delta * 0.003).exp()).clamp(1.0, self.bars.len() as f64);
        let fraction = ((anchor - self.start) / span).clamp(0.0, 1.0);
        let start = anchor - new_span * fraction;
        self.set_range(start, new_span);
    }

    /// Convert a pointer location to an absolute time/price anchor for a
    /// drawing. Anchors remain stable when the chart is zoomed or resized.
    pub fn screen_to_anchor(&self, x: f64, y: f64) -> Option<Anchor> {
        if !x.is_finite() || !y.is_finite() || self.width == 0 || self.height <= 1 {
            return None;
        }
        let (_, _, transform) = self.view_transform(self.width, self.height)?;
        let bar_position = transform
            .x_to_time(x.clamp(0.0, self.width as f64))
            .clamp(0.0, self.bars.len().saturating_sub(1) as f64);
        let left = bar_position.floor() as usize;
        let right = (left + 1).min(self.bars.len() - 1);
        let fraction = bar_position - left as f64;
        let left_time = self.bars[left].ts as f64;
        let right_time = self.bars[right].ts as f64;
        let timestamp_ms = (left_time + (right_time - left_time) * fraction) * 1000.0;
        let price = transform.y_to_price(y.clamp(0.0, self.height.saturating_sub(1) as f64));
        if !timestamp_ms.is_finite()
            || timestamp_ms < 0.0
            || timestamp_ms > i64::MAX as f64
            || !price.is_finite()
        {
            return None;
        }
        Some(Anchor {
            timestamp_ms: timestamp_ms.round() as i64,
            price,
        })
    }

    pub fn anchor_to_screen(&self, anchor: Anchor) -> Option<(f64, f64)> {
        if self.width == 0 || self.height <= 1 || !anchor.price.is_finite() {
            return None;
        }
        let (_, _, transform) = self.view_transform(self.width, self.height)?;
        let point = (
            self.time_to_x(anchor.timestamp_ms, transform),
            transform.price_to_y(anchor.price),
        );
        (point.0.is_finite() && point.1.is_finite()).then_some(point)
    }

    /// Render the current view into an RGBA8 pixel buffer.
    pub fn render_frame(&self, scale: f32) -> SharedPixelBuffer<Rgba8Pixel> {
        let width = self.width.max(1);
        let height = self.height.max(1);
        let mut frame = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
        let mut canvas = Canvas::new(frame.make_mut_slice(), width as usize, height as usize);
        canvas.fill(BACKGROUND);
        let Some((i0, i1, transform)) = self.view_transform(width, height) else {
            drop(canvas);
            return frame;
        };

        if self.price_style.grid_visible {
            for tick in self.axis_ticks(scale).1 {
                let y = (height as f32 * tick.position).round() as i32;
                canvas.hline(0, width as i32 - 1, y, GRID);
            }
        }

        let bars_per_pixel = (self.end - self.start) / width as f64;
        if bars_per_pixel > 2.0 {
            for x in 0..width as usize {
                let (first, past_last) = dense_column_range(i0, i1 + 1, width as usize, x);
                if let Some(candle) = aggregate_ohlc(&self.bars[first..past_last]) {
                    self.draw_candle(&mut canvas, transform, x as f64, 1.0, candle);
                }
            }
        } else {
            let candle_width = ((width as f64 / (self.end - self.start)) * 0.7).max(1.0);
            for i in i0..=i1 {
                let candle = Candle {
                    open: self.bars[i].open,
                    high: self.bars[i].high,
                    low: self.bars[i].low,
                    close: self.bars[i].close,
                };
                let center = transform.time_to_x(i as f64 + 0.5);
                self.draw_candle(&mut canvas, transform, center, candle_width, candle);
            }
        }
        if let Some(marker) = self
            .current_price_marker()
            .filter(|marker| self.price_style.visible && (0.0..1.0).contains(&marker.position))
        {
            let y = (height as f32 * marker.position).round() as i32;
            let scale = if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            };
            let (dash, gap) = match self.price_style.pattern {
                PricePattern::Solid => (width as usize, 1),
                PricePattern::Dashed => (
                    (4.0 * scale).round().max(1.0) as usize,
                    (3.0 * scale).round().max(1.0) as usize,
                ),
                PricePattern::Dotted => (
                    (1.0 * scale).round().max(1.0) as usize,
                    (3.0 * scale).round().max(1.0) as usize,
                ),
            };
            let thickness = ((self.price_style.thickness as f32 * scale).round() as i32).max(1);
            let radius = (3.0 * scale).round().max(1.0) as i32;
            let shadow_offset = (2.0 * scale).round().max(1.0) as i32;
            if self.price_style.glow_intensity > 0 || self.price_style.shadow_intensity > 0 {
                for x in 0..width as usize {
                    if x % (dash + gap) >= dash {
                        continue;
                    }
                    for (intensity, offset, color) in [
                        (
                            self.price_style.glow_intensity,
                            0,
                            self.price_style.glow_color.color(),
                        ),
                        (
                            self.price_style.shadow_intensity,
                            shadow_offset,
                            self.price_style.shadow_color.color(),
                        ),
                    ] {
                        if intensity == 0 {
                            continue;
                        }
                        for dy in -radius..=radius {
                            let alpha = (intensity as f32 * 0.12)
                                * (1.0 - dy.abs() as f32 / (radius + 1) as f32);
                            canvas.blend(x as i32, y + offset + dy, color, alpha);
                        }
                    }
                }
            }
            for x in (0..width as usize).step_by(dash + gap) {
                for dy in 0..thickness {
                    canvas.hline(
                        x as i32,
                        (x + dash - 1) as i32,
                        y + dy - (thickness - 1) / 2,
                        self.price_style.color.color(),
                    );
                }
            }
        }
        drop(canvas);
        frame
    }

    /// Render symbol-scoped annotations over the chart bitmap.
    pub fn render_frame_with_drawings(
        &self,
        drawings: &[crate::drawings::Drawing],
        scale: f32,
    ) -> SharedPixelBuffer<Rgba8Pixel> {
        let mut frame = self.render_frame(scale);
        let width = frame.width() as usize;
        let height = frame.height() as usize;
        let Some((_, _, transform)) = self.view_transform(width as u32, height as u32) else {
            return frame;
        };
        let mut canvas = Canvas::new(frame.make_mut_slice(), width, height);
        for drawing in drawings.iter().filter(|drawing| drawing.visible) {
            if drawing.validate().is_err() {
                continue;
            }
            let Some(color) = Color::from_hex(&drawing.style.color) else {
                continue;
            };
            let x0 = self.time_to_x(drawing.start.timestamp_ms, transform);
            let y0 = transform.price_to_y(drawing.start.price);
            let endpoint = drawing.end.unwrap_or(drawing.start);
            let x1 = self.time_to_x(endpoint.timestamp_ms, transform);
            let y1 = transform.price_to_y(endpoint.price);
            let right = width.saturating_sub(1) as f64;
            let bottom = height.saturating_sub(1) as f64;
            match drawing.kind {
                crate::drawings::DrawingKind::HorizontalLine => {
                    canvas.styled_line(0.0, y0, right, y0, color, &drawing.style);
                }
                crate::drawings::DrawingKind::HorizontalRay => {
                    canvas.styled_line(x0, y0, right, y0, color, &drawing.style);
                }
                crate::drawings::DrawingKind::VerticalLine => {
                    canvas.styled_line(x0, 0.0, x0, bottom, color, &drawing.style);
                }
                crate::drawings::DrawingKind::Crosshair => {
                    canvas.styled_line(0.0, y0, right, y0, color, &drawing.style);
                    canvas.styled_line(x0, 0.0, x0, bottom, color, &drawing.style);
                }
                crate::drawings::DrawingKind::Ray => {
                    let (end_x, end_y) = if (x1 - x0).abs() <= f64::EPSILON {
                        (x0, if y1 >= y0 { bottom } else { 0.0 })
                    } else if x1 > x0 {
                        (right, y0 + (y1 - y0) * (right - x0) / (x1 - x0))
                    } else {
                        (0.0, y0 + (y1 - y0) * (0.0 - x0) / (x1 - x0))
                    };
                    canvas.styled_line(x0, y0, end_x, end_y, color, &drawing.style);
                }
                crate::drawings::DrawingKind::TrendLine
                | crate::drawings::DrawingKind::ExtendedLine => {
                    canvas.styled_line(x0, y0, x1, y1, color, &drawing.style);
                }
                crate::drawings::DrawingKind::Measure => {
                    canvas.styled_line(x0, y0, x1, y0, color, &drawing.style);
                    canvas.styled_line(x1, y0, x1, y1, color, &drawing.style);
                    canvas.styled_line(x1, y1, x0, y1, color, &drawing.style);
                    canvas.styled_line(x0, y1, x0, y0, color, &drawing.style);
                }
            }
        }
        frame
    }

    fn time_to_x(&self, timestamp_ms: i64, transform: Transform) -> f64 {
        let insertion = self
            .bars
            .partition_point(|bar| bar.ts.saturating_mul(1000) < timestamp_ms);
        let index = if insertion == 0 {
            0.0
        } else if insertion >= self.bars.len() {
            self.bars.len().saturating_sub(1) as f64
        } else {
            let left = insertion - 1;
            let right = insertion;
            let start_ms = self.bars[left].ts.saturating_mul(1000);
            let end_ms = self.bars[right].ts.saturating_mul(1000);
            let span = end_ms.saturating_sub(start_ms);
            let fraction = if span > 0 {
                (timestamp_ms.saturating_sub(start_ms) as f64 / span as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };
            left as f64 + fraction
        };
        transform.time_to_x(index)
    }

    fn draw_candle(
        &self,
        canvas: &mut Canvas<'_>,
        transform: Transform,
        center_x: f64,
        candle_width: f64,
        candle: Candle,
    ) {
        let color = if candle.close >= candle.open {
            UP
        } else {
            DOWN
        };
        let x = center_x.round() as i32;
        canvas.line(
            x as f64,
            transform.price_to_y(candle.high),
            x as f64,
            transform.price_to_y(candle.low),
            color,
        );
        let top = transform.price_to_y(candle.open.max(candle.close));
        let bottom = transform.price_to_y(candle.open.min(candle.close));
        let left = (center_x - candle_width / 2.0).floor() as i32;
        let right = (center_x + candle_width / 2.0).ceil() as i32;
        let bottom = (bottom.round() as i32).max(top.round() as i32 + 1);
        canvas.fill_rect(left, top.round() as i32, right, bottom, color);
    }

    fn view_transform(&self, width: u32, height: u32) -> Option<(usize, usize, Transform)> {
        if self.bars.is_empty() || self.end <= self.start || height <= 1 {
            return None;
        }
        let i0 = self.start.floor().max(0.0) as usize;
        let past_last = (self.end.ceil() as usize).min(self.bars.len());
        if i0 >= past_last {
            return None;
        }
        let visible = &self.bars[i0..past_last];
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for bar in visible {
            if bar.low.is_finite() && bar.high.is_finite() {
                low = low.min(bar.low);
                high = high.max(bar.high);
            }
        }
        if !low.is_finite() || !high.is_finite() {
            return None;
        }
        let padding = if high > low {
            (high - low) * 0.04
        } else {
            (high.abs() * 0.004).max(0.01)
        };
        Some((
            i0,
            past_last - 1,
            Transform {
                start: self.start,
                end: self.end,
                low: low - padding,
                high: high + padding,
                width: width as f64,
                height: height as f64,
            },
        ))
    }

    fn set_range(&mut self, start: f64, span: f64) {
        let count = self.bars.len() as f64;
        if count == 0.0 {
            self.start = 0.0;
            self.end = 0.0;
            return;
        }
        let span = span.clamp(1.0, count);
        let start = start.clamp(0.0, count - span);
        self.start = start;
        self.end = start + span;
        debug_assert!(self.start >= 0.0 && self.end <= count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(ts: i64, open: f64, high: f64, low: f64, close: f64) -> Bar {
        Bar {
            ts,
            open,
            high,
            low,
            close,
            volume: 1.0,
        }
    }

    #[test]
    fn dense_ohlc_aggregation_keeps_first_open_last_close_and_extremes() {
        let bars = [
            bar(10, 10.0, 13.0, 9.0, 12.0),
            bar(11, 12.0, 15.0, 11.0, 11.5),
            bar(12, 11.5, 14.0, 8.0, 13.0),
        ];
        assert_eq!(
            aggregate_ohlc(&bars),
            Some(Candle {
                open: 10.0,
                high: 15.0,
                low: 8.0,
                close: 13.0,
            })
        );
        assert_eq!(aggregate_ohlc(&[]), None);
        assert_eq!(dense_column_range(1, 6, 2, 0), (1, 3));
        assert_eq!(dense_column_range(1, 6, 2, 1), (3, 6));
    }

    #[test]
    fn coordinate_transforms_map_both_axes_and_invert() {
        let transform = Transform {
            start: 2.0,
            end: 6.0,
            low: 10.0,
            high: 20.0,
            width: 8.0,
            height: 11.0,
        };
        assert_eq!(transform.time_to_x(2.0), 0.0);
        assert_eq!(transform.time_to_x(4.0), 4.0);
        assert_eq!(transform.time_to_x(6.0), 8.0);
        assert_eq!(transform.x_to_time(3.0), 3.5);
        assert_eq!(transform.price_to_y(20.0), 0.0);
        assert_eq!(transform.price_to_y(10.0), 10.0);
        assert_eq!(transform.price_to_y(15.0), 5.0);
        assert_eq!(transform.y_to_price(5.0), 15.0);
    }

    #[test]
    fn latest_close_marker_tracks_price_transform_and_renders_dashes() {
        let mut chart = Chart::new(vec![
            bar(1, 10.0, 15.0, 5.0, 10.0),
            bar(2, 10.0, 15.0, 5.0, 12.34),
            bar(3, 10.0, 15.0, 5.0, 11.25),
        ]);
        chart.resize(240, 101);
        chart.set_range(0.0, 2.0);
        let marker = chart.current_price_marker().expect("visible latest close");
        assert_eq!(marker.label, "11.25");
        let transform = chart.view_transform(240, 101).unwrap().2;
        assert!((marker.position as f64 * 101.0 - transform.price_to_y(11.25)).abs() < 0.0001);
        let y = (marker.position * 101.0).round() as usize;
        let frame = chart.render_frame(1.0);
        let row = &frame.as_slice()[y * 240..(y + 1) * 240];
        assert_eq!(row[0], Color(230, 138, 0).pixel());
        assert_eq!(row[4], BACKGROUND.pixel());
        assert_eq!(row[7], Color(230, 138, 0).pixel());

        std::sync::Arc::make_mut(&mut chart.bars)
            .last_mut()
            .unwrap()
            .close = 12.34;
        let updated = chart.current_price_marker().unwrap();
        assert_eq!(updated.label, "12.34");
        assert!(updated.position < marker.position);
        chart.resize(240, 201);
        let resized = chart.current_price_marker().unwrap();
        assert!(
            (resized.position as f64 * 201.0
                - chart.view_transform(240, 201).unwrap().2.price_to_y(12.34))
            .abs()
                < 0.0001
        );
        std::sync::Arc::make_mut(&mut chart.bars)
            .last_mut()
            .unwrap()
            .close = 100.0;
        let above = chart.current_price_marker().unwrap();
        assert_eq!(above.label, "100.00");
        assert!(above.position < 0.0);
        assert!(!chart
            .render_frame(1.0)
            .as_slice()
            .contains(&LAST_PRICE.pixel()));
        std::sync::Arc::make_mut(&mut chart.bars)
            .last_mut()
            .unwrap()
            .close = 1.0;
        let below = chart.current_price_marker().unwrap();
        assert_eq!(below.label, "1.00");
        assert!(below.position > 1.0);
        assert!(!chart
            .render_frame(1.0)
            .as_slice()
            .contains(&LAST_PRICE.pixel()));
        std::sync::Arc::make_mut(&mut chart.bars)
            .last_mut()
            .unwrap()
            .close = f64::NAN;
        assert!(chart.current_price_marker().is_none());
        assert!(Chart::new(Vec::new()).current_price_marker().is_none());
    }

    #[test]
    fn price_line_style_changes_color_and_pattern_without_affecting_other_charts() {
        let bars = vec![bar(1, 10.0, 15.0, 5.0, 11.25)];
        let mut chart = Chart::new(bars.clone());
        let mut other = Chart::new(bars);
        chart.resize(240, 101);
        other.resize(240, 101);
        chart.set_price_style(PriceStyle {
            color: PriceColor::Teal,
            pattern: PricePattern::Solid,
            ..PriceStyle::default()
        });
        let y = (chart.current_price_marker().unwrap().position * 101.0).round() as usize;
        let frame = chart.render_frame(1.0);
        assert!(frame.as_slice()[y * 240..(y + 1) * 240]
            .iter()
            .all(|p| *p == PriceColor::Teal.pixel()));
        assert_eq!(other.price_style(), PriceStyle::default());
        assert_ne!(other.render_frame(1.0).as_slice(), frame.as_slice());
    }

    #[test]
    fn price_visibility_thickness_effects_and_grid_change_pixels() {
        let mut chart = Chart::new(vec![bar(1, 10.0, 15.0, 5.0, 11.25)]);
        chart.resize(240, 101);
        let y = (chart.current_price_marker().unwrap().position * 101.0).round() as usize;
        let style = PriceStyle {
            pattern: PricePattern::Solid,
            grid_visible: false,
            ..PriceStyle::default()
        };
        chart.set_price_style(style);
        let baseline = chart.render_frame(1.0);
        assert_eq!(baseline.as_slice()[y * 240], LAST_PRICE.pixel());
        chart.set_price_style(PriceStyle {
            visible: false,
            ..style
        });
        assert_ne!(
            chart.render_frame(1.0).as_slice()[y * 240],
            LAST_PRICE.pixel()
        );
        chart.set_price_style(PriceStyle {
            thickness: 3,
            ..style
        });
        let thick = chart.render_frame(1.0);
        assert_eq!(thick.as_slice()[(y - 1) * 240], LAST_PRICE.pixel());
        assert_eq!(thick.as_slice()[(y + 1) * 240], LAST_PRICE.pixel());
        chart.set_price_style(PriceStyle {
            glow_color: PriceColor::Teal,
            glow_intensity: 2,
            shadow_color: PriceColor::Red,
            shadow_intensity: 2,
            ..style
        });
        let effects = chart.render_frame(1.0);
        assert_ne!(
            effects.as_slice()[(y - 2) * 240],
            baseline.as_slice()[(y - 2) * 240]
        );
        assert_ne!(
            effects.as_slice()[(y + 4) * 240],
            baseline.as_slice()[(y + 4) * 240]
        );
        chart.set_price_style(PriceStyle {
            visible: false,
            grid_visible: true,
            ..style
        });
        let with_grid = chart.render_frame(1.0);
        chart.set_price_style(PriceStyle {
            visible: false,
            grid_visible: false,
            ..style
        });
        let without_grid = chart.render_frame(1.0);
        assert_ne!(with_grid.as_slice(), without_grid.as_slice());
    }

    #[test]
    fn marker_remains_available_after_pan_and_zoom_move_price_out_of_view() {
        let bars = (0..100)
            .map(|i| {
                let price = if i < 50 { 10.0 } else { 30.0 };
                bar(i, price, price + 1.0, price - 1.0, price)
            })
            .collect();
        let mut chart = Chart::new(bars);
        chart.resize(400, 200);
        chart.zoom_at(0.0, 1000.0);
        let zoom_marker = chart.current_price_marker().unwrap();
        assert_eq!(zoom_marker.label, "30.00");
        assert!(zoom_marker.position < 0.0);
        chart.zoom_at(200.0, -1000.0);
        assert!((0.0..1.0).contains(&chart.current_price_marker().unwrap().position));
        chart.zoom_at(400.0, 1000.0);
        chart.pan_px(10_000.0);
        let pan_marker = chart.current_price_marker().unwrap();
        assert_eq!(pan_marker.label, "30.00");
        assert!(pan_marker.position < 0.0);
    }

    #[test]
    fn axis_ticks_track_stable_values_and_grid_on_pan() {
        let bars = (0..1000)
            .map(|i| {
                let price = 10.0 + i as f64 / 1000.0;
                bar(
                    1_700_000_000 + i * 60,
                    price,
                    price + 0.02,
                    price - 0.02,
                    price,
                )
            })
            .collect();
        let mut chart = Chart::new(bars);
        chart.resize(600, 300);
        chart.set_range(300.0, 300.0);
        let (before_time, before_price) = chart.axis_ticks(1.0);
        assert!(before_time.len() >= 2 && before_price.len() >= 2);
        let before_transform = chart.view_transform(600, 300).unwrap().2;
        for tick in &before_price {
            let price = tick.label.parse::<f64>().unwrap();
            let y = before_transform.price_to_y(price);
            assert!((tick.position as f64 * 300.0 - y).abs() < 0.001);
        }
        chart.pan_px(30.0);
        let (after_time, after_price) = chart.axis_ticks(1.0);
        let (old_time, new_time) = before_time
            .iter()
            .find_map(|old| {
                after_time
                    .iter()
                    .find(|new| new.label == old.label)
                    .map(|new| (old, new))
            })
            .expect("a time tick should survive a small pan");
        assert!(((new_time.position - old_time.position) * 600.0 - 30.0).abs() < 0.01);
        assert!(
            before_price.iter().any(|old| {
                after_price.iter().any(|new| {
                    new.label == old.label && (new.position - old.position).abs() > 0.0001
                })
            }),
            "a surviving price tick should move when the price range changes"
        );
        let transform = chart.view_transform(600, 300).unwrap().2;
        for tick in &after_price {
            let y = (tick.position as f64 * 300.0).round() as usize;
            let frame = chart.render_frame(1.0);
            assert!(
                frame.as_slice()[y * 600..(y + 1) * 600]
                    .iter()
                    .filter(|&&pixel| pixel == GRID.pixel())
                    .count()
                    > 100,
                "grid and price tick must align"
            );
            assert!(
                (transform.price_to_y(tick.label.parse().unwrap()) - tick.position as f64 * 300.0)
                    .abs()
                    < 0.001
            );
        }
        assert!(Chart::new(Vec::new()).axis_ticks(1.0).0.is_empty());
    }

    #[test]
    fn time_ticks_use_round_utc_intervals_and_handle_gaps_and_single_bar() {
        let bars = (0..100)
            .map(|i| {
                let gap = if i >= 50 { 86_400 } else { 0 };
                bar(1_700_000_017 + i * 60 + gap, 10.0, 12.0, 8.0, 11.0)
            })
            .collect();
        let mut chart = Chart::new(bars);
        chart.resize(900, 300);
        chart.set_range(5.0, 30.0);
        let (ticks, _) = chart.axis_ticks(1.0);
        assert!(ticks.len() >= 2);
        let transform = chart.view_transform(900, 300).unwrap().2;
        for tick in &ticks {
            let time = chrono::NaiveTime::parse_from_str(&tick.label, "%H:%M").unwrap();
            assert_eq!(chrono::Timelike::second(&time), 0);
            let position = transform.x_to_time(tick.position as f64 * 900.0) - 0.5;
            let left = position.floor() as usize;
            let ts = chart.bars[left].ts as f64
                + (chart.bars[left + 1].ts - chart.bars[left].ts) as f64 * (position - left as f64);
            assert!((ts.rem_euclid(60.0)).min(60.0 - ts.rem_euclid(60.0)) < 0.001);
        }
        chart.set_range(40.0, 20.0);
        let (ticks, _) = chart.axis_ticks(1.0);
        assert!(ticks.len() <= 10);
        assert!(ticks.windows(2).all(|pair| pair[0].label != pair[1].label));
        chart.set_range(20.0, 1.0);
        let (ticks, _) = chart.axis_ticks(1.0);
        assert_eq!(ticks.len(), 1);
        assert!((0.0..1.0).contains(&ticks[0].position));
        chart.resize(0, 10);
        assert!(chart.axis_ticks(1.0).0.is_empty());
    }

    #[test]
    fn chart_range_stays_within_data_and_zoom_preserves_anchor() {
        let mut chart = Chart::new((0..100).map(|i| bar(i, 10.0, 12.0, 9.0, 11.0)).collect());
        chart.resize(100, 40);
        chart.zoom_at(50.0, 300.0);
        let expected_span = 100.0 * (-300.0_f64 * 0.003).exp();
        assert!((chart.end - chart.start - expected_span).abs() < 1e-10);
        assert!(((chart.start + chart.end) / 2.0 - 50.0).abs() < 1e-10);

        let midpoint = (chart.start + chart.end) / 2.0;
        chart.pan_px(10.0);
        assert!(
            (chart.start - (midpoint - expected_span / 2.0 - expected_span / 10.0)).abs() < 1e-10
        );
        chart.pan_px(10_000.0);
        assert_eq!(chart.start, 0.0);
        assert!(chart.end <= chart.bars.len() as f64);
        chart.zoom_at(50.0, -100_000.0);
        assert_eq!(chart.start, 0.0);
        assert_eq!(chart.end, chart.bars.len() as f64);
    }

    #[test]
    fn raster_primitives_clip_to_the_buffer_bounds() {
        let mut pixels = vec![BACKGROUND.pixel(); 12];
        let mut canvas = Canvas::new(&mut pixels, 4, 3);
        canvas.put(-1, 0, UP);
        canvas.put(4, 2, UP);
        canvas.hline(-20, 20, 1, GRID);
        canvas.line(2.0, -10.0, 2.0, 20.0, DOWN);
        canvas.fill_rect(-5, -5, 2, 2, UP);
        assert_eq!(pixels.len(), 12);
        assert_eq!(pixels[0], UP.pixel());
        assert_eq!(pixels[1], UP.pixel());
        assert_eq!(pixels[2], DOWN.pixel());
        assert_eq!(pixels[3], BACKGROUND.pixel());
        assert_eq!(pixels[4], UP.pixel());
        assert_eq!(pixels[5], UP.pixel());
        assert_eq!(pixels[6], DOWN.pixel());
        assert_eq!(pixels[7], GRID.pixel());
        assert_eq!(pixels[10], DOWN.pixel());
        assert_eq!(pixels[11], BACKGROUND.pixel());
    }

    #[test]
    fn pointer_maps_to_stable_absolute_time_and_price_anchor() {
        let bars = vec![
            bar(1_700_000_000, 10.0, 12.0, 8.0, 11.0),
            bar(1_700_000_060, 11.0, 13.0, 9.0, 12.0),
        ];
        let mut chart = Chart::new(bars);
        chart.resize(100, 101);
        let first = chart.screen_to_anchor(0.0, 50.0).unwrap();
        let last = chart.screen_to_anchor(100.0, 50.0).unwrap();
        assert_eq!(first.timestamp_ms, 1_700_000_000_000);
        assert_eq!(last.timestamp_ms, 1_700_000_060_000);
        assert!(first.price.is_finite());
        assert_eq!(chart.screen_to_anchor(f64::NAN, 1.0), None);
    }

    #[test]
    fn drawings_are_rasterized_and_line_clipping_bounds_extreme_coordinates() {
        let bars = (0..4)
            .map(|index| bar(1_700_000_000 + index * 60, 10.0, 12.0, 8.0, 11.0))
            .collect();
        let mut chart = Chart::new(bars);
        chart.resize(64, 32);
        let drawing = crate::drawings::Drawing {
            id: "price".into(),
            symbol: "SPY".into(),
            kind: crate::drawings::DrawingKind::HorizontalLine,
            start: Anchor {
                timestamp_ms: 1_700_000_000_000,
                price: 10.0,
            },
            end: None,
            style: crate::drawings::DrawingStyle {
                color: "#FFFFFF".into(),
                width: 1,
                pattern: crate::drawings::LinePattern::Solid,
            },
            visible: true,
            locked: false,
        };
        let frame = chart.render_frame_with_drawings(&[drawing], 1.0);
        assert!(frame
            .as_slice()
            .iter()
            .any(|pixel| pixel.r == 255 && pixel.g == 255 && pixel.b == 255));

        assert_eq!(
            Canvas::clip_line(-1e200, -1e200, 1e200, 1e200, 63.0, 31.0),
            Some((0.0, 0.0, 31.0, 31.0))
        );
        assert_eq!(Canvas::clip_line(-1.0, -2.0, -3.0, -4.0, 63.0, 31.0), None);
    }

    #[test]
    fn dense_render_aggregates_visible_columns_and_ignores_offscreen_extremes() {
        let bars = vec![
            bar(0, 1.0, 1000.0, -1000.0, 1.0),
            bar(1, 10.0, 12.0, 9.0, 11.0),
            bar(2, 11.0, 14.0, 8.0, 12.0),
            bar(3, 12.0, 15.0, 7.0, 10.0),
            bar(4, 10.0, 13.0, 6.0, 12.0),
            bar(5, 12.0, 16.0, 5.0, 9.0),
        ];
        let mut chart = Chart::new(bars);
        chart.start = 1.0;
        chart.end = 6.0;
        chart.resize(2, 20);
        let (i0, i1, transform) = chart.view_transform(2, 20).unwrap();
        assert_eq!((i0, i1), (1, 5));
        assert_eq!(transform.low, 4.56);
        assert_eq!(transform.high, 16.44);

        let frame = chart.render_frame(1.0);
        assert_eq!((frame.width(), frame.height()), (2, 20));
        let pixels = frame.as_slice();
        assert!(pixels
            .iter()
            .any(|pixel| { pixel.r == UP.0 && pixel.g == UP.1 && pixel.b == UP.2 }));
        assert!(pixels
            .iter()
            .any(|pixel| { pixel.r == DOWN.0 && pixel.g == DOWN.1 && pixel.b == DOWN.2 }));
    }
}
