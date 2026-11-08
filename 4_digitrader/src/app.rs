use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use slint::{ComponentHandle, Model, VecModel};

use crate::broker::{BrokerProvider, FixtureBroker};
use crate::chart::{Chart, PriceBorder, PriceColor, PricePattern, PriceStyle};
use crate::drawings::{Anchor, Drawing, DrawingKind, DrawingStore, DrawingStyle};
use crate::model::{normalize_symbol, OrderKind, OrderRequest, Side};
use crate::workspace::{ChartEntry, Workspace};
use crate::{AppWindow, AxisTick, ChartItem, PriceOptions};

fn slint_price_options(style: PriceStyle) -> PriceOptions {
    PriceOptions {
        visible: style.visible,
        border: style.border.name().into(),
        grid: style.grid_visible,
        labels: style.labels_visible,
        thickness: style.thickness.into(),
        glow_color: style.glow_color.name().into(),
        glow: style.glow_intensity.into(),
        shadow_color: style.shadow_color.name().into(),
        shadow: style.shadow_intensity.into(),
    }
}

fn slint_ticks(ticks: &[crate::chart::AxisTick]) -> slint::ModelRc<AxisTick> {
    Rc::new(VecModel::from(
        ticks
            .iter()
            .map(|tick| AxisTick {
                position: tick.position,
                label: tick.label.as_str().into(),
            })
            .collect::<Vec<_>>(),
    ))
    .into()
}

fn slint_marker(marker: Option<&crate::chart::AxisTick>) -> AxisTick {
    match marker {
        Some(marker) => AxisTick {
            position: marker.position,
            label: marker.label.as_str().into(),
        },
        None => AxisTick::default(),
    }
}

struct ChartWorkspace {
    workspace: Workspace,
    charts: BTreeMap<u32, Rc<RefCell<Chart>>>,
    detached: BTreeMap<u32, ChartEntry>,
    next_popout_id: u32,
    popout_interactions: BTreeMap<u32, PopoutInteraction>,
    rows: Rc<VecModel<ChartItem>>,
    persistence: bool,
    storage_error: Option<String>,
    #[cfg(not(target_family = "wasm"))]
    popouts: BTreeMap<u32, crate::CompactChartWindow>,
}

struct PopoutInteraction {
    tool: String,
    anchor: Option<Anchor>,
    pressed: bool,
    last_x: f64,
}

impl ChartWorkspace {
    fn new() -> Self {
        let mut charts = BTreeMap::new();
        charts.insert(
            1,
            Rc::new(RefCell::new(Chart::new(datagen::generate(0, 1)))),
        );
        Self {
            workspace: Workspace::default(),
            charts,
            detached: BTreeMap::new(),
            next_popout_id: 1 << 30,
            popout_interactions: BTreeMap::new(),
            rows: Rc::new(VecModel::default()),
            persistence: false,
            storage_error: None,
            #[cfg(not(target_family = "wasm"))]
            popouts: BTreeMap::new(),
        }
    }

    fn persistent() -> Self {
        let mut state = Self::new();
        match crate::settings::load() {
            Ok(Some(settings)) => {
                state.restore(&settings);
                state.persistence = true;
            }
            Ok(None) => state.persistence = true,
            Err(error) => {
                state.storage_error = Some(format!("Settings not loaded; saving disabled: {error}"))
            }
        }
        state
    }

    fn restore(&mut self, settings: &crate::settings::Settings) {
        self.workspace = Workspace::from_saved(&settings.charts);
        self.charts.clear();
        for (entry, saved) in self.workspace.charts().iter().zip(&settings.charts) {
            let mut chart = Chart::new(datagen::generate(0, 1));
            chart.set_price_style(saved.style);
            self.charts.insert(entry.id, Rc::new(RefCell::new(chart)));
        }
    }

    fn save(&self, ui: &AppWindow) {
        if let Some(error) = &self.storage_error {
            ui.set_interaction_status(error.as_str().into());
        } else if self.persistence {
            let settings = crate::settings::Settings {
                version: 1,
                charts: self
                    .workspace
                    .charts()
                    .iter()
                    .map(|entry| crate::settings::SavedChart {
                        symbol: entry.symbol.clone(),
                        linked: entry.linked,
                        style: self.charts[&entry.id].borrow().price_style(),
                    })
                    .collect(),
            };
            if let Err(error) = crate::settings::save(&settings) {
                ui.set_interaction_status(format!("Cannot save chart settings: {error}").into());
            }
        }
    }

    fn active_chart(&self) -> Rc<RefCell<Chart>> {
        self.charts[&self.workspace.active()].clone()
    }

    fn refresh(&self, ui: &AppWindow, drawings: &DrawingStore) {
        self.refresh_frames(ui, drawings, None);
    }

    fn refresh_active(&self, ui: &AppWindow, drawings: &DrawingStore) {
        self.refresh_frames(ui, drawings, Some(self.workspace.active()));
    }

    fn refresh_frames(&self, ui: &AppWindow, drawings: &DrawingStore, target: Option<u32>) {
        let mut rows = Vec::new();
        for (index, entry) in self.workspace.charts().iter().enumerate() {
            let (x, y, width, height) = self.workspace.grid_rect(index).expect("chart grid");
            let column_divider = index % 2 == 0 && index + 1 < self.workspace.charts().len();
            let row_divider = index % 2 == 0 && index + 2 < self.workspace.charts().len();
            if target.is_some_and(|id| id != entry.id) {
                if let Some(mut row) = self.rows.row_data(index) {
                    if row.id == entry.id as i32 {
                        row.selected = entry.id == self.workspace.active();
                        row.x = x;
                        row.y = y;
                        row.width = width;
                        row.height = height;
                        row.column_divider = column_divider;
                        row.row_divider = row_divider;
                        rows.push(row);
                        continue;
                    }
                }
            }
            let chart = self.charts[&entry.id].borrow();
            let scale = ui.window().scale_factor();
            let frame = chart.render_frame_with_drawings(drawings.for_symbol(&entry.symbol), scale);
            let (time_ticks, price_ticks) = chart.axis_ticks(scale);
            let current_price = chart.current_price_marker();
            let style = chart.price_style();
            #[cfg(target_family = "wasm")]
            if crate::web_popout::chart_popup_is_open(entry.id) {
                let pixels: Vec<u8> = frame
                    .as_slice()
                    .iter()
                    .flat_map(|pixel| [pixel.r, pixel.g, pixel.b, pixel.a])
                    .collect();
                crate::web_popout::chart_popup_update(
                    entry.id,
                    &entry.symbol,
                    &pixels,
                    frame.width(),
                    frame.height(),
                    &serde_json::to_string(&time_ticks).expect("axis ticks serialize"),
                    &serde_json::to_string(&price_ticks).expect("axis ticks serialize"),
                    &serde_json::to_string(&current_price).expect("price marker serializes"),
                    style.color.name(),
                    style.pattern.name(),
                    &serde_json::to_string(&style).expect("price style serializes"),
                    entry.linked,
                    "Cursor",
                );
            }
            let image = slint::Image::from_rgba8(frame);
            #[cfg(not(target_family = "wasm"))]
            if let Some(window) = self.popouts.get(&entry.id) {
                window.set_symbol(entry.symbol.as_str().into());
                window.set_chart_image(image.clone());
                window.set_time_ticks(slint_ticks(&time_ticks));
                window.set_price_ticks(slint_ticks(&price_ticks));
                window.set_current_price(slint_marker(current_price.as_ref()));
                window.set_price_color(style.color.name().into());
                window.set_price_pattern(style.pattern.name().into());
                window.set_price_options(slint_price_options(style));
            }
            if entry.id == self.workspace.active() {
                ui.set_chart_image(image.clone());
            }
            rows.push(ChartItem {
                id: entry.id as i32,
                symbol: entry.symbol.as_str().into(),
                linked: entry.linked,
                selected: entry.id == self.workspace.active(),
                image,
                time_ticks: slint_ticks(&time_ticks),
                price_ticks: slint_ticks(&price_ticks),
                current_price: slint_marker(current_price.as_ref()),
                price_color: style.color.name().into(),
                price_pattern: style.pattern.name().into(),
                price_options: slint_price_options(style),
                x,
                y,
                width,
                height,
                column_divider,
                row_divider,
            });
        }
        if target.is_none() || target.is_some_and(|id| self.detached.contains_key(&id)) {
            for entry in self.detached.values() {
                if target.is_some_and(|id| id != entry.id) {
                    continue;
                }
                let chart = self.charts[&entry.id].borrow();
                let scale = ui.window().scale_factor();
                let frame =
                    chart.render_frame_with_drawings(drawings.for_symbol(&entry.symbol), scale);
                let (time_ticks, price_ticks) = chart.axis_ticks(scale);
                let current_price = chart.current_price_marker();
                let style = chart.price_style();
                #[cfg(target_family = "wasm")]
                if crate::web_popout::chart_popup_is_open(entry.id) {
                    let pixels: Vec<u8> = frame
                        .as_slice()
                        .iter()
                        .flat_map(|pixel| [pixel.r, pixel.g, pixel.b, pixel.a])
                        .collect();
                    crate::web_popout::chart_popup_update(
                        entry.id,
                        &entry.symbol,
                        &pixels,
                        frame.width(),
                        frame.height(),
                        &serde_json::to_string(&time_ticks).expect("axis ticks serialize"),
                        &serde_json::to_string(&price_ticks).expect("axis ticks serialize"),
                        &serde_json::to_string(&current_price).expect("price marker serializes"),
                        style.color.name(),
                        style.pattern.name(),
                        &serde_json::to_string(&style).expect("price style serializes"),
                        entry.linked,
                        &self.popout_interactions[&entry.id].tool,
                    );
                }
                #[cfg(not(target_family = "wasm"))]
                if let Some(window) = self.popouts.get(&entry.id) {
                    window.set_symbol(entry.symbol.as_str().into());
                    window.set_linked(entry.linked);
                    window
                        .set_active_tool(self.popout_interactions[&entry.id].tool.as_str().into());
                    window.set_chart_image(slint::Image::from_rgba8(frame));
                    window.set_time_ticks(slint_ticks(&time_ticks));
                    window.set_price_ticks(slint_ticks(&price_ticks));
                    window.set_current_price(slint_marker(current_price.as_ref()));
                    window.set_price_color(style.color.name().into());
                    window.set_price_pattern(style.pattern.name().into());
                    window.set_price_options(slint_price_options(style));
                }
            }
        }
        if self.rows.row_count() == rows.len() {
            for (index, row) in rows.into_iter().enumerate() {
                self.rows.set_row_data(index, row);
            }
        } else {
            self.rows.set_vec(rows);
        }
    }

