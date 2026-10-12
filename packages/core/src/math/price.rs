//! Conservative USD conversion of an authenticated cumulative DEX price.
//!
//! Pool prices use raw token atoms in Q32.32. Public USD values use nine decimal
//! places. Bounds retain oracle uncertainty and round outwards, never towards a
//! threshold. Account identity, observation continuity and time belong to adapters.

use ethnum::U256;

use crate::error::DomainError;

/// One USD in the public fixed-point representation.
pub const USD_SCALE: u64 = 1_000_000_000;

/// A conservative interval in nano-USD per complete token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PriceBounds {
    /// Lower bound, rounded down.
    pub lower: u64,
    /// Upper bound, rounded up.
    pub upper: u64,
}

/// Normalize the difference of a DEX's cumulative Q32.32 prices over a window.
/// Quote bounds are nano-USD per complete quote token, not token atoms.
pub fn usd_twap(
    cumulative_delta: u128,
    elapsed: u64,
    base_decimals: u8,
    quote_decimals: u8,
    quote_usd: PriceBounds,
) -> Result<PriceBounds, DomainError> {
    if elapsed == 0
        || base_decimals > 18
        || quote_decimals > 18
        || quote_usd.lower == 0
        || quote_usd.lower > quote_usd.upper
    {
        return Err(DomainError::InvalidAccounting);
    }
    let numerator = U256::from(cumulative_delta)
        .checked_mul(U256::from(10_u64.pow(u32::from(base_decimals))))
        .ok_or(DomainError::ArithmeticOverflow)?;
    let denominator = U256::from(1_u64 << 32)
        * U256::from(elapsed)
        * U256::from(10_u64.pow(u32::from(quote_decimals)));
    let convert = |price: u64, ceiling: bool| -> Result<u64, DomainError> {
        // CPMM floors each instantaneous Q32.32 price before accumulation.
        // The true cumulative is below delta + elapsed, even across many swaps.
        let bound = if ceiling {
            numerator + U256::from(elapsed) * U256::from(10_u64.pow(u32::from(base_decimals)))
        } else {
            numerator
        };
        let value = bound
            .checked_mul(U256::from(price))
            .ok_or(DomainError::ArithmeticOverflow)?;
        let floor = value / denominator;
        let rounded = floor + U256::from(u8::from(ceiling && value % denominator != U256::ZERO));
        u64::try_from(rounded).map_err(|_| DomainError::ArithmeticOverflow)
    };
    Ok(PriceBounds {
        lower: convert(quote_usd.lower, false)?,
        upper: convert(quote_usd.upper, true)?,
    })
}

/// Convert a signed oracle price/confidence/exponent into outward-rounded USD bounds.
pub fn oracle_usd(price: i64, confidence: u64, exponent: i32) -> Result<PriceBounds, DomainError> {
    if price <= 0 || confidence >= price as u64 || !(-18..=9).contains(&exponent) {
        return Err(DomainError::InvalidAccounting);
    }
    let lower = price as u64 - confidence;
    let upper = (price as u64)
        .checked_add(confidence)
        .ok_or(DomainError::ArithmeticOverflow)?;
    let power = exponent + 9;
    if power >= 0 {
        let multiplier = 10_u64.pow(power as u32);
        Ok(PriceBounds {
            lower: lower
                .checked_mul(multiplier)
                .ok_or(DomainError::ArithmeticOverflow)?,
            upper: upper
                .checked_mul(multiplier)
                .ok_or(DomainError::ArithmeticOverflow)?,
        })
    } else {
        let divisor = 10_u64.pow((-power) as u32);
        Ok(PriceBounds {
            lower: lower / divisor,
            upper: upper.div_ceil(divisor),
        })
    }
}
