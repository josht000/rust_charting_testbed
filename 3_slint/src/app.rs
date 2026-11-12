//! UI wiring: window creation, per-chart image updates, pan/zoom callbacks,
//! live-update timers and benchmark startup.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::bench::{self, Bench};
use crate::chart::Chart;
use crate::plat::{bench_flag, now_ms, stream_rate};
use crate::AppWindow;

pub struct App {
    pub charts: RefCell<Vec<Chart>>,
    pub ui: slint::Weak<AppWindow>,
    pub stats_model: Rc<VecModel<SharedString>>,
    pub pan_idx: Cell<i32>,
    pub pan_x: Cell<f32>,
    pub bench: RefCell<Option<Rc<Bench>>>,
    /// Last rendered frame per chart, for incremental (blit) re-rendering.
    pub frame_cache: RefCell<Vec<Option<crate::chart::Frame>>>,
    mode: RenderMode,
    /// wgpu render mode: shared device/queue pipeline + per-chart textures.
    gpu: RefCell<Option<crate::gpu::Gpu>>,
    chart_gpu: RefCell<Vec<Option<crate::gpu::ChartGpu>>>,
    transform_gpu: RefCell<Option<crate::gpu_transform::Gpu>>,
    chart_transform_gpu: RefCell<Vec<Option<crate::gpu_transform::ChartGpu>>>,
    updates_started: Cell<bool>,
}

fn set_chart_image(ui: &AppWindow, idx: usize, img: slint::Image) {
    match idx {
        0 => ui.set_chart0(img),
        1 => ui.set_chart1(img),
        2 => ui.set_chart2(img),
        3 => ui.set_chart3(img),
        4 => ui.set_chart4(img),
        5 => ui.set_chart5(img),
        6 => ui.set_chart6(img),
        7 => ui.set_chart7(img),
        _ => {}
    }
}

fn set_chart_layers(ui: &AppWindow, idx: usize, layers: Vec<crate::ChartLayer>) {
    let m = ModelRc::from(Rc::new(VecModel::from(layers)));
    match idx {
        0 => ui.set_layers0(m),
        1 => ui.set_layers1(m),
        2 => ui.set_layers2(m),
        3 => ui.set_layers3(m),
        4 => ui.set_layers4(m),
        5 => ui.set_layers5(m),
        6 => ui.set_layers6(m),
        7 => ui.set_layers7(m),
        _ => {}
    }
}

/// Chart presentation path: pixel-buffer images (default), emitted Path
/// geometry (`SLINT_TB_RENDER=paths` / `&render=paths`), or wgpu-rendered
/// chart textures (`SLINT_TB_RENDER=wgpu` / `?render=wgpu`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum RenderMode {
    Pixels,
    Paths,
    Wgpu,
    WgpuTransform,
}

impl RenderMode {
    fn uses_wgpu(self) -> bool {
        matches!(self, Self::Wgpu | Self::WgpuTransform)
    }
}

#[cfg(not(target_family = "wasm"))]
fn render_mode() -> RenderMode {
    match std::env::var("SLINT_TB_RENDER") {
        Err(std::env::VarError::NotPresent) => RenderMode::Pixels,
        Err(e) => panic!("invalid SLINT_TB_RENDER: {e}"),
        Ok(value) => parse_render_mode(Some(&value))
            .unwrap_or_else(|| panic!("unsupported chart renderer: {value}")),
    }
}

#[cfg(target_family = "wasm")]
fn render_mode() -> RenderMode {
    let query = web_sys::window().expect("browser window").location().search().expect("URL query");
    let params = web_sys::UrlSearchParams::new_with_str(&query).expect("valid URL query");
    let value = params.get("render");
    parse_render_mode(value.as_deref())
        .unwrap_or_else(|| panic!("unsupported chart renderer: {}", value.unwrap_or_default()))
}

fn parse_render_mode(value: Option<&str>) -> Option<RenderMode> {
    match value {
        None | Some("pixels") => Some(RenderMode::Pixels),
        Some("paths") => Some(RenderMode::Paths),
        Some("wgpu") => Some(RenderMode::Wgpu),
        Some("wgpu-transform") => Some(RenderMode::WgpuTransform),
        _ => None,
    }
}

