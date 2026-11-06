//! Scripted benchmark for `--bench` (native) / `?bench=1` (web).
//!
//! Metrics per results/schema.md:
//!  - startup_ms_to_first_chart: app-start (post datagen) -> first rendered frame
//!  - indicator_compute_ms: 20 runs of compute_all_ta over all 8 charts (median)
//!  - frame_time: update/action -> AfterRendering latency samples
//!      * pan/zoom: scripted pan+zoom every 16 ms on rotating charts, 10 s
//!      * live 1/s per chart: append 1 bar to all 8 charts each second, 10 s
//!      * live 100/s per chart: append 1 bar to all 8 charts every 10 ms, 10 s
//!  - memory: RSS (native) / usedJSHeapSize (web)
//!  - artifact sizes: exe (native) / wasm+js+dist via sizes.json (web)

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use serde_json::json;
use slint::{Timer, TimerMode};

use crate::app::App;
use crate::chart::compute_all_ta;
use crate::plat::*;

const PHASE_MS: f64 = 10_000.0;

#[derive(Default, Clone, Copy)]
pub struct FrameStat {
    pub n: usize,
    pub avg_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

impl FrameStat {
    fn compute(samples: &[f64], duration_s: f64) -> (Self, f64) {
        if samples.is_empty() {
            return (Self::default(), 0.0);
        }
        let mut s = samples.to_vec();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = s.len();
        let avg = s.iter().sum::<f64>() / n as f64;
        let pct = |p: f64| s[((p * n as f64).ceil() as usize).saturating_sub(1).min(n - 1)];
        let fps = n as f64 / duration_s.max(0.001);
        (FrameStat { n, avg_ms: avg, p95_ms: pct(0.95), p99_ms: pct(0.99) }, fps)
    }

    fn as_json(&self, fps: f64) -> serde_json::Value {
        json!({ "avg_ms": self.avg_ms, "p95_ms": self.p95_ms, "p99_ms": self.p99_ms, "fps": fps })
    }
}

pub struct Bench {
    pub t_app: f64,
    pub startup_ms: Cell<Option<f64>>,
    /// Timestamp of the in-flight UI action awaiting its rendered frame.
    pub pending: Cell<Option<f64>>,
    pub samples: RefCell<Vec<f64>>,

    pub ind_all: RefCell<Vec<f64>>,
    pub ind_per: RefCell<Vec<f64>>,

    pub pan_zoom: RefCell<(FrameStat, f64)>,
    pub live1: RefCell<(FrameStat, f64)>,
    pub live100: RefCell<(FrameStat, f64)>,

    pub mem_idle: Cell<f64>,
    pub mem_30s: Cell<f64>,
    /// (exe_mb, wasm_mb, js_mb, dist_mb)
    pub sizes: RefCell<(f64, f64, f64, f64)>,
    /// wasm: fetched asynchronously from sizes.json
    pub web_sizes: Rc<RefCell<Option<(f64, f64, f64)>>>,

    /// Streaming replay: configured rate and catch-up results.
    pub stream_rate: f64,
    pub stream_t0: Cell<f64>,
    pub stream_ms: Cell<f64>,
    pub stream_stat: RefCell<(FrameStat, f64)>,
}

impl Bench {
    pub fn new(t_app: f64, stream_rate: f64) -> Rc<Self> {
        Rc::new(Self {
            t_app,
            startup_ms: Cell::new(None),
            pending: Cell::new(None),
            samples: RefCell::new(Vec::new()),
            ind_all: RefCell::new(Vec::new()),
            ind_per: RefCell::new(Vec::new()),
            pan_zoom: RefCell::new((FrameStat::default(), 0.0)),
            live1: RefCell::new((FrameStat::default(), 0.0)),
            live100: RefCell::new((FrameStat::default(), 0.0)),
            mem_idle: Cell::new(0.0),
            mem_30s: Cell::new(0.0),
            sizes: RefCell::new((0.0, 0.0, 0.0, 0.0)),
            web_sizes: Rc::new(RefCell::new(None)),
            stream_rate,
            stream_t0: Cell::new(0.0),
            stream_ms: Cell::new(0.0),
            stream_stat: RefCell::new((FrameStat::default(), 0.0)),
        })
    }

    /// Called from the window rendering notifier.
    pub fn on_frame(&self) {
        let t = now_ms();
        if let Some(t0) = self.pending.take() {
            self.samples.borrow_mut().push(t - t0);
        }
    }

    /// Called on the first real chart render (startup-to-first-chart).
    pub fn on_first_chart(&self) {
        if self.startup_ms.get().is_none() {
            self.startup_ms.set(Some(now_ms() - self.t_app));
        }
    }

