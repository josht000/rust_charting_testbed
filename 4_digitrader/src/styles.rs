use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const MAX_LINE_WIDTH: u8 = 6;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppTheme {
    pub name: String,
    pub background: String,
    pub surface: String,
    pub foreground: String,
    pub muted: String,
    pub accent: String,
    pub positive: String,
    pub negative: String,
}

impl AppTheme {
    pub fn dark() -> Self {
        Self {
            name: "Default Dark".into(),
            background: "#101010".into(),
            surface: "#191919".into(),
            foreground: "#EFEFEF".into(),
            muted: "#919191".into(),
            accent: "#BDBDBD".into(),
            positive: "#43D6B5".into(),
            negative: "#EF6575".into(),
        }
    }

    pub fn light() -> Self {
        Self {
            name: "Default Light".into(),
            background: "#F2F5F8".into(),
            surface: "#FFFFFF".into(),
            foreground: "#152235".into(),
            muted: "#69798D".into(),
            accent: "#087F73".into(),
            positive: "#087F73".into(),
            negative: "#C83D52".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChartStyle {
    pub name: String,
    pub background: String,
    pub grid: String,
    pub up_candle: String,
    pub down_candle: String,
    pub volume: String,
    pub line_width: u8,
    pub volume_opacity: u8,
}

impl ChartStyle {
    pub fn default_dark() -> Self {
        Self {
            name: "Default Dark".into(),
            background: "#111111".into(),
            grid: "#262626".into(),
            up_candle: "#26A69A".into(),
            down_candle: "#EF5350".into(),
            volume: "#858585".into(),
            line_width: 1,
            volume_opacity: 50,
        }
    }

    pub fn default_light() -> Self {
        Self {
            name: "Default Light".into(),
            background: "#FFFFFF".into(),
            grid: "#D6DEE8".into(),
            up_candle: "#168B72".into(),
            down_candle: "#D64A56".into(),
            volume: "#527FB3".into(),
            line_width: 1,
            volume_opacity: 50,
        }
    }

    pub fn validate(&self) -> Result<(), StyleError> {
        for color in [
            &self.background,
            &self.grid,
            &self.up_candle,
            &self.down_candle,
            &self.volume,
        ] {
            if !valid_color(color) {
                return Err(StyleError::InvalidColor(color.clone()));
            }
        }
        if !(1..=MAX_LINE_WIDTH).contains(&self.line_width) {
            return Err(StyleError::InvalidLineWidth);
        }
        if self.volume_opacity > 100 {
            return Err(StyleError::InvalidOpacity);
        }
        Ok(())
    }
}

fn valid_color(color: &str) -> bool {
    let color = color.strip_prefix('#').unwrap_or(color);
    matches!(color.len(), 6 | 8) && color.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StyleError {
    EmptyName,
    ReservedName,
    InvalidColor(String),
    InvalidLineWidth,
    InvalidOpacity,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StyleLibrary {
    styles: BTreeMap<String, ChartStyle>,
}

impl StyleLibrary {
    pub fn save(&mut self, name: &str, mut style: ChartStyle) -> Result<(), StyleError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StyleError::EmptyName);
        }
        if reserved_name(name) {
            return Err(StyleError::ReservedName);
        }
        style.name = name.to_owned();
        style.validate()?;
        self.styles.insert(name.to_ascii_lowercase(), style);
        Ok(())
    }

    pub fn load(&self, name: &str) -> Option<ChartStyle> {
        if reserved_name(name) {
            return Self::load_builtin(name);
        }
        self.styles.get(&name.trim().to_ascii_lowercase()).cloned()
    }

    pub fn load_builtin(name: &str) -> Option<ChartStyle> {
        match canonical_reserved_name(name).as_str() {
            "default dark" => Some(ChartStyle::default_dark()),
            "default light" => Some(ChartStyle::default_light()),
            _ => None,
        }
    }

    pub fn remove(&mut self, name: &str) -> bool {
        if reserved_name(name) {
            return false;
        }
        self.styles
            .remove(&name.trim().to_ascii_lowercase())
            .is_some()
    }

    pub fn names(&self) -> Vec<String> {
        let mut names = vec!["Default Dark".into(), "Default Light".into()];
        names.extend(self.styles.values().map(|style| style.name.clone()));
        names
    }
}

fn canonical_reserved_name(name: &str) -> String {
    let key: String = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| !matches!(ch, ' ' | '_' | '-'))
        .collect();
    match key.as_str() {
        "defaultdark" => "default dark".into(),
        "defaultlight" => "default light".into(),
        _ => String::new(),
    }
}

fn reserved_name(name: &str) -> bool {
    !canonical_reserved_name(name).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_and_chart_themes_are_distinct_and_have_defaults() {
        assert_ne!(AppTheme::dark().background, AppTheme::light().background);
        assert_ne!(
            ChartStyle::default_dark().background,
            ChartStyle::default_light().background
        );
    }

    #[test]
    fn user_presets_roundtrip_and_names_are_case_insensitive() {
        let mut library = StyleLibrary::default();
        let mut style = ChartStyle::default_dark();
        style.background = "#101820".into();
        library.save("Ocean", style.clone()).unwrap();
        let encoded = serde_json::to_string(&library).unwrap();
        let restored: StyleLibrary = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            restored.load("ocean"),
            Some(ChartStyle {
                name: "Ocean".into(),
                ..style
            })
        );
        assert_eq!(
            restored.load("Default Dark"),
            Some(ChartStyle::default_dark())
        );
        assert!(restored.names().contains(&"Ocean".to_string()));
    }

    #[test]
    fn builtins_cannot_be_shadowed_or_deleted() {
        let mut library = StyleLibrary::default();
        assert_eq!(
            library.save("default_dark", ChartStyle::default_dark()),
            Err(StyleError::ReservedName)
        );
        assert!(!library.remove("Default-Light"));
        assert_eq!(
            StyleLibrary::load_builtin("DEFAULT light").unwrap(),
            ChartStyle::default_light()
        );
    }

    #[test]
    fn invalid_style_values_are_rejected() {
        let mut style = ChartStyle::default_dark();
        style.volume = "not-a-color".into();
        assert_eq!(
            style.validate(),
            Err(StyleError::InvalidColor("not-a-color".into()))
        );
        style = ChartStyle::default_dark();
        style.line_width = MAX_LINE_WIDTH + 1;
        assert_eq!(style.validate(), Err(StyleError::InvalidLineWidth));
        style = ChartStyle::default_dark();
        style.volume_opacity = 101;
        assert_eq!(style.validate(), Err(StyleError::InvalidOpacity));
    }
}