impl App {
    pub fn render_mode_name(&self) -> &'static str {
        match self.mode {
            RenderMode::Pixels => "pixels",
            RenderMode::Paths => "paths",
            RenderMode::Wgpu => "wgpu",
            RenderMode::WgpuTransform => "wgpu-transform",
        }
    }

    pub fn gpu_backend_name(&self) -> Option<String> {
        self.gpu.borrow().as_ref().map(|gpu| format!("{:?}", gpu.device.adapter_info().backend))
    }

    pub fn gpu_adapter_name(&self) -> Option<String> {
        self.gpu.borrow().as_ref().map(|gpu| gpu.device.adapter_info().name)
    }

    /// Software-render chart `idx` and push the image into the UI.
    /// Re-renders incrementally (pan blit) when possible.
    pub fn render_chart(&self, idx: usize) {
        let Some(ui) = self.ui.upgrade() else { return };
        match self.mode {
            RenderMode::Paths => {
                let layers = self.charts.borrow()[idx].layers();
                set_chart_layers(&ui, idx, layers);
            }
            RenderMode::Wgpu | RenderMode::WgpuTransform => self.render_chart_gpu(&ui, idx),
            _ => {
                let prev = self.frame_cache.borrow_mut()[idx].take();
                let out = self.charts.borrow()[idx].render_frame(prev.as_ref());
                match out {
                    Some(f) => {
                        set_chart_image(&ui, idx, slint::Image::from_rgba8(f.pb.clone()));
                        self.frame_cache.borrow_mut()[idx] = Some(f);
                    }
                    None => self.frame_cache.borrow_mut()[idx] = prev, // unchanged
                }
            }
        }
        if let Some(b) = self.bench.borrow().as_ref() {
            b.on_first_chart();
        }
    }

    /// wgpu mode: draw chart `idx` into its texture via geometry primitives;
    /// (re)bind the imported image only when the texture handle changed (first frame / resize).
    fn render_chart_gpu(&self, ui: &AppWindow, idx: usize) {
        let guard = self.gpu.borrow();
        let Some(g) = guard.as_ref() else { return }; // device not captured yet
        let charts = self.charts.borrow();
        let chart = &charts[idx];

        if self.mode == RenderMode::WgpuTransform {
            let transform = self.transform_gpu.borrow();
            let g = transform.as_ref().expect("transform GPU initialized with device");
            let mut slots = self.chart_transform_gpu.borrow_mut();
            let rebind = slots[idx].as_ref().map_or(true, |s| s.size() != (chart.w.max(8), chart.h.max(8)));
            g.render(&mut slots[idx], chart);
            if rebind {
                set_chart_image(ui, idx, slots[idx].as_ref().expect("transform chart slot").img.clone());
            }
            return;
        }

        // Phase 2: Extract geometry primitives (CPU cost, but decoupled)
        let prims = crate::geom::emit_primitives(chart);

        // Phase 2: Render primitives as GPU triangle mesh
        let mut slots = self.chart_gpu.borrow_mut();
        let rebind = slots[idx].as_ref().map_or(true, |s| (s.w, s.h) != (chart.w.max(8), chart.h.max(8)));
        g.render_from_primitives(&mut slots[idx], &prims, chart.w, chart.h);
        if rebind {
            set_chart_image(ui, idx, slots[idx].as_ref().expect("chart GPU slot").img.clone());
        }
    }

    /// Render every chart into an image. Native builds rasterize the 8 chart
    /// buffers in parallel with rayon; wasm stays sequential (no threads).
    /// These call sites always change bar data (stream/live), so incremental
    /// reuse is never eligible — prev = None on purpose.
    fn render_all(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let charts = self.charts.borrow();
        match self.mode {
            RenderMode::Wgpu | RenderMode::WgpuTransform => {
                drop(charts);
                for i in 0..8 {
                    self.render_chart_gpu(&ui, i);
                }
            }
            RenderMode::Paths => {
                for (i, c) in charts.iter().enumerate() {
                    set_chart_layers(&ui, i, c.layers());
                }
            }
            _ => {
                // SharedPixelBuffer is Send (atomic-refcounted); slint::Image
                // is not (VRc variants), so wrap on the UI thread after the map.
                #[cfg(not(target_family = "wasm"))]
                let frames = {
                    use rayon::prelude::*;
                    charts.par_iter().map(|c| c.render_frame(None)).collect::<Vec<_>>()
                };
                #[cfg(target_family = "wasm")]
                let frames: Vec<_> =
                    charts.iter().map(|c| c.render_frame(None)).collect();
                let mut cache = self.frame_cache.borrow_mut();
                for (i, f) in frames.into_iter().enumerate() {
                    if let Some(f) = f {
                        set_chart_image(&ui, i, slint::Image::from_rgba8(f.pb.clone()));
                        cache[i] = Some(f);
                    }
                }
            }
        }
        if let Some(b) = self.bench.borrow().as_ref() {
            b.on_first_chart();
        }
    }

    /// Append one live bar to every chart and re-render them.
    pub fn push_bar_all(&self) {
        {
            let mut charts = self.charts.borrow_mut();
            for c in charts.iter_mut() {
                c.push_live_bar();
            }
        }
        self.update_stats();
        self.render_all();
        self.request_redraw();
    }

    fn update_stats(&self) {
        let charts = self.charts.borrow();
        let stats: Vec<SharedString> =
            charts.iter().map(|c| c.stats_line().into()).collect();
        drop(charts);
        for (i, s) in stats.into_iter().enumerate() {
            self.stats_model.set_row_data(i, s);
        }
    }

    /// One streaming tick: push `batch` real generated bars through every
    /// chart's incremental indicator path, re-render, return
    /// (finished_charts, total_consumed_bars).
    pub fn stream_tick_all(&self, batch: usize) -> (usize, usize) {
        if let Some(b) = self.bench.borrow().as_ref() {
            b.begin_action();
        }
        let mut done = 0usize;
        let mut consumed = 0usize;
        {
            let mut charts = self.charts.borrow_mut();
            for c in charts.iter_mut() {
                c.stream_batch(batch);
                consumed += c.bars.len();
                if c.streaming_done() {
                    done += 1;
                }
            }
        }
        self.update_stats();
        self.render_all();
        self.request_redraw();
        (done, consumed)
    }

    /// Replay the remaining (BARS_PER_DAY - INITIAL_BARS) bars of every chart
    /// at `rate` bars/sec in ~50 ms batches. `on_done` fires once all 8
    /// streams are exhausted.
    pub fn start_stream(self: &Rc<Self>, rate: f64, on_done: Rc<dyn Fn(Rc<App>)>) {
        const PERIOD_MS: u64 = 50;
        let batch = ((rate * PERIOD_MS as f64 / 1000.0).ceil() as usize).max(1);
        let slot: Rc<RefCell<Option<Timer>>> = Rc::new(RefCell::new(None));
        let slot2 = slot.clone();
        let app = self.clone();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(PERIOD_MS), move || {
            let (done, consumed) = app.stream_tick_all(batch);
            if let Some(ui) = app.ui.upgrade() {
                let total = (datagen::NUM_CHARTS * datagen::BARS_PER_DAY) as f64;
                ui.set_status(
                    format!("streaming: {:.0}% ({consumed} bars)", consumed as f64 / total * 100.0).into(),
                );
            }
            if done >= 8 {
                if let Some(t) = slot2.borrow_mut().take() {
                    t.stop();
                }
                on_done(app.clone());
            }
        });
        *slot.borrow_mut() = Some(timer);
    }

    pub fn request_redraw(&self) {
        if let Some(ui) = self.ui.upgrade() {
            ui.window().request_redraw();
        }
    }
}

