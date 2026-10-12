//! Agreed constants only; no proposed fee, month, numerical or ABI defaults.

/// A UTC calendar day in Unix seconds.
pub const SECONDS_PER_DAY: i64 = 86_400;

/// The personal withdrawal window opened by new capital: exactly 48 hours.
pub const WITHDRAWAL_WINDOW_SECONDS: i64 = 2 * SECONDS_PER_DAY;
