use digitrader::{app::create_app, AppWindow};
use i_slint_backend_testing::{
    mock_elapsed_time, ElementHandle, TestingBackend, TestingBackendOptions,
};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, LogicalSize, Model};
use std::time::Duration;

fn element(ui: &AppWindow, id: &str, index: usize) -> ElementHandle {
    ElementHandle::find_by_element_id(ui, &format!("AppWindow::{id}"))
        .nth(index)
        .expect("missing chart element")
}

fn click(ui: &AppWindow, id: &str, index: usize) {
    element(ui, id, index).mock_single_click(PointerEventButton::Left);
    mock_elapsed_time(Duration::from_millis(1));
}

fn assert_outline(ui: &AppWindow, index: usize) {
    click(ui, "chart_select_button", index);
    let tile = element(ui, "chart_tile", index);
    let position = tile.absolute_position();
    let size = tile.size();
    let snapshot = ui.window().take_snapshot().expect("render chart workspace");
    let pixels = snapshot.as_slice();
    let width = snapshot.width() as usize;
    let is_highlight = |x: i32, y: i32| {
        let pixel = pixels[y as usize * width + x as usize];
        pixel.r >= 150 && pixel.g >= 150 && pixel.b >= 150
    };
    let left = position.x.round() as i32;
    let right = (position.x + size.width).round() as i32;
    let top = position.y.round() as i32;
    let bottom = (position.y + size.height).round() as i32;
    let center_x = (left + right) / 2;
    let center_y = (top + bottom) / 2;
    for (name, found) in [
        ("top", (top..top + 3).any(|y| is_highlight(center_x, y))),
        (
            "bottom",
            (bottom - 3..bottom).any(|y| is_highlight(center_x, y)),
        ),
        ("left", (left..left + 3).any(|x| is_highlight(x, center_y))),
        (
            "right",
            (right - 3..right).any(|x| is_highlight(x, center_y)),
        ),
    ] {
        assert!(found, "chart {index} missing selected {name} outline");
    }
}

