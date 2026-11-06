//! Scripted benchmark per results/schema.md.
//! Native: `--bench` writes results/egui-desktop.json.
//! Web: `?bench=1` logs JSON to console and offers a download.

use serde::Serialize;
use web_time::Instant;

use crate::chart::{self, Chart, Lcg};
use crate::timing;

#[derive(Serialize, Default, Clone)]
pub struct FrameStats {
    pub avg_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub fps: f64,
}

#[derive(Serialize, Default)]
pub struct IndicatorMs {
    pub all_charts_median: f64,
    pub per_chart_median: f64,
    pub runs: usize,
}

#[derive(Serialize, Default)]
pub struct FrameTime {
    /// Frame stats recorded while the catch-up stream was running
    /// (optional per schema "stream_catchup").
    pub stream_catchup: FrameStats,
    pub pan_zoom_10s: FrameStats,
    pub live_update_1s_per_chart_10s: FrameStats,
    pub live_update_100hz_per_chart_10s: FrameStats,
}

#[derive(Serialize, Default)]
pub struct Env {
    pub os: String,
    pub cpu: String,
    pub browser: String,
    pub version: String,
}

#[derive(Serialize, Default)]
pub struct Mem {
    pub idle_after_load: f64,
    pub after_30s: f64,
    pub method: String,
}

#[derive(Serialize, Default)]
pub struct Artifacts {
    pub exe: f64,
    pub wasm: f64,
    pub js_bundle: f64,
    pub total_web_dist: f64,
}

#[derive(Serialize, Default)]
pub struct BenchResult {
    pub config: String,
    pub target: String,
    pub timestamp: String,
    pub environment: Env,
    pub charts: usize,
    pub bars_per_chart: usize,
    pub startup_ms_to_first_chart: f64,
    /// Stream start → all 8 charts fully caught up (streaming data model).
    pub stream_catchup_ms: f64,
    /// Measured achieved delivery rate across all charts (bars/sec).
    pub stream_rate_bars_per_sec: f64,
    pub indicator_compute_ms: IndicatorMs,
    pub frame_time: FrameTime,
    pub memory_mb: Mem,
    pub artifact_size_mb: Artifacts,
    pub notes: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Warmup,
    Stream,
    Indicators,
    PanZoom,
    Live1,
    Live100,
    Finish,
    Done,
}

fn stats(times: &[f64], wall_secs: f64) -> FrameStats {
    if times.is_empty() {
        return FrameStats::default();
    }
    let mut s = times.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let avg = s.iter().sum::<f64>() / s.len() as f64;
    let pct = |p: f64| s[((s.len() - 1) as f64 * p).round() as usize];
    FrameStats {
        avg_ms: avg * 1000.0,
        p95_ms: pct(0.95) * 1000.0,
        p99_ms: pct(0.99) * 1000.0,
        fps: times.len() as f64 / wall_secs.max(1e-9),
    }
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if v.is_empty() {
        0.0
    } else {
        v[v.len() / 2]
    }
}

pub struct Bench {
    phase: Phase,
    phase_t0: Instant,
    last_frame: Option<Instant>,
    times: Vec<f64>,
    warmup_frames: u32,
    pending_updates: f64,
    next_tick: f64,
    lcg: Lcg,
    pub result: BenchResult,
}

const PANZOOM_SECS: f64 = 10.0;
const LIVE_SECS: f64 = 10.0;

impl Bench {
    pub fn new(startup_ms: f64, mem_idle: f64, mem_method: &str, gpu_used: bool) -> Self {
        Self {
            phase: Phase::Warmup,
            phase_t0: Instant::now(),
            last_frame: None,
            times: Vec::with_capacity(4096),
            warmup_frames: 0,
            pending_updates: 0.0,
            next_tick: 1.0,
            lcg: Lcg(0xBECB_5EED),
            result: BenchResult {
                config: "egui".into(),
                target: if cfg!(target_arch = "wasm32") {
                    "web".into()
                } else {
                    "desktop".into()
                },
                timestamp: timing::timestamp_iso(),
                environment: Env {
                    os: timing::os_name().into(),
                    cpu: timing::cpu_name(),
                    browser: timing::browser_ua(),
                    version: env!("CARGO_PKG_VERSION").into(),
                },
                charts: datagen::NUM_CHARTS,
                bars_per_chart: datagen::BARS_PER_DAY,
                startup_ms_to_first_chart: startup_ms,
                memory_mb: Mem {
                    idle_after_load: mem_idle,
                    method: mem_method.into(),
                    ..Default::default()
                },
                notes: {
                    let mut s = format!(
                        "renderer: {}",
                        if gpu_used {
                            "custom wgpu paint callback (egui-wgpu CallbackTrait)"
                        } else {
                            "egui::Painter shapes (wgpu callback unavailable)"
                        }
                    );
                    if cfg!(target_arch = "wasm32") {
                        s.push_str("; memory = JS heap only (wasm linear memory not counted by performance.memory)");
                    }
                    s
                },
                ..Default::default()
            },
        }
    }

