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

/// The incremental (blit + strip repaint) path must produce pixel-identical
/// output to a full redraw whenever it is eligible. Scans fixed 1000-bar
/// windows shifted by exactly 5 bars (= 4 px at 800 px wide) so the
/// translation is integer-exact; when the visible price range survives the
/// shift, the blit path fires and is byte-compared against the full render.
#[test]
fn blit_render_matches_full_render() {
    let mut chart = slint_testbed::chart::Chart::new(0);
    chart.resize(800, 400);
    while !chart.streaming_done() {
        chart.stream_batch(4096);
    }
    let mut blits = 0;
    for s in (0..18_000).step_by(997) {
        chart.start = s as f64;
        chart.end = chart.start + 1000.0;
        let f0 = chart.render_frame(None).expect("frame");
        chart.start += 5.0;
        chart.end += 5.0;
        let inc = chart.render_frame(Some(&f0)).expect("frame");
        let full = chart.render_frame(None).expect("frame");
        if inc.pb.as_bytes() != full.pb.as_bytes() {
            let a = inc.pb.as_bytes();
            let b = full.pb.as_bytes();
            let pos = a.iter().zip(b.iter()).position(|(x, y)| x != y).unwrap();
            let (x, y) = (pos / 4 % 800, pos / 4 / 800);
            panic!(
                "pixel mismatch at start {s} (blit={}): col {x} row {y} byte {}: {:?} vs {:?}",
                inc.blit,
                pos % 4,
                &a[pos / 4 * 4..pos / 4 * 4 + 4],
                &b[pos / 4 * 4..pos / 4 * 4 + 4]
            );
        }
        if inc.blit {
            blits += 1;
        }
    }
    assert!(blits > 0, "blit path never exercised");
}

/// A no-op pan (view clamped at the data edge) must report "unchanged" so
/// the caller can skip the image update entirely.
#[test]
fn unchanged_frame_returns_none() {
    let mut chart = slint_testbed::chart::Chart::new(0);
    chart.resize(800, 400);
    while !chart.streaming_done() {
        chart.stream_batch(4096);
    }
    let f0 = chart.render_frame(None).expect("frame");
    assert!(chart.render_frame(Some(&f0)).is_none());
}