    fn open_popout(&mut self, source_id: u32) -> Result<u32, String> {
        let source = self
            .workspace
            .charts()
            .iter()
            .find(|c| c.id == source_id)
            .ok_or("Chart not found.")?;
        let id = self.next_popout_id;
        let entry = ChartEntry {
            id,
            symbol: source.symbol.clone(),
            linked: false,
        };
        let chart = self.charts[&source_id].borrow().clone();
        #[cfg(not(target_family = "wasm"))]
        {
            let window = crate::CompactChartWindow::new().map_err(|error| error.to_string())?;
            window.show().map_err(|error| error.to_string())?;
            self.popouts.insert(id, window);
        }
        #[cfg(target_family = "wasm")]
        if !crate::web_popout::chart_popup_open(id) {
            return Err("Pop-up blocked. Allow pop-ups for this page and try again.".into());
        }
        self.next_popout_id = self
            .next_popout_id
            .checked_add(1)
            .ok_or("Too many pop-outs.")?;
        self.detached.insert(id, entry);
        self.charts.insert(id, Rc::new(RefCell::new(chart)));
        self.popout_interactions.insert(
            id,
            PopoutInteraction {
                tool: "Cursor".into(),
                anchor: None,
                pressed: false,
                last_x: 0.0,
            },
        );
        Ok(id)
    }

    fn popout_is_open(&self, id: u32) -> bool {
        #[cfg(not(target_family = "wasm"))]
        {
            self.popouts
                .get(&id)
                .is_some_and(|window| window.window().is_visible())
        }
        #[cfg(target_family = "wasm")]
        {
            crate::web_popout::chart_popup_is_open(id)
        }
    }

    fn remove_grid_chart(&mut self, id: u32) -> bool {
        if !self.workspace.remove(id) {
            return false;
        }
        self.charts.remove(&id);
        true
    }

    fn prune_closed_popouts(&mut self) {
        let closed: Vec<_> = self
            .detached
            .keys()
            .copied()
            .filter(|id| !self.popout_is_open(*id))
            .collect();
        for id in closed {
            self.detached.remove(&id);
            self.charts.remove(&id);
            self.popout_interactions.remove(&id);
            #[cfg(not(target_family = "wasm"))]
            self.popouts.remove(&id);
        }
    }

    fn set_symbol(&mut self, id: u32, symbol: &str) {
        let changed_ids: Vec<u32> = self
            .detached
            .values()
            .filter(|entry| {
                entry.id == id
                    || (entry.linked
                        && (self.detached.get(&id).is_some_and(|e| e.linked)
                            || self
                                .workspace
                                .charts()
                                .iter()
                                .any(|e| e.id == id && e.linked)))
            })
            .map(|entry| entry.id)
            .collect();
        for changed in changed_ids {
            if let Some(interaction) = self.popout_interactions.get_mut(&changed) {
                interaction.anchor = None;
            }
        }
        if let Some(entry) = self.detached.get_mut(&id) {
            entry.symbol = symbol.to_owned();
            if entry.linked {
                for entry in self.detached.values_mut().filter(|e| e.linked) {
                    entry.symbol = symbol.to_owned();
                }
                if let Some(grid_id) = self
                    .workspace
                    .charts()
                    .iter()
                    .find(|e| e.linked)
                    .map(|e| e.id)
                {
                    self.workspace.set_symbol(grid_id, symbol);
                }
            }
            return;
        }
        let linked = self
            .workspace
            .charts()
            .iter()
            .find(|chart| chart.id == id)
            .is_some_and(|chart| chart.linked);
        self.workspace.set_symbol(id, symbol);
        if linked {
            for entry in self.detached.values_mut().filter(|entry| entry.linked) {
                entry.symbol = symbol.to_owned();
            }
        }
    }

    fn toggle_link(&mut self, id: u32) -> bool {
        if let Some(entry) = self.detached.get(&id) {
            let joining = !entry.linked;
            let symbol = if joining {
                self.workspace
                    .charts()
                    .iter()
                    .chain(self.detached.values())
                    .find(|e| e.linked)
                    .map(|e| e.symbol.clone())
            } else {
                None
            };
            let entry = self.detached.get_mut(&id).expect("detached chart exists");
            entry.linked = joining;
            if let Some(symbol) = symbol {
                entry.symbol = symbol;
            }
            return true;
        }
        let Some(entry) = self.workspace.charts().iter().find(|chart| chart.id == id) else {
            return false;
        };
        let detached_symbol =
            if !entry.linked && !self.workspace.charts().iter().any(|chart| chart.linked) {
                self.detached
                    .values()
                    .find(|chart| chart.linked)
                    .map(|chart| chart.symbol.clone())
            } else {
                None
            };
        if !self.workspace.toggle_link(id) {
            return false;
        }
        if let Some(symbol) = detached_symbol {
            self.set_symbol(id, &symbol);
        }
        true
    }
}

fn render_chart(ui: &AppWindow, workspace: &ChartWorkspace, drawings: &DrawingStore) {
    workspace.refresh(ui, drawings);
}

fn zoom_popout(
    ui: &AppWindow,
    workspace: &Rc<RefCell<ChartWorkspace>>,
    drawings: &Rc<RefCell<DrawingStore>>,
    id: u32,
    delta: f64,
    fraction: f64,
) {
    if !delta.is_finite() || !fraction.is_finite() {
        ui.set_interaction_status("Invalid chart zoom.".into());
        return;
    }
    let state = workspace.borrow();
    let Some(chart) = state.charts.get(&id) else {
        ui.set_interaction_status("Chart not found.".into());
        return;
    };
    let mut chart = chart.borrow_mut();
    let x = fraction.clamp(0.0, 1.0) * f64::from(chart.screen_size().0);
    chart.zoom_at(x, delta);
    drop(chart);
    state.refresh_frames(ui, &drawings.borrow(), Some(id));
}

