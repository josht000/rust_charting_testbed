use std::fs::File;
use std::io::Write;

/// Generates data files for all 8 charts into the given output dir:
///   chart{i}.json       - full day (batch/bench use)
///   chart{i}.init.json  - first INITIAL_BARS bars (static preload)
///   chart{i}.rest.json  - remaining bars (streamed replay)
/// Usage: gen_data <out_dir> [days]
fn main() {
    let out = std::env::args().nth(1).expect("usage: gen_data <out_dir> [days]");
    let days: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    std::fs::create_dir_all(&out).unwrap();
    for i in 0..datagen::NUM_CHARTS {
        let bars = datagen::generate(i, days);
        let (init, rest) = bars.split_at(datagen::INITIAL_BARS.min(bars.len()));
        for (name, slice) in [
            (format!("chart{}.json", i), &bars[..]),
            (format!("chart{}.init.json", i), init),
            (format!("chart{}.rest.json", i), rest),
        ] {
            let mut f = File::create(format!("{}/{}", out, name)).unwrap();
            f.write_all(serde_json::to_string(slice).unwrap().as_bytes())
                .unwrap();
        }
        println!(
            "chart{}: {} bars ({} init + {} streamed)",
            i,
            bars.len(),
            init.len(),
            rest.len()
        );
    }
}
