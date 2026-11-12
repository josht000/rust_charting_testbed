//! Experimental data-space chart renderer. Geometry is rebuilt for each view.

use crate::chart::{Chart, ViewMeta};
use crate::raster::{Rgb, BG, DOWN, EMA10, EMA20, EMA50, GRID, PROFILE, SMA200, UP, VWAP};
use slint::wgpu_30::wgpu;

const SHADER: &str = r#"
struct View {
    // start relative to the index origin, index span, price span, unused
    bounds: vec4<f32>,
    // physical width and height
    viewport: vec4<f32>,
};
@group(0) @binding(0) var<uniform> view: View;

struct Vertex {
    @location(0) a: vec2<f32>,
    @location(1) b: vec2<f32>,
    // primitive type, corner x, corner y, unused
    @location(2) options: vec4<f32>,
    @location(3) color: vec4<f32>,
};
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};
fn point(p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        (p.x - view.bounds.x) / view.bounds.y * view.viewport.x,
        -p.y / view.bounds.z * (view.viewport.y - 1.0));
}
@vertex fn vs(v: Vertex) -> Output {
    let kind = u32(v.options.x);
    let corner = v.options.yz;
    var xy: vec2<f32>;
    if kind == 5u {
        let a = point(v.a);
        let b = point(v.b);
        let d = b - a;
        var normal = vec2<f32>(0.0, 0.5);
        if length(d) > 0.000000001 {
            normal = vec2<f32>(-d.y, d.x) / length(d) * 0.5;
        }
        xy = mix(a, b, corner.x) + normal * (1.0 - 2.0 * corner.y);
    } else {
        var left = v.a.x;
        var right = v.b.x;
        var top = v.a.y;
        var bottom = v.b.y;
        if kind != 0u {
            top = -v.a.y / view.bounds.z * (view.viewport.y - 1.0);
            bottom = -v.b.y / view.bounds.z * (view.viewport.y - 1.0);
        }
        if kind == 1u || kind == 2u {
            let center = (v.a.x + 0.5 - view.bounds.x) / view.bounds.y * view.viewport.x;
            if kind == 1u {
                left = floor(center + 0.5);
                right = left + 1.0;
            } else {
                let half = max(max(view.viewport.x / view.bounds.y * 0.7, 1.0) / 2.0, 0.5);
                left = floor(center - half);
                right = ceil(center + half);
            }
        }
        if kind == 2u || kind == 4u {
            bottom = max(bottom, top + 1.0);
        }
        xy = vec2<f32>(mix(left, right, corner.x), mix(top, bottom, corner.y));
    }
    var out: Output;
    out.position = vec4<f32>(xy.x / view.viewport.x * 2.0 - 1.0,
                             1.0 - xy.y / view.viewport.y * 2.0, 0.0, 1.0);
    out.color = v.color;
    return out;
}
@fragment fn fs(v: Output) -> @location(0) vec4<f32> { return v.color; }
"#;

const CORNERS: [[f32; 2]; 6] = [
    [0.0, 0.0],
    [1.0, 0.0],
    [1.0, 1.0],
    [0.0, 0.0],
    [1.0, 1.0],
    [0.0, 1.0],
];

#[derive(Clone, Copy, Debug)]
struct Vertex {
    a: [f32; 2],
    b: [f32; 2],
    options: [f32; 4],
    color: [f32; 4],
}

