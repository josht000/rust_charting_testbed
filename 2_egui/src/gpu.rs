//! wgpu paint-callback renderer for the heavy chart series (candles, wicks,
//! indicator lines, volume profile). Falls back to egui::Painter when the
//! wgpu render state is unavailable.
//!
//! Geometry is generated CPU-side per frame (aggregated to one candle per
//! pixel column, one line vertex per column), uploaded as pos+color triangles
//! in normalized-device-coordinates relative to the paint callback's viewport.

use egui::{Color32, Pos2, Rect};
use egui_wgpu::wgpu;
use std::collections::HashMap;

use crate::chart::{Chart, Col, LINE_SERIES, COLOR_DOWN, COLOR_PROFILE, COLOR_UP};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vert {
    pos: [f32; 2], // NDC within callback viewport
    color: [f32; 4],
}

const SHADER: &str = r#"
struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
};
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};
@vertex
fn vs(input: VIn) -> VOut {
    var out: VOut;
    out.pos = vec4<f32>(input.pos, 0.0, 1.0);
    out.color = input.color;
    return out;
}
@fragment
fn fs(input: VOut) -> @location(0) vec4<f32> {
    return input.color; // premultiplied alpha
}
"#;

struct ChartPipeline {
    pipeline: wgpu::RenderPipeline,
    buffers: HashMap<usize, ChartBuf>,
}

struct ChartBuf {
    buf: wgpu::Buffer,
    cap: usize,
    count: u32,
}

pub struct ChartCallback {
    pub chart: usize,
    pub verts: Vec<Vert>,
}

/// Wrap in a paint callback for `painter.add(...)`. `rect` becomes the
/// callback's NDC viewport — must be the same `plot` rect used by
/// [`build_verts`].
pub fn callback(chart: usize, verts: Vec<Vert>, rect: Rect) -> egui::epaint::PaintCallback {
    egui_wgpu::Callback::new_paint_callback(rect, ChartCallback { chart, verts })
}

impl egui_wgpu::CallbackTrait for ChartCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if !resources.contains::<ChartPipeline>() {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("chart"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("chart"),
                bind_group_layouts: &[],
                immediate_size: 0,
            });
            let target_format = TARGET_FORMAT.with(|f| f.get());
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("chart"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vert>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });
            resources.insert(ChartPipeline {
                pipeline,
                buffers: HashMap::new(),
            });
        }
        let store = resources.get_mut::<ChartPipeline>().unwrap();
        let bytes = bytemuck::cast_slice(&self.verts);
        let slot = store.buffers.entry(self.chart).or_insert_with(|| ChartBuf {
            buf: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("chart-verts"),
                size: 4,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            cap: 0,
            count: 0,
        });
        if self.verts.len() > slot.cap {
            slot.buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("chart-verts"),
                size: (self.verts.len() * std::mem::size_of::<Vert>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            slot.cap = self.verts.len();
        }
        slot.count = self.verts.len() as u32;
        if !bytes.is_empty() {
            queue.write_buffer(&slot.buf, 0, bytes);
        }
        Vec::new()
    }

    fn paint(
        &self,
        info: egui::epaint::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(store) = resources.get::<ChartPipeline>() else {
            return;
        };
        let Some(slot) = store.buffers.get(&self.chart) else {
            return;
        };
        if slot.count == 0 {
            return;
        }
        let clip = info.clip_rect_in_pixels();
        render_pass.set_scissor_rect(
            clip.left_px.max(0) as u32,
            clip.top_px.max(0) as u32,
            clip.width_px.max(0) as u32,
            clip.height_px.max(0) as u32,
        );
        render_pass.set_pipeline(&store.pipeline);
        render_pass.set_vertex_buffer(0, slot.buf.slice(..));
        render_pass.draw(0..slot.count, 0..1);
    }
}

// Target format is captured once from the CreationContext (see app.rs).
thread_local! {
    static TARGET_FORMAT: std::cell::Cell<wgpu::TextureFormat> =
        const { std::cell::Cell::new(wgpu::TextureFormat::Rgba8Unorm) };
}

pub fn set_target_format(f: wgpu::TextureFormat) {
    TARGET_FORMAT.with(|c| c.set(f));
}

fn c32(c: Color32) -> [f32; 4] {
    // premultiplied alpha
    let a = c.a() as f32 / 255.0;
    [
        c.r() as f32 / 255.0 * a,
        c.g() as f32 / 255.0 * a,
        c.b() as f32 / 255.0 * a,
        a,
    ]
}

struct Ndc {
    left: f32,
    top: f32,
    w: f32,
    h: f32,
}
impl Ndc {
    fn x(&self, x: f32) -> f32 {
        (x - self.left) / self.w * 2.0 - 1.0
    }
    fn y(&self, y: f32) -> f32 {
        1.0 - (y - self.top) / self.h * 2.0
    }
    fn pt(&self, x: f32, y: f32, color: [f32; 4]) -> Vert {
        Vert {
            pos: [self.x(x), self.y(y)],
            color,
        }
    }
}