    pub fn begin_action(&self) {
        self.pending.set(Some(now_ms()));
    }
}

fn set_status(app: &App, s: &str) {
    if let Some(ui) = app.ui.upgrade() {
        ui.set_status(s.into());
    }
}

/// Kick off the scripted run. `t_app` is the app-start timestamp (after data
/// generation, before window creation); `rate` is the configured stream rate.
/// The streaming replay runs first — `on_stream_done` starts the phases.
pub fn start(app: Rc<App>, t_app: f64, rate: f64) {
    let bench = Bench::new(t_app, rate);
    bench.stream_t0.set(now_ms());
    app.bench.replace(Some(bench.clone()));
    set_status(&app, "bench: startup + streaming");

    #[cfg(target_family = "wasm")]
    fetch_sizes(bench.web_sizes.clone());

    #[cfg(not(target_family = "wasm"))]
    {
        let exe_mb = std::env::current_exe()
            .and_then(std::fs::metadata)
            .map(|m| m.len() as f64 / 1_048_576.0)
            .unwrap_or(0.0);
        bench.sizes.borrow_mut().0 = exe_mb;
    }

    // "idle after load": after the 4000-bar preload is up, mid-stream.
    let bench2 = bench.clone();
    Timer::single_shot(Duration::from_millis(800), move || {
        bench2.mem_idle.set(mem_mb());
    });
}

/// All 8 streams exhausted -> fold catch-up stats, then run the phases.
pub fn on_stream_done(app: Rc<App>) {
    let bench = match app.bench.borrow().as_ref() {
        Some(b) => b.clone(),
        None => return,
    };
    let catchup_ms = now_ms() - bench.stream_t0.get();
    bench.stream_ms.set(catchup_ms);
    let (stat, fps) = FrameStat::compute(&bench.samples.borrow(), catchup_ms / 1000.0);
    *bench.stream_stat.borrow_mut() = (stat, fps);
    bench.samples.borrow_mut().clear();
    set_status(
        &app,
        &format!("bench: catch-up {:.1}s — indicator compute", catchup_ms / 1000.0),
    );
    step_indicators(app.clone(), bench.clone());
    step_pan_zoom(app, bench);
}

fn step_indicators(app: Rc<App>, bench: Rc<Bench>) {
    set_status(&app, "bench: indicator compute x20");
    let charts = app.charts.borrow();
    for _ in 0..20 {
        let t0 = now_ms();
        for c in charts.iter() {
            let tc = now_ms();
            // black_box: keep the computation out of dead-code elimination
            let ind = std::hint::black_box(compute_all_ta(&c.bars));
            std::hint::black_box(ind.ema10.len());
            bench.ind_per.borrow_mut().push(now_ms() - tc);
        }
        bench.ind_all.borrow_mut().push(now_ms() - t0);
    }
}

/// Run a repeated timer for `dur_ms`; `tick` performs the UI action.
/// Collected action->render samples are folded into `out`.
fn run_phase(
    app: Rc<App>,
    bench: Rc<Bench>,
    interval: Duration,
    dur_ms: f64,
    out: Rc<RefCell<(FrameStat, f64)>>,
    mut tick: impl FnMut(&App, &Bench, u64) + 'static,
    next: impl FnOnce(Rc<App>, Rc<Bench>) + 'static,
) {
    let slot: Rc<RefCell<Option<Timer>>> = Rc::new(RefCell::new(None));
    let t0 = now_ms();
    let mut n = 0u64;
    let mut next = Some(next);
    bench.samples.borrow_mut().clear();
    bench.pending.set(None);
    {
        let app = app.clone();
        let bench = bench.clone();
        let slot2 = slot.clone();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, interval, move || {
            if now_ms() - t0 >= dur_ms {
                // Finalize this phase.
                let (stat, fps) = FrameStat::compute(&bench.samples.borrow(), dur_ms / 1000.0);
                *out.borrow_mut() = (stat, fps);
                if let Some(t) = slot2.borrow_mut().take() {
                    t.stop();
                }
                if let Some(f) = next.take() {
                    f(app.clone(), bench.clone());
                }
                return;
            }
            tick(&app, &bench, n);
            n += 1;
        });
        *slot.borrow_mut() = Some(timer);
    }
}

fn step_pan_zoom(app: Rc<App>, bench: Rc<Bench>) {
    set_status(&app, "bench: pan/zoom 10s");
    let out = Rc::new(RefCell::new((FrameStat::default(), 0.0)));
    let out2 = out.clone();

    run_phase(
        app,
        bench,
        Duration::from_millis(16),
        PHASE_MS,
        out,
        |app: &App, bench: &Bench, n: u64| {
            let idx = (n % 8) as usize;
            {
                let mut charts = app.charts.borrow_mut();
                let c = &mut charts[idx];
                if n % 24 == 16 {
                    c.zoom_at(c.w as f64 * 0.75, if n % 48 < 24 { 60.0 } else { -60.0 });
                } else {
                    c.pan_px(if n % 48 < 24 { 6.0 } else { -6.0 });
                }
            }
            bench.begin_action();
            app.render_chart(idx);
            app.request_redraw();
        },
        move |app, bench| {
            *bench.pan_zoom.borrow_mut() = *out2.borrow();
            step_live_1hz(app, bench);
        },
    );
}

