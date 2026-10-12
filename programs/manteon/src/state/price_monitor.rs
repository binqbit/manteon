use anchor_lang::prelude::*;

/// Temporary price observations, separate from the market's event definition and result.
#[account]
#[derive(InitSpace)]
pub struct PriceMonitor {
    pub pool: Pubkey,
    /// Present only while the associated price Pool is Active and unresolved.
    pub baseline: Option<PriceBaseline>,
}

impl PriceMonitor {
    pub const SEED: &'static [u8] = b"price_monitor";
}

#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, Debug, Eq, PartialEq)]
pub struct PriceBaseline {
    /// Timestamp of the authenticated CPMM cumulative endpoint, not supplied by a caller.
    pub observed_at: i64,
    pub cumulative_price_x32: u128,
    pub reference_observed_at: i64,
    pub reference_cumulative_price_x32: u128,
}
