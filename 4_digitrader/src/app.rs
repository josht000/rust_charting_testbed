use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use slint::{ComponentHandle, Model, VecModel};

use crate::broker::{BrokerProvider, FixtureBroker};
use crate::chart::Chart;
use crate::drawings::{Anchor, Drawing, DrawingKind, DrawingStore, DrawingStyle};
use crate::model::{normalize_symbol, OrderKind, OrderRequest, Side};
use crate::workspace::{ChartEntry, Workspace};
use crate::{AppWindow, AxisTick, ChartItem};

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
    rows: Rc<VecModel<ChartItem>>,
    #[cfg(not(target_family = "wasm"))]
    popouts: BTreeMap<u32, crate::CompactChartWindow>,
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
            rows: Rc::new(VecModel::default()),
            #[cfg(not(target_family = "wasm"))]
            popouts: BTreeMap::new(),
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
                x,
                y,
                width,
                height,
                column_divider,
                row_divider,
            });
        }
        if target.is_none() {
            for entry in self.detached.values() {
                let chart = self.charts[&entry.id].borrow();
                let scale = ui.window().scale_factor();
                let frame =
                    chart.render_frame_with_drawings(drawings.for_symbol(&entry.symbol), scale);
                let (time_ticks, price_ticks) = chart.axis_ticks(scale);
                let current_price = chart.current_price_marker();
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
                    );
                }
                #[cfg(not(target_family = "wasm"))]
                if let Some(window) = self.popouts.get(&entry.id) {
                    window.set_symbol(entry.symbol.as_str().into());
                    window.set_chart_image(slint::Image::from_rgba8(frame));
                    window.set_time_ticks(slint_ticks(&time_ticks));
                    window.set_price_ticks(slint_ticks(&price_ticks));
                    window.set_current_price(slint_marker(current_price.as_ref()));
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

    fn open_popout(&mut self, id: u32) -> Result<(), String> {
        if !self.charts.contains_key(&id) {
            return Err("Chart not found.".into());
        }
        #[cfg(not(target_family = "wasm"))]
        {
            if let Some(window) = self.popouts.get(&id) {
                return window.show().map_err(|error| error.to_string());
            }
            let window = crate::CompactChartWindow::new().map_err(|error| error.to_string())?;
            window.show().map_err(|error| error.to_string())?;
            self.popouts.insert(id, window);
        }
        #[cfg(target_family = "wasm")]
        if !crate::web_popout::chart_popup_open(id) {
            return Err("Pop-up blocked. Allow pop-ups for this page and try again.".into());
        }
        Ok(())
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

    fn remove_grid_chart(&mut self, id: u32) -> Option<bool> {
        let entry = self
            .workspace
            .charts()
            .iter()
            .find(|entry| entry.id == id)?
            .clone();
        let popout_open = self.popout_is_open(id);
        if !self.workspace.remove(id) {
            return None;
        }
        if popout_open {
            self.detached.insert(id, entry);
        } else {
            self.charts.remove(&id);
            #[cfg(not(target_family = "wasm"))]
            self.popouts.remove(&id);
        }
        Some(popout_open)
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
            #[cfg(not(target_family = "wasm"))]
            self.popouts.remove(&id);
        }
    }

    fn set_symbol(&mut self, id: u32, symbol: &str) {
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
    let workspace = Rc::new(RefCell::new(ChartWorkspace::new()));
    ui.set_charts(workspace.borrow().rows.clone().into());
    let drawings = Rc::new(RefCell::new(DrawingStore::new("local-viewer")));
    let next_drawing_id = Rc::new(Cell::new(1));
    let pending_anchor = Rc::new(Cell::<Option<Anchor>>::new(None));
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
            let Some(popout_open) = state.remove_grid_chart(id) else {
                ui.set_interaction_status("Keep at least one chart open.".into());
                return;
            };
            let active = state.workspace.active();
            pending_anchor.set(None);
            select_chart(&ui, &mut state, active, &broker);
            state.refresh(&ui, &drawings.borrow());
            ui.set_interaction_status(if popout_open {
                "Chart removed from grid; pop-out remains open".into()
            } else {
                "Chart closed".into()
            });
        });
    }

    {
        let weak = ui.as_weak();
        let workspace = workspace.clone();
        let drawings = drawings.clone();
        ui.on_chart_popout(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let Ok(id) = u32::try_from(id) else { return };
            let mut state = workspace.borrow_mut();
            match state.open_popout(id) {
                Ok(()) => {
                    state.refresh(&ui, &drawings.borrow());
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
            let Some(anchor) = chart.borrow().screen_to_anchor(x, y) else {
                ui.set_interaction_status("Chart is not ready for drawing.".into());
                return;
            };

            let symbol = ui.get_symbol().to_string();
            let result = match tool.as_str() {
                "Horizontal line" => add_drawing(
                    &mut drawings.borrow_mut(),
                    &next_drawing_id,
                    &symbol,
                    DrawingKind::HorizontalLine,
                    anchor,
                    None,
                )
                .map(|()| "Horizontal line placed".to_owned()),
                "Trend line" | "Ray" | "Measure" => {
                    if let Some(first) = pending_anchor.take() {
                        let kind = match tool.as_str() {
                            "Ray" => DrawingKind::Ray,
                            "Measure" => DrawingKind::Measure,
                            _ => DrawingKind::TrendLine,
                        };
                        add_drawing(
                            &mut drawings.borrow_mut(),
                            &next_drawing_id,
                            &symbol,
                            kind,
                            first,
                            Some(anchor),
                        )
                        .map(|()| format!("{tool} placed"))
                    } else {
                        pending_anchor.set(Some(anchor));
                        ui.set_interaction_status("Choose the second anchor".into());
                        return;
                    }
                }
                "Vertical line" => add_drawing(
                    &mut drawings.borrow_mut(),
                    &next_drawing_id,
                    &symbol,
                    DrawingKind::VerticalLine,
                    anchor,
                    None,
                )
                .map(|()| "Vertical line placed".to_owned()),
                "Crosshair" => add_drawing(
                    &mut drawings.borrow_mut(),
                    &next_drawing_id,
                    &symbol,
                    DrawingKind::Crosshair,
                    anchor,
                    None,
                )
                .map(|()| "Crosshair placed".to_owned()),
                "Eraser" => {
                    let symbol = normalize_symbol(&symbol);
                    let closest = {
                        let store = drawings.borrow();
                        store
                            .for_symbol(&symbol)
                            .iter()
                            .filter(|drawing| !drawing.locked && drawing.visible)
                            .filter_map(|drawing| {
                                drawing_distance(&chart.borrow(), drawing, (x, y))
                                    .map(|distance| (drawing.id.clone(), distance))
                            })
                            .filter(|(_, distance)| *distance <= 8.0)
                            .min_by(|a, b| a.1.total_cmp(&b.1))
                            .map(|(id, _)| id)
                    };
                    match closest.and_then(|id| drawings.borrow_mut().remove(&symbol, &id)) {
                        Some(_) => {
                            ui.set_interaction_status("Drawing erased".into());
                            render_chart(&ui, &workspace.borrow(), &drawings.borrow());
                        }
                        None => ui.set_interaction_status("No unlocked drawing nearby".into()),
                    }
                    return;
                }
                _ => {
                    ui.set_interaction_status("This tool is not available yet.".into());
                    return;
                }
            };
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
    let ui = create_app();
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
        assert_eq!(
            ui.get_interaction_status(),
            "Chart removed from grid; pop-out remains open"
        );
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
        state.open_popout(id).unwrap();
        let popout = state.popouts[&id].clone_strong();
        assert!(popout.window().is_visible());
        assert_eq!(state.remove_grid_chart(id), Some(true));
        assert!(popout.window().is_visible());
        assert_eq!(state.workspace.charts().len(), 1);
        assert_eq!(state.remove_grid_chart(1), None);

        let mut drawings = DrawingStore::new("local-viewer");
        state.set_symbol(1, "AAPL");
        state.refresh(&ui, &drawings);
        assert_eq!(popout.get_symbol(), "AAPL");
        assert!(popout.get_time_ticks().row_count() > 0);
        assert!(popout.get_price_ticks().row_count() > 0);
        assert_eq!(
            popout.get_current_price().label,
            state.charts[&id]
                .borrow()
                .current_price_marker()
                .unwrap()
                .label
        );
        let before = popout.get_chart_image().to_rgba8().unwrap();
        let anchor = state.charts[&id]
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
        assert!(!state.detached.contains_key(&id));
        assert!(!state.charts.contains_key(&id));
        assert!(!state.popouts.contains_key(&id));
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