fn quad(n: &Ndc, x0: f32, y0: f32, x1: f32, y1: f32, color: [f32; 4], out: &mut Vec<Vert>) {
    out.push(n.pt(x0, y0, color));
    out.push(n.pt(x1, y0, color));
    out.push(n.pt(x1, y1, color));
    out.push(n.pt(x0, y0, color));
    out.push(n.pt(x1, y1, color));
    out.push(n.pt(x0, y1, color));
}

/// Thick line segment (oriented screen-space quad, 2 triangles).
fn seg(n: &Ndc, p0: Pos2, p1: Pos2, w: f32, color: [f32; 4], out: &mut Vec<Vert>) {
    let dx = p1.x - p0.x;
    let dy = p1.y - p0.y;
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    let nx = -dy / len * w / 2.0;
    let ny = dx / len * w / 2.0;
    out.push(n.pt(p0.x + nx, p0.y + ny, color));
    out.push(n.pt(p1.x + nx, p1.y + ny, color));
    out.push(n.pt(p1.x - nx, p1.y - ny, color));
    out.push(n.pt(p0.x + nx, p0.y + ny, color));
    out.push(n.pt(p1.x - nx, p1.y - ny, color));
    out.push(n.pt(p0.x - nx, p0.y - ny, color));
}

/// Build all chart vertices (profile → candles → lines) for one frame.
/// `plot` and coordinates are in egui points; `viewport` is the callback rect.
pub fn build_verts(chart: &Chart, plot: Rect, cols: &[Col], lo: f64, hi: f64) -> Vec<Vert> {
    let n = Ndc {
        left: plot.left(),
        top: plot.top(),
        w: plot.width(),
        h: plot.height(),
    };
    let y_of = |v: f64| plot.bottom() - ((v - lo) / (hi - lo)) as f32 * plot.height();
    let mut out: Vec<Vert> = Vec::with_capacity(cols.len() * 48);

    // --- volume profile: right edge, independent x scale, 50% alpha blue ---
    let max_vol = chart
        .ind
        .profile
        .iter()
        .map(|b| b.volume)
        .fold(0.0f64, f64::max)
        .max(1e-9);
    let prof_w = plot.width() * 0.22;
    let pc = c32(COLOR_PROFILE);
    for b in &chart.ind.profile {
        let y0 = y_of(b.price_high).max(plot.top());
        let y1 = y_of(b.price_low).min(plot.bottom());
        if y1 <= y0 {
            continue;
        }
        let w = (b.volume / max_vol) as f32 * prof_w;
        quad(&n, plot.right() - w, y0, plot.right(), y1, pc, &mut out);
    }

    // --- candles: one per bar when zoomed in (bpp < 1), else one per column ---
    let up = c32(COLOR_UP);
    let down = c32(COLOR_DOWN);
    let (bi0, bi1, bpp) = chart.visible_range(plot);
    if bpp < 1.0 {
        let bw = ((1.0 / bpp) as f32 * 0.7).max(1.0);
        for i in bi0..bi1 {
            let b = &chart.arrived()[i];
            let col = if b.close >= b.open { up } else { down };
            let xc = chart.bar_x(i, plot, bpp);
            // wick
            quad(&n, xc - 0.75, y_of(b.high), xc + 0.75, y_of(b.low), col, &mut out);
            // body
            let y0 = y_of(b.open.max(b.close));
            let y1 = (y_of(b.open.min(b.close)) - y0).max(1.0) + y0;
            quad(&n, xc - bw / 2.0, y0, xc + bw / 2.0, y1, col, &mut out);
        }
    } else {
        for c in cols {
            let col = if c.close >= c.open { up } else { down };
            let xc = 0.5 * (c.x0 + c.x1);
            let body_w = ((c.x1 - c.x0) * 0.7).max(1.0);
            // wick
            quad(&n, xc - 0.5, y_of(c.high), xc + 0.5, y_of(c.low), col, &mut out);
            // body
            let y0 = y_of(c.open.max(c.close));
            let y1 = (y_of(c.open.min(c.close)) - y0).max(1.0) + y0;
            quad(&n, xc - body_w / 2.0, y0, xc + body_w / 2.0, y1, col, &mut out);
        }
    }

    // --- indicator lines: per-bar points when zoomed in, else per column at
    // its last bar (per-column sampling repeats the same bar's value across
    // many columns when zoomed in → staircase steps) ---
    let series = [
        &chart.ind.ema10,
        &chart.ind.ema20,
        &chart.ind.ema50,
        &chart.ind.sma200,
        &chart.ind.vwap,
    ];
    for &(si, color) in LINE_SERIES {
        let s = series[si];
        let cc = c32(color);
        let mut prev: Option<Pos2> = None;
        let mut emit = |x: f32, v: f64, out: &mut Vec<Vert>| {
            if v.is_nan() {
                prev = None;
                return;
            }
            let p = Pos2::new(x, y_of(v));
            if let Some(q) = prev {
                seg(&n, q, p, 1.5, cc, out);
            }
            prev = Some(p);
        };
        if bpp < 1.0 {
            for i in bi0..bi1 {
                emit(chart.bar_x(i, plot, bpp), s[i], &mut out);
            }
        } else {
            for c in cols {
                emit(0.5 * (c.x0 + c.x1), s[c.i_last], &mut out);
            }
        }
    }

    out
}
