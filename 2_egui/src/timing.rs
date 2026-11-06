//! Small helpers for cross-platform (native + wasm) timing/memory/env info.

pub use web_time::Instant;

/// RSS / JS heap size in megabytes.
pub fn memory_mb() -> (f64, &'static str) {
    memory_mb_impl()
}

#[cfg(not(target_arch = "wasm32"))]
fn memory_mb_impl() -> (f64, &'static str) {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    let pid = sysinfo::Pid::from_u32(std::process::id());
    match sys.process(pid) {
        Some(p) => (p.memory() as f64 / 1_048_576.0, "sysinfo RSS"),
        None => (0.0, "sysinfo RSS (unavailable)"),
    }
}

#[cfg(target_arch = "wasm32")]
fn memory_mb_impl() -> (f64, &'static str) {
    // performance.memory is Chrome-only (non-standard); read via Reflect.
    let Some(window) = web_sys::window() else {
        return (0.0, "performance.memory (no window)");
    };
    let Ok(perf) = js_sys::Reflect::get(&window, &wasm_bindgen::JsValue::from_str("performance"))
    else {
        return (0.0, "performance.memory (no performance)");
    };
    let Ok(mem) = js_sys::Reflect::get(&perf, &wasm_bindgen::JsValue::from_str("memory")) else {
        return (0.0, "performance.memory (unavailable)");
    };
    let Ok(used) =
        js_sys::Reflect::get(&mem, &wasm_bindgen::JsValue::from_str("usedJSHeapSize"))
    else {
        return (0.0, "performance.memory (unavailable)");
    };
    (used.as_f64().unwrap_or(0.0) / 1_048_576.0, "performance.memory.usedJSHeapSize")
}

pub fn os_name() -> &'static str {
    #[cfg(target_arch = "wasm32")]
    {
        "wasm32"
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::consts::OS
    }
}

pub fn cpu_name() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_all();
        sys.cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .map(|w| format!("{} logical cores (browser)", w.navigator().hardware_concurrency()))
            .unwrap_or_else(|| "unknown".to_string())
    }
}

pub fn browser_ua() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.navigator().user_agent().ok())
            .unwrap_or_default()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        String::new()
    }
}

pub fn timestamp_iso() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        chrono::Utc::now().to_rfc3339()
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::new_0()
            .to_iso_string()
            .as_string()
            .unwrap_or_default()
    }
}