fn resize_popout(
    ui: &AppWindow,
    workspace: &Rc<RefCell<ChartWorkspace>>,
    drawings: &Rc<RefCell<DrawingStore>>,
    id: u32,
    width: f64,
    height: f64,
    scale: f64,
) {
    if !width.is_finite()
        || !height.is_finite()
        || !scale.is_finite()
        || width <= 0.0
        || height <= 0.0
        || scale <= 0.0
    {
        return;
    }
    let state = workspace.borrow();
    let Some(chart) = state.charts.get(&id) else {
        return;
    };
    let size = (
        (width * scale).round().clamp(1.0, 4096.0) as u32,
        (height * scale).round().clamp(2.0, 4096.0) as u32,
    );
    if chart.borrow().screen_size() == size {
        return;
    }
    chart.borrow_mut().resize(size.0, size.1);
    state.refresh_frames(ui, &drawings.borrow(), Some(id));
}

#[cfg(not(target_family = "wasm"))]
fn connect_popout_zoom(
    window: &crate::CompactChartWindow,
    ui: &AppWindow,
    workspace: &Rc<RefCell<ChartWorkspace>>,
    drawings: &Rc<RefCell<DrawingStore>>,
    id: u32,
) {
    let weak = ui.as_weak();
    let workspace = workspace.clone();
    let drawings = drawings.clone();
    window.on_chart_zoomed(move |delta, fraction| {
        if let Some(ui) = weak.upgrade() {
            zoom_popout(
                &ui,
                &workspace,
                &drawings,
                id,
                delta.into(),
                fraction.into(),
            );
        }
    });
}

fn change_price_style(
    ui: &AppWindow,
    workspace: &Rc<RefCell<ChartWorkspace>>,
    drawings: &Rc<RefCell<DrawingStore>>,
    id: u32,
    color: &str,
    pattern: &str,
) {
    let (Some(color), Some(pattern)) = (
        PriceColor::from_name(color),
        PricePattern::from_name(pattern),
    ) else {
        ui.set_interaction_status("Invalid price line style.".into());
        return;
    };
    let state = workspace.borrow();
    let Some(chart) = state.charts.get(&id) else {
        ui.set_interaction_status("Chart not found.".into());
        return;
    };
    let style = PriceStyle {
        color,
        pattern,
        ..chart.borrow().price_style()
    };
    chart.borrow_mut().set_price_style(style);
    state.refresh(ui, &drawings.borrow());
    ui.set_interaction_status("Price line style updated".into());
    state.save(ui);
}

fn change_price_option(
    ui: &AppWindow,
    workspace: &Rc<RefCell<ChartWorkspace>>,
    drawings: &Rc<RefCell<DrawingStore>>,
    id: u32,
    field: &str,
    value: &str,
) {
    let state = workspace.borrow();
    let Some(chart) = state.charts.get(&id) else {
        ui.set_interaction_status("Chart not found.".into());
        return;
    };
    let mut style = chart.borrow().price_style();
    let valid = match field {
        "visible" => value.parse::<bool>().map(|v| style.visible = v).is_ok(),
        "grid" => value
            .parse::<bool>()
            .map(|v| style.grid_visible = v)
            .is_ok(),
        "labels" => value
            .parse::<bool>()
            .map(|v| style.labels_visible = v)
            .is_ok(),
        "border" => PriceBorder::from_name(value)
            .map(|v| style.border = v)
            .is_some(),
        "thickness" => value
            .parse::<u8>()
            .ok()
            .filter(|v| (1..=3).contains(v))
            .map(|v| style.thickness = v)
            .is_some(),
        "glow" => value
            .parse::<u8>()
            .ok()
            .filter(|v| *v <= 2)
            .map(|v| style.glow_intensity = v)
            .is_some(),
        "shadow" => value
            .parse::<u8>()
            .ok()
            .filter(|v| *v <= 2)
            .map(|v| style.shadow_intensity = v)
            .is_some(),
        "glow_color" => PriceColor::from_name(value)
            .map(|v| style.glow_color = v)
            .is_some(),
        "shadow_color" => PriceColor::from_name(value)
            .map(|v| style.shadow_color = v)
            .is_some(),
        _ => false,
    };
    if !valid {
        ui.set_interaction_status("Invalid chart setting.".into());
        return;
    }
    chart.borrow_mut().set_price_style(style);
    state.refresh(ui, &drawings.borrow());
    ui.set_interaction_status("Chart settings updated".into());
    state.save(ui);
}

fn select_chart(ui: &AppWindow, state: &mut ChartWorkspace, id: u32, broker: &FixtureBroker) {
    if !state.workspace.select(id) {
        ui.set_interaction_status("Chart not found.".into());
        return;
    }
    let symbol = &state
        .workspace
        .charts()
        .iter()
        .find(|chart| chart.id == id)
        .expect("selected chart must exist")
        .symbol;
    match broker.quote(symbol) {
        Ok(quote) => {
            if ui.get_symbol().as_str() != symbol {
                ui.set_order_price(format!("{:.2}", quote.last).into());
            }
            ui.set_symbol(symbol.as_str().into());
            ui.set_bid_price(format!("{:.2}", quote.bid).into());
            ui.set_ask_price(format!("{:.2}", quote.ask).into());
        }
        Err(error) => ui.set_interaction_status(error.to_string().into()),
    }
}

fn submit_chart_symbol(
    ui: &AppWindow,
    state: &mut ChartWorkspace,
    broker: &FixtureBroker,
    drawings: &DrawingStore,
    pending_anchor: &Cell<Option<Anchor>>,
    id: u32,
    input: &str,
) -> slint::SharedString {
    let Some(entry) = state.workspace.charts().iter().find(|chart| chart.id == id) else {
        ui.set_interaction_status("Chart not found.".into());
        return input.into();
    };
    let previous = entry.symbol.clone();
    let symbol = normalize_symbol(input);
    if symbol.is_empty() {
        ui.set_interaction_status("Enter a symbol.".into());
        return previous.into();
    }
    if let Err(error) = broker.quote(&symbol) {
        ui.set_interaction_status(error.to_string().into());
        return previous.into();
    }
    pending_anchor.set(None);
    state.set_symbol(id, &symbol);
    select_chart(ui, state, id, broker);
    state.refresh(ui, drawings);
    ui.set_interaction_status("Static sample bars".into());
    state.save(ui);
    symbol.into()
}

fn add_drawing(
    store: &mut DrawingStore,
    next_id: &Cell<u64>,
    symbol: &str,
    kind: DrawingKind,
    start: Anchor,
    end: Option<Anchor>,
) -> Result<(), crate::drawings::DrawingError> {
    let id = next_id.get();
    next_id.set(id + 1);
    store.add(Drawing {
        id: format!("drawing-{id}"),
        symbol: normalize_symbol(symbol),
        kind,
        start,
        end,
        style: DrawingStyle::default(),
        visible: true,
        locked: false,
    })
}

fn apply_drawing_tool(
    chart: &Chart,
    store: &mut DrawingStore,
    next_id: &Cell<u64>,
    symbol: &str,
    tool: &str,
    pending: &mut Option<Anchor>,
    x: f64,
    y: f64,
) -> Result<String, String> {
    let anchor = chart
        .screen_to_anchor(x, y)
        .ok_or("Chart is not ready for drawing.")?;
    let kind = match tool {
        "Horizontal line" => DrawingKind::HorizontalLine,
        "Vertical line" => DrawingKind::VerticalLine,
        "Crosshair" => DrawingKind::Crosshair,
        "Trend line" => DrawingKind::TrendLine,
        "Ray" => DrawingKind::Ray,
        "Measure" => DrawingKind::Measure,
        "Eraser" => {
            let closest = store
                .for_symbol(symbol)
                .iter()
                .filter(|drawing| !drawing.locked && drawing.visible)
                .filter_map(|drawing| {
                    drawing_distance(chart, drawing, (x, y))
                        .map(|distance| (drawing.id.clone(), distance))
                })
                .filter(|(_, distance)| *distance <= 8.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id);
            return Ok(
                if closest.and_then(|id| store.remove(symbol, &id)).is_some() {
                    "Drawing erased".into()
                } else {
                    "No unlocked drawing nearby".into()
                },
            );
        }
        _ => return Err("This tool is not available yet.".into()),
    };
    let two_points = matches!(tool, "Trend line" | "Ray" | "Measure");
    let (start, end) = if two_points {
        match pending.take() {
            Some(first) => (first, Some(anchor)),
            None => {
                *pending = Some(anchor);
                return Ok("Choose the second anchor".into());
            }
        }
    } else {
        (anchor, None)
    };
    add_drawing(store, next_id, symbol, kind, start, end).map_err(|e| e.to_string())?;
    Ok(format!("{tool} placed"))
}