#[test]
fn selected_chart_outline_remains_visible_on_all_edges() {
    slint::platform::set_platform(Box::new(TestingBackend::new(TestingBackendOptions {
        mock_time: true,
        renderer_name: Some("software".into()),
        ..Default::default()
    })))
    .expect("initialize testing renderer");
    let ui = create_app();
    ui.window().set_size(LogicalSize::new(1200.0, 800.0));
    ui.show().unwrap();
    mock_elapsed_time(Duration::from_millis(1));
    let plot = element(&ui, "ta", 0);
    let marker = ui.get_charts().row_data(0).unwrap().current_price;
    assert!(!marker.label.is_empty());
    let tag = ElementHandle::find_by_element_id(&ui, "ChartAxes::current_price_tag")
        .next()
        .expect("current price tag");
    let tag_position = tag.absolute_position();
    let tag_size = tag.size();
    let plot_position = plot.absolute_position();
    let plot_size = plot.size();
    assert!(
        (tag_position.y + tag_size.height / 2.0
            - plot_position.y
            - plot_size.height * marker.position)
            .abs()
            < 1.0
    );
    let snapshot = ui.window().take_snapshot().unwrap();
    let width = snapshot.width() as usize;
    let tag_x = (tag_position.x + 10.0) as usize;
    let tag_y = (tag_position.y + tag_size.height / 2.0) as usize;
    let pixel = snapshot.as_slice()[tag_y * width + tag_x];
    assert!(
        pixel.r > 150 && pixel.g > 70 && pixel.b < 30,
        "tag must be visible"
    );
    let y = (plot_position.y + plot_size.height * marker.position).round() as usize;
    let left = plot_position.x as usize;
    let right = (plot_position.x + plot_size.width) as usize;
    assert!(
        snapshot.as_slice()[y * width + left..y * width + right]
            .iter()
            .any(|p| p.r > 150 && p.g > 70 && p.b < 30),
        "price line must align with tag"
    );
    tag.mock_single_click(PointerEventButton::Right);
    mock_elapsed_time(Duration::from_millis(1));
    let teal = ElementHandle::find_by_element_id(&ui, "ChartAxes::style_color")
        .nth(1)
        .expect("price style teal control");
    teal.mock_single_click(PointerEventButton::Left);
    mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(ui.get_charts().row_data(0).unwrap().price_color, "teal");
    let solid = ElementHandle::find_by_element_id(&ui, "ChartAxes::style_pattern")
        .nth(1)
        .expect("price style solid control");
    solid.mock_single_click(PointerEventButton::Left);
    mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(ui.get_charts().row_data(0).unwrap().price_pattern, "solid");
    let visibility = ElementHandle::find_by_element_id(&ui, "ChartAxes::style_visibility")
        .next()
        .unwrap();
    visibility.mock_single_click(PointerEventButton::Left);
    mock_elapsed_time(Duration::from_millis(1));
    assert!(!ui.get_charts().row_data(0).unwrap().price_options.visible);
    click(&ui, "chart_select_button", 0);
    let axis_touch = ElementHandle::find_by_element_id(&ui, "ChartAxes::axis_touch")
        .next()
        .unwrap();
    axis_touch.mock_single_click(PointerEventButton::Right);
    mock_elapsed_time(Duration::from_millis(1));
    let visibility = ElementHandle::find_by_element_id(&ui, "ChartAxes::style_visibility")
        .next()
        .unwrap();
    visibility.mock_single_click(PointerEventButton::Left);
    mock_elapsed_time(Duration::from_millis(1));
    assert!(ui.get_charts().row_data(0).unwrap().price_options.visible);
    ui.invoke_price_option_changed(1, "thickness".into(), "3".into());
    ui.invoke_price_option_changed(1, "glow".into(), "2".into());
    ui.invoke_price_option_changed(1, "shadow_color".into(), "red".into());
    let options = ui.get_charts().row_data(0).unwrap().price_options;
    assert_eq!(options.thickness, 3);
    assert_eq!(options.glow, 2);
    assert_eq!(options.shadow_color, "red");
    click(&ui, "chart_select_button", 0);
    let position = LogicalPosition::new(
        plot_position.x + 40.0,
        plot_position.y + plot_size.height * marker.position,
    );
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Right,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Right,
    });
    mock_elapsed_time(Duration::from_millis(1));
    let red = ElementHandle::find_by_element_id(&ui, "ChartAxes::style_color")
        .nth(2)
        .expect("price style red control");
    red.mock_single_click(PointerEventButton::Left);
    mock_elapsed_time(Duration::from_millis(1));
    assert_eq!(ui.get_charts().row_data(0).unwrap().price_color, "red");
    let teal = ElementHandle::find_by_element_id(&ui, "ChartAxes::style_color")
        .nth(1)
        .unwrap();
    teal.mock_single_click(PointerEventButton::Left);
    mock_elapsed_time(Duration::from_millis(1));
    click(&ui, "chart_select_button", 0);
    ui.invoke_chart_zoomed(3000.0, 0.0);
    mock_elapsed_time(Duration::from_millis(1));
    let offscreen = ui.get_charts().row_data(0).unwrap().current_price;
    assert_eq!(offscreen.label, marker.label);
    assert!(
        offscreen.position < 0.0 || offscreen.position >= 1.0,
        "zoom should put the latest price off-screen"
    );
    let tag = ElementHandle::find_by_element_id(&ui, "ChartAxes::current_price_tag")
        .next()
        .unwrap();
    let tag_position = tag.absolute_position();
    let axis = ElementHandle::find_by_element_id(&ui, "ChartAxes::axis_price")
        .next()
        .unwrap();
    let axis_position = axis.absolute_position();
    let axis_size = axis.size();
    if offscreen.position < 0.0 {
        assert!((tag_position.y - axis_position.y).abs() < 1.0);
    } else {
        assert!(
            (tag_position.y + tag.size().height - axis_position.y - axis_size.height).abs() < 1.0
        );
    }
    let snapshot = ui.window().take_snapshot().unwrap();
    let x = (tag_position.x + 12.0) as usize;
    let y = (tag_position.y + 2.0) as usize;
    let pixel = snapshot.as_slice()[y * snapshot.width() as usize + x];
    assert!(
        pixel.r < 80 && pixel.g > 120 && pixel.b > 100,
        "off-screen tag must be visible"
    );
    ui.invoke_chart_zoomed(-10_000.0, 0.0);
    mock_elapsed_time(Duration::from_millis(1));
    for (name, horizontal) in [("axis_price", false), ("axis_time", true)] {
        let axis = ElementHandle::find_by_element_id(&ui, &format!("ChartAxes::{name}"))
            .next()
            .expect("axis must exist");
        let p = axis.absolute_position();
        let s = axis.size();
        let q = plot.absolute_position();
        let ps = plot.size();
        if horizontal {
            assert!((p.x - q.x).abs() < 0.1 && (p.y - q.y - ps.height).abs() < 0.1);
        } else {
            assert!((p.x - q.x - ps.width).abs() < 0.1 && (p.y - q.y).abs() < 0.1);
        }
        let snapshot = ui.window().take_snapshot().unwrap();
        let width = snapshot.width() as usize;
        let mut count = 0;
        for y in (p.y as usize)..((p.y + s.height) as usize) {
            for x in (p.x as usize)..((p.x + s.width) as usize) {
                let px = snapshot.as_slice()[y * width + x];
                if px.r > 100 && px.g > 100 && px.b > 100 {
                    count += 1;
                }
            }
        }
        assert!(count > 5, "{name} has no visible labels");
    }
    for index in 1..8 {
        click(&ui, "chart_add_button", 0);
        if [1, 2, 3, 7].contains(&index) {
            for chart in 0..=index {
                assert_outline(&ui, chart);
            }
        }
    }
    let divider = element(&ui, "row_divider", 0);
    let pos = divider.absolute_position();
    let size = divider.size();
    divider.mock_drag(
        LogicalPosition::new(pos.x + size.width * 0.3, pos.y + 20.0),
        PointerEventButton::Left,
    );
    mock_elapsed_time(Duration::from_millis(1));
    for chart in 0..4 {
        assert_outline(&ui, chart);
    }
}