impl Vertex {
    fn append_bytes(&self, out: &mut Vec<u8>) {
        for value in self
            .a
            .into_iter()
            .chain(self.b)
            .chain(self.options)
            .chain(self.color)
        {
            out.extend_from_slice(&value.to_ne_bytes());
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct View {
    index_origin: f64,
    price_origin: f64,
    bounds: [f32; 4],
    viewport: [f32; 4],
}

impl View {
    fn new(m: &ViewMeta, w: u32, h: u32) -> Self {
        let index_origin = m.start.floor();
        let price_origin = m.hi;
        let span = m.end - m.start;
        let price_span = m.hi - m.lo;
        assert!(
            span.is_finite() && span > 0.0 && price_span.is_finite() && price_span > 0.0,
            "invalid GPU chart view"
        );
        let bounds = [
            (m.start - index_origin) as f32,
            span as f32,
            price_span as f32,
            0.0,
        ];
        assert!(
            bounds[1].is_finite() && bounds[1] > 0.0 && bounds[2].is_finite() && bounds[2] > 0.0,
            "GPU chart view is outside f32 range"
        );
        Self {
            index_origin,
            price_origin,
            bounds,
            viewport: [w as f32, h as f32, 0.0, 0.0],
        }
    }

    fn index(&self, i: usize) -> f32 {
        (i as f64 - self.index_origin) as f32
    }
    fn price(&self, p: f64) -> f32 {
        let value = (p - self.price_origin) as f32;
        assert!(value.is_finite(), "non-finite GPU chart price");
        value
    }
    fn bytes(&self) -> Vec<u8> {
        self.bounds
            .into_iter()
            .chain(self.viewport)
            .flat_map(f32::to_ne_bytes)
            .collect()
    }
}

fn quad(out: &mut Vec<Vertex>, kind: u32, a: [f32; 2], b: [f32; 2], col: Rgb, alpha: u8) {
    let color = [
        col.0 as f32 / 255.0,
        col.1 as f32 / 255.0,
        col.2 as f32 / 255.0,
        alpha as f32 / 255.0,
    ];
    out.extend(CORNERS.iter().map(|corner| Vertex {
        a,
        b,
        options: [kind as f32, corner[0], corner[1], 0.0],
        color,
    }));
}

fn geometry(chart: &Chart, view: &View, m: &ViewMeta) -> Vec<Vertex> {
    let w = view.viewport[0] as f64;
    let h = view.viewport[1] as f64;
    let mut out = Vec::new();
    for k in 1..6 {
        let y = (h * k as f64 / 6.0).round() as f32;
        quad(&mut out, 0, [0.0, y], [w as f32, y + 1.0], GRID, 255);
    }
    let visible = m.i1 - m.i0 + 1;
    if (m.end - m.start) / w > 2.0 {
        for x in 0..w as usize {
            let a = m.i0 + (x as f64 * visible as f64 / w).floor() as usize;
            let b = m.i0 + (((x + 1) as f64 * visible as f64 / w).floor() as usize).max(1) - 1;
            let b = b.min(m.i1).max(a);
            let slice = &chart.bars[a.min(m.i1)..=b];
            let low = slice
                .iter()
                .map(|bar| bar.low)
                .fold(f64::INFINITY, f64::min);
            let high = slice.iter().map(|bar| bar.high).fold(0.0f64, f64::max);
            let open = slice[0].open;
            let close = slice[slice.len() - 1].close;
            let color = if close >= open { UP } else { DOWN };
            quad(
                &mut out,
                3,
                [x as f32, view.price(high)],
                [x as f32 + 1.0, view.price(low)],
                color,
                255,
            );
            quad(
                &mut out,
                3,
                [x as f32, view.price(open.max(close))],
                [x as f32 + 1.0, view.price(open.min(close))],
                color,
                255,
            );
        }
    } else {
        for i in m.i0..=m.i1 {
            let bar = &chart.bars[i];
            let color = if bar.close >= bar.open { UP } else { DOWN };
            let x = view.index(i);
            quad(
                &mut out,
                1,
                [x, view.price(bar.high)],
                [x, view.price(bar.low)],
                color,
                255,
            );
            quad(
                &mut out,
                2,
                [x, view.price(bar.open.max(bar.close))],
                [x, view.price(bar.open.min(bar.close))],
                color,
                255,
            );
        }
    }
    let stride = (visible / (w as usize * 3)).max(1).next_power_of_two();
    for (series, color) in [
        (&chart.ind.sma200, SMA200),
        (&chart.ind.ema50, EMA50),
        (&chart.ind.ema20, EMA20),
        (&chart.ind.ema10, EMA10),
        (&chart.ind.vwap, VWAP),
    ] {
        let mut i = m.i0 / stride * stride;
        while i < m.i1 {
            let j = (i + stride).min(m.i1);
            if series[i].is_finite() && series[j].is_finite() {
                quad(
                    &mut out,
                    5,
                    [view.index(i), view.price(series[i])],
                    [view.index(j), view.price(series[j])],
                    color,
                    255,
                );
            }
            i = j;
        }
    }
    let maxv = chart
        .ind
        .profile
        .iter()
        .map(|bin| bin.volume)
        .fold(0.0f64, f64::max);
    if maxv > 0.0 {
        let region = (w * 0.22).min(150.0);
        for bin in &chart.ind.profile {
            let left = w - bin.volume / maxv * region;
            quad(
                &mut out,
                4,
                [left as f32, view.price(bin.price_high)],
                [w as f32, view.price(bin.price_low)],
                PROFILE,
                128,
            );
        }
    }
    out
}

pub struct ChartGpu {
    view: wgpu::TextureView,
    pub img: slint::Image,
    vbuf: wgpu::Buffer,
    capacity: u64,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    w: u32,
    h: u32,
}

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

impl ChartGpu {
    pub fn size(&self) -> (u32, u32) {
        (self.w, self.h)
    }
}

impl Gpu {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("data-space chart shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("chart view"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("data-space chart pipeline"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("data-space chart pipeline"), layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader, entry_point: Some("vs"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 48, step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4],
                })], compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm, blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })], compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(), depth_stencil: None, multisample: Default::default(),
            multiview_mask: None, cache: None,
        });
        Self {
            device,
            queue,
            pipeline,
            layout,
        }
    }

    fn new_slot(&self, w: u32, h: u32) -> ChartGpu {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("data-space chart texture"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let img = slint::Image::try_from(texture).expect("chart texture import");
        let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("chart view uniform"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("chart view bind group"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let capacity = 1 << 20;
        let vbuf = self.vertex_buffer(capacity);
        ChartGpu {
            view,
            img,
            vbuf,
            capacity,
            uniform,
            bind_group,
            w,
            h,
        }
    }

    fn vertex_buffer(&self, size: u64) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("data-space chart vertices"),
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    pub fn render(&self, slot: &mut Option<ChartGpu>, chart: &Chart) {
        let (w, h) = (chart.w.max(8), chart.h.max(8));
        if slot.as_ref().map_or(true, |s| s.w != w || s.h != h) {
            *slot = Some(self.new_slot(w, h));
        }
        let slot = slot.as_mut().expect("chart GPU slot");
        let view_and_vertices = chart.view_meta().map(|meta| {
            let view = View::new(&meta, w, h);
            let vertices = geometry(chart, &view, &meta);
            (view, vertices)
        });
        self.draw(
            slot,
            view_and_vertices
                .as_ref()
                .map(|(view, vertices)| (view, vertices.as_slice())),
        );
    }

    fn draw(&self, slot: &mut ChartGpu, data: Option<(&View, &[Vertex])>) {
        let mut bytes = Vec::new();
        if let Some((view, vertices)) = data {
            self.queue.write_buffer(&slot.uniform, 0, &view.bytes());
            for vertex in vertices {
                vertex.append_bytes(&mut bytes);
            }
        }
        let size = bytes.len() as u64;
        if size > slot.capacity {
            slot.capacity = size.next_power_of_two();
            slot.vbuf = self.vertex_buffer(slot.capacity);
        }
        if size > 0 {
            self.queue.write_buffer(&slot.vbuf, 0, &bytes);
        }
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("data-space chart"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &slot.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: BG.0 as f64 / 255.0,
                            g: BG.1 as f64 / 255.0,
                            b: BG.2 as f64 / 255.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if size > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &slot.bind_group, &[]);
                pass.set_vertex_buffer(0, slot.vbuf.slice(0..size));
                pass.draw(
                    0..u32::try_from(bytes.len() / 48).expect("too many chart vertices"),
                    0..1,
                );
            }
        }
        self.queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebased_view_preserves_precision() {
        let m = ViewMeta {
            start: 100_000_000.25,
            end: 100_000_010.25,
            lo: 1_000_000_000.0,
            hi: 1_000_000_000.01,
            len: 0,
            i0: 0,
            i1: 0,
        };
        let view = View::new(&m, 1200, 800);
        assert_eq!(view.bytes().len(), 32);
        assert_eq!(view.index(100_000_001), 1.0);
        for i in 0..=1000 {
            let p = m.lo + (m.hi - m.lo) * i as f64 / 1000.0;
            let gpu_y = -(view.price(p) / view.bounds[2]) * 799.0;
            let reference_y = (m.hi - p) / (m.hi - m.lo) * 799.0;
            assert!((gpu_y as f64 - reference_y).abs() < 0.5);
        }
        for i in 100_000_000..=100_000_010 {
            let gpu_x = (view.index(i) - view.bounds[0]) / view.bounds[1] * 1200.0;
            let reference_x = (i as f64 - m.start) / (m.end - m.start) * 1200.0;
            assert!((gpu_x as f64 - reference_x).abs() < 0.5);
        }
    }

    #[test]
    #[should_panic(expected = "invalid GPU chart view")]
    fn zero_view_span_is_rejected() {
        let m = ViewMeta {
            start: 5.0,
            end: 5.0,
            lo: 1.0,
            hi: 2.0,
            len: 8,
            i0: 0,
            i1: 7,
        };
        View::new(&m, 64, 32);
    }

    #[test]
    fn geometry_includes_all_layers_and_alpha() {
        let mut chart = Chart::new(0);
        chart.resize(400, 200);
        let m = chart.view_meta().unwrap();
        let view = View::new(&m, 400, 200);
        let vertices = geometry(&chart, &view, &m);
        assert!(vertices.iter().any(|v| v.options[0] == 3.0));
        assert!(vertices.iter().any(|v| v.options[0] == 5.0));
        assert!(vertices
            .iter()
            .any(|v| v.options[0] == 4.0 && v.color[3] == 128.0 / 255.0));
        chart.end = chart.start + 100.0;
        let m = chart.view_meta().unwrap();
        let view = View::new(&m, 400, 200);
        let vertices = geometry(&chart, &view, &m);
        assert!(vertices.iter().any(|v| v.options[0] == 1.0));
        assert!(vertices.iter().any(|v| v.options[0] == 2.0));
        let mut bytes = Vec::new();
        vertices[0].append_bytes(&mut bytes);
        assert_eq!(bytes.len(), 48);
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    #[ignore = "requires a working native GPU adapter"]
    fn transformed_primitives_render_and_blend_on_gpu() {
        use std::future::Future;
        use std::sync::Arc;
        use std::task::{Context, Poll, Wake, Waker};
        use std::time::Duration;

        struct ThreadWake(std::thread::Thread);
        impl Wake for ThreadWake {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }
        fn block_on<T>(future: impl Future<Output = T>) -> T {
            let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
            let mut context = Context::from_waker(&waker);
            let mut future = std::pin::pin!(future);
            loop {
                match future.as_mut().poll(&mut context) {
                    Poll::Ready(value) => return value,
                    Poll::Pending => std::thread::park_timeout(Duration::from_secs(5)),
                }
            }
        }

        let instance = wgpu::Instance::default();
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("GPU adapter");
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("GPU device");
        let gpu = Gpu::new(device, queue);
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &gpu.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let mut slot = ChartGpu {
            view: texture.create_view(&Default::default()),
            img: slint::Image::default(),
            vbuf: gpu.vertex_buffer(48),
            capacity: 48,
            uniform,
            bind_group,
            w: 64,
            h: 32,
        };
        let m = ViewMeta {
            start: 100_000_000.0,
            end: 100_000_064.0,
            lo: 1_000_000_000.0,
            hi: 1_000_000_031.0,
            len: 64,
            i0: 0,
            i1: 63,
        };
        let view = View::new(&m, 64, 32);
        let mut vertices = Vec::new();
        quad(
            &mut vertices,
            3,
            [8.0, view.price(m.hi - 4.0)],
            [16.0, view.price(m.hi - 12.0)],
            UP,
            255,
        );
        quad(
            &mut vertices,
            4,
            [8.0, view.price(m.hi - 4.0)],
            [16.0, view.price(m.hi - 12.0)],
            PROFILE,
            128,
        );
        quad(
            &mut vertices,
            2,
            [view.index(100_000_025), view.price(m.hi - 5.0)],
            [view.index(100_000_025), view.price(m.hi - 15.0)],
            DOWN,
            255,
        );
        quad(
            &mut vertices,
            5,
            [view.index(100_000_035), view.price(m.hi - 20.0)],
            [view.index(100_000_045), view.price(m.hi - 20.0)],
            EMA10,
            255,
        );
        quad(&mut vertices, 0, [0.0, 28.0], [64.0, 29.0], GRID, 255);
        quad(
            &mut vertices,
            1,
            [view.index(100_000_050), view.price(m.hi - 3.0)],
            [view.index(100_000_050), view.price(m.hi - 15.0)],
            UP,
            255,
        );
        gpu.draw(&mut slot, Some((&view, &vertices)));
        assert!(slot.capacity >= (vertices.len() * 48) as u64);

        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * 32,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(32),
                },
            },
            wgpu::Extent3d {
                width: 64,
                height: 32,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).unwrap()
            });
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for GPU");
        receiver
            .recv()
            .expect("map callback")
            .expect("map readback");
        let pixels = readback
            .slice(..)
            .get_mapped_range()
            .expect("mapped readback range");
        let pixel = |x: usize, y: usize| &pixels[y * 256 + x * 4..y * 256 + x * 4 + 4];
        for (actual, expected) in pixel(10, 8).iter().zip([34u8, 151, 192, 255]) {
            assert!(
                actual.abs_diff(expected) <= 1,
                "blended pixel: {:?}",
                pixel(10, 8)
            );
        }
        assert_eq!(pixel(25, 10), &[DOWN.0, DOWN.1, DOWN.2, 255]);
        assert_eq!(pixel(51, 8), &[UP.0, UP.1, UP.2, 255]);
        assert_eq!(pixel(4, 28), &[GRID.0, GRID.1, GRID.2, 255]);
        assert_eq!(pixel(0, 0), &[BG.0, BG.1, BG.2, 255]);
        assert!((19..=20).any(|y| pixel(40, y) == [EMA10.0, EMA10.1, EMA10.2, 255]));
        drop(pixels);
        readback.unmap();

        let old_gpu = crate::gpu::Gpu::new(gpu.device.clone(), gpu.queue.clone());
        let old_texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let read_texture = |texture: &wgpu::Texture| {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(32),
                    },
                },
                wgpu::Extent3d {
                    width: 64,
                    height: 32,
                    depth_or_array_layers: 1,
                },
            );
            gpu.queue.submit([encoder.finish()]);
            let (sender, receiver) = std::sync::mpsc::channel();
            readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    sender.send(result).unwrap()
                });
            gpu.device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("wait for GPU");
            receiver
                .recv()
                .expect("map callback")
                .expect("map readback");
            let data = readback
                .slice(..)
                .get_mapped_range()
                .expect("mapped readback range")
                .to_vec();
            readback.unmap();
            data
        };
        let mut chart = Chart::new(0);
        chart.resize(64, 32);
        for scenario in 0..5 {
            match scenario {
                1 => {
                    chart.start = 100.25;
                    chart.end = 140.25;
                }
                2 => chart.pan_px(-7.0),
                3 => chart.zoom_at(30.0, -180.0),
                4 => {
                    chart.stream_batch(100);
                }
                _ => {}
            }
            let meta = chart.view_meta().expect("fixture view");
            let view = View::new(&meta, 64, 32);
            let verts = geometry(&chart, &view, &meta);
            gpu.draw(&mut slot, Some((&view, &verts)));
            old_gpu.render_test_texture(
                &old_texture,
                &crate::geom::emit_primitives(&chart),
                64,
                32,
            );
            let transformed = read_texture(&texture);
            let old = read_texture(&old_texture);
            let different = transformed
                .chunks_exact(4)
                .zip(old.chunks_exact(4))
                .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 2))
                .count();
            assert!(
                different <= 102,
                "scenario {scenario}: {different} of 2048 pixels differ"
            );
            assert!(transformed
                .chunks_exact(4)
                .any(|p| p != [BG.0, BG.1, BG.2, 255]));
            let cpu = chart.render_frame(None).expect("CPU fixture frame");
            let cpu_different = transformed
                .chunks_exact(4)
                .zip(cpu.pb.as_bytes().chunks_exact(4))
                .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 2))
                .count();
            assert!(
                cpu_different <= 716,
                "scenario {scenario}: {cpu_different} of 2048 pixels differ from CPU"
            );
        }
        gpu.draw(&mut slot, None);
        let empty = read_texture(&texture);
        assert!(empty.chunks_exact(4).all(|p| p == [BG.0, BG.1, BG.2, 255]));
        let mut imported_slot = None;
        gpu.render(&mut imported_slot, &chart);
        let initial_capacity = imported_slot.as_ref().unwrap().capacity;
        gpu.render(&mut imported_slot, &chart);
        assert_eq!(initial_capacity, imported_slot.as_ref().unwrap().capacity);
        assert_eq!(imported_slot.as_ref().unwrap().size(), (64, 32));
        assert!(chart.resize(96, 64));
        gpu.render(&mut imported_slot, &chart);
        assert_eq!(imported_slot.as_ref().unwrap().size(), (96, 64));
        chart.bars.clear();
        gpu.render(&mut imported_slot, &chart);
        assert!(imported_slot.is_some());
    }
}
