//! eframe::App with an egui_dock workspace of 8 chart tabs.

use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use web_time::Instant;

use crate::bench::{self, Bench};
use crate::chart::{Chart, Lcg, Streamer};
use crate::timing;

pub struct TestbedApp {
    state: DockState<usize>,
    charts: Vec<Chart>,
    bench: Option<Bench>,
    streamer: Streamer,
    bench_mode: bool,
    lcg: Lcg,
    live_pending: f64,
    last_frame: Option<Instant>,
    start: Instant,
    datagen_ms: f64,
    first_paint_ms: Option<f64>,
    use_gpu: bool,
    status: String,
}

fn grid_layout() -> DockState<usize> {
    let mut state = DockState::new(vec![0]);
    let s = state.main_surface_mut();
    let [top, bottom] = s.split_below(NodeIndex::root(), 0.5, vec![4]);
    let [tl, tr] = s.split_right(top, 0.5, vec![2]);
    let [bl, br] = s.split_right(bottom, 0.5, vec![6]);
    s.split_right(tl, 0.5, vec![1]);
    s.split_right(tr, 0.5, vec![3]);
    s.split_right(bl, 0.5, vec![5]);
    s.split_right(br, 0.5, vec![7]);
    state
}

impl TestbedApp {
    /// `rate` = stream delivery in bars/sec per chart (`?rate=` / `--rate=`).
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        bench_mode: bool,
        start: Instant,
        rate: f64,
    ) -> Self {
        let t_dg = Instant::now();
        // Preload: only INITIAL_BARS bars arrive statically; the rest streams.
        let charts: Vec<Chart> = (0..datagen::NUM_CHARTS).map(Chart::new).collect();
        let datagen_ms = t_dg.elapsed().as_secs_f64() * 1000.0;
        // Bench defaults to a faster stream (2000/s/chart) unless --rate /
        // ?rate overrides; ~10 s of streaming frames stays representative.
        let rate = if rate > 0.0 {
            rate
        } else if bench_mode {
            2000.0
        } else {
            1000.0
        };

        let use_gpu = cc
            .wgpu_render_state
            .as_ref()
            .map(|rs| {
                crate::gpu::set_target_format(rs.target_format);
                true
            })
            .unwrap_or(false);

        let bench = if bench_mode {
            // startup_ms is back-filled on first paint (see ui()).
            let (mem_idle, method) = timing::memory_mb();
            Some(Bench::new(0.0, mem_idle, method, use_gpu))
        } else {
            None
        };

        Self {
            state: grid_layout(),
            charts,
            bench,
            streamer: Streamer::new(rate),
            bench_mode,
            lcg: Lcg(0x11EC_CAFE),
            live_pending: 0.0,
            last_frame: None,
            start,
            datagen_ms,
            first_paint_ms: None,
            use_gpu,
            status: format!(
                "egui+wgpu testbed | renderer: {} | stream: {rate:.0} bars/s/chart",
                if use_gpu { "wgpu callback" } else { "egui::Painter" }
            ),
        }
    }

    pub fn first_paint_ms(&self) -> Option<f64> {
        self.first_paint_ms
    }
}

struct Viewer<'a> {
    charts: &'a mut Vec<Chart>,
    gpu: bool,
    painted: &'a mut bool,
}

impl TabViewer for Viewer<'_> {
    type Tab = usize;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(format!("chart-{tab}"))
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        format!("Chart {}", *tab + 1).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        if self.charts[*tab].render(ui, self.gpu) {
            *self.painted = true;
        }
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }

    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        false
    }
}

impl eframe::App for TestbedApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        let now = Instant::now();
        let dt = self
            .last_frame
            .map(|l| (now - l).as_secs_f64())
            .unwrap_or(0.016);
        self.last_frame = Some(now);

        // --- streaming data model: replay unstreamed bars in batches ---
        if self.streamer.active {
            self.streamer.tick(&mut self.charts, dt);
            ctx.request_repaint();
        } else if !self.bench_mode {
            // Interactive mode: after catch-up, 1 synthetic bar/sec/chart.
            self.live_pending += dt;
            if self.live_pending >= 1.0 {
                self.live_pending = 0.0;
                for c in self.charts.iter_mut() {
                    let prev = *c.arrived().last().unwrap();
                    for b in self.lcg.make_bars(&prev, 1) {
                        c.push_bar(b);
                    }
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        if let Some(b) = &mut self.bench {
            b.tick(ctx, &mut self.charts, &self.streamer);
        }

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                if self.streamer.active {
                    let c0 = &self.charts[0];
                    ui.label(format!(
                        "| streaming: {}/{}",
                        c0.streamed,
                        c0.bars.len()
                    ));
                } else {
                    ui.label(format!(
                        "| stream done in {:.1}s",
                        self.streamer.catchup_ms / 1000.0
                    ));
                }
                if let Some(t) = self.first_paint_ms {
                    ui.label(format!("| first chart: {t:.0} ms"));
                }
                if let Some(b) = &self.bench {
                    ui.label(if b.is_done() {
                        "| bench: DONE (results written)"
                    } else {
                        "| bench: running…"
                    });
                }
            });
        });

        // The root Ui has no margins — the dock fills all remaining space.
        let mut painted = false;
        DockArea::new(&mut self.state)
            .id(egui::Id::new("dock"))
            .show_inside(
                ui,
                &mut Viewer {
                    charts: &mut self.charts,
                    gpu: self.use_gpu,
                    painted: &mut painted,
                },
            );

        if painted && self.first_paint_ms.is_none() {
            let ms = self.start.elapsed().as_secs_f64() * 1000.0;
            self.first_paint_ms = Some(ms);
            // Startup-to-first-chart excludes data generation (per spec).
            if let Some(b) = &mut self.bench {
                b.result.startup_ms_to_first_chart = ms - self.datagen_ms;
            }
        }
    }
}

/// Re-export for main.rs startup timing + bench flag plumbing.
pub use bench::BenchResult;