fn popout_action(
    ui: &AppWindow,
    workspace: &Rc<RefCell<ChartWorkspace>>,
    drawings: &Rc<RefCell<DrawingStore>>,
    broker: &FixtureBroker,
    id: u32,
    action: &str,
    value: &str,
) -> String {
    let mut state = workspace.borrow_mut();
    if !state.detached.contains_key(&id) {
        return "Chart not found.".into();
    }
    match action {
        "symbol" => {
            let symbol = normalize_symbol(value);
            if symbol.is_empty() {
                return "Enter a symbol.".into();
            }
            if let Err(error) = broker.quote(&symbol) {
                return error.to_string();
            }
            state.set_symbol(id, &symbol);
            state.popout_interactions.get_mut(&id).unwrap().anchor = None;
            let active = state.workspace.active();
            select_chart(ui, &mut state, active, broker);
            state.save(ui);
        }
        "link" => {
            state.toggle_link(id);
            state.popout_interactions.get_mut(&id).unwrap().anchor = None;
            let active = state.workspace.active();
            select_chart(ui, &mut state, active, broker);
            state.save(ui);
        }
        "tool" => {
            if ![
                "Cursor",
                "Trend line",
                "Horizontal line",
                "Vertical line",
                "Measure",
                "Crosshair",
                "Ray",
                "Eraser",
            ]
            .contains(&value)
            {
                return "Invalid chart tool.".into();
            }
            let interaction = state.popout_interactions.get_mut(&id).unwrap();
            interaction.tool = value.to_owned();
            interaction.anchor = None;
        }
        _ => return "Invalid chart action.".into(),
    }
    state.refresh(ui, &drawings.borrow());
    String::new()
}

fn popout_pointer(
    ui: &AppWindow,
    workspace: &Rc<RefCell<ChartWorkspace>>,
    drawings: &Rc<RefCell<DrawingStore>>,
    next_id: &Cell<u64>,
    id: u32,
    x: f64,
    y: f64,
    pressed: bool,
) -> String {
    if !x.is_finite() || !y.is_finite() {
        return "Invalid chart pointer.".into();
    }
    let mut state = workspace.borrow_mut();
    let Some(entry) = state.detached.get(&id) else {
        return "Chart not found.".into();
    };
    let symbol = entry.symbol.clone();
    let chart = state.charts[&id].clone();
    let mut chart = chart.borrow_mut();
    let (width, height) = chart.screen_size();
    let x = x.clamp(0.0, 1.0) * f64::from(width);
    let y = y.clamp(0.0, 1.0) * f64::from(height);
    let interaction = state.popout_interactions.get_mut(&id).unwrap();
    if !pressed {
        interaction.pressed = false;
        return String::new();
    }
    let is_new = !std::mem::replace(&mut interaction.pressed, true);
    if interaction.tool == "Cursor" {
        if !is_new {
            chart.pan_px(x - interaction.last_x);
        }
        interaction.last_x = x;
        drop(chart);
        state.refresh_frames(ui, &drawings.borrow(), Some(id));
        return String::new();
    }
    if !is_new {
        return String::new();
    }
    let result = apply_drawing_tool(
        &chart,
        &mut drawings.borrow_mut(),
        next_id,
        &symbol,
        &interaction.tool,
        &mut interaction.anchor,
        x,
        y,
    );
    drop(chart);
    state.refresh(ui, &drawings.borrow());
    result.unwrap_or_else(|e| e)
}

fn point_segment_distance(point: (f64, f64), start: (f64, f64), end: (f64, f64)) -> f64 {
    let delta = (end.0 - start.0, end.1 - start.1);
    let length_squared = delta.0 * delta.0 + delta.1 * delta.1;
    if length_squared <= f64::EPSILON {
        return (point.0 - start.0).hypot(point.1 - start.1);
    }
    let projection = (((point.0 - start.0) * delta.0 + (point.1 - start.1) * delta.1)
        / length_squared)
        .clamp(0.0, 1.0);
    (point.0 - (start.0 + projection * delta.0)).hypot(point.1 - (start.1 + projection * delta.1))
}

fn drawing_distance(chart: &Chart, drawing: &Drawing, point: (f64, f64)) -> Option<f64> {
    let start = chart.anchor_to_screen(drawing.start)?;
    let end = chart.anchor_to_screen(drawing.end.unwrap_or(drawing.start))?;
    let (width, height) = chart.screen_size();
    let right = width.saturating_sub(1) as f64;
    let bottom = height.saturating_sub(1) as f64;
    let distance = |a, b| point_segment_distance(point, a, b);
    Some(match drawing.kind {
        DrawingKind::HorizontalLine => distance((0.0, start.1), (right, start.1)),
        DrawingKind::HorizontalRay => distance(start, (right, start.1)),
        DrawingKind::VerticalLine => distance((start.0, 0.0), (start.0, bottom)),
        DrawingKind::Crosshair => distance((0.0, start.1), (right, start.1))
            .min(distance((start.0, 0.0), (start.0, bottom))),
        DrawingKind::Measure => distance(start, (end.0, start.1))
            .min(distance((end.0, start.1), end))
            .min(distance(end, (start.0, end.1)))
            .min(distance((start.0, end.1), start)),
        DrawingKind::Ray => {
            let ray_end = if (end.0 - start.0).abs() <= f64::EPSILON {
                (start.0, if end.1 >= start.1 { bottom } else { 0.0 })
            } else if end.0 > start.0 {
                (
                    right,
                    start.1 + (end.1 - start.1) * (right - start.0) / (end.0 - start.0),
                )
            } else {
                (
                    0.0,
                    start.1 + (end.1 - start.1) * (0.0 - start.0) / (end.0 - start.0),
                )
            };
            distance(start, ray_end)
        }
        DrawingKind::TrendLine | DrawingKind::ExtendedLine => distance(start, end),
    })
}

pub fn create_app() -> AppWindow {
    create_app_with_workspace(ChartWorkspace::new())
}

