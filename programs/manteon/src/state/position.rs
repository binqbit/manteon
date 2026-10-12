use anchor_lang::prelude::*;

use crate::error::ManteonError;

/// Aggregate participating principal for one Pool and UTC forecast day.
#[account]
#[derive(InitSpace)]
pub struct Position {
    // Pool and forecast identity.
    pub pool: Pubkey,
    /// Signed UTC day since the Unix epoch, not a timestamp.
    pub forecast_day: i64,

    // Participation basis and settlement consumption.
    /// Current participating principal; immutable after settlement freeze.
    pub principal: u64,
    /// Whether this date has contributed to the single settlement calculation.
    pub computed: bool,
}

impl Position {
    /// Followed by the Pool key and forecast_day encoded as little-endian i64.
    pub const SEED: &'static [u8] = b"position";

    /// Accept a fresh zeroed account or this Pool's uncomputed forecast date.
    pub(crate) fn validate_deposit(&self, pool: Pubkey, forecast_day: i64) -> Result<()> {
        if self.pool == Pubkey::default() {
            require!(
                self.forecast_day == 0 && self.principal == 0 && !self.computed,
                ManteonError::InvalidPosition
            );
        } else {
            require!(
                self.pool == pool && self.forecast_day == forecast_day && !self.computed,
                ManteonError::InvalidPosition
            );
        }
        Ok(())
    }
}
