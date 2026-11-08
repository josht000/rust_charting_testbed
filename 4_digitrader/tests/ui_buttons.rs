use digitrader::{app::create_app, AppWindow, CompactChartWindow};
use i_slint_backend_testing::{mock_elapsed_time, ElementHandle};
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, LogicalSize, Model};
use std::time::Duration;

fn elements(ui: &AppWindow, id: &str) -> Vec<ElementHandle> {
    let name = format!("AppWindow::{id}");
    let matches: Vec<_> = ElementHandle::find_by_element_id(ui, &name).collect();
    assert!(!matches.is_empty(), "missing UI element {name}");
    matches
}

fn click(ui: &AppWindow, id: &str, index: usize) {
    let matches = elements(ui, id);
    let element = &matches[index];
    let size = element.size();
    assert!(
        size.width > 0.0 && size.height > 0.0,
        "{id} has no hit area"
    );
    element.mock_single_click(PointerEventButton::Left);
    mock_elapsed_time(Duration::from_millis(1));
}

fn click_at(ui: &AppWindow, position: LogicalPosition) {
    let window = ui.window();
    window.dispatch_event(WindowEvent::PointerMoved { position });
    window.dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    window.dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
    mock_elapsed_time(Duration::from_millis(1));
}

fn submit_symbol(ui: &AppWindow, symbol: &str) {
    let index = (0..ui.get_charts().row_count())
        .find(|&index| ui.get_charts().row_data(index).unwrap().selected)
        .unwrap();
    replace_text_at(ui, "chart_symbol_input", index, symbol);
    type_key(ui, Key::Return.into());
}

fn type_key(ui: &AppWindow, text: slint::SharedString) {
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
    mock_elapsed_time(Duration::from_millis(1));
}

fn replace_text_at(ui: &AppWindow, id: &str, index: usize, text: &str) {
    click(ui, id, index);
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Control.into(),
    });
    type_key(ui, "a".into());
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Control.into(),
    });
    for character in text.chars() {
        type_key(ui, character.to_string().into());
    }
}

fn replace_text(ui: &AppWindow, id: &str, text: &str) {
    replace_text_at(ui, id, 0, text);
}

