//! Jev 1.13 display compatibility. The TypeSafe score is an expectation, but
//! its documented contract does not prescribe display precision. This adapter
//! assumes 0.01 rounding for both score and each displayed probability.

use tect_domain::{Error, Result};

const HALF_DISPLAY_UNIT: f64 = 0.005;
const FLOAT_EPSILON: f64 = 1e-12;

pub(super) fn validate_jev_113_score(levels: &[(f64, f64)], reported: f64) -> Result<()> {
    if levels.is_empty()
        || !reported.is_finite()
        || !centimal(reported)
        || levels.iter().any(|(value, probability)| {
            !value.is_finite()
                || !(0.0..=9.0).contains(value)
                || !probability.is_finite()
                || !(0.0..=1.0).contains(probability)
                || !centimal(*probability)
        })
    {
        return Err(Error::InvalidArguments);
    }
    let mut intervals = levels
        .iter()
        .map(|(value, probability)| {
            (
                *value,
                (probability - HALF_DISPLAY_UNIT).max(0.0),
                (probability + HALF_DISPLAY_UNIT).min(1.0),
            )
        })
        .collect::<Vec<_>>();
    let lower_sum = intervals.iter().map(|(_, lower, _)| lower).sum::<f64>();
    let upper_sum = intervals.iter().map(|(_, _, upper)| upper).sum::<f64>();
    if lower_sum > 1.0 + FLOAT_EPSILON || upper_sum < 1.0 - FLOAT_EPSILON {
        return Err(Error::InvalidArguments);
    }
    intervals.sort_by(|left, right| left.0.total_cmp(&right.0));
    let minimum = expectation(&intervals);
    intervals.reverse();
    let maximum = expectation(&intervals);
    if reported + HALF_DISPLAY_UNIT + FLOAT_EPSILON < minimum
        || reported - HALF_DISPLAY_UNIT - FLOAT_EPSILON > maximum
    {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

fn centimal(value: f64) -> bool {
    (value * 100.0 - (value * 100.0).round()).abs() <= FLOAT_EPSILON
}

fn expectation(intervals: &[(f64, f64, f64)]) -> f64 {
    let mut remaining = (1.0 - intervals.iter().map(|(_, lower, _)| lower).sum::<f64>()).max(0.0);
    let mut result = intervals
        .iter()
        .map(|(value, lower, _)| value * lower)
        .sum::<f64>();
    for (value, lower, upper) in intervals {
        let added = remaining.min(upper - lower);
        result += value * added;
        remaining -= added;
    }
    result
}

#[cfg(test)]
#[path = "score_rounding_tests.rs"]
mod tests;