pub fn run() {
    // Data + indicators are generated before the app timer starts:
    // data generation is excluded from startup per the benchmark rules.
    let charts: Vec<Chart> = (0..8).map(Chart::new).collect();

    let mode = render_mode();
    // wgpu chart mode requires Slint's wgpu 30 renderer (femtovg-on-wgpu).
    if mode.uses_wgpu() {
        let settings = slint::wgpu_30::WGPUSettings::default();
        #[cfg(target_family = "wasm")]
        let settings = {
            let mut settings = settings;
            let query = web_sys::window().expect("browser window").location().search().expect("URL query");
            let params = web_sys::UrlSearchParams::new_with_str(&query).expect("valid URL query");
            settings.backends = match params.get("gpu").as_deref() {
                None | Some("auto") => settings.backends,
                Some("webgpu") => slint::wgpu_30::wgpu::Backends::BROWSER_WEBGPU,
                Some("webgl") => slint::wgpu_30::wgpu::Backends::GL,
                Some(other) => panic!("unsupported GPU backend: {other}"),
            };
            settings
        };
        slint::BackendSelector::new()
            .require_wgpu_30(slint::wgpu_30::WGPUConfiguration::Automatic(settings))
            .select()
            .expect("wgpu 30 renderer required");
    }
    #[cfg(target_family = "wasm")]
    if !mode.uses_wgpu() {
        slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name("femtovg".into())
            .select()
            .expect("the web CPU baseline requires Slint's WebGL FemtoVG renderer");
    }

    let t_app = now_ms();
    let ui = match AppWindow::new() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("failed to create window: {e}");
            return;
        }
    };

    let titles: Vec<SharedString> = (0..8).map(|i| format!("Chart {i} — 1s bars").into()).collect();
    let stats_model = Rc::new(VecModel::from(
        charts.iter().map(|c| c.stats_line().into()).collect::<Vec<SharedString>>(),
    ));
    ui.set_titles(ModelRc::from(Rc::new(VecModel::from(titles))));
    ui.set_stats(ModelRc::from(stats_model.clone()));
    ui.set_status("drag: pan | wheel: zoom | --bench / ?bench=1: scripted benchmark".into());

    let app = Rc::new(App {
        charts: RefCell::new(charts),
        ui: ui.as_weak(),
        stats_model,
        pan_idx: Cell::new(-1),
        pan_x: Cell::new(0.0f32),
        bench: RefCell::new(None),
        frame_cache: RefCell::new((0..8).map(|_| None).collect()),
        mode,
        gpu: RefCell::new(None),
        chart_gpu: RefCell::new((0..8).map(|_| None).collect()),
        transform_gpu: RefCell::new(None),
        chart_transform_gpu: RefCell::new((0..8).map(|_| None).collect()),
        updates_started: Cell::new(false),
    });

    {
        let app = app.clone();
        ui.on_resized(move |idx, w, h| {
            if w < 2.0 || h < 2.0 {
                return;
            }
            // Render at physical pixels — the panel size is fractional logical
            // px, and stretching a truncated buffer is a per-frame shimmer.
            let sf = app.ui.upgrade().map(|u| u.window().scale_factor()).unwrap_or(1.0);
            let nw = (w * sf).round().max(2.0) as u32;
            let nh = (h * sf).round().max(2.0) as u32;
            if std::env::var("SLINT_TB_DEBUG").is_ok() {
                eprintln!("resized chart={idx} logical={w:.2}x{h:.2} sf={sf} phys={nw}x{nh}");
            }
            let changed = app.charts.borrow_mut()[idx as usize].resize(nw, nh);
            if changed {
                app.render_chart(idx as usize);
            }
        });
    }
    {
        let app = app.clone();
        ui.on_pan_start(move |idx, x| {
            app.pan_idx.set(idx);
            app.pan_x.set(x);
        });
    }
    {
        let app = app.clone();
        ui.on_pan_move(move |idx, x| {
            if app.pan_idx.get() != idx {
                return;
            }
            let sf = app.ui.upgrade().map(|u| u.window().scale_factor()).unwrap_or(1.0);
            let dx = (x - app.pan_x.get()) as f64 * sf as f64;
            app.pan_x.set(x);
            app.charts.borrow_mut()[idx as usize].pan_px(dx);
            app.render_chart(idx as usize);
            app.request_redraw();
        });
    }
    {
        let app = app.clone();
        ui.on_pan_end(move |idx| {
            if app.pan_idx.get() == idx {
                app.pan_idx.set(-1);
            }
        });
    }
    {
        let app = app.clone();
        ui.on_zoomed(move |idx, dy, x| {
            let sf = app.ui.upgrade().map(|u| u.window().scale_factor()).unwrap_or(1.0);
            app.charts.borrow_mut()[idx as usize].zoom_at(x as f64 * sf as f64, dy as f64);
            app.render_chart(idx as usize);
            app.request_redraw();
        });
    }

    let is_bench = bench_flag();
    let rate = stream_rate().unwrap_or(if is_bench { 2000.0 } else { 1000.0 });

    if is_bench || mode.uses_wgpu() {
        // Frame-latency sampling + wgpu device/queue capture.
        let app2 = app.clone();
        ui.window().set_rendering_notifier(move |state, api| {
            if app2.mode.uses_wgpu() {
                if let slint::RenderingState::RenderingSetup = state {
                    if let slint::GraphicsAPI::WGPU30 { device, queue, .. } = api {
                        if app2.gpu.borrow().is_none() {
                            if app2.mode == RenderMode::WgpuTransform {
                                *app2.transform_gpu.borrow_mut() = Some(
                                    crate::gpu_transform::Gpu::new(device.clone(), queue.clone())
                                );
                            }
                            *app2.gpu.borrow_mut() =
                                Some(crate::gpu::Gpu::new(device.clone(), queue.clone()));
                            let weak = Rc::downgrade(&app2);
                            Timer::single_shot(Duration::ZERO, move || {
                                if let Some(app) = weak.upgrade() {
                                    start_updates(&app, t_app, is_bench, rate);
                                    app.render_all();
                                    app.request_redraw();
                                }
                            });
                        }
                    } else {
                        panic!("wgpu chart renderer requires Slint's WGPU30 graphics API");
                    }
                }
                if let slint::RenderingState::RenderingTeardown = state {
                    app2.chart_gpu.borrow_mut().iter_mut().for_each(|slot| *slot = None);
                    app2.chart_transform_gpu.borrow_mut().iter_mut().for_each(|slot| *slot = None);
                    app2.gpu.borrow_mut().take();
                    app2.transform_gpu.borrow_mut().take();
                }
            }
            if let slint::RenderingState::AfterRendering = state {
                if let Some(b) = app2.bench.borrow().as_ref() {
                    b.on_frame();
                }
            }
        }).expect("rendering notifier is required for GPU rendering and benchmarks");
    }

    if !mode.uses_wgpu() {
        start_updates(&app, t_app, is_bench, rate);
    }

    ui.run().expect("Slint event loop failed");
}

