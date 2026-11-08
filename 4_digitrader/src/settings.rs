use crate::chart::PriceStyle;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedChart {
    pub symbol: String,
    pub linked: bool,
    pub style: PriceStyle,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub version: u8,
    pub charts: Vec<SavedChart>,
}

impl Settings {
    pub fn parse(json: &str) -> Result<Self, String> {
        let value: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if value.version != 1
            || value.charts.is_empty()
            || value.charts.len() > crate::workspace::MAX_CHARTS
        {
            return Err("unsupported or invalid chart settings".into());
        }
        let linked_symbol = value.charts.iter().find(|c| c.linked).map(|c| &c.symbol);
        for chart in &value.charts {
            if chart.symbol.is_empty()
                || crate::model::normalize_symbol(&chart.symbol) != chart.symbol
                || !chart
                    .symbol
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'/'))
                || !chart.symbol.bytes().any(|b| b.is_ascii_alphanumeric())
                || (chart.linked && Some(&chart.symbol) != linked_symbol)
                || !(1..=3).contains(&chart.style.thickness)
                || chart.style.glow_intensity > 2
                || chart.style.shadow_intensity > 2
            {
                return Err("invalid chart settings values".into());
            }
        }
        Ok(value)
    }
}

#[cfg(not(target_family = "wasm"))]
fn path() -> Result<std::path::PathBuf, String> {
    let root = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                std::path::PathBuf::from(home)
                    .join(".config")
                    .into_os_string()
            })
        })
        .ok_or("No user configuration directory is available")?;
    Ok(std::path::PathBuf::from(root)
        .join("DigiTrader")
        .join("chart-settings.json"))
}

#[cfg(not(target_family = "wasm"))]
pub fn load() -> Result<Option<Settings>, String> {
    load_path(&path()?)
}

#[cfg(not(target_family = "wasm"))]
fn load_path(path: &std::path::Path) -> Result<Option<Settings>, String> {
    match std::fs::read_to_string(&path) {
        Ok(json) => Settings::parse(&json).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Cannot read {}: {e}", path.display())),
    }
}

#[cfg(not(target_family = "wasm"))]
pub fn save(settings: &Settings) -> Result<(), String> {
    save_path(&path()?, settings)
}

#[cfg(not(target_family = "wasm"))]
fn save_path(path: &std::path::Path, settings: &Settings) -> Result<(), String> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or("Invalid settings path")?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
    let data = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(
        ".chart-settings-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| format!("Cannot create {}: {e}", temp.display()))?;
    let result = file
        .write_all(data.as_bytes())
        .and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = result.and_then(|_| std::fs::rename(&temp, path)) {
        let cleanup = std::fs::remove_file(&temp);
        return Err(format!(
            "Cannot write {}: {error}{}",
            path.display(),
            cleanup
                .err()
                .map(|e| format!("; cannot remove temporary file: {e}"))
                .unwrap_or_default()
        ));
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function settings_load() { return localStorage.getItem("digitrader.chart-settings.v1"); }
export function settings_save(data) { localStorage.setItem("digitrader.chart-settings.v1", data); }
"#)]
extern "C" {
    #[wasm_bindgen(catch)]
    fn settings_load() -> Result<Option<String>, wasm_bindgen::JsValue>;
    #[wasm_bindgen(catch)]
    fn settings_save(data: &str) -> Result<(), wasm_bindgen::JsValue>;
}

#[cfg(target_family = "wasm")]
pub fn load() -> Result<Option<Settings>, String> {
    settings_load()
        .map_err(|e| format!("Cannot read browser storage: {e:?}"))?
        .map(|json| Settings::parse(&json))
        .transpose()
}

#[cfg(target_family = "wasm")]
pub fn save(settings: &Settings) -> Result<(), String> {
    let json = serde_json::to_string(settings).map_err(|e| e.to_string())?;
    settings_save(&json).map_err(|e| format!("Cannot save browser storage: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_round_trip_and_reject_invalid_documents() {
        let settings = Settings {
            version: 1,
            charts: vec![SavedChart {
                symbol: "SPY".into(),
                linked: true,
                style: PriceStyle {
                    visible: false,
                    grid_visible: false,
                    labels_visible: false,
                    border: crate::chart::PriceBorder::None,
                    ..PriceStyle::default()
                },
            }],
        };
        let restored = Settings::parse(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored.charts[0].style, settings.charts[0].style);
        assert!(Settings::parse("{").is_err());
        assert!(Settings::parse(r#"{"version":2,"charts":[]}"#).is_err());
        let invalid = settings.clone();
        let mut invalid = invalid;
        invalid.charts[0].style.glow_intensity = 255;
        assert!(Settings::parse(&serde_json::to_string(&invalid).unwrap()).is_err());
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn file_storage_round_trip_replaces_existing_file_and_reports_bad_data() {
        let path = std::env::temp_dir().join(format!(
            "digitrader-settings-test-{}.json",
            std::process::id()
        ));
        let settings = Settings {
            version: 1,
            charts: vec![SavedChart {
                symbol: "SPY".into(),
                linked: true,
                style: PriceStyle::default(),
            }],
        };
        save_path(&path, &settings).unwrap();
        let mut changed = settings.clone();
        changed.charts[0].style.glow_intensity = 2;
        save_path(&path, &changed).unwrap();
        assert_eq!(
            load_path(&path).unwrap().unwrap().charts[0].style,
            changed.charts[0].style
        );
        std::fs::write(&path, "broken").unwrap();
        assert!(load_path(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
