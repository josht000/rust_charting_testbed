use crate::model::normalize_symbol;

pub const MAX_CHARTS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChartEntry {
    pub id: u32,
    pub symbol: String,
    pub linked: bool,
}

#[derive(Debug)]
pub struct Workspace {
    charts: Vec<ChartEntry>,
    active: u32,
    next_id: u32,
    row_heights: Vec<f32>,
    column_splits: [f32; 4],
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            charts: vec![ChartEntry {
                id: 1,
                symbol: "SPY".into(),
                linked: true,
            }],
            active: 1,
            next_id: 2,
            row_heights: vec![1.0],
            column_splits: [0.5; 4],
        }
    }
}

impl Workspace {
    pub(crate) fn from_saved(charts: &[crate::settings::SavedChart]) -> Self {
        let entries = charts
            .iter()
            .enumerate()
            .map(|(index, chart)| ChartEntry {
                id: index as u32 + 1,
                symbol: chart.symbol.clone(),
                linked: chart.linked,
            })
            .collect::<Vec<_>>();
        let mut workspace = Self {
            next_id: entries.len() as u32 + 1,
            charts: entries,
            active: 1,
            row_heights: Vec::new(),
            column_splits: [0.5; 4],
        };
        workspace.reset_grid();
        workspace
    }
    pub fn charts(&self) -> &[ChartEntry] {
        &self.charts
    }

    pub fn active(&self) -> u32 {
        self.active
    }

    fn reset_grid(&mut self) {
        let rows = self.charts.len().div_ceil(2);
        self.row_heights = vec![1.0 / rows as f32; rows];
        self.column_splits = [0.5; 4];
    }

    pub fn grid_rect(&self, index: usize) -> Option<(f32, f32, f32, f32)> {
        if index >= self.charts.len() {
            return None;
        }
        let row = index / 2;
        let y = self.row_heights[..row].iter().sum();
        let height = self.row_heights[row];
        let paired = row * 2 + 1 < self.charts.len();
        let split = self.column_splits[row];
        let (x, width) = if !paired {
            (0.0, 1.0)
        } else if index % 2 == 0 {
            (0.0, split)
        } else {
            (split, 1.0 - split)
        };
        Some((x, y, width, height))
    }

    pub fn resize_column(&mut self, row: usize, delta: f32, minimum: f32) {
        if delta.is_finite() && minimum.is_finite() && row * 2 + 1 < self.charts.len() {
            let minimum = minimum.clamp(0.0, 0.5);
            self.column_splits[row] =
                (self.column_splits[row] + delta).clamp(minimum, 1.0 - minimum);
        }
    }

    pub fn resize_row(&mut self, row: usize, delta: f32, minimum: f32) {
        if !delta.is_finite() || !minimum.is_finite() || row + 1 >= self.row_heights.len() {
            return;
        }
        let upper = self.row_heights[row];
        let lower = self.row_heights[row + 1];
        let minimum = minimum.clamp(0.0, (upper + lower) / 2.0);
        let change = delta.clamp(minimum - upper, lower - minimum);
        self.row_heights[row] += change;
        self.row_heights[row + 1] -= change;
    }

    pub fn select(&mut self, id: u32) -> bool {
        if self.charts.iter().any(|chart| chart.id == id) {
            self.active = id;
            true
        } else {
            false
        }
    }

    pub fn add(&mut self) -> Option<u32> {
        if self.charts.len() == MAX_CHARTS {
            return None;
        }
        let current = self.charts.iter().find(|c| c.id == self.active)?;
        let id = self.next_id;
        self.next_id += 1;
        self.charts.push(ChartEntry {
            id,
            symbol: current.symbol.clone(),
            linked: current.linked,
        });
        self.reset_grid();
        Some(id)
    }

    pub fn remove(&mut self, id: u32) -> bool {
        if self.charts.len() <= 1 {
            return false;
        }
        let Some(index) = self.charts.iter().position(|chart| chart.id == id) else {
            return false;
        };
        self.charts.remove(index);
        self.reset_grid();
        if self.active == id {
            self.active = self.charts[0].id;
        }
        true
    }

    pub fn set_symbol(&mut self, id: u32, symbol: &str) -> bool {
        let Some(chart) = self.charts.iter().find(|chart| chart.id == id) else {
            return false;
        };
        let linked = chart.linked;
        let symbol = normalize_symbol(symbol);
        for chart in &mut self.charts {
            if chart.id == id || (linked && chart.linked) {
                chart.symbol.clone_from(&symbol);
            }
        }
        true
    }

