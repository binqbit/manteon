//! Signed UTC calendar arithmetic. Mathematical range is not market eligibility.

use crate::{DomainError, constants::SECONDS_PER_DAY};

/// Floor signed Unix seconds to a UTC day index, including times before epoch.
pub fn utc_day(timestamp: i64) -> i64 {
    timestamp.div_euclid(SECONDS_PER_DAY)
}

/// Compute a UTC day's start, rejecting timestamps outside the representation.
pub fn day_start(day: i64) -> Result<i64, DomainError> {
    day.checked_mul(SECONDS_PER_DAY)
        .ok_or(DomainError::ArithmeticOverflow)
}

/// Absolute difference without overflowing when signed day inputs straddle zero.
///
/// This function does not select an accuracy curve's numerical domain or approximation.
pub fn day_error(forecast: i64, observed: i64) -> u64 {
    forecast.abs_diff(observed)
}
