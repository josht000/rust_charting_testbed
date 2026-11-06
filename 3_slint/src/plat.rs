//! Platform shims: monotonic clock, process memory, ISO timestamp,
//! bench-flag detection and JSON result emission for native vs wasm.

// ---------- monotonic clock (ms) ----------

#[cfg(not(target_family = "wasm"))]
pub fn now_ms() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static T0: OnceLock<Instant> = OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

#[cfg(target_family = "wasm")]
pub fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or_else(|| js_sys::Date::now())
}

// ---------- process memory (MB) ----------

#[cfg(windows)]
pub fn mem_mb() -> f64 {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) != 0 {
            c.WorkingSetSize as f64 / 1_048_576.0
        } else {
            0.0
        }
    }
}

#[cfg(all(unix, not(target_family = "wasm")))]
pub fn mem_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS"))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse::<f64>().ok()))
        })
        .map(|kb| kb / 1024.0)
        .unwrap_or(0.0)
}

#[cfg(target_family = "wasm")]
pub fn mem_mb() -> f64 {
    let perf = match web_sys::window().and_then(|w| w.performance()) {
        Some(p) => p,
        None => return 0.0,
    };
    // performance.memory is non-standard but available in Chromium.
    let mem = js_sys::Reflect::get(&perf, &"memory".into()).ok();
    mem.and_then(|m| js_sys::Reflect::get(&m, &"usedJSHeapSize".into()).ok())
        .and_then(|v| v.as_f64())
        .map(|b| b / 1_048_576.0)
        .unwrap_or(0.0)
}

#[cfg(not(any(windows, unix, target_family = "wasm")))]
pub fn mem_mb() -> f64 {
    0.0
}

// ---------- ISO-8601 timestamp ----------

#[cfg(target_family = "wasm")]
pub fn iso_now() -> String {
    String::from(js_sys::Date::new_0().to_iso_string())
}

#[cfg(not(target_family = "wasm"))]
pub fn iso_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

// ---------- bench flag ----------

#[cfg(not(target_family = "wasm"))]
pub fn bench_flag() -> bool {
    std::env::args().any(|a| a == "--bench")
}

#[cfg(target_family = "wasm")]
pub fn bench_flag() -> bool {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .map(|s| s.contains("bench=1"))
        .unwrap_or(false)
}

// ---------- streaming rate (bars/sec), None -> caller default ----------

/// `--rate=N` or `--rate N` on native; `?rate=N` / `&rate=N` on web.
#[cfg(not(target_family = "wasm"))]
pub fn stream_rate() -> Option<f64> {
    let args: Vec<String> = std::env::args().collect();
    for (i, a) in args.iter().enumerate() {
        if a == "--rate" {
            if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                return Some(v);
            }
        } else if let Some(v) = a.strip_prefix("--rate=").and_then(|s| s.parse().ok()) {
            return Some(v);
        }
    }
    None
}

#[cfg(target_family = "wasm")]
pub fn stream_rate() -> Option<f64> {
    let q = web_sys::window().and_then(|w| w.location().search().ok())?;
    q.trim_start_matches('?')
        .split('&')
        .find_map(|kv| kv.strip_prefix("rate=").and_then(|v| v.parse().ok()))
}

// ---------- environment info ----------

pub fn env_info() -> serde_json::Value {
    let os = std::env::consts::OS;
    serde_json::json!({
        "os": if os.is_empty() { "wasm32/browser" } else { os },
        "cpu": cpu_string(),
        "browser": browser_string(),
        "version": format!("slint 1.18.1, {}", env!("RUSTC_VERSION")),
    })
}

#[cfg(not(target_family = "wasm"))]
fn cpu_string() -> String {
    std::env::var("PROCESSOR_IDENTIFIER")
        .or_else(|_| std::env::var("PROCESSOR_MODEL"))
        .unwrap_or_else(|_| "unknown".into())
}

#[cfg(target_family = "wasm")]
fn cpu_string() -> String {
    "wasm32".into()
}

#[cfg(target_family = "wasm")]
fn browser_string() -> String {
    web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(not(target_family = "wasm"))]
fn browser_string() -> String {
    "n/a".into()
}

// ---------- emit result JSON ----------

#[cfg(not(target_family = "wasm"))]
pub fn emit_json(json_text: &str) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("results")
        .join("slint-desktop.json");
    match std::fs::write(&path, json_text) {
        Ok(()) => eprintln!("bench: wrote {}", path.display()),
        Err(e) => eprintln!("bench: failed to write {}: {}", path.display(), e),
    }
    println!("{json_text}");
}

#[cfg(target_family = "wasm")]
pub fn emit_json(json_text: &str) {
    use wasm_bindgen::JsValue;
    web_sys::console::log_1(&JsValue::from_str(json_text));

    // Preferred: POST back to the dev server, which writes results/slint-web.json.
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_body(&JsValue::from_str(json_text));
    if let Some(win) = web_sys::window() {
        if let Ok(req) = web_sys::Request::new_with_str_and_init("/save?name=slint-web.json", &init) {
            let _ = win.fetch_with_request(&req);
        }

        // Fallback: trigger a browser download of the JSON.
        if let Some(doc) = win.document() {
            use wasm_bindgen::JsCast;
            let bag = web_sys::BlobPropertyBag::new();
            bag.set_type("application/json");
            let parts = js_sys::Array::of1(&JsValue::from_str(json_text));
            if let Ok(blob) =
                web_sys::Blob::new_with_str_sequence_and_options(parts.unchecked_ref(), &bag)
            {
                if let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) {
                    if let Ok(a) = doc.create_element("a") {
                        let _ = a.set_attribute("href", &url);
                        let _ = a.set_attribute("download", "slint-web.json");
                        let _ = a.set_attribute("style", "display:none");
                        let _ = doc.body().map(|b| b.append_child(&a));
                        if let Some(el) = a.dyn_ref::<web_sys::HtmlElement>() {
                            el.click();
                        }
                    }
                }
            }
        }
    }
}

// ---------- wasm-only: fetch dist sizes manifest ----------

#[cfg(target_family = "wasm")]
pub fn fetch_sizes(dst: std::rc::Rc<std::cell::RefCell<Option<(f64, f64, f64)>>>) {
    use wasm_bindgen::JsCast;
    wasm_bindgen_futures::spawn_local(async move {
        let win = match web_sys::window() {
            Some(w) => w,
            None => return,
        };
        let resp = match wasm_bindgen_futures::JsFuture::from(win.fetch_with_str("sizes.json")).await {
            Ok(v) => match v.dyn_into::<web_sys::Response>() {
                Ok(r) => r,
                Err(_) => return,
            },
            Err(_) => return,
        };
        let text = match resp.text().ok() {
            Some(p) => match wasm_bindgen_futures::JsFuture::from(p).await {
                Ok(t) => t.as_string().unwrap_or_default(),
                Err(_) => return,
            },
            None => return,
        };
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let g = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
            *dst.borrow_mut() = Some((g("wasm_mb"), g("js_bundle_mb"), g("total_web_dist_mb")));
        }
    });
}
