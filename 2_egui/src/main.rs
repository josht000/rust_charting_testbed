//! Entry point: native exe (`cargo run --release`, `--bench` for the scripted
//! benchmark) and wasm (served by trunk, `?bench=1` for the scripted bench).

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    let bench = args.iter().any(|a| a == "--bench");
    // --rate=N or --rate N (bars/sec/chart; 0/omitted => default)
    let mut rate = 0.0f64;
    for (i, a) in args.iter().enumerate() {
        if let Some(v) = a.strip_prefix("--rate=") {
            rate = v.parse().unwrap_or(0.0);
        } else if a == "--rate" {
            rate = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        }
    }
    let start = web_time::Instant::now();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1680.0, 1050.0])
            .with_title("egui + wgpu trading testbed"),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "egui trading testbed",
        options,
        Box::new(move |cc| {
            Ok(Box::new(egui_testbed::app::TestbedApp::new(
                cc, bench, start, rate,
            )))
        }),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;

    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    let start = web_time::Instant::now();
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    let bench = search.contains("bench=1");
    // ?rate=N (bars/sec/chart; 0/omitted => default)
    let rate = search
        .split('&')
        .find_map(|p| p.strip_prefix("rate=").or_else(|| p.strip_prefix("?rate=")))
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);

    let web_options = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async move {
        let document = web_sys::window().unwrap().document().unwrap();
        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("canvas #the_canvas_id missing")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("element is not a canvas");
        let runner = eframe::WebRunner::new();
        runner
            .start(
                canvas,
                web_options,
                Box::new(move |cc| {
                    Ok(Box::new(egui_testbed::app::TestbedApp::new(
                        cc, bench, start, rate,
                    )))
                }),
            )
            .await
            .expect("failed to start eframe");
    });
}