#[test]
fn buttons_produce_observable_actions() {
    i_slint_backend_testing::init_no_event_loop();
    let ui = create_app();
    ui.window().set_size(LogicalSize::new(1540.0, 920.0));
    ui.show().unwrap();
    mock_elapsed_time(Duration::from_millis(1));

    for (id, tool) in [
        ("tool_cursor", "Cursor"),
        ("tool_trend", "Trend line"),
        ("tool_horizontal", "Horizontal line"),
        ("tool_vertical", "Vertical line"),
        ("tool_measure", "Measure"),
        ("tool_crosshair", "Crosshair"),
        ("tool_ray", "Ray"),
        ("tool_erase", "Eraser"),
    ] {
        click(&ui, id, 0);
        assert_eq!(ui.get_active_tool(), tool, "{id} did not select its tool");
        assert_eq!(ui.get_interaction_status(), format!("{tool} selected"));
    }

    let chart = &elements(&ui, "ta")[0];
    let origin = chart.absolute_position();
    let size = chart.size();
    let first = LogicalPosition::new(origin.x + size.width * 0.4, origin.y + size.height * 0.4);
    let second = LogicalPosition::new(origin.x + size.width * 0.6, origin.y + size.height * 0.6);
    for (index, (button, tool, two_points)) in [
        ("tool_horizontal", "Horizontal line", false),
        ("tool_vertical", "Vertical line", false),
        ("tool_crosshair", "Crosshair", false),
        ("tool_trend", "Trend line", true),
        ("tool_measure", "Measure", true),
        ("tool_ray", "Ray", true),
    ]
    .into_iter()
    .enumerate()
    {
        let first =
            LogicalPosition::new(first.x + index as f32 * 15.0, first.y + index as f32 * 13.0);
        let second = LogicalPosition::new(
            second.x + index as f32 * 10.0,
            second.y + index as f32 * 9.0,
        );
        click(&ui, button, 0);
        let before = ui.get_chart_image().to_rgba8().unwrap();
        click_at(&ui, first);
        if two_points {
            assert_eq!(ui.get_interaction_status(), "Choose the second anchor");
            click_at(&ui, second);
        }
        assert_eq!(ui.get_interaction_status(), format!("{tool} placed"));
        let after = ui.get_chart_image().to_rgba8().unwrap();
        assert!(
            before.as_slice() != after.as_slice(),
            "{tool} did not change chart pixels"
        );
    }
    click(&ui, "tool_erase", 0);
    let before = ui.get_chart_image().to_rgba8().unwrap();
    click_at(&ui, first);
    assert_eq!(ui.get_interaction_status(), "Drawing erased");
    assert!(before.as_slice() != ui.get_chart_image().to_rgba8().unwrap().as_slice());
    click(&ui, "tool_cursor", 0);
    let before = ui.get_chart_image().to_rgba8().unwrap();
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: first,
        delta_x: 0.0,
        delta_y: 120.0,
    });
    mock_elapsed_time(Duration::from_millis(1));
    assert!(
        before.as_slice() != ui.get_chart_image().to_rgba8().unwrap().as_slice(),
        "zoom did not change chart"
    );
    let before = ui.get_chart_image().to_rgba8().unwrap();
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position: first,
        button: PointerEventButton::Left,
    });
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position: second });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position: second,
        button: PointerEventButton::Left,
    });
    mock_elapsed_time(Duration::from_millis(1));
    assert!(
        before.as_slice() != ui.get_chart_image().to_rgba8().unwrap().as_slice(),
        "pan did not change chart"
    );

    click(&ui, "chart_remove_button", 0);
    assert_eq!(ui.get_interaction_status(), "Keep at least one chart open.");
    click(&ui, "chart_add_button", 0);
    assert_eq!(ui.get_charts().row_count(), 2);
    assert!(ui.get_charts().row_data(1).unwrap().selected);
    for i in 0..2 {
        let row = ui.get_charts().row_data(i).unwrap();
        assert!(row.time_ticks.row_count() > 0 && row.price_ticks.row_count() > 0);
    }
    let before_time = ui
        .get_charts()
        .row_data(0)
        .unwrap()
        .time_ticks
        .row_data(0)
        .unwrap()
        .label;
    let other_time = ui
        .get_charts()
        .row_data(1)
        .unwrap()
        .time_ticks
        .row_data(0)
        .unwrap()
        .label;
    let target = &elements(&ui, "ta")[0];
    let p = target.absolute_position();
    let s = target.size();
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: LogicalPosition::new(p.x + s.width / 2.0, p.y + s.height / 2.0),
        delta_x: 0.0,
        delta_y: 120.0,
    });
    mock_elapsed_time(Duration::from_millis(1));
    assert_ne!(
        ui.get_charts()
            .row_data(0)
            .unwrap()
            .time_ticks
            .row_data(0)
            .unwrap()
            .label,
        before_time
    );
    assert_eq!(
        ui.get_charts()
            .row_data(1)
            .unwrap()
            .time_ticks
            .row_data(0)
            .unwrap()
            .label,
        other_time
    );
    let tiles = elements(&ui, "chart_tile");
    let (a, b) = (tiles[0].size(), tiles[1].size());
    assert!((a.width - b.width).abs() < 1.0 && (a.height - b.height).abs() < 1.0);
    assert!(tiles[1].absolute_position().x > tiles[0].absolute_position().x);
    click(&ui, "chart_add_button", 0);
    let tiles = elements(&ui, "chart_tile");
    assert!((tiles[2].size().width - 2.0 * tiles[0].size().width).abs() < 2.0);
    assert!(tiles[2].absolute_position().y > tiles[0].absolute_position().y);
    click(&ui, "chart_add_button", 0);
    let tiles = elements(&ui, "chart_tile");
    for tile in &tiles[1..] {
        assert!((tile.size().width - tiles[0].size().width).abs() < 1.0);
        assert!((tile.size().height - tiles[0].size().height).abs() < 1.0);
    }
    let divider = &elements(&ui, "column_divider")[0];
    let pos = divider.absolute_position();
    let size = divider.size();
    let line = &elements(&ui, "column_divider_line")[0];
    assert_eq!(size.width, 8.0, "column resize hit target changed");
    assert_eq!(line.size().width, 1.0, "column divider must be 1px wide");
    assert!((line.absolute_position().x + 0.5 - (pos.x + size.width / 2.0)).abs() < 0.1);
    divider.mock_drag(
        LogicalPosition::new(pos.x + size.width / 2.0 + 100.0, pos.y + size.height / 2.0),
        PointerEventButton::Left,
    );
    mock_elapsed_time(Duration::from_millis(1));
    let tiles = elements(&ui, "chart_tile");
    assert!(tiles[0].size().width > tiles[1].size().width + 50.0);
    assert!((tiles[2].size().width - tiles[3].size().width).abs() < 1.0);
    let divider = &elements(&ui, "row_divider")[0];
    let pos = divider.absolute_position();
    let size = divider.size();
    let line = &elements(&ui, "row_divider_line")[0];
    assert_eq!(size.height, 8.0, "row resize hit target changed");
    assert_eq!(line.size().height, 1.0, "row divider must be 1px high");
    assert!((line.absolute_position().y + 0.5 - (pos.y + size.height / 2.0)).abs() < 0.1);
    divider.mock_drag(
        LogicalPosition::new(pos.x + size.width / 2.0, pos.y + size.height / 2.0 + 60.0),
        PointerEventButton::Left,
    );
    mock_elapsed_time(Duration::from_millis(1));
    let tiles = elements(&ui, "chart_tile");
    assert!(tiles[0].size().height > tiles[2].size().height + 30.0);
    for (index, target) in elements(&ui, "ta").iter().enumerate() {
        let frame = ui.get_charts().row_data(index).unwrap().image.size();
        assert!(
            (frame.width as f32 - target.size().width).abs() <= 1.0,
            "chart {index} did not rerender at its width"
        );
        assert!(
            (frame.height as f32 - target.size().height).abs() <= 1.0,
            "chart {index} did not rerender at its height"
        );
    }
    click(&ui, "tool_horizontal", 0);
    let target = &elements(&ui, "ta")[0];
    let pos = target.absolute_position();
    let size = target.size();
    click_at(
        &ui,
        LogicalPosition::new(pos.x + size.width * 0.3, pos.y + size.height * 0.3),
    );
    assert!(ui.get_charts().row_data(0).unwrap().selected);
    assert_eq!(ui.get_interaction_status(), "Horizontal line placed");
    let images: Vec<_> = (0..4)
        .map(|i| {
            ui.get_charts()
                .row_data(i)
                .unwrap()
                .image
                .to_rgba8()
                .unwrap()
        })
        .collect();
    assert!(
        images.iter().all(|image| image
            .as_slice()
            .iter()
            .any(|pixel| pixel.r == 67 && pixel.g == 214 && pixel.b == 181)),
        "shared drawing must appear in every chart"
    );
    click(&ui, "chart_remove_button", 3);
    click(&ui, "chart_remove_button", 2);
    click(&ui, "chart_symbol_input", 0);
    for character in "aapl".chars() {
        type_key(&ui, character.to_string().into());
    }
    type_key(&ui, Key::Return.into());
    assert_eq!(ui.get_symbol(), "AAPL");
    assert_eq!(ui.get_charts().row_data(0).unwrap().symbol, "AAPL");
    assert_eq!(ui.get_charts().row_data(1).unwrap().symbol, "AAPL");
    assert!(ui.get_charts().row_data(0).unwrap().selected);
    assert_eq!(
        elements(&ui, "chart_symbol_input")[1]
            .accessible_value()
            .unwrap(),
        "AAPL"
    );
    replace_text_at(&ui, "chart_symbol_input", 0, "SPY");
    click(&ui, "chart_symbol_input", 1);
    assert_eq!(
        elements(&ui, "chart_symbol_input")[0]
            .accessible_value()
            .unwrap(),
        "AAPL",
        "unsubmitted edits must be discarded on focus loss"
    );
    assert_eq!(ui.get_charts().row_data(0).unwrap().symbol, "AAPL");
    replace_text_at(&ui, "chart_symbol_input", 1, "UNKNOWN");
    type_key(&ui, Key::Return.into());
    assert_eq!(ui.get_charts().row_data(1).unwrap().symbol, "AAPL");
    assert!(ui.get_interaction_status().contains("UNKNOWN"));
    assert!(ui.get_charts().row_data(1).unwrap().selected);
    assert_eq!(
        elements(&ui, "chart_symbol_input")[1]
            .accessible_value()
            .unwrap(),
        "AAPL"
    );
    replace_text_at(&ui, "chart_symbol_input", 1, "SPY");
    type_key(&ui, Key::Escape.into());
    assert_eq!(ui.get_charts().row_data(1).unwrap().symbol, "AAPL");
    assert_eq!(
        elements(&ui, "chart_symbol_input")[1]
            .accessible_value()
            .unwrap(),
        "AAPL"
    );
    submit_symbol(&ui, "UNKNOWN");
    assert_eq!(ui.get_symbol(), "AAPL");
    assert!(ui.get_interaction_status().contains("UNKNOWN"));
    click(&ui, "chart_link_button", 1);
    assert!(!ui.get_charts().row_data(1).unwrap().linked);
    replace_text_at(&ui, "chart_symbol_input", 1, "SPY");
    type_key(&ui, Key::Return.into());
    assert_eq!(ui.get_charts().row_data(0).unwrap().symbol, "AAPL");
    assert_eq!(ui.get_charts().row_data(1).unwrap().symbol, "SPY");
    assert_eq!(ui.get_symbol(), "SPY");
    assert_eq!(ui.get_order_price(), "540.25");
    click(&ui, "chart_link_button", 1);
    assert_eq!(ui.get_symbol(), "AAPL");
    assert!(ui.get_charts().row_data(1).unwrap().linked);
    click(&ui, "chart_select_button", 0);
    assert!(ui.get_charts().row_data(0).unwrap().selected);
    click(&ui, "chart_pop_button", 1);
    assert_eq!(ui.get_interaction_status(), "Compact chart opened");
    click(&ui, "chart_remove_button", 1);
    assert_eq!(ui.get_charts().row_count(), 1);
    assert_eq!(ui.get_interaction_status(), "Chart closed");

    click(&ui, "sell_button", 0);
    assert_eq!(ui.get_order_side(), "sell");
    click(&ui, "order_kind_button", 0);
    assert_eq!(ui.get_order_kind(), "market");
    click(&ui, "order_review_button", 0);
    assert!(
        ui.get_order_status().contains("SELL"),
        "{}",
        ui.get_order_status()
    );
    assert!(
        ui.get_order_status().contains("MARKET"),
        "{}",
        ui.get_order_status()
    );
    click(&ui, "buy_button", 0);
    assert_eq!(ui.get_order_side(), "buy");
    assert!(ui.get_order_status().contains("Review order again"));
    click(&ui, "order_kind_button", 0);
    click(&ui, "order_review_button", 0);
    assert!(
        ui.get_order_status().contains("BUY"),
        "{}",
        ui.get_order_status()
    );
    assert!(
        ui.get_order_status().contains("LIMIT"),
        "{}",
        ui.get_order_status()
    );
    replace_text(&ui, "quantity_input", "0");
    assert!(ui.get_order_status().contains("Review order again"));
    click(&ui, "order_review_button", 0);
    assert!(!ui.get_order_status().contains("PREVIEW"));
    replace_text(&ui, "quantity_input", "2");
    replace_text(&ui, "price_input", "bad");
    click(&ui, "order_review_button", 0);
    assert_eq!(ui.get_order_status(), "Limit price must be a valid number.");
    click(&ui, "order_kind_button", 0);
    click(&ui, "order_review_button", 0);
    assert!(ui.get_order_status().contains("BUY 2 AAPL @ MARKET"));
    submit_symbol(&ui, "SPY");
    assert!(ui.get_order_status().contains("Review order again"));
    click(&ui, "order_kind_button", 0);
    click(&ui, "order_review_button", 0);
    assert!(
        ui.get_order_status().contains("BUY 2 SPY @ LIMIT $540.25"),
        "{}",
        ui.get_order_status()
    );

    for _ in 1..8 {
        click(&ui, "chart_add_button", 0);
    }
    assert_eq!(ui.get_charts().row_count(), 8);
    click(&ui, "chart_add_button", 0);
    assert_eq!(ui.get_charts().row_count(), 8);
    assert_eq!(ui.get_interaction_status(), "Maximum of 8 charts reached.");

    ui.window().set_size(LogicalSize::new(800.0, 520.0));
    mock_elapsed_time(Duration::from_millis(1));
    for id in [
        "tool_cursor",
        "tool_trend",
        "tool_horizontal",
        "tool_vertical",
        "tool_measure",
        "tool_crosshair",
        "tool_ray",
        "tool_erase",
        "chart_add_button",
        "trade_scroll",
        "sell_button",
    ] {
        let element = &elements(&ui, id)[0];
        let position = element.absolute_position();
        let size = element.size();
        assert!(
            size.width > 0.0
                && size.height > 0.0
                && position.x >= 0.0
                && position.y >= 0.0
                && position.x + size.width <= 800.0
                && position.y + size.height <= 520.0,
            "{id} is not visible at the minimum window size: position {position:?}, size {size:?}"
        );
    }
    click(&ui, "tool_erase", 0);
    assert_eq!(ui.get_active_tool(), "Eraser");
    click(&ui, "sell_button", 0);
    assert_eq!(ui.get_order_side(), "sell");

    assert_eq!(elements(&ui, "chart_tile").len(), 8);
    for id in [
        "chart_select_button",
        "chart_link_button",
        "chart_pop_button",
    ] {
        for element in elements(&ui, id) {
            assert_eq!(
                element.size().height,
                24.0,
                "{id} should fit the compact chart header"
            );
        }
    }
    let divider = &elements(&ui, "column_divider")[0];
    let p = divider.absolute_position();
    let s = divider.size();
    divider.mock_drag(
        LogicalPosition::new(p.x - 1000.0, p.y + s.height / 2.0),
        PointerEventButton::Left,
    );
    mock_elapsed_time(Duration::from_millis(1));
    for (index, input) in elements(&ui, "chart_select_button").iter().enumerate() {
        let tile = &elements(&ui, "chart_tile")[index];
        let p = input.absolute_position();
        let s = input.size();
        let t = tile.absolute_position();
        let ts = tile.size();
        assert!(
            s.width >= 35.0
                && s.height >= 20.0
                && p.x >= t.x
                && p.x + s.width <= t.x + ts.width
                && p.y >= t.y
                && p.y + s.height <= t.y + ts.height,
            "chart {index} symbol entry is clipped: {p:?} {s:?}"
        );
    }
    for (index, target) in elements(&ui, "ta").iter().enumerate() {
        let frame = ui.get_charts().row_data(index).unwrap().image.size();
        assert!(
            (frame.width as f32 - target.size().width).abs() <= 1.0
                && (frame.height as f32 - target.size().height).abs() <= 1.0,
            "chart {index} did not resize with the workspace"
        );
    }
    click(&ui, "chart_select_button", 7);
    assert!(ui.get_charts().row_data(7).unwrap().selected);

    let trade = &elements(&ui, "trade_scroll")[0];
    let p = trade.absolute_position();
    let s = trade.size();
    let position = LogicalPosition::new(p.x + s.width * 0.5, p.y + s.height * 0.7);
    let old_y = elements(&ui, "order_review_button")[0]
        .absolute_position()
        .y;
    for _ in 0..8 {
        ui.window().dispatch_event(WindowEvent::PointerScrolled {
            position,
            delta_x: 0.0,
            delta_y: -120.0,
        });
        mock_elapsed_time(Duration::from_millis(100));
    }
    let review_y = elements(&ui, "order_review_button")[0]
        .absolute_position()
        .y;
    assert!(
        review_y <= old_y
            && review_y >= p.y
            && review_y + elements(&ui, "order_review_button")[0].size().height <= p.y + s.height,
        "trade panel did not show review: {old_y} -> {review_y}"
    );
    click(&ui, "order_review_button", 0);
    assert!(ui.get_order_status().contains("PREVIEW"));

    for id in ["app_minimize", "app_maximize", "app_close"] {
        let element = &elements(&ui, id)[0];
        let p = element.absolute_position();
        let s = element.size();
        assert!(
            s.width >= 28.0
                && s.height >= 26.0
                && p.x >= 0.0
                && p.x + s.width <= 800.0
                && p.y >= 0.0
                && p.y + s.height <= 520.0,
            "{id} is not visible"
        );
    }
    click(&ui, "app_maximize", 0);
    assert!(ui.window().is_maximized());
    click(&ui, "app_maximize", 0);
    assert!(!ui.window().is_maximized());
    click(&ui, "app_minimize", 0);
    assert!(ui.window().is_minimized());
    ui.window().set_minimized(false);

    let pop = CompactChartWindow::new().unwrap();
    pop.window().set_size(LogicalSize::new(240.0, 160.0));
    pop.show().unwrap();
    mock_elapsed_time(Duration::from_millis(1));
    let actions = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let captured = actions.clone();
    pop.on_chart_action(move |action, value| {
        captured
            .borrow_mut()
            .push((action.to_string(), value.to_string()));
        "".into()
    });
    for (index, expected) in [
        "Cursor",
        "Trend line",
        "Horizontal line",
        "Vertical line",
        "Measure",
        "Crosshair",
        "Ray",
        "Eraser",
    ]
    .iter()
    .enumerate()
    {
        let control = ElementHandle::find_by_element_id(&pop, "CompactChartWindow::compact_tool")
            .nth(index)
            .unwrap();
        let p = control.absolute_position();
        let size = control.size();
        assert!(size.width > 0.0 && size.height > 0.0 && p.x >= 0.0 && p.x + size.width <= 240.0);
        control.mock_single_click(PointerEventButton::Left);
        assert_eq!(
            actions.borrow().last().unwrap(),
            &("tool".to_string(), expected.to_string())
        );
    }
    ElementHandle::find_by_element_id(&pop, "CompactChartWindow::compact_link")
        .next()
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    assert_eq!(actions.borrow().last().unwrap().0, "link");
    let plot = ElementHandle::find_by_element_id(&pop, "CompactChartWindow::compact_plot_touch")
        .next()
        .unwrap();
    assert!(plot.size().width > 0.0 && plot.size().height > 0.0);
    let pointer_events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let captured = pointer_events.clone();
    pop.on_chart_pointer(move |x, y, down| {
        captured.borrow_mut().push((x, y, down));
        if down {
            "Choose the second anchor".into()
        } else {
            "".into()
        }
    });
    plot.mock_single_click(PointerEventButton::Left);
    assert!(pointer_events
        .borrow()
        .iter()
        .any(|e| e.2 && e.0 >= 0.0 && e.0 <= 1.0));
    assert!(pointer_events.borrow().iter().any(|e| !e.2));
    assert_eq!(pop.get_chart_status(), "Choose the second anchor");
    for id in ["pop_minimize", "pop_maximize", "pop_close"] {
        let name = format!("CompactChartWindow::{id}");
        let control = ElementHandle::find_by_element_id(&pop, &name)
            .next()
            .expect(&name);
        let p = control.absolute_position();
        let s = control.size();
        assert!(
            s.width >= 28.0
                && s.height >= 26.0
                && p.x >= 0.0
                && p.x + s.width <= 240.0
                && p.y >= 0.0
                && p.y + s.height <= 160.0,
            "{id} is not visible"
        );
        if id == "pop_maximize" {
            control.mock_single_click(PointerEventButton::Left);
            assert!(pop.window().is_maximized());
            control.mock_single_click(PointerEventButton::Left);
            assert!(!pop.window().is_maximized());
        } else if id == "pop_minimize" {
            control.mock_single_click(PointerEventButton::Left);
            assert!(pop.window().is_minimized());
            pop.window().set_minimized(false);
        } else {
            control.mock_single_click(PointerEventButton::Left);
            assert!(!pop.window().is_visible());
        }
    }
    click(&ui, "app_close", 0);
    assert!(!ui.window().is_visible());
}