fn step_live_1hz(app: Rc<App>, bench: Rc<Bench>) {
    set_status(&app, "bench: live updates 1/s x8 charts, 10s");
    let out = Rc::new(RefCell::new((FrameStat::default(), 0.0)));
    let out2 = out.clone();
    run_phase(
        app,
        bench,
        Duration::from_millis(1000),
        PHASE_MS,
        out,
        |app: &App, bench: &Bench, _n: u64| {
            bench.begin_action();
            app.push_bar_all();
        },
        move |app, bench| {
            *bench.live1.borrow_mut() = *out2.borrow();
            step_live_100hz(app, bench);
        },
    );
}

fn step_live_100hz(app: Rc<App>, bench: Rc<Bench>) {
    set_status(&app, "bench: live updates 100/s x8 charts, 10s");
    let out = Rc::new(RefCell::new((FrameStat::default(), 0.0)));
    let out2 = out.clone();
    run_phase(
        app,
        bench,
        Duration::from_millis(10),
        PHASE_MS,
        out,
        |app: &App, bench: &Bench, _n: u64| {
            bench.begin_action();
            app.push_bar_all();
        },
        move |app, bench| {
            *bench.live100.borrow_mut() = *out2.borrow();
            finish(app, bench);
        },
    );
}

fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

fn finish(app: Rc<App>, bench: Rc<Bench>) {
    // On wasm, sizes.json arrives async; give it a short grace period.
    #[cfg(target_family = "wasm")]
    {
        let app2 = app.clone();
        let bench2 = bench.clone();
        if bench.web_sizes.borrow().is_none() {
            Timer::single_shot(Duration::from_millis(700), move || finish(app2, bench2));
            return;
        }
    }
    bench.mem_30s.set(mem_mb());
    set_status(&app, "bench: done");

    #[cfg(target_family = "wasm")]
    {
        if let Some((w, j, t)) = *bench.web_sizes.borrow() {
            let mut s = bench.sizes.borrow_mut();
            s.1 = w;
            s.2 = j;
            s.3 = t;
        }
    }

    let (pz, pz_fps) = *bench.pan_zoom.borrow();
    let (l1, l1_fps) = *bench.live1.borrow();
    let (l100, l100_fps) = *bench.live100.borrow();
    let (exe, wasm, js, dist) = *bench.sizes.borrow();

    let result = json!({
        "config": "slint",
        "target": if cfg!(target_family = "wasm") { "web" } else { "desktop" },
        "timestamp": iso_now(),
        "environment": env_info(),
        "charts": datagen::NUM_CHARTS,
        "bars_per_chart": datagen::BARS_PER_DAY,
        "startup_ms_to_first_chart": bench.startup_ms.get().unwrap_or(0.0),
        "indicator_compute_ms": {
            "all_charts_median": median(&bench.ind_all.borrow()),
            "per_chart_median": median(&bench.ind_per.borrow()),
            "runs": 20,
        },
        "stream_catchup_ms": bench.stream_ms.get(),
        "stream_rate_bars_per_sec": bench.stream_rate,
        "frame_time": {
            "stream_catchup": bench.stream_stat.borrow().0.as_json(bench.stream_stat.borrow().1),
            "pan_zoom_10s": pz.as_json(pz_fps),
            "live_update_1s_per_chart_10s": l1.as_json(l1_fps),
            "live_update_100hz_per_chart_10s": l100.as_json(l100_fps),
        },
        "memory_mb": {
            "idle_after_load": bench.mem_idle.get(),
            "after_30s": bench.mem_30s.get(),
            "method": if cfg!(target_family = "wasm") {
                "performance.memory.usedJSHeapSize (Chromium)"
            } else if cfg!(windows) {
                "GetProcessMemoryInfo WorkingSetSize"
            } else {
                "/proc/self/status VmRSS"
            },
        },
        "artifact_size_mb": {
            "exe": exe,
            "wasm": wasm,
            "js_bundle": js,
            "total_web_dist": dist,
        },
        "notes": format!(
            "frame = UI action -> rendered frame latency; fps = rendered frames/s during the phase. \
             Data generation excluded. Streaming replay: {} bars preload + {rate:.0} bars/s in 50ms \
             batches through the incremental indicator path. EMA/SMA via pure-Rust `ta` crate \
             (TA-Lib-compatible SMA seed); VWAP+volume profile via shared/indicators. Charts \
             software-rendered in Rust into SharedPixelBuffer.",
            datagen::INITIAL_BARS,
            rate = bench.stream_rate
        ),
    });

    let text = serde_json::to_string_pretty(&result).unwrap_or_default();
    emit_json(&text);

    // Native: exit after the run so scripted benches don't hang.
    // wasm: keep the UI alive so results stay visible.
    #[cfg(not(target_family = "wasm"))]
    {
        set_status(&app, "bench: done — exiting");
        slint::Timer::single_shot(Duration::from_millis(300), || {
            let _ = slint::quit_event_loop();
        });
    }
}