fn create_app_with_workspace(initial_workspace: ChartWorkspace) -> AppWindow {
    let ui = AppWindow::new().expect("failed to create DigiTrader window");
    ui.set_desktop_controls(!cfg!(target_family = "wasm"));
    let broker = Rc::new(FixtureBroker::bundled());
    let account = broker
        .account_summary()
        .expect("embedded account fixture must be valid");
    ui.set_account_equity(format!("${:.2}", account.equity).into());
    ui.set_buying_power(format!("${:.2}", account.buying_power).into());
    let initial_quote = broker
        .quote(&ui.get_symbol())
        .expect("embedded quote fixture must include the initial symbol");
    ui.set_order_price(format!("{:.2}", initial_quote.last).into());
    ui.set_bid_price(format!("{:.2}", initial_quote.bid).into());
    ui.set_ask_price(format!("{:.2}", initial_quote.ask).into());
    let workspace = Rc::new(RefCell::new(initial_workspace));
    ui.set_charts(workspace.borrow().rows.clone().into());
    let drawings = Rc::new(RefCell::new(DrawingStore::new("local-viewer")));
    let next_drawing_id = Rc::new(Cell::new(1));
    let pending_anchor = Rc::new(Cell::<Option<Anchor>>::new(None));
    {
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        ui.on_price_option_changed(move |id, field, value| {
            let Some(ui) = weak.upgrade() else { return };
            if let Ok(id) = u32::try_from(id) {
                change_price_option(&ui, &workspace, &drawings, id, &field, &value);
            } else {
                ui.set_interaction_status("Chart not found.".into());
            }
        });
    }
    {
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        ui.on_price_style_changed(move |id, color, pattern| {
            let Some(ui) = weak.upgrade() else { return };
            let Ok(id) = u32::try_from(id) else {
                ui.set_interaction_status("Chart not found.".into());
                return;
            };
            change_price_style(&ui, &workspace, &drawings, id, &color, &pattern);
        });
    }
    #[cfg(target_family = "wasm")]
    {
        use wasm_bindgen::closure::Closure;
        {
            let weak = ui.as_weak();
            let workspace = workspace.clone();
            let drawings = drawings.clone();
            let callback = Closure::<dyn FnMut(u32, String, String)>::new(
                move |id: u32, color: String, pattern: String| {
                    if let Some(ui) = weak.upgrade() {
                        change_price_style(&ui, &workspace, &drawings, id, &color, &pattern);
                    }
                },
            );
            crate::web_popout::chart_popup_set_style_handler(callback.as_ref());
            callback.forget();
        }
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        let callback = Closure::<dyn FnMut(u32, String, String)>::new(
            move |id: u32, field: String, value: String| {
                if let Some(ui) = weak.upgrade() {
                    change_price_option(&ui, &workspace, &drawings, id, &field, &value);
                }
            },
        );
        crate::web_popout::chart_popup_set_option_handler(callback.as_ref());
        callback.forget();
    }
    #[cfg(target_family = "wasm")]
    {
        use wasm_bindgen::closure::Closure;
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        let callback =
            Closure::<dyn FnMut(u32, f64, f64)>::new(move |id: u32, delta: f64, fraction: f64| {
                if let Some(ui) = weak.upgrade() {
                    zoom_popout(&ui, &workspace, &drawings, id, delta, fraction);
                }
            });
        crate::web_popout::chart_popup_set_zoom_handler(callback.as_ref());
        callback.forget();
    }
    #[cfg(target_family = "wasm")]
    {
        use wasm_bindgen::closure::Closure;
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        let broker = broker.clone();
        let pending_anchor = pending_anchor.clone();
        let callback = Closure::<dyn FnMut(u32, String, String) -> String>::new(
            move |id: u32, action: String, value: String| match weak.upgrade() {
                Some(ui) => {
                    if action == "symbol" || action == "link" {
                        pending_anchor.set(None);
                    }
                    popout_action(&ui, &workspace, &drawings, &broker, id, &action, &value)
                }
                None => "Workspace closed.".into(),
            },
        );
        crate::web_popout::chart_popup_set_action_handler(callback.as_ref());
        callback.forget();
    }
    #[cfg(target_family = "wasm")]
    {
        use wasm_bindgen::closure::Closure;
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        let next_id = next_drawing_id.clone();
        let callback =
            Closure::<dyn FnMut(u32, f64, f64, bool) -> String>::new(move |id, x, y, pressed| {
                match weak.upgrade() {
                    Some(ui) => {
                        popout_pointer(&ui, &workspace, &drawings, &next_id, id, x, y, pressed)
                    }
                    None => "Workspace closed.".into(),
                }
            });
        crate::web_popout::chart_popup_set_pointer_handler(callback.as_ref());
        callback.forget();
    }
    #[cfg(target_family = "wasm")]
    {
        use wasm_bindgen::closure::Closure;
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        let callback =
            Closure::<dyn FnMut(u32, f64, f64, f64)>::new(move |id, width, height, scale| {
                if let Some(ui) = weak.upgrade() {
                    resize_popout(&ui, &workspace, &drawings, id, width, height, scale);
                }
            });
        crate::web_popout::chart_popup_set_resize_handler(callback.as_ref());
        callback.forget();
    }
    let pointer_down = Rc::new(Cell::new(false));
    let last_pointer_x = Rc::new(Cell::new(0.0f64));
    let popout_cleanup = Rc::new(slint::Timer::default());
    {
        let workspace = Rc::downgrade(&workspace);
        popout_cleanup.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_secs(1),
            move || {
                if let Some(workspace) = workspace.upgrade() {
                    workspace.borrow_mut().prune_closed_popouts();
                }
            },
        );
    }

    {
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        let broker = broker.clone();
        let pending_anchor = pending_anchor.clone();
        ui.on_chart_add(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut state = workspace.borrow_mut();
            let Some(id) = state.workspace.add() else {
                ui.set_interaction_status("Maximum of 8 charts reached.".into());
                return;
            };
            state.charts.insert(
                id,
                Rc::new(RefCell::new(Chart::new(datagen::generate(0, 1)))),
            );
            pending_anchor.set(None);
            select_chart(&ui, &mut state, id, &broker);
            state.refresh(&ui, &drawings.borrow());
            ui.set_interaction_status("Chart added".into());
            state.save(&ui);
        });
    }

    {
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        let broker = broker.clone();
        let pending_anchor = pending_anchor.clone();
        ui.on_chart_select(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let Ok(id) = u32::try_from(id) else { return };
            let mut state = workspace.borrow_mut();
            pending_anchor.set(None);
            select_chart(&ui, &mut state, id, &broker);
            state.refresh(&ui, &drawings.borrow());
        });
    }

    {
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        let broker = broker.clone();
        let pending_anchor = pending_anchor.clone();
        let popout_cleanup = popout_cleanup.clone();
        ui.on_chart_remove(move |id| {
            let _keep_timer_alive = &popout_cleanup;
            let Some(ui) = weak.upgrade() else { return };
            let Ok(id) = u32::try_from(id) else { return };
            let mut state = workspace.borrow_mut();
            if !state.remove_grid_chart(id) {
                ui.set_interaction_status("Keep at least one chart open.".into());
                return;
            }
            let active = state.workspace.active();
            pending_anchor.set(None);
            select_chart(&ui, &mut state, active, &broker);
            state.refresh(&ui, &drawings.borrow());
            ui.set_interaction_status("Chart closed".into());
            state.save(&ui);
        });
    }

    {
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        #[cfg(not(target_family = "wasm"))]
        let broker = broker.clone();
        #[cfg(not(target_family = "wasm"))]
        let next_drawing_id = next_drawing_id.clone();
        #[cfg(not(target_family = "wasm"))]
        let pending_anchor = pending_anchor.clone();
        ui.on_chart_popout(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let Ok(id) = u32::try_from(id) else { return };
            let mut state = workspace.borrow_mut();
            match state.open_popout(id) {
                Ok(id) => {
                    #[cfg(not(target_family = "wasm"))]
                    if let Some(window) = state.popouts.get(&id) {
                        let weak = ui.as_weak();
                        let workspace = workspace.clone();
                        let drawings = drawings.clone();
                        window.on_price_style_changed(move |color, pattern| {
                            if let Some(ui) = weak.upgrade() {
                                change_price_style(
                                    &ui, &workspace, &drawings, id, &color, &pattern,
                                );
                            }
                        });
                    }
                    #[cfg(not(target_family = "wasm"))]
                    if let Some(window) = state.popouts.get(&id) {
                        let weak = ui.as_weak();
                        let workspace = workspace.clone();
                        let drawings = drawings.clone();
                        window.on_price_option_changed(move |field, value| {
                            if let Some(ui) = weak.upgrade() {
                                change_price_option(&ui, &workspace, &drawings, id, &field, &value);
                            }
                        });
                    }
                    #[cfg(not(target_family = "wasm"))]
                    if let Some(window) = state.popouts.get(&id) {
                        connect_popout_zoom(window, &ui, &workspace, &drawings, id);
                    }
                    #[cfg(not(target_family = "wasm"))]
                    if let Some(window) = state.popouts.get(&id) {
                        let weak = ui.as_weak();
                        let ws = workspace.clone();
                        let ds = drawings.clone();
                        let broker = broker.clone();
                        let pending_anchor = pending_anchor.clone();
                        window.on_chart_action(move |action, value| {
                            if action == "symbol" || action == "link" {
                                pending_anchor.set(None);
                            }
                            weak.upgrade()
                                .map(|ui| {
                                    popout_action(&ui, &ws, &ds, &broker, id, &action, &value)
                                        .into()
                                })
                                .unwrap_or_else(|| "Workspace closed.".into())
                        });
                        let weak = ui.as_weak();
                        let ws = workspace.clone();
                        let ds = drawings.clone();
                        let next_id = next_drawing_id.clone();
                        window.on_chart_pointer(move |x, y, pressed| {
                            weak.upgrade()
                                .map(|ui| {
                                    popout_pointer(
                                        &ui,
                                        &ws,
                                        &ds,
                                        &next_id,
                                        id,
                                        x.into(),
                                        y.into(),
                                        pressed,
                                    )
                                    .into()
                                })
                                .unwrap_or_else(|| "Workspace closed.".into())
                        });
                        let weak = ui.as_weak();
                        let weak_window = window.as_weak();
                        let ws = workspace.clone();
                        let ds = drawings.clone();
                        window.on_chart_resized(move |width, height| {
                            if let (Some(ui), Some(window)) =
                                (weak.upgrade(), weak_window.upgrade())
                            {
                                resize_popout(
                                    &ui,
                                    &ws,
                                    &ds,
                                    id,
                                    width.into(),
                                    height.into(),
                                    window.window().scale_factor().into(),
                                );
                            }
                        });
                    }
                    state.refresh(&ui, &drawings.borrow());
                    #[cfg(not(target_family = "wasm"))]
                    let popout = state.popouts.get(&id).map(|w| w.clone_strong());
                    drop(state);
                    #[cfg(not(target_family = "wasm"))]
                    if let Some(window) = popout {
                        let scale = window.window().scale_factor();
                        let size = window.window().size().to_logical(scale);
                        window.invoke_chart_resized(
                            (size.width - 56.0).max(1.0),
                            (size.height - 91.0).max(2.0),
                        );
                    }
                    #[cfg(target_family = "wasm")]
                    crate::web_popout::chart_popup_request_resize(id);
                    ui.set_interaction_status("Compact chart opened".into());
                }
                Err(error) => ui.set_interaction_status(error.into()),
            }
        });
    }

    {
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        let broker = broker.clone();
        let pending_anchor = pending_anchor.clone();
        ui.on_chart_link(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let Ok(id) = u32::try_from(id) else { return };
            let mut state = workspace.borrow_mut();
            if state.toggle_link(id) {
                let active = state.workspace.active();
                select_chart(&ui, &mut state, active, &broker);
                pending_anchor.set(None);
                state.refresh(&ui, &drawings.borrow());
                ui.set_interaction_status("Chart link updated".into());
                state.save(&ui);
            }
        });
    }

    {
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        ui.on_grid_resize(move |id, column, delta, minimum| {
            let Some(ui) = weak.upgrade() else { return };
            let Ok(id) = u32::try_from(id) else { return };
            let mut state = workspace.borrow_mut();
            let Some(index) = state
                .workspace
                .charts()
                .iter()
                .position(|chart| chart.id == id)
            else {
                return;
            };
            if column {
                state.workspace.resize_column(index / 2, delta, minimum);
            } else {
                state.workspace.resize_row(index / 2, delta, minimum);
            }
            state.refresh_frames(&ui, &drawings.borrow(), Some(0));
        });
    }

    {
        let weak = ui.as_weak();
        let pending_anchor = pending_anchor.clone();
        ui.on_tool_selected(move |tool| {
            pending_anchor.set(None);
            if let Some(ui) = weak.upgrade() {
                ui.set_active_tool(tool.clone());
                ui.set_interaction_status(format!("{} selected", tool).into());
            }
        });
    }

    {
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        let broker = broker.clone();
        let pending_anchor = pending_anchor.clone();
        ui.on_chart_symbol_submitted(move |id, symbol| {
            let Some(ui) = weak.upgrade() else {
                return symbol;
            };
            let Ok(id) = u32::try_from(id) else {
                ui.set_interaction_status("Chart not found.".into());
                return symbol;
            };
            submit_chart_symbol(
                &ui,
                &mut workspace.borrow_mut(),
                &broker,
                &drawings.borrow(),
                &pending_anchor,
                id,
                &symbol,
            )
        });
    }

    {
        let weak = ui.as_weak();
        let broker = broker.clone();
        ui.on_order_submitted(move |side, kind, quantity, price| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let quantity = match quantity.trim().parse::<f64>() {
                Ok(quantity) => quantity,
                Err(_) => {
                    ui.set_order_status("Quantity must be a valid number.".into());
                    return;
                }
            };
            let side = match side.as_str() {
                "buy" => Side::Buy,
                "sell" => Side::Sell,
                _ => {
                    ui.set_order_status("Choose Buy or Sell.".into());
                    return;
                }
            };
            let (kind, limit_price) = match kind.as_str() {
                "market" => (OrderKind::Market, None),
                "limit" => match price.trim().parse::<f64>() {
                    Ok(price) => (OrderKind::Limit, Some(price)),
                    Err(_) => {
                        ui.set_order_status("Limit price must be a valid number.".into());
                        return;
                    }
                },
                _ => {
                    ui.set_order_status("Choose a supported order type.".into());
                    return;
                }
            };
            let request = OrderRequest {
                symbol: ui.get_symbol().to_string(),
                side,
                kind,
                quantity,
                limit_price,
            };
            let preview = match broker.preview_order(&request) {
                Ok(preview) => preview,
                Err(error) => {
                    ui.set_order_status(error.to_string().into());
                    return;
                }
            };
            let side = match preview.request.side {
                Side::Buy => "BUY",
                Side::Sell => "SELL",
            };
            let order_kind = match preview.request.kind {
                OrderKind::Market => "MARKET",
                OrderKind::Limit => "LIMIT",
            };
            ui.set_order_status(
                format!(
                    "PREVIEW  ·  {side} {} {} @ {order_kind} ${:.2}  ·  ${:.2}",
                    preview.request.quantity,
                    preview.request.symbol,
                    preview.reference_price,
                    preview.estimated_notional
                )
                .into(),
            );
        });
    }

    {
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        ui.on_chart_resized(move |id, width, height| {
            if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
                return;
            }
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let scale = ui.window().scale_factor() as f64;
            let Ok(id) = u32::try_from(id) else { return };
            let Some(chart) = workspace.borrow().charts.get(&id).cloned() else {
                return;
            };
            let new_size = (
                (width as f64 * scale).round().max(1.0) as u32,
                (height as f64 * scale).round().max(2.0) as u32,
            );
            if chart.borrow().screen_size() == new_size {
                return;
            }
            chart.borrow_mut().resize(new_size.0, new_size.1);
            workspace
                .borrow()
                .refresh_frames(&ui, &drawings.borrow(), Some(id));
        });
    }

    {
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        let next_drawing_id = next_drawing_id.clone();
        let pending_anchor = pending_anchor.clone();
        let pointer_down = pointer_down.clone();
        let last_pointer_x = last_pointer_x.clone();
        ui.on_chart_pointer(move |x, y, pressed| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let chart = workspace.borrow().active_chart();
            if !pressed {
                pointer_down.set(false);
                return;
            }
            let scale = ui.window().scale_factor() as f64;
            let (x, y) = (x as f64 * scale, y as f64 * scale);
            let is_new_press = !pointer_down.replace(true);
            let tool = ui.get_active_tool().to_string();

            if tool == "Cursor" {
                if is_new_press {
                    last_pointer_x.set(x);
                } else {
                    let delta = x - last_pointer_x.replace(x);
                    chart.borrow_mut().pan_px(delta);
                    workspace.borrow().refresh_active(&ui, &drawings.borrow());
                }
                return;
            }
            if !is_new_press {
                return;
            }
            let symbol = ui.get_symbol().to_string();
            let mut pending = pending_anchor.get();
            let result = apply_drawing_tool(
                &chart.borrow(),
                &mut drawings.borrow_mut(),
                &next_drawing_id,
                &symbol,
                &tool,
                &mut pending,
                x,
                y,
            );
            pending_anchor.set(pending);
            match result {
                Ok(message) => {
                    ui.set_interaction_status(message.into());
                    render_chart(&ui, &workspace.borrow(), &drawings.borrow());
                }
                Err(error) => ui.set_interaction_status(error.to_string().into()),
            }
        });
    }

    {
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        ui.on_chart_zoomed(move |delta, x| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let scale = ui.window().scale_factor() as f64;
            let chart = workspace.borrow().active_chart();
            chart.borrow_mut().zoom_at(x as f64 * scale, delta as f64);
            workspace.borrow().refresh_active(&ui, &drawings.borrow());
        });
    }

    render_chart(&ui, &workspace.borrow(), &drawings.borrow());
    ui
}

