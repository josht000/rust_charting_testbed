//! Cross-check the `ta`-crate indicator path against the shared reference
//! (`shared/indicators`). EMA uses an SMA-seed feed through `ta`, so outputs
//! match the TA-Lib-style reference to < 1e-9 with NO warmup tolerance;
//! SMA200 matches from index `period - 1` onward (earlier values are NaN in
//! both representations).

use slint_testbed::chart::compute_all_ta;

fn assert_close(a: &[f64], b: &[f64], name: &str) {
    assert_eq!(a.len(), b.len(), "{name} length");
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x.is_nan() && y.is_nan() {
            continue;
        }
        assert!(
            (x - y).abs() < 1e-9,
            "{name}[{i}] mismatch: {x} vs {y}"
        );
    }
}

#[test]
fn all_indicators_match_reference_all_charts() {
    for idx in 0..datagen::NUM_CHARTS {
        let bars = datagen::generate(idx, 1);
        assert_eq!(bars.len(), datagen::BARS_PER_DAY);

        let mine = compute_all_ta(&bars);
        let refr = indicators::compute_all(&bars);

        assert_close(&mine.ema10, &refr.ema10, "ema10");
        assert_close(&mine.ema20, &refr.ema20, "ema20");
        assert_close(&mine.ema50, &refr.ema50, "ema50");
        assert_close(&mine.sma200, &refr.sma200, "sma200");
        assert_close(&mine.vwap, &refr.vwap, "vwap");

        assert_eq!(mine.profile.len(), refr.profile.len());
        for (i, (a, b)) in mine.profile.iter().zip(refr.profile.iter()).enumerate() {
            assert!((a.volume - b.volume).abs() < 1e-6, "profile[{i}].volume");
            assert!((a.price_low - b.price_low).abs() < 1e-9, "profile[{i}].price_low");
        }
    }
}

/// Warmup tolerance note: `ta`'s incremental EMA state (used for live bars)
/// is seeded from the first close, so appended-bar values can differ from the
/// SMA-seeded reference during the first ~4*period bars after seeding. After
/// 23,400 seeded bars the difference is far below 1e-9.
#[test]
fn live_incremental_matches_after_warmup() {
    let mut chart = slint_testbed::chart::Chart::new(0);
    assert_eq!(chart.bars.len(), datagen::INITIAL_BARS); // preload only
    // Drain the streaming replay, then append synthetic live bars.
    let mut consumed = 0usize;
    while !chart.streaming_done() {
        consumed += chart.stream_batch(512);
    }
    assert_eq!(consumed, datagen::BARS_PER_DAY - datagen::INITIAL_BARS);
    assert_eq!(chart.bars.len(), datagen::BARS_PER_DAY);
    // Streamed indicator state must match a full recompute.
    let refr = indicators::compute_all(&chart.bars);
    assert_close(&chart.ind.ema10, &refr.ema10, "ema10 (streamed)");
    assert_close(&chart.ind.ema50, &refr.ema50, "ema50 (streamed)");
    assert_close(&chart.ind.sma200, &refr.sma200, "sma200 (streamed)");
    assert_close(&chart.ind.vwap, &refr.vwap, "vwap (streamed)");

    let n0 = chart.bars.len();
    for _ in 0..50 {
        chart.push_live_bar();
    }
    assert_eq!(chart.bars.len(), n0 + 50);
    assert_eq!(chart.ind.ema10.len(), chart.bars.len());
    // Incremental EMA10 vs full reference recompute on the extended series.
    let refr = indicators::compute_all(&chart.bars);
    let tail = &chart.ind.ema10[n0 - 100..];
    let refr_tail = &refr.ema10[n0 - 100..];
    assert_close(tail, refr_tail, "ema10 tail (live)");
    assert_close(&chart.ind.sma200[n0 - 100..], &refr.sma200[n0 - 100..], "sma200 tail");
    assert_close(&chart.ind.vwap[n0 - 100..], &refr.vwap[n0 - 100..], "vwap tail");
}
