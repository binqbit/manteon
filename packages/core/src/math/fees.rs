//! Profit-only fees and an included platform share, calculated in token atoms.

use crate::{error::DomainError, types::Amount};

/// Basis points in one whole percentage (100%).
pub const BASIS_POINTS: u16 = 10_000;
/// A configuration may charge at most 20% of positive gross profit.
pub const MAX_PROFIT_FEE_BPS: u16 = 2_000;

/// Transfers that together consume the original gross payout exactly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfitFees {
    /// Payment to the forecast owner after fees.
    pub net: Amount,
    /// Total fee deducted from positive profit, including the platform share.
    pub total: Amount,
    /// Portion of the total fee reserved for the platform.
    pub platform: Amount,
    /// Remainder of the total fee paid to the configuration's recipient.
    pub recipient: Amount,
}

/// Validate rates independently of whether this particular claim makes a profit.
pub fn validate_fee_rates(profit_fee_bps: u16, platform_share_bps: u16) -> Result<(), DomainError> {
    if profit_fee_bps > MAX_PROFIT_FEE_BPS || platform_share_bps > BASIS_POINTS {
        return Err(DomainError::InvalidFeeRates);
    }
    Ok(())
}

/// Floor the profit fee, then floor the platform's share of that fee.
/// Losing and break-even claims pay no fee; principal is never the fee base.
pub fn quote_profit_fees(
    principal: Amount,
    gross: Amount,
    profit_fee_bps: u16,
    platform_share_bps: u16,
) -> Result<ProfitFees, DomainError> {
    validate_fee_rates(profit_fee_bps, platform_share_bps)?;
    let profit = gross.get().saturating_sub(principal.get());
    let total = (u128::from(profit) * u128::from(profit_fee_bps) / u128::from(BASIS_POINTS)) as u64;
    let platform =
        (u128::from(total) * u128::from(platform_share_bps) / u128::from(BASIS_POINTS)) as u64;
    Ok(ProfitFees {
        net: Amount::new(gross.get() - total),
        total: Amount::new(total),
        platform: Amount::new(platform),
        recipient: Amount::new(total - platform),
    })
}