pub fn run() {
    let ui = create_app_with_workspace(ChartWorkspace::persistent());
    #[cfg(target_family = "wasm")]
    configure_web_resize(&ui);
    ui.run().expect("DigiTrader event loop failed");
}

#[cfg(target_family = "wasm")]
fn configure_web_resize(ui: &AppWindow) {
    use wasm_bindgen::{closure::Closure, JsCast};

    fn resize(ui: &AppWindow) {
        let window = web_sys::window().expect("browser window is unavailable");
        let width = window
            .inner_width()
            .expect("cannot read browser width")
            .as_f64()
            .expect("browser width is not numeric");
        let height = window
            .inner_height()
            .expect("cannot read browser height")
            .as_f64()
            .expect("browser height is not numeric");
        ui.window().set_size(slint::LogicalSize::new(
            width.max(800.0) as f32,
            height.max(520.0) as f32,
        ));
    }

    let window = web_sys::window().expect("browser window is unavailable");
    let weak = ui.as_weak();
    resize(ui);
    let callback = Closure::<dyn FnMut()>::new(move || {
        let Some(ui) = weak.upgrade() else { return };
        resize(&ui);
    });
    window
        .add_event_listener_with_callback("resize", callback.as_ref().unchecked_ref())
        .expect("cannot listen for browser resize");
    callback.forget();
}

