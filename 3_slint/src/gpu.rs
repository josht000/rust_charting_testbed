//! wgpu chart renderer (native only, improvement item 4): the same
//! per-column aggregation as the software rasterizer, emitted as colored
//! triangles into a per-chart `wgpu::Texture` imported into Slint via
//! `Image::try_from<wgpu::Texture>` — no CPU raster, no texture upload.
//!
//! Uses the same `wgpu::Device`/`wgpu::Queue` as Slint itself, captured from
//! `GraphicsAPI::WGPU30` in the rendering notifier.

use crate::chart::Chart;
use crate::raster::{Rgb, BG, DOWN, EMA10, EMA20, EMA50, GRID, PROFILE, SMA200, UP, VWAP};
use slint::wgpu_30::wgpu;

#[repr(C)]
#[derive(Clone, Copy)]
struct Vert {
    pos: [f32; 2],
    col: [f32; 4],
}

const SHADER: &str = r#"
struct V { @location(0) pos: vec2<f32>, @location(1) col: vec4<f32> };
struct O { @builtin(position) pos: vec4<f32>, @location(0) col: vec4<f32> };
@vertex fn vs(v: V) -> O {
    var o: O;
    o.pos = vec4<f32>(v.pos, 0.0, 1.0);
    o.col = v.col;
    return o;
}
@fragment fn fs(o: O) -> @location(0) vec4<f32> { return o.col; }
"#;

fn rgba(c: Rgb, a: f32) -> [f32; 4] {
    [c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0, a]
}

/// Primitive emitter producing NDC-space triangles for one chart.
struct Emit<'a> {
    v: &'a mut Vec<Vert>,
    w: f32,
    h: f32,
}

impl<'a> Emit<'a> {
    fn px(&self, x: f64, y: f64, col: [f32; 4]) -> Vert {
        Vert {
            pos: [(x as f32 / self.w) * 2.0 - 1.0, 1.0 - (y as f32 / self.h) * 2.0],
            col,
        }
    }
    /// Axis-aligned rect [x0,x1) x [y0,y1).
    fn rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, col: [f32; 4]) {
        let (a, b, c, d) = (
            self.px(x0, y0, col),
            self.px(x1, y0, col),
            self.px(x1, y1, col),
            self.px(x0, y1, col),
        );
        self.v.extend_from_slice(&[a, b, c, a, c, d]);
    }
    /// 1-px line as a thin quad (perpendicular half-width 0.5).
    fn seg(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, col: [f32; 4]) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = dx.hypot(dy);
        let (nx, ny) = if len > 1e-9 { (-dy / len * 0.5, dx / len * 0.5) } else { (0.0, 0.5) };
        let (a, b, c, d) = (
            self.px(x0 + nx, y0 + ny, col),
            self.px(x0 - nx, y0 - ny, col),
            self.px(x1 - nx, y1 - ny, col),
            self.px(x1 + nx, y1 + ny, col),
        );
        self.v.extend_from_slice(&[a, b, c, a, c, d]);
    }
}

/// One chart's GPU state: target texture + imported slint::Image + vertex buffer.
pub struct ChartGpu {
    view: wgpu::TextureView,
    pub img: slint::Image,
    vbuf: wgpu::Buffer,
    vcap: u64,
    pub w: u32,
    pub h: u32,
}

impl ChartGpu {
    pub fn new(device: &wgpu::Device, w: u32, h: u32) -> Self {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let img = slint::Image::try_from(tex.clone()).expect("texture import");
        Self {
            view,
            img,
            vbuf: device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 1 << 20,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            vcap: 1 << 20,
            w,
            h,
        }
    }

    fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue, pipeline: &wgpu::RenderPipeline, verts: &[Vert]) {
        if verts.is_empty() {
            return;
        }
        let need = (verts.len() * std::mem::size_of::<Vert>()) as u64;
        if need > self.vcap {
            return; // oversized frame — drop rather than stall; next resize fixes
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(verts.as_ptr() as *const u8, need as usize)
        };
        queue.write_buffer(&self.vbuf, 0, bytes);
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: BG.0 as f64 / 255.0, g: BG.1 as f64 / 255.0, b: BG.2 as f64 / 255.0, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_vertex_buffer(0, self.vbuf.slice(0..need));
            pass.draw(0..verts.len() as u32, 0..1);
        }
        queue.submit([enc.finish()]);
    }
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
}

impl Gpu {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vert>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { device, queue, pipeline }
    }

    /// Emit chart geometry and draw it into the chart's texture.
    /// `slot` is the per-chart ChartGpu (recreated on size change).
    pub fn render_chart(&self, slot: &mut Option<ChartGpu>, chart: &Chart) {
        let (w, h) = (chart.w.max(8), chart.h.max(8));
        if slot.as_ref().map_or(true, |s| s.w != w || s.h != h) {
            *slot = Some(ChartGpu::new(&self.device, w, h));
        }
        let mut verts = Vec::with_capacity(48_000);
        emit_verts(chart, &mut verts);
        slot.as_ref().unwrap().render(&self.device, &self.queue, &self.pipeline, &verts);
    }
}

