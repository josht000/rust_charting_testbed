//! Writes golden indicator output (from shared/indicators, the Rust reference)
//! for every chart to `1_tauri/tests/golden/chart{i}.json`. The vitest suite
//! compares the JS/TS indicator implementations against these files (1e-6).
//!
//! Run: `cargo run --release --bin gen_golden` from src-tauri/.

use std::fs;
use std::path::Path;

fn main() {
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/golden");
    fs::create_dir_all(&out_dir).unwrap();
    for i in 0..datagen::NUM_CHARTS {
        let bars = datagen::generate(i, 1);
        let ind = indicators::compute_all(&bars);
        let json = serde_json::json!({
            "ema10": ind.ema10,
            "ema20": ind.ema20,
            "ema50": ind.ema50,
            "sma200": ind.sma200,
            "vwap": ind.vwap,
            "profile": ind.profile.iter().map(|b| serde_json::json!({
                "price_low": b.price_low,
                "price_high": b.price_high,
                "volume": b.volume,
            })).collect::<Vec<_>>(),
        });
        fs::write(out_dir.join(format!("chart{i}.json")), serde_json::to_string(&json).unwrap())
            .unwrap();
        println!("chart{i}: golden written");
    }
}