    /// Drive scripted phases. Called every frame before the dock renders.
    /// `streamer` feeds the catch-up stream (phases wait for it to finish).
    pub fn tick(&mut self, ctx: &egui::Context, charts: &mut [Chart], streamer: &chart::Streamer) {
        let now = Instant::now();
        let dt = self.last_frame.map(|l| (now - l).as_secs_f64()).unwrap_or(0.016);
        self.last_frame = Some(now);
        let elapsed = (now - self.phase_t0).as_secs_f64();

        match self.phase {
            Phase::Warmup => {
                self.warmup_frames += 1;
                ctx.request_repaint();
                if self.warmup_frames >= 20 {
                    self.advance(now);
                }
            }
            Phase::Stream => {
                // Record frame stats during the catch-up stream until all
                // charts have received their full day.
                self.times.push(dt);
                if !streamer.active {
                    self.result.stream_catchup_ms = streamer.catchup_ms;
                    self.result.stream_rate_bars_per_sec = streamer.achieved_rate();
                    self.result.frame_time.stream_catchup = stats(&self.times, elapsed);
                    self.advance(now);
                }
            }
            Phase::Indicators => {
                self.run_indicator_bench(charts);
                self.advance(now);
            }
            Phase::PanZoom => {
                self.times.push(dt);
                let t = elapsed.min(PANZOOM_SECS);
                for c in charts.iter_mut() {
                    let len = c.streamed as f64;
                    let zoom = 0.5 - 0.45 * (t * std::f64::consts::TAU / 4.0).cos();
                    let w = (len * zoom).max(64.0);
                    let range = (len - w).max(0.0);
                    let start = range * (0.5 + 0.5 * (t * std::f64::consts::TAU / 6.0).sin());
                    c.view.start = start;
                    c.view.end = start + w;
                    c.view.clamp(c.streamed);
                }
                ctx.request_repaint();
                if elapsed >= PANZOOM_SECS {
                    self.result.frame_time.pan_zoom_10s = stats(&self.times, elapsed);
                    self.advance(now);
                }
            }
            Phase::Live1 => {
                self.times.push(dt);
                while elapsed >= self.next_tick {
                    self.next_tick += 1.0;
                    for c in charts.iter_mut() {
                        let prev = *c.arrived().last().unwrap();
                        for b in self.lcg.make_bars(&prev, 1) {
                            c.push_bar(b);
                        }
                    }
                }
                ctx.request_repaint();
                if elapsed >= LIVE_SECS {
                    self.result.frame_time.live_update_1s_per_chart_10s =
                        stats(&self.times, elapsed);
                    self.advance(now);
                }
            }
            Phase::Live100 => {
                self.times.push(dt);
                // 100 new bars per chart per second, spread over frames.
                self.pending_updates += 100.0 * dt.max(0.0);
                let n = (self.pending_updates as usize).min(32);
                self.pending_updates -= n as f64;
                if n > 0 {
                    for c in charts.iter_mut() {
                        let prev = *c.arrived().last().unwrap();
                        for b in self.lcg.make_bars(&prev, n) {
                            c.push_bar(b);
                        }
                    }
                }
                ctx.request_repaint();
                if elapsed >= LIVE_SECS {
                    self.result.frame_time.live_update_100hz_per_chart_10s =
                        stats(&self.times, elapsed);
                    self.advance(now);
                }
            }
            Phase::Finish => {
                let (mb, method) = timing::memory_mb();
                self.result.memory_mb.after_30s = mb;
                if self.result.memory_mb.method.is_empty() {
                    self.result.memory_mb.method = method.into();
                }
                self.measure_artifacts();
                self.emit();
                self.phase = Phase::Done;
            }
            Phase::Done => {}
        }
    }

    fn advance(&mut self, now: Instant) {
        self.phase = match self.phase {
            Phase::Warmup => Phase::Stream,
            Phase::Stream => Phase::Indicators,
            Phase::Indicators => Phase::PanZoom,
            Phase::PanZoom => Phase::Live1,
            Phase::Live1 => Phase::Live100,
            Phase::Live100 => Phase::Finish,
            Phase::Finish | Phase::Done => Phase::Done,
        };
        self.phase_t0 = now;
        self.times.clear();
        self.pending_updates = 0.0;
        self.next_tick = 1.0;
    }