#[cfg(test)]
mod tests {
    use super::*;
    use datagen::Bar;

    #[test]
    fn closing_grid_chart_preserves_open_popout_until_it_is_closed() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = create_app();
        ui.window().set_size(slint::LogicalSize::new(1200.0, 800.0));
        ui.show().unwrap();
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(1));
        ui.invoke_chart_add();
        ui.invoke_chart_popout(2);
        ui.invoke_chart_remove(2);
        assert_eq!(ui.get_charts().row_count(), 1);
        assert_eq!(ui.get_interaction_status(), "Chart closed");
    }

    #[test]
    fn detached_native_window_stays_visible_and_synced_and_is_cleaned_up() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = AppWindow::new().unwrap();
        let mut state = ChartWorkspace::new();
        let id = state.workspace.add().unwrap();
        let mut chart = Chart::new(datagen::generate(0, 1));
        chart.resize(240, 160);
        state.charts.insert(id, Rc::new(RefCell::new(chart)));
        let popup_id = state.open_popout(id).unwrap();
        let popout = state.popouts[&popup_id].clone_strong();
        assert!(popout.window().is_visible());
        assert!(state.remove_grid_chart(id));
        assert!(popout.window().is_visible());
        assert_eq!(state.workspace.charts().len(), 1);
        assert!(!state.remove_grid_chart(1));

        let mut drawings = DrawingStore::new("local-viewer");
        state.set_symbol(1, "AAPL");
        state.refresh(&ui, &drawings);
        assert_eq!(popout.get_symbol(), "SPY");
        assert!(!popout.get_linked());
        assert!(state.toggle_link(popup_id));
        state.refresh(&ui, &drawings);
        assert_eq!(popout.get_symbol(), "AAPL");
        assert!(popout.get_time_ticks().row_count() > 0);
        assert!(popout.get_price_ticks().row_count() > 0);
        assert_eq!(
            popout.get_current_price().label,
            state.charts[&popup_id]
                .borrow()
                .current_price_marker()
                .unwrap()
                .label
        );
        state.charts[&popup_id]
            .borrow_mut()
            .set_price_style(PriceStyle {
                color: PriceColor::Teal,
                pattern: PricePattern::Dotted,
                ..PriceStyle::default()
            });
        state.refresh(&ui, &drawings);
        assert_eq!(popout.get_price_color(), "teal");
        assert_eq!(popout.get_price_pattern(), "dotted");
        assert!(popout.get_price_options().visible);
        assert_eq!(popout.get_price_options().shadow_color, "black");
        let before = popout.get_chart_image().to_rgba8().unwrap();
        let anchor = state.charts[&popup_id]
            .borrow()
            .screen_to_anchor(100.0, 80.0)
            .unwrap();
        add_drawing(
            &mut drawings,
            &Cell::new(1),
            "AAPL",
            DrawingKind::HorizontalLine,
            anchor,
            None,
        )
        .unwrap();
        state.refresh(&ui, &drawings);
        assert!(before.as_slice() != popout.get_chart_image().to_rgba8().unwrap().as_slice());

        assert!(state.toggle_link(1));
        state.set_symbol(1, "SPY");
        state.refresh(&ui, &drawings);
        assert_eq!(popout.get_symbol(), "AAPL");
        assert!(state.toggle_link(1));
        assert_eq!(state.workspace.charts()[0].symbol, "AAPL");
        popout.hide().unwrap();
        state.prune_closed_popouts();
        assert!(!state.detached.contains_key(&popup_id));
        assert!(!state.charts.contains_key(&popup_id));
        assert!(!state.popouts.contains_key(&popup_id));
    }

    #[test]
    fn popout_wheel_zooms_its_chart_and_works_after_grid_removal() {
        use i_slint_backend_testing::{mock_elapsed_time, ElementHandle};
        use slint::platform::WindowEvent;
        use std::time::Duration;

        i_slint_backend_testing::init_no_event_loop();
        let ui = create_app();
        ui.window().set_size(slint::LogicalSize::new(1200.0, 800.0));
        ui.show().unwrap();
        let workspace = Rc::new(RefCell::new(ChartWorkspace::new()));
        let drawings = Rc::new(RefCell::new(DrawingStore::new("local-viewer")));
        let popup_id = {
            let mut state = workspace.borrow_mut();
            let id = state.workspace.add().unwrap();
            let mut chart = Chart::new(datagen::generate(0, 1));
            chart.resize(600, 300);
            state.charts.insert(id, Rc::new(RefCell::new(chart)));
            let popup_id = state.open_popout(id).unwrap();
            connect_popout_zoom(
                &state.popouts[&popup_id],
                &ui,
                &workspace,
                &drawings,
                popup_id,
            );
            state.refresh(&ui, &drawings.borrow());
            popup_id
        };
        let popout = workspace.borrow().popouts[&popup_id].clone_strong();
        popout
            .window()
            .set_size(slint::LogicalSize::new(420.0, 270.0));
        mock_elapsed_time(Duration::from_millis(1));
        let touch =
            ElementHandle::find_by_element_id(&popout, "CompactChartWindow::compact_plot_touch")
                .next()
                .unwrap();
        let point = slint::LogicalPosition::new(
            touch.absolute_position().x + touch.size().width * 0.25,
            touch.absolute_position().y + touch.size().height * 0.5,
        );
        let other_chart = workspace
            .borrow()
            .rows
            .row_data(0)
            .unwrap()
            .image
            .to_rgba8()
            .unwrap();
        let grid_before = workspace
            .borrow()
            .rows
            .row_data(1)
            .unwrap()
            .image
            .to_rgba8()
            .unwrap();
        let anchor = workspace.borrow().charts[&popup_id]
            .borrow()
            .screen_to_anchor(150.0, 100.0)
            .unwrap();
        let before = popout.get_chart_image().to_rgba8().unwrap();
        popout
            .window()
            .dispatch_event(WindowEvent::PointerMoved { position: point });
        popout
            .window()
            .dispatch_event(WindowEvent::PointerScrolled {
                position: point,
                delta_x: 0.0,
                delta_y: 120.0,
            });
        mock_elapsed_time(Duration::from_millis(1));
        let zoomed = popout.get_chart_image().to_rgba8().unwrap();
        assert_ne!(before.as_slice(), zoomed.as_slice());
        assert_eq!(
            workspace
                .borrow()
                .rows
                .row_data(0)
                .unwrap()
                .image
                .to_rgba8()
                .unwrap()
                .as_slice(),
            other_chart.as_slice()
        );
        let anchored = workspace.borrow().charts[&popup_id]
            .borrow()
            .screen_to_anchor(150.0, 100.0)
            .unwrap();
        assert!((anchored.timestamp_ms - anchor.timestamp_ms).abs() <= 1);
        assert_eq!(
            workspace
                .borrow()
                .rows
                .row_data(1)
                .unwrap()
                .image
                .to_rgba8()
                .unwrap()
                .as_slice(),
            grid_before.as_slice()
        );
        assert!(workspace.borrow_mut().remove_grid_chart(2));
        popout
            .window()
            .dispatch_event(WindowEvent::PointerScrolled {
                position: point,
                delta_x: 0.0,
                delta_y: 120.0,
            });
        mock_elapsed_time(Duration::from_millis(1));
        assert_ne!(
            zoomed.as_slice(),
            popout.get_chart_image().to_rgba8().unwrap().as_slice()
        );
        popout.hide().unwrap();
    }

    #[test]
    fn independent_popout_symbols_linking_drawing_and_styles() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = create_app();
        let workspace = Rc::new(RefCell::new(ChartWorkspace::new()));
        workspace.borrow().charts[&1].borrow_mut().resize(300, 200);
        let id = workspace.borrow_mut().open_popout(1).unwrap();
        let other = workspace.borrow_mut().open_popout(1).unwrap();
        let drawings = Rc::new(RefCell::new(DrawingStore::new("local-viewer")));
        let broker = FixtureBroker::bundled();
        let next = Cell::new(1);
        assert_ne!(id, other);
        assert!(!workspace.borrow().detached[&id].linked);
        assert_eq!(
            popout_action(&ui, &workspace, &drawings, &broker, id, "symbol", "AAPL"),
            ""
        );
        assert_eq!(workspace.borrow().detached[&id].symbol, "AAPL");
        assert_eq!(workspace.borrow().workspace.charts()[0].symbol, "SPY");
        assert_eq!(workspace.borrow().detached[&other].symbol, "SPY");
        assert!(
            !popout_action(&ui, &workspace, &drawings, &broker, id, "symbol", "INVALID").is_empty()
        );
        assert_eq!(workspace.borrow().detached[&id].symbol, "AAPL");
        assert_eq!(
            popout_action(&ui, &workspace, &drawings, &broker, id, "link", ""),
            ""
        );
        assert_eq!(workspace.borrow().detached[&id].symbol, "SPY");
        assert!(workspace.borrow().detached[&id].linked);
        assert_eq!(
            popout_action(&ui, &workspace, &drawings, &broker, id, "symbol", "AAPL"),
            ""
        );
        assert_eq!(workspace.borrow().workspace.charts()[0].symbol, "AAPL");
        assert_eq!(workspace.borrow().detached[&other].symbol, "SPY");
        change_price_option(&ui, &workspace, &drawings, id, "visible", "false");
        assert!(
            !workspace.borrow().charts[&id]
                .borrow()
                .price_style()
                .visible
        );
        assert!(workspace.borrow().charts[&1].borrow().price_style().visible);
        assert_eq!(
            popout_action(
                &ui,
                &workspace,
                &drawings,
                &broker,
                id,
                "tool",
                "Trend line"
            ),
            ""
        );
        assert_eq!(
            popout_pointer(&ui, &workspace, &drawings, &next, id, 0.2, 0.3, true),
            "Choose the second anchor"
        );
        popout_pointer(&ui, &workspace, &drawings, &next, id, 0.2, 0.3, false);
        assert_eq!(
            popout_pointer(&ui, &workspace, &drawings, &next, id, 0.6, 0.5, true),
            "Trend line placed"
        );
        assert_eq!(drawings.borrow().for_symbol("AAPL").len(), 1);
        assert_eq!(drawings.borrow().for_symbol("SPY").len(), 0);
        popout_pointer(&ui, &workspace, &drawings, &next, id, 0.6, 0.5, false);
        assert_eq!(
            popout_action(&ui, &workspace, &drawings, &broker, id, "tool", "Eraser"),
            ""
        );
        assert_eq!(
            popout_pointer(&ui, &workspace, &drawings, &next, id, 0.2, 0.3, true),
            "Drawing erased"
        );
        assert!(drawings.borrow().for_symbol("AAPL").is_empty());
        workspace.borrow().popouts[&id].hide().unwrap();
        workspace.borrow().popouts[&other].hide().unwrap();
    }

    #[test]
    fn price_style_change_is_independent_and_rejects_invalid_values() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = create_app();
        ui.window().set_size(slint::LogicalSize::new(1200.0, 800.0));
        ui.show().unwrap();
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(1));
        ui.invoke_chart_add();
        ui.invoke_chart_popout(2);
        ui.invoke_price_style_changed(2, "teal".into(), "dotted".into());
        let first = ui.get_charts().row_data(0).unwrap();
        let second = ui.get_charts().row_data(1).unwrap();
        assert_eq!(
            (first.price_color.as_str(), first.price_pattern.as_str()),
            ("orange", "dashed")
        );
        assert_eq!(
            (second.price_color.as_str(), second.price_pattern.as_str()),
            ("teal", "dotted")
        );
        ui.invoke_price_style_changed(2, "invalid".into(), "solid".into());
        assert_eq!(ui.get_interaction_status(), "Invalid price line style.");
        assert_eq!(ui.get_charts().row_data(1).unwrap().price_pattern, "dotted");
        ui.invoke_price_option_changed(2, "visible".into(), "false".into());
        ui.invoke_price_option_changed(2, "border".into(), "none".into());
        ui.invoke_price_option_changed(2, "grid".into(), "false".into());
        ui.invoke_price_option_changed(2, "thickness".into(), "3".into());
        ui.invoke_price_option_changed(2, "glow".into(), "2".into());
        ui.invoke_price_option_changed(2, "shadow_color".into(), "red".into());
        let first = ui.get_charts().row_data(0).unwrap().price_options;
        let second = ui.get_charts().row_data(1).unwrap().price_options;
        assert!(first.visible && first.grid);
        assert!(!second.visible && !second.grid);
        assert_eq!(second.border, "none");
        assert_eq!(second.thickness, 3);
        assert_eq!(second.glow, 2);
        assert_eq!(second.shadow_color, "red");
        ui.invoke_price_option_changed(2, "thickness".into(), "4".into());
        assert_eq!(ui.get_interaction_status(), "Invalid chart setting.");
        assert_eq!(
            ui.get_charts().row_data(1).unwrap().price_options.thickness,
            3
        );
    }

    #[test]
    fn restore_preserves_each_chart_identity_and_style() {
        let settings = crate::settings::Settings {
            version: 1,
            charts: vec![
                crate::settings::SavedChart {
                    symbol: "SPY".into(),
                    linked: true,
                    style: PriceStyle {
                        thickness: 3,
                        ..PriceStyle::default()
                    },
                },
                crate::settings::SavedChart {
                    symbol: "AAPL".into(),
                    linked: false,
                    style: PriceStyle {
                        visible: false,
                        glow_intensity: 2,
                        ..PriceStyle::default()
                    },
                },
            ],
        };
        let mut state = ChartWorkspace::new();
        state.restore(&settings);
        assert_eq!(state.workspace.charts()[0].symbol, "SPY");
        assert_eq!(state.workspace.charts()[1].symbol, "AAPL");
        assert!(!state.workspace.charts()[1].linked);
        assert_eq!(state.charts[&1].borrow().price_style().thickness, 3);
        assert!(!state.charts[&2].borrow().price_style().visible);
        assert_eq!(state.charts[&2].borrow().price_style().glow_intensity, 2);
        assert_eq!(state.workspace.add(), Some(3));
    }

    #[test]
    fn segment_distance_handles_projection_and_degenerate_lines() {
        assert_eq!(
            point_segment_distance((5.0, 3.0), (0.0, 0.0), (10.0, 0.0)),
            3.0
        );
        assert_eq!(
            point_segment_distance((12.0, 0.0), (0.0, 0.0), (10.0, 0.0)),
            2.0
        );
        assert_eq!(
            point_segment_distance((3.0, 4.0), (0.0, 0.0), (0.0, 0.0)),
            5.0
        );
    }

    #[test]
    fn horizontal_line_eraser_hit_distance_tracks_rendered_position() {
        let bars = (0..4)
            .map(|index| Bar {
                ts: 1_700_000_000 + index * 60,
                open: 10.0,
                high: 12.0,
                low: 8.0,
                close: 11.0,
                volume: 1.0,
            })
            .collect();
        let mut chart = Chart::new(bars);
        chart.resize(200, 100);
        let drawing = Drawing {
            id: "level".into(),
            symbol: "SPY".into(),
            kind: DrawingKind::HorizontalLine,
            start: Anchor {
                timestamp_ms: 1_700_000_060_000,
                price: 11.0,
            },
            end: None,
            style: DrawingStyle::default(),
            visible: true,
            locked: false,
        };
        let (_, y) = chart.anchor_to_screen(drawing.start).unwrap();
        assert!(drawing_distance(&chart, &drawing, (100.0, y)).unwrap() < 1e-9);
        assert!((drawing_distance(&chart, &drawing, (100.0, y + 9.0)).unwrap() - 9.0).abs() < 1e-9);
    }
}