/// Emit all chart geometry as triangles — same aggregation as `draw`/`layers`.
fn emit_verts(chart: &Chart, out: &mut Vec<Vert>) {
    let (w, h) = (chart.w.max(8) as f64, chart.h.max(8) as f64);
    let mut e = Emit { v: out, w: w as f32, h: h as f32 };
    let Some(m) = chart.view_meta() else { return };
    let (lo, hi) = (m.lo, m.hi);
    let py = |p: f64| -> f64 { (hi - p) / (hi - lo) * (h - 1.0) };
    let span = m.end - m.start;
    let px_of = |i: usize| -> f64 { (i as f64 - m.start) / span * w };
    let (i0, i1) = (m.i0, m.i1);

    // Grid (thin rects).
    let gc = rgba(GRID, 1.0);
    for k in 1..6 {
        let y = (h * k as f64 / 6.0).round();
        e.rect(0.0, y, w, y + 1.0, gc);
    }

    // Candles.
    let (uc, dc) = (rgba(UP, 1.0), rgba(DOWN, 1.0));
    let bars_per_px = span / w;
    let visible = i1 - i0 + 1;
    if bars_per_px > 2.0 {
        for x in 0..w as usize {
            let a = i0 + (x as f64 * visible as f64 / w).floor() as usize;
            let b = i0 + (((x + 1) as f64 * visible as f64 / w).floor() as usize).max(1) - 1;
            let b = b.min(i1).max(a);
            let slice = &chart.bars[a.min(i1)..=b];
            let mut clo = f64::INFINITY;
            let mut chi = 0.0f64;
            for bar in slice {
                clo = clo.min(bar.low);
                chi = chi.max(bar.high);
            }
            let o = slice[0].open;
            let c = slice[slice.len() - 1].close;
            let col = if c >= o { uc } else { dc };
            let (x0, x1) = (x as f64, x as f64 + 1.0);
            e.rect(x0, py(chi), x1, py(clo), col);
            e.rect(x0, py(o.max(c)), x1, py(o.min(c)), col);
        }
    } else {
        let cw = ((w / span) * 0.7).max(1.0);
        let half = (cw / 2.0).max(0.5);
        for i in i0..=i1 {
            let b = &chart.bars[i];
            let x = px_of(i) + w / span * 0.5;
            let col = if b.close >= b.open { uc } else { dc };
            let wx = x.round();
            e.rect(wx, py(b.high), wx + 1.0, py(b.low), col);
            let (top, bot) = (py(b.open.max(b.close)), py(b.open.min(b.close)));
            e.rect((x - half).floor(), top, (x + half).ceil(), bot.max(top + 1.0), col);
        }
    }

    // Indicator polylines (thin quads), grid-anchored like draw().
    let stride = (visible / (w as usize * 3)).max(1).next_power_of_two();
    let emit_series = |e: &mut Emit, series: &[f64], color: Rgb| {
        let col = rgba(color, 1.0);
        let pt = |i: usize| -> Option<(f64, f64)> {
            let v = series[i];
            if v.is_finite() { Some((px_of(i), py(v))) } else { None }
        };
        let mut i = i0 / stride * stride;
        let mut prev = pt(i);
        while i < i1 {
            let j = (i + stride).min(i1);
            let cur = pt(j);
            if let (Some(a), Some(b)) = (prev, cur) {
                e.seg(a.0, a.1, b.0, b.1, col);
            }
            prev = cur;
            i = j;
        }
    };
    emit_series(&mut e, &chart.ind.sma200, SMA200);
    emit_series(&mut e, &chart.ind.ema50, EMA50);
    emit_series(&mut e, &chart.ind.ema20, EMA20);
    emit_series(&mut e, &chart.ind.ema10, EMA10);
    emit_series(&mut e, &chart.ind.vwap, VWAP);

    // Volume profile: right edge, independent x-scale, 50% alpha.
    let prof = &chart.ind.profile;
    if !prof.is_empty() {
        let maxv = prof.iter().map(|b| b.volume).fold(0.0f64, f64::max);
        if maxv > 0.0 {
            let region = (w * 0.22).min(150.0);
            let pc = rgba(PROFILE, 0.5);
            for bin in prof {
                let y0 = py(bin.price_high);
                let y1 = py(bin.price_low);
                let len = bin.volume / maxv * region;
                e.rect(w - len, y0, w, y1.max(y0 + 1.0), pc);
            }
        }
    }
}