    fn run_indicator_bench(&mut self, charts: &mut [Chart]) {
        let mut all_runs: Vec<f64> = Vec::with_capacity(20);
        let mut per_chart: Vec<f64> = Vec::with_capacity(160);
        for _ in 0..20 {
            let t_all = Instant::now();
            for c in charts.iter() {
                let t1 = Instant::now();
                let ind = chart::compute_indicators(&c.bars);
                std::hint::black_box(&ind);
                per_chart.push(t1.elapsed().as_secs_f64() * 1000.0);
            }
            all_runs.push(t_all.elapsed().as_secs_f64() * 1000.0);
        }
        self.result.indicator_compute_ms = IndicatorMs {
            all_charts_median: median(&mut all_runs),
            per_chart_median: median(&mut per_chart),
            runs: 20,
        };
    }

    fn measure_artifacts(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Ok(exe) = std::env::current_exe() {
                if let Ok(m) = exe.metadata() {
                    self.result.artifact_size_mb.exe = m.len() as f64 / 1_048_576.0;
                }
            }
            // Web dist (if a trunk build exists next to us).
            let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("dist");
            if dist.is_dir() {
                let mut wasm = 0u64;
                let mut js = 0u64;
                let mut total = 0u64;
                for e in std::fs::read_dir(&dist).into_iter().flatten().flatten() {
                    if let Ok(m) = e.metadata() {
                        total += m.len();
                        match e.path().extension().and_then(|x| x.to_str()) {
                            Some("wasm") => wasm += m.len(),
                            Some("js") => js += m.len(),
                            _ => {}
                        }
                    }
                }
                let a = &mut self.result.artifact_size_mb;
                a.wasm = wasm as f64 / 1_048_576.0;
                a.js_bundle = js as f64 / 1_048_576.0;
                a.total_web_dist = total as f64 / 1_048_576.0;
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            // Web artifacts: report fetched resources via the Performance
            // resource-timing API (encodedBodySize where available).
            let get = |o: &wasm_bindgen::JsValue, k: &str| -> wasm_bindgen::JsValue {
                js_sys::Reflect::get(o, &k.into()).unwrap_or_default()
            };
            if let Some(w) = web_sys::window() {
                if let Some(perf) = w.performance() {
                    let arr = perf.get_entries_by_type("resource");
                    let mut total = 0f64;
                    let mut wasm = 0f64;
                    let mut js = 0f64;
                    for e in arr.iter() {
                        let name = get(&e, "name").as_string().unwrap_or_default();
                        let size = get(&e, "encodedBodySize").as_f64().unwrap_or(0.0);
                        if name.contains(".wasm") {
                            wasm += size;
                        } else if name.contains(".js") {
                            js += size;
                        }
                        total += size;
                    }
                    let a = &mut self.result.artifact_size_mb;
                    a.wasm = wasm / 1_048_576.0;
                    a.js_bundle = js / 1_048_576.0;
                    a.total_web_dist = total / 1_048_576.0;
                }
            }
        }
    }

    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }

    fn emit(&self) {
        let json = serde_json::to_string_pretty(&self.result).unwrap_or_default();
        emit_json(&json);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn emit_json(json: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("results")
        .join("egui-desktop.json");
    match std::fs::write(&path, json) {
        Ok(()) => log::info!("bench results written to {}", path.display()),
        Err(e) => {
            log::error!("failed to write {}: {e}", path.display());
            println!("{json}");
        }
    }
    println!("=== BENCH DONE ===\n{json}");
}

#[cfg(target_arch = "wasm32")]
fn emit_json(json: &str) {
    use wasm_bindgen::JsCast;
    web_sys::console::log_1(&"==BENCH_JSON_BEGIN==".into());
    web_sys::console::log_1(&json.into());
    web_sys::console::log_1(&"==BENCH_JSON_END==".into());
    // Also stash on window for easy extraction.
    if let Some(w) = web_sys::window() {
        let _ = js_sys::Reflect::set(&w, &"__bench_json".into(), &json.into());
        // Try to trigger a download of egui-web.json.
        if let Some(doc) = w.document() {
            if let Ok(blob) = web_sys::Blob::new_with_str_sequence(
                &js_sys::Array::of1(&json.into()),
            ) {
                if let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) {
                    if let Ok(a) = doc
                        .create_element("a")
                        .map(|e| e.dyn_into::<web_sys::HtmlAnchorElement>().unwrap())
                    {
                        a.set_href(&url);
                        a.set_download("egui-web.json");
                        let _ = js_sys::Reflect::set(
                            &a,
                            &"style".into(),
                            &"display:none".into(),
                        );
                        let _ = doc.body().map(|b| b.append_child(&a));
                        a.click();
                    }
                }
            }
        }
    }
}
