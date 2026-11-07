//! Typed chart-facing access to the shared, pure-Rust indicator algorithms.
//!
//! EMA and SMA series align one-for-one with their input bars. The first
//! `period - 1` values are `NaN`; inputs shorter than the period produce an
//! all-`NaN` series. A zero period is rejected. Empty inputs produce empty
//! series/profile data. Daily VWAP resets when the UTC epoch day
//! (`timestamp / 86_400`) changes and has no warmup values.

use datagen::Bar;

pub use indicators::ProfileBin;

/// A chart indicator to calculate over its bars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndicatorRequest {
    Ema { period: usize },
    Sma { period: usize },
    DailyVwap,
    VolumeProfile { lookback: usize, bins: usize },
}

/// Result data returned by [`compute`].
#[derive(Clone, Debug)]
pub enum IndicatorData {
    Series(Vec<f64>),
    Profile(Vec<ProfileBin>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndicatorError {
    InvalidPeriod,
    InvalidProfileBins,
}

impl std::fmt::Display for IndicatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidPeriod => "moving-average period must be greater than zero",
            Self::InvalidProfileBins => "volume-profile bin count must be greater than zero",
        })
    }
}

impl std::error::Error for IndicatorError {}

/// Computes a requested chart indicator using the canonical shared formulas.
///
/// The profile uses the shared implementation's range and close-to-bin
/// assignment semantics. A zero lookback is valid and returns no bins.
pub fn compute(bars: &[Bar], request: IndicatorRequest) -> Result<IndicatorData, IndicatorError> {
    match request {
        IndicatorRequest::Ema { period } => {
            if period == 0 {
                return Err(IndicatorError::InvalidPeriod);
            }
            Ok(IndicatorData::Series(indicators::ema(
                &indicators::closes(bars),
                period,
            )))
        }
        IndicatorRequest::Sma { period } => {
            if period == 0 {
                return Err(IndicatorError::InvalidPeriod);
            }
            Ok(IndicatorData::Series(indicators::sma(
                &indicators::closes(bars),
                period,
            )))
        }
        IndicatorRequest::DailyVwap => Ok(IndicatorData::Series(indicators::vwap_daily(bars))),
        IndicatorRequest::VolumeProfile { lookback, bins } => {
            if bins == 0 {
                return Err(IndicatorError::InvalidProfileBins);
            }
            Ok(IndicatorData::Profile(indicators::volume_profile(
                bars, lookback, bins,
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars(closes: &[f64], timestamps: &[i64]) -> Vec<Bar> {
        closes
            .iter()
            .zip(timestamps)
            .map(|(&close, &ts)| Bar {
                ts,
                open: close,
                high: close + 1.0,
                low: close - 1.0,
                close,
                volume: 2.0,
            })
            .collect()
    }

    fn series(data: IndicatorData) -> Vec<f64> {
        match data {
            IndicatorData::Series(values) => values,
            IndicatorData::Profile(_) => panic!("expected a series"),
        }
    }

    fn assert_series_matches(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len());
        for (&actual, &expected) in actual.iter().zip(expected) {
            assert!(
                (actual.is_nan() && expected.is_nan()) || (actual - expected).abs() < 1e-12,
                "expected {expected}, got {actual}"
            );
        }
    }

    #[test]
    fn moving_averages_preserve_short_input_warmup() {
        let input = bars(&[1.0, 2.0], &[0, 1]);
        for request in [
            IndicatorRequest::Ema { period: 3 },
            IndicatorRequest::Sma { period: 3 },
        ] {
            let values = series(compute(&input, request).unwrap());
            assert_eq!(values.len(), input.len());
            assert!(values.iter().all(|value| value.is_nan()));
        }
    }

    #[test]
    fn rejects_invalid_periods_and_bins() {
        let input = bars(&[1.0], &[0]);
        assert!(matches!(
            compute(&input, IndicatorRequest::Ema { period: 0 }),
            Err(IndicatorError::InvalidPeriod)
        ));
        assert!(matches!(
            compute(&input, IndicatorRequest::Sma { period: 0 }),
            Err(IndicatorError::InvalidPeriod)
        ));
        assert!(matches!(
            compute(
                &input,
                IndicatorRequest::VolumeProfile {
                    lookback: 10,
                    bins: 0
                }
            ),
            Err(IndicatorError::InvalidProfileBins)
        ));
    }

    #[test]
    fn empty_input_returns_empty_outputs() {
        let input = [];
        for request in [
            IndicatorRequest::Ema { period: 2 },
            IndicatorRequest::Sma { period: 2 },
            IndicatorRequest::DailyVwap,
        ] {
            assert!(series(compute(&input, request).unwrap()).is_empty());
        }
        assert!(matches!(
            compute(
                &input,
                IndicatorRequest::VolumeProfile {
                    lookback: 10,
                    bins: 4
                }
            )
            .unwrap(),
            IndicatorData::Profile(profile) if profile.is_empty()
        ));
    }

    #[test]
    fn daily_vwap_resets_at_epoch_day_boundary() {
        let input = bars(&[10.0, 20.0, 30.0], &[86_399, 86_399, 86_400]);
        assert_eq!(
            series(compute(&input, IndicatorRequest::DailyVwap).unwrap()),
            indicators::vwap_daily(&input)
        );
        let values = series(compute(&input, IndicatorRequest::DailyVwap).unwrap());
        assert!((values[0] - 10.0).abs() < 1e-12);
        assert!((values[1] - 15.0).abs() < 1e-12);
        assert!((values[2] - 30.0).abs() < 1e-12);
    }

    #[test]
    fn outputs_match_shared_reference_for_each_indicator() {
        let input = bars(&[11.0, 12.0, 10.0, 13.0, 15.0], &[0, 1, 2, 86_400, 86_401]);
        let closes = indicators::closes(&input);
        assert_series_matches(
            &series(compute(&input, IndicatorRequest::Ema { period: 3 }).unwrap()),
            &indicators::ema(&closes, 3),
        );
        assert_series_matches(
            &series(compute(&input, IndicatorRequest::Sma { period: 2 }).unwrap()),
            &indicators::sma(&closes, 2),
        );
        assert_series_matches(
            &series(compute(&input, IndicatorRequest::DailyVwap).unwrap()),
            &indicators::vwap_daily(&input),
        );
        match compute(
            &input,
            IndicatorRequest::VolumeProfile {
                lookback: 4,
                bins: 5,
            },
        )
        .unwrap()
        {
            IndicatorData::Profile(actual) => {
                let expected = indicators::volume_profile(&input, 4, 5);
                assert_eq!(actual.len(), expected.len());
                for (actual, expected) in actual.iter().zip(expected) {
                    assert_eq!(actual.price_low, expected.price_low);
                    assert_eq!(actual.price_high, expected.price_high);
                    assert_eq!(actual.volume, expected.volume);
                }
            }
            IndicatorData::Series(_) => panic!("expected a profile"),
        }
    }
}
