//! Cross-check the `ta` crate (EMA/SMA used by this config) against the
//! shared/indicators reference implementation. EMA seeding differs between
//! the libraries (TA-Lib seeds with an SMA of the first `period` values;
//! `ta` seeds with the first value and blends immediately), so values are
//! compared only after a warmup of 10x period with a 1e-9 relative tolerance;
//! residual divergence decays as (1-k)^n.

use egui_testbed::chart::compute_indicators;

const WARMUP_MULT: usize = 10;
const REL_TOL: f64 = 1e-9;

fn close_enough(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    (a - b).abs() <= REL_TOL * b.abs().max(1.0)
}

fn compare_series(name: &str, ours: &[f64], reference: &[f64], warmup: usize) {
    assert_eq!(ours.len(), reference.len(), "{name}: length mismatch");
    let mut checked = 0usize;
    for i in warmup..ours.len() {
        assert!(
            close_enough(ours[i], reference[i]),
            "{name}[{i}]: ours={} ref={}",
            ours[i],
            reference[i]
        );
        checked += 1;
    }
    assert!(checked > 20_000, "{name}: too few points checked");
}

#[test]
fn ema_sma_match_shared_indicators_after_warmup() {
    for chart_idx in [0usize, 7] {
        let bars = datagen::generate(chart_idx, 1);
        let ours = compute_indicators(&bars);
        let c = indicators::closes(&bars);

        compare_series(
            "ema10",
            &ours.ema10,
            &indicators::ema(&c, 10),
            10 * WARMUP_MULT,
        );
        compare_series(
            "ema20",
            &ours.ema20,
            &indicators::ema(&c, 20),
            20 * WARMUP_MULT,
        );
        compare_series(
            "ema50",
            &ours.ema50,
            &indicators::ema(&c, 50),
            50 * WARMUP_MULT,
        );
        // SMA: identical algorithm, only the NaN warmup differs.
        compare_series("sma200", &ours.sma200, &indicators::sma(&c, 200), 199);
        // VWAP + profile come from shared/indicators directly — exact match.
        compare_series("vwap", &ours.vwap, &indicators::vwap_daily(&bars), 0);
        let prof = indicators::volume_profile(&bars, 2000, 40);
        assert_eq!(ours.profile.len(), prof.len());
        for (a, b) in ours.profile.iter().zip(prof.iter()) {
            assert!((a.volume - b.volume).abs() < 1e-6);
        }
    }
}

/// Streaming model: INITIAL_BARS preload + incremental pushes must produce
/// the same indicator series as computing over the full day at once.
#[test]
fn streamed_indicators_match_bulk() {
    let mut chart = egui_testbed::chart::Chart::new(3);
    assert_eq!(chart.streamed, datagen::INITIAL_BARS);
    assert!(chart.is_streaming());
    // Stream in irregular batches like the replay driver does.
    while chart.is_streaming() {
        chart.push_streamed(75);
    }
    assert_eq!(chart.streamed, datagen::BARS_PER_DAY);
    let bulk = compute_indicators(&chart.bars);

    compare_series("ema10", &chart.ind.ema10, &bulk.ema10, 10 * WARMUP_MULT);
    compare_series("ema20", &chart.ind.ema20, &bulk.ema20, 20 * WARMUP_MULT);
    compare_series("ema50", &chart.ind.ema50, &bulk.ema50, 50 * WARMUP_MULT);
    compare_series("sma200", &chart.ind.sma200, &bulk.sma200, 200);
    compare_series("vwap", &chart.ind.vwap, &bulk.vwap, 0);
    let prof = indicators::volume_profile(&chart.bars, 2000, 40);
    assert_eq!(chart.ind.profile.len(), prof.len());
    for (a, b) in chart.ind.profile.iter().zip(prof.iter()) {
        assert!((a.volume - b.volume).abs() < 1e-6);
    }
}

#[test]
fn nan_warmup_convention() {
    let bars = datagen::generate(0, 1);
    let ours = compute_indicators(&bars);
    assert!(ours.ema10[..9].iter().all(|v| v.is_nan()));
    assert!(ours.ema20[..19].iter().all(|v| v.is_nan()));
    assert!(ours.ema50[..49].iter().all(|v| v.is_nan()));
    assert!(ours.sma200[..199].iter().all(|v| v.is_nan()));
    assert!(ours.ema10[9].is_finite());
    assert!(ours.sma200[199].is_finite());
}
