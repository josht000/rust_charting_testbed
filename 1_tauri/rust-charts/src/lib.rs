//! wgpu candlestick renderer for the Tauri web shell (item 3 hybrid).
//! JS passes the visible window's bars as flat f64 arrays plus the view
//! range; we rasterize candles/lines to quads in NDC space and draw with a
//! single pipeline/draw call. Coordinate conversion (index->x, price->y)
//! happens here from the range params — same model as 2_egui/src/gpu.rs.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct GpuChart {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    vbuf: wgpu::Buffer,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vert {
    pos: [f32; 2],
    col: [f32; 4],
}

const WGSL: &str = r#"
struct V { @location(0) pos: vec2<f32>, @location(1) col: vec4<f32> };
struct O { @builtin(position) p: vec4<f32>, @location(0) col: vec4<f32> };
@vertex fn vs(v: V) -> O { var o: O; o.p = vec4(v.pos, 0., 1.); o.col = v.col; return o; }
@fragment fn fs(o: O) -> @location(0) vec4<f32> { return o.col; }
"#;

#[wasm_bindgen]
impl GpuChart {
    /// Attach to an existing canvas. Async because adapter/device request.
    pub async fn create(canvas: web_sys::HtmlCanvasElement) -> Result<GpuChart, JsValue> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| JsValue::from_str(&format!("surface: {e}")))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|e| JsValue::from_str(&format!("adapter: {e}")))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| JsValue::from_str(&format!("device: {e}")))?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 1,
            height: 1,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(WGSL.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vert>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 1 << 22,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(GpuChart { surface, device, queue, config, pipeline, vbuf })
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
    }

    /// Draw the visible window.
    /// `bars`: flat [open, high, low, close] * n for indices `i0..i0+n`.
    /// `view`: [i0, i1, price_min, price_max, bar_spacing_px, pane_w, pane_h].
    /// TODO next iteration: per-line vertex segments + volume profile bins.
    pub fn render(&self, bars: &[f64], view: &[f64]) -> Result<(), JsValue> {
        let (_i0, _i1, pmin, pmax, spacing, pw, ph) = (
            view[0], view[1], view[2], view[3], view[4], view[5], view[6],
        );
        let mut verts: Vec<Vert> = Vec::with_capacity(bars.len() / 4 * 18);
        let mut push_quad = |x0: f32, y0: f32, x1: f32, y1: f32, c: [f32; 4]| {
            let tl = Vert { pos: [x0, y0], col: c };
            let tr = Vert { pos: [x1, y0], col: c };
            let bl = Vert { pos: [x0, y1], col: c };
            let br = Vert { pos: [x1, y1], col: c };
            verts.extend_from_slice(&[tl, bl, tr, tr, bl, br]);
        };
        let bw = (spacing * 0.7).max(1.0) as f32;
        for (i, b) in bars.chunks_exact(4).enumerate() {
            let (_o, h, l, c, o) = (b[0], b[1], b[2], b[3], b[0]);
            let col = if b[3] >= b[0] {
                [0.15, 0.65, 0.60, 1.0]
            } else {
                [0.94, 0.33, 0.31, 1.0]
            };
            let xc = ((i as f64 + 0.5) * spacing / pw) as f32 * 2.0 - 1.0;
            let ny = |p: f64| (1.0 - (p - pmin) / (pmax - pmin)) as f32 * 2.0 - 1.0;
            let (yh, yl, yo, yc) = (ny(h), ny(l), ny(o), ny(c));
            // wick
            push_quad(xc - 0.5 / pw as f32, yh, xc + 0.5 / pw as f32, yl, col);
            // body
            let bx = (bw / pw as f32) as f32;
            push_quad(xc - bx, yo.min(yc), xc + bx, yo.max(yc), col);
            let _ = o;
        }
        let n = verts.len() as u32;
        if n == 0 {
            return Ok(());
        }
        self.queue.write_buffer(&self.vbuf, 0, bytemuck::cast_slice(&verts));
        let frame = self
            .surface
            .get_current_texture()
            .map_err(|e| JsValue::from_str(&format!("frame: {e}")))?;
        let view = frame.texture.create_view(&Default::default());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(&self.pipeline);
            rp.set_vertex_buffer(0, self.vbuf.slice(..));
            rp.draw(0..n, 0..1);
        }
        self.queue.submit([enc.finish()]);
        frame.present();
        Ok(())
    }
}