    pub fn toggle_link(&mut self, id: u32) -> bool {
        let Some(index) = self.charts.iter().position(|chart| chart.id == id) else {
            return false;
        };
        if !self.charts[index].linked {
            if let Some(symbol) = self
                .charts
                .iter()
                .find(|chart| chart.linked)
                .map(|c| c.symbol.clone())
            {
                self.charts[index].symbol = symbol;
            }
        }
        self.charts[index].linked = !self.charts[index].linked;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_splits_rows_in_pairs_and_resets_after_chart_changes() {
        let mut ws = Workspace::default();
        assert_eq!(ws.grid_rect(0), Some((0.0, 0.0, 1.0, 1.0)));
        ws.add();
        assert_eq!(ws.grid_rect(0), Some((0.0, 0.0, 0.5, 1.0)));
        assert_eq!(ws.grid_rect(1), Some((0.5, 0.0, 0.5, 1.0)));
        ws.add();
        assert_eq!(ws.grid_rect(0), Some((0.0, 0.0, 0.5, 0.5)));
        assert_eq!(ws.grid_rect(2), Some((0.0, 0.5, 1.0, 0.5)));
        ws.add();
        assert_eq!(ws.grid_rect(3), Some((0.5, 0.5, 0.5, 0.5)));
        ws.resize_column(0, 0.2, 0.2);
        ws.resize_row(0, 0.1, 0.1);
        assert!((ws.grid_rect(0).unwrap().2 - 0.7).abs() < 0.0001);
        assert!((ws.grid_rect(0).unwrap().3 - 0.6).abs() < 0.0001);
        assert!((ws.grid_rect(2).unwrap().3 - 0.4).abs() < 0.0001);
        assert_eq!(ws.grid_rect(2).unwrap().2, 0.5);
        ws.remove(4);
        assert_eq!(ws.grid_rect(2), Some((0.0, 0.5, 1.0, 0.5)));
        for _ in 3..8 {
            ws.add();
        }
        assert_eq!(ws.grid_rect(7), Some((0.5, 0.75, 0.5, 0.25)));
    }

    #[test]
    fn divider_resizing_is_bounded_and_invalid_deltas_are_ignored() {
        let mut ws = Workspace::default();
        for _ in 1..8 {
            ws.add();
        }
        ws.resize_column(0, 50.0, 0.2);
        assert_eq!(ws.grid_rect(0).unwrap().2, 0.8);
        ws.resize_column(0, -50.0, 0.2);
        assert_eq!(ws.grid_rect(0).unwrap().2, 0.2);
        ws.resize_row(0, 50.0, 0.1);
        assert!((ws.grid_rect(1).unwrap().3 - 0.4).abs() < 0.0001);
        let before = ws.grid_rect(0);
        ws.resize_row(0, f32::NAN, 0.1);
        ws.resize_column(0, f32::INFINITY, 0.2);
        assert_eq!(before, ws.grid_rect(0));
    }

    #[test]
    fn linked_symbols_propagate_but_unlinked_charts_remain_independent() {
        let mut ws = Workspace::default();
        let second = ws.add().unwrap();
        let third = ws.add().unwrap();
        ws.toggle_link(third);
        ws.set_symbol(1, " aapl ");
        assert_eq!(ws.charts()[0].symbol, "AAPL");
        assert_eq!(ws.charts()[1].symbol, "AAPL");
        assert_eq!(ws.charts()[2].symbol, "SPY");
        ws.set_symbol(third, "btc/usd");
        assert_eq!(ws.charts()[0].symbol, "AAPL");
        ws.toggle_link(third);
        assert_eq!(ws.charts()[2].symbol, "AAPL");
        ws.set_symbol(second, "SPY");
        assert!(ws.charts().iter().all(|chart| chart.symbol == "SPY"));
    }

    #[test]
    fn chart_lifecycle_preserves_selection_and_caps_count() {
        let mut ws = Workspace::default();
        assert!(!ws.remove(1));
        let second = ws.add().unwrap();
        assert!(ws.select(second));
        assert!(ws.remove(second));
        assert_eq!(ws.active(), 1);
        for _ in 1..MAX_CHARTS {
            assert!(ws.add().is_some());
        }
        assert_eq!(ws.add(), None);
        assert!(!ws.select(999));
    }
}
