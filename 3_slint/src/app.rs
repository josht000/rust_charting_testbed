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

impl App {
    /// Software-render chart `idx` and push the image into the UI.
    pub fn render_chart(&self, idx: usize) {
        let Some(ui) = self.ui.upgrade() else { return };
        let img = self.charts.borrow()[idx].render();
        set_chart_image(&ui, idx, img);
        if let Some(b) = self.bench.borrow().as_ref() {
            b.on_first_chart();
        }
    }

    /// Render every chart into an image. Native builds rasterize the 8 chart
    /// buffers in parallel with rayon; wasm stays sequential (no threads).
    fn render_all(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let charts = self.charts.borrow();
        // SharedPixelBuffer is Send (atomic-refcounted); slint::Image is not
        // (VRc variants), so wrap on the UI thread after the parallel map.
        #[cfg(not(target_family = "wasm"))]
        let buffers = {
            use rayon::prelude::*;
            charts.par_iter().map(|c| c.render_pixels()).collect::<Vec<_>>()
        };
        #[cfg(target_family = "wasm")]
        let buffers: Vec<_> = charts.iter().map(|c| c.render_pixels()).collect();
        for (i, pb) in buffers.into_iter().enumerate() {
            set_chart_image(&ui, i, slint::Image::from_rgba8(pb));
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
    });

    {
        let app = app.clone();
        ui.on_resized(move |idx, w, h| {
            if w < 2.0 || h < 2.0 {
                return;
            }
            let changed = app.charts.borrow_mut()[idx as usize].resize(w as u32, h as u32);
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
            let dx = (x - app.pan_x.get()) as f64;
            app.pan_x.set(x);
            app.charts.borrow_mut()[idx as usize].pan_px(dx);
            app.render_chart(idx as usize);
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
            app.charts.borrow_mut()[idx as usize].zoom_at(x as f64, dy as f64);
            app.render_chart(idx as usize);
        });
    }

    let is_bench = bench_flag();

    if is_bench {
        // Frame-latency sampling via the rendering notifier.
        let app2 = app.clone();
        let _ = ui.window().set_rendering_notifier(move |state, _api| {
            if let slint::RenderingState::AfterRendering = state {
                if let Some(b) = app2.bench.borrow().as_ref() {
                    b.on_frame();
                }
            }
        });
    }

    // Streaming replay: after the 4000-bar preload, stream the remaining
    // 19,400 bars/chart at `rate` bars/sec in ~50 ms batches.
    // Bench default rate = 2000 bars/s (documented); interactive default 1000.
    let rate = stream_rate().unwrap_or(if is_bench { 2000.0 } else { 1000.0 });
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

    let _ = ui.run();
}
