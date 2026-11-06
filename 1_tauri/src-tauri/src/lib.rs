use serde::Serialize;

/// Serialized profile bin for the frontend (mirrors indicators::ProfileBin).
#[derive(Serialize)]
struct ProfileBinOut {
    price_low: f64,
    price_high: f64,
    volume: f64,
}

/// All six indicator series for one chart.
/// NaN warmup values serialize as JSON null; the frontend converts to LWC whitespace.
#[derive(Serialize)]
struct IndicatorsOut {
    ema10: Vec<f64>,
    ema20: Vec<f64>,
    ema50: Vec<f64>,
    sma200: Vec<f64>,
    vwap: Vec<f64>,
    profile: Vec<ProfileBinOut>,
}

fn bars_for(chart: usize) -> Result<Vec<datagen::Bar>, String> {
    if chart >= datagen::NUM_CHARTS {
        return Err(format!("chart index {chart} out of range (0..{})", datagen::NUM_CHARTS));
    }
    Ok(datagen::generate(chart, 1))
}

/// Return all 1s bars for `chart` (23_400 bars, one trading day).
#[tauri::command]
fn get_bars(chart: usize) -> Result<Vec<datagen::Bar>, String> {
    bars_for(chart)
}

/// Static preload: the first `INITIAL_BARS` (4000) bars for `chart`.
#[tauri::command]
fn get_initial_bars(chart: usize) -> Result<Vec<datagen::Bar>, String> {
    let bars = bars_for(chart)?;
    Ok(bars.into_iter().take(datagen::INITIAL_BARS).collect())
}

/// Streaming path: emit the remaining bars in ~50ms batches at `rate`
/// bars/sec on `bar-batch-{chart}`, then a final `stream-done-{chart}`.
/// One std::thread per chart; generation time is not measured (data is
/// generated once, up front, same as before).
#[tauri::command]
fn start_stream(app: tauri::AppHandle, chart: usize, rate: Option<u32>) -> Result<(), String> {
    const BATCH_MS: u64 = 50;
    const DEFAULT_RATE: u32 = 1000;
    let bars = bars_for(chart)?;
    let rest: Vec<datagen::Bar> = bars
        .into_iter()
        .skip(datagen::INITIAL_BARS)
        .collect();
    let rate = rate.filter(|r| *r > 0).unwrap_or(DEFAULT_RATE) as f64;
    let per_batch = ((rate * BATCH_MS as f64 / 1000.0).ceil() as usize).max(1);
    let batch_event = format!("bar-batch-{chart}");
    let done_event = format!("stream-done-{chart}");
    std::thread::spawn(move || {
        use tauri::Emitter;
        for chunk in rest.chunks(per_batch) {
            if app.emit(&batch_event, chunk.to_vec()).is_err() {
                return; // window gone
            }
            std::thread::sleep(std::time::Duration::from_millis(BATCH_MS));
        }
        let _ = app.emit(&done_event, ());
    });
    Ok(())
}

/// Return the six indicator series for `chart`, computed in Rust via
/// the shared `indicators` crate (pure-Rust reference implementations).
/// `count` limits computation to the first N bars (streaming preload).
#[tauri::command]
fn compute_indicators(chart: usize, count: Option<usize>) -> Result<IndicatorsOut, String> {
    let mut bars = bars_for(chart)?;
    if let Some(n) = count {
        bars.truncate(n);
    }
    let ind = indicators::compute_all(&bars);
    Ok(IndicatorsOut {
        ema10: ind.ema10,
        ema20: ind.ema20,
        ema50: ind.ema50,
        sma200: ind.sma200,
        vwap: ind.vwap,
        profile: ind
            .profile
            .iter()
            .map(|b| ProfileBinOut {
                price_low: b.price_low,
                price_high: b.price_high,
                volume: b.volume,
            })
            .collect(),
    })
}

/// Write benchmark results JSON to <workspace>/results/<filename>.
/// src-tauri/ -> 1_tauri/ -> rust_testbed/, so ../../results from CARGO_MANIFEST_DIR.
#[tauri::command]
fn save_results(filename: String, contents: String) -> Result<String, String> {
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err("invalid filename".into());
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../results");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&filename);
    // Inject the desktop exe size into artifact_size_mb.exe before writing.
    let mut value: serde_json::Value =
        serde_json::from_str(&contents).unwrap_or(serde_json::Value::Null);
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(meta) = std::fs::metadata(&exe) {
            if let Some(obj) = value.get_mut("artifact_size_mb").and_then(|v| v.as_object_mut()) {
                obj.insert(
                    "exe".to_string(),
                    serde_json::json!(meta.len() as f64 / 1_048_576.0),
                );
            }
        }
    }
    let out = serde_json::to_string_pretty(&value).unwrap_or(contents);
    std::fs::write(&path, &out).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// True when the app was launched with `--bench` (desktop scripted benchmark).
#[tauri::command]
fn bench_requested() -> bool {
    std::env::args().any(|a| a == "--bench")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_bars,
            get_initial_bars,
            start_stream,
            compute_indicators,
            save_results,
            bench_requested
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_bars_returns_full_day() {
        let bars = get_bars(0).unwrap();
        assert_eq!(bars.len(), datagen::BARS_PER_DAY);
    }

    #[test]
    fn indicators_have_expected_lengths() {
        let out = compute_indicators(3, None).unwrap();
        assert_eq!(out.ema10.len(), datagen::BARS_PER_DAY);
        assert_eq!(out.sma200.len(), datagen::BARS_PER_DAY);
        assert_eq!(out.vwap.len(), datagen::BARS_PER_DAY);
        assert_eq!(out.profile.len(), 40);
        assert!(out.ema10[..9].iter().all(|v| v.is_nan()));
        assert!(out.sma200[..199].iter().all(|v| v.is_nan()));
    }

    #[test]
    fn initial_bars_and_count_slice() {
        let init = get_initial_bars(0).unwrap();
        assert_eq!(init.len(), datagen::INITIAL_BARS);
        let out = compute_indicators(0, Some(datagen::INITIAL_BARS)).unwrap();
        assert_eq!(out.ema10.len(), datagen::INITIAL_BARS);
        assert_eq!(out.vwap.len(), datagen::INITIAL_BARS);
    }

    #[test]
    fn rejects_bad_chart() {
        assert!(get_bars(8).is_err());
        assert!(get_initial_bars(99).is_err());
        assert!(compute_indicators(99, None).is_err());
    }
}