fn start_updates(app: &Rc<App>, t_app: f64, is_bench: bool, rate: f64) {
    if app.updates_started.replace(true) {
        return;
    }
    let done_cb: Rc<dyn Fn(Rc<App>)> = if is_bench {
        let app2 = app.clone();
        bench::start(app2, t_app, rate);
        Rc::new(bench::on_stream_done)
    } else {
        Rc::new(|app: Rc<App>| {
            // Catch-up finished -> synthetic live updates at 1 Hz.
            if let Some(ui) = app.ui.upgrade() {
                ui.set_status(
                    "caught up — live 1 Hz | drag: pan | wheel: zoom".into(),
                );
            }
            let a2 = app.clone();
            let timer = Timer::default();
            timer.start(TimerMode::Repeated, Duration::from_millis(1000), move || {
                a2.push_bar_all();
            });
            std::mem::forget(timer); // keep alive for the app lifetime
        })
    };
    app.start_stream(rate, done_cb);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_render_mode_is_explicit() {
        assert!(parse_render_mode(None) == Some(RenderMode::Pixels));
        assert!(parse_render_mode(Some("pixels")) == Some(RenderMode::Pixels));
        assert!(parse_render_mode(Some("paths")) == Some(RenderMode::Paths));
        assert!(parse_render_mode(Some("wgpu")) == Some(RenderMode::Wgpu));
        assert!(parse_render_mode(Some("wgpu-transform")) == Some(RenderMode::WgpuTransform));
        assert!(parse_render_mode(Some("webgpu")).is_none());
    }
}
