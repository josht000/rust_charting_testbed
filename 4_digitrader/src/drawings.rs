use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const MAX_DRAWING_WIDTH: u8 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawingKind {
    #[default]
    HorizontalLine,
    TrendLine,
    Ray,
    ExtendedLine,
    HorizontalRay,
    VerticalLine,
    Crosshair,
    Measure,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinePattern {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawingStyle {
    pub color: String,
    pub width: u8,
    pub pattern: LinePattern,
}

impl Default for DrawingStyle {
    fn default() -> Self {
        Self {
            color: "#43D6B5".into(),
            width: 2,
            pattern: LinePattern::Solid,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub timestamp_ms: i64,
    pub price: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    pub id: String,
    pub symbol: String,
    pub kind: DrawingKind,
    pub start: Anchor,
    pub end: Option<Anchor>,
    pub style: DrawingStyle,
    pub visible: bool,
    pub locked: bool,
}

impl Drawing {
    pub fn validate(&self) -> Result<(), DrawingError> {
        if self.id.trim().is_empty() {
            return Err(DrawingError::EmptyId);
        }
        let symbol = normalize_symbol(&self.symbol);
        if symbol.is_empty() {
            return Err(DrawingError::EmptySymbol);
        }
        if !valid_anchor(self.start) || self.end.is_some_and(|point| !valid_anchor(point)) {
            return Err(DrawingError::InvalidAnchor);
        }
        if !(1..=MAX_DRAWING_WIDTH).contains(&self.style.width) {
            return Err(DrawingError::InvalidWidth);
        }
        if !valid_color(&self.style.color) {
            return Err(DrawingError::InvalidColor);
        }
        if self.kind != DrawingKind::HorizontalLine
            && self.kind != DrawingKind::HorizontalRay
            && self.kind != DrawingKind::VerticalLine
            && self.kind != DrawingKind::Crosshair
            && self.end.is_none()
        {
            return Err(DrawingError::MissingSecondAnchor);
        }
        Ok(())
    }
}

fn valid_anchor(anchor: Anchor) -> bool {
    anchor.timestamp_ms >= 0 && anchor.price.is_finite()
}

fn valid_color(color: &str) -> bool {
    let color = color.strip_prefix('#').unwrap_or(color);
    matches!(color.len(), 6 | 8) && color.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn normalize_symbol(symbol: &str) -> String {
    symbol.trim().to_ascii_uppercase()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrawingError {
    EmptyId,
    EmptySymbol,
    InvalidAnchor,
    MissingSecondAnchor,
    InvalidWidth,
    InvalidColor,
    DuplicateId,
}

impl std::fmt::Display for DrawingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::EmptyId => "Drawing id cannot be empty.",
            Self::EmptySymbol => "Drawing symbol cannot be empty.",
            Self::InvalidAnchor => "Drawing anchors need a non-negative time and finite price.",
            Self::MissingSecondAnchor => "This drawing needs a second time/price anchor.",
            Self::InvalidWidth => "Drawing width must be between 1 and 8 pixels.",
            Self::InvalidColor => "Drawing color must be a six- or eight-digit hex color.",
            Self::DuplicateId => "A drawing with this id already exists for the symbol.",
        })
    }
}

impl std::error::Error for DrawingError {}

/// Per-viewer annotation collection. Every chart bound to this collection
/// asks for drawings using its normalized symbol; timeframe is deliberately
/// not part of the key because anchors use absolute market timestamps.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DrawingStore {
    pub viewer_id: String,
    drawings: BTreeMap<String, Vec<Drawing>>,
}

impl DrawingStore {
    pub fn new(viewer_id: impl Into<String>) -> Self {
        Self {
            viewer_id: viewer_id.into(),
            drawings: BTreeMap::new(),
        }
    }

    pub fn add(&mut self, drawing: Drawing) -> Result<(), DrawingError> {
        drawing.validate()?;
        let symbol = normalize_symbol(&drawing.symbol);
        let entries = self.drawings.entry(symbol.clone()).or_default();
        if entries.iter().any(|entry| entry.id == drawing.id) {
            return Err(DrawingError::DuplicateId);
        }
        let mut drawing = drawing;
        drawing.symbol = symbol;
        entries.push(drawing);
        Ok(())
    }

    pub fn for_symbol(&self, symbol: &str) -> &[Drawing] {
        self.drawings
            .get(&normalize_symbol(symbol))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn remove(&mut self, symbol: &str, id: &str) -> Option<Drawing> {
        let entries = self.drawings.get_mut(&normalize_symbol(symbol))?;
        let index = entries.iter().position(|entry| entry.id == id)?;
        let removed = entries.remove(index);
        if entries.is_empty() {
            self.drawings.remove(&normalize_symbol(symbol));
        }
        Some(removed)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trend(id: &str, symbol: &str) -> Drawing {
        Drawing {
            id: id.into(),
            symbol: symbol.into(),
            kind: DrawingKind::TrendLine,
            start: Anchor {
                timestamp_ms: 1_700_000_000_000,
                price: 520.0,
            },
            end: Some(Anchor {
                timestamp_ms: 1_700_003_600_000,
                price: 525.0,
            }),
            style: DrawingStyle::default(),
            visible: true,
            locked: false,
        }
    }

    #[test]
    fn same_symbol_drawings_are_shared_across_timeframes() {
        let mut store = DrawingStore::new("viewer-1");
        store.add(trend("trend-1", " spy ")).unwrap();
        assert_eq!(store.for_symbol("SPY").len(), 1);
        assert_eq!(store.for_symbol("spy").len(), 1);
        assert!(store.for_symbol("AAPL").is_empty());
        assert_eq!(store.for_symbol("SPY")[0].symbol, "SPY");
    }

    #[test]
    fn drawing_store_roundtrips_and_removes_last_symbol_entry() {
        let mut store = DrawingStore::new("viewer-1");
        store.add(trend("trend-1", "SPY")).unwrap();
        let restored = DrawingStore::from_json(&store.to_json().unwrap()).unwrap();
        assert_eq!(restored, store);
        assert!(store.remove("spy", "trend-1").is_some());
        assert!(store.for_symbol("SPY").is_empty());
    }

    #[test]
    fn rejects_invalid_anchors_styles_and_duplicate_ids() {
        let mut store = DrawingStore::new("viewer-1");
        let mut drawing = trend("trend-1", "SPY");
        drawing.start.price = f64::NAN;
        assert_eq!(store.add(drawing), Err(DrawingError::InvalidAnchor));

        let mut drawing = trend("trend-1", "SPY");
        drawing.style.width = 0;
        assert_eq!(store.add(drawing), Err(DrawingError::InvalidWidth));

        let mut drawing = trend("trend-1", "SPY");
        drawing.style.color = "#xyzxyz".into();
        assert_eq!(store.add(drawing), Err(DrawingError::InvalidColor));

        store.add(trend("trend-1", "SPY")).unwrap();
        assert_eq!(
            store.add(trend("trend-1", "SPY")),
            Err(DrawingError::DuplicateId)
        );
    }
}
