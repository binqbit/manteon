//! Integer basis-point premium units and wide, coefficient-first payout weights.

use crate::{
    DomainError,
    math::accuracy,
    types::{Amount, PoolAlgorithm},
};

/// Basis points per unit of real principal. Premium units are never collateral.
pub const PREMIUM_SCALE: u128 = 10_000;

/// Raw Q64 accuracy multiplied by principal measured in basis-point units.
///
/// At the u64 principal boundary and the 30% maximum, the weight uses at most
/// 142 bits; multiplying by pool principal for a quote uses at most 206 bits.
pub type Weight = ethnum::U256;

/// Validate the exact immutable policy choices, including the disabled control.
pub fn validate_max_bps(max_bps: u16) -> Result<(), DomainError> {
    if matches!(max_bps, 0 | 1_000 | 2_000 | 3_000) {
        Ok(())
    } else {
        Err(DomainError::InvalidPremiumPolicy)
    }
}

/// Floor the linear premium once, to one basis point, at the commitment anchor.
///
/// Anchor may equal LOCK, yielding zero. Runtime admission must separately
/// authenticate Clock and select NOW or the earlier of deadline and LOCK.
pub fn rate_bps(
    max_bps: u16,
    anchor_at: i64,
    opens_at: i64,
    lock_at: i64,
) -> Result<u16, DomainError> {
    validate_max_bps(max_bps)?;
    if opens_at >= lock_at || anchor_at < opens_at || anchor_at > lock_at {
        return Err(DomainError::InvalidSchedule);
    }
    let duration = u128::from(lock_at.abs_diff(opens_at));
    let remaining = u128::from(lock_at.abs_diff(anchor_at));
    // A signed-i64 difference needs u64; multiplying by <=3000 fits u128.
    Ok((u128::from(max_bps) * remaining / duration) as u16)
}

/// Convert a retained real amount and its fixed rate into exact additive units.
pub fn units(amount: Amount, rate_bps: u16) -> Result<u128, DomainError> {
    if rate_bps > 3_000 {
        return Err(DomainError::InvalidPremiumPolicy);
    }
    Ok(u128::from(amount.get()) * u128::from(rate_bps))
}

/// Require premium units to fit the immutable maximum for real principal.
pub fn validate_units(
    principal: Amount,
    premium_units: u128,
    max_bps: u16,
) -> Result<(), DomainError> {
    validate_max_bps(max_bps)?;
    if premium_units > u128::from(principal.get()) * u128::from(max_bps) {
        Err(DomainError::InvalidAccounting)
    } else {
        Ok(())
    }
}

/// Check the exact pending-window units and the remaining locked-unit capacity.
///
/// Stored withdrawable principal may be expired; its original fixed units still
/// belong to the retained amount until a later ordinary top-up replaces it.
pub fn validate_allowance(
    principal: Amount,
    withdrawable: Amount,
    withdraw_until: i64,
    premium_units: u128,
    max_bps: u16,
    opens_at: i64,
    lock_at: i64,
) -> Result<(), DomainError> {
    validate_units(principal, premium_units, max_bps)?;
    if withdrawable > principal {
        return Err(DomainError::InvalidAccounting);
    }
    if withdrawable != Amount::ZERO {
        let rate = rate_bps(max_bps, withdraw_until.min(lock_at), opens_at, lock_at)?;
        let pending_units = units(withdrawable, rate)?;
        if premium_units < pending_units
            || premium_units - pending_units
                > u128::from(principal.get() - withdrawable.get()) * u128::from(max_bps)
        {
            return Err(DomainError::InvalidAccounting);
        }
    }
    Ok(())
}

/// Greatest possible raw weight for a given real principal and premium policy.
pub fn max_weight(principal: Amount, max_bps: u16) -> Result<Weight, DomainError> {
    validate_max_bps(max_bps)?;
    Ok(Weight::from(principal.get())
        * Weight::from(PREMIUM_SCALE + u128::from(max_bps))
        * Weight::from(accuracy::SCALE))
}

/// Multiply the floored baseline coefficient by exact real-plus-premium units.
///
/// No average-rate floor, per-contribution payout floor, or cancellation of the
/// shared basis-point scale occurs. Date aggregation therefore stays additive.
pub fn weighted_principal(
    algorithm: PoolAlgorithm,
    principal: Amount,
    premium_units: u128,
    max_bps: u16,
    error_days: u64,
) -> Result<Weight, DomainError> {
    validate_units(principal, premium_units, max_bps)?;
    let effective_units = u128::from(principal.get()) * PREMIUM_SCALE + premium_units;
    Ok(Weight::from(accuracy::coefficient(algorithm, error_days)) * Weight::from(effective_units))
}

/// Quote one owner/date gross from the frozen real principal and wide weight.
///
/// Necessary local capacity checks do not establish membership, completeness or
/// custody. The runtime must authenticate those before consuming a claim once.
pub fn quote_gross(
    algorithm: PoolAlgorithm,
    pool_principal: Amount,
    stake_principal: Amount,
    premium_units: u128,
    max_bps: u16,
    error_days: u64,
    total_weight: Weight,
) -> Result<Amount, DomainError> {
    if total_weight == Weight::ZERO {
        return Err(DomainError::ZeroDenominator);
    }
    if stake_principal > pool_principal {
        return Err(DomainError::InvalidAccounting);
    }
    let weight = weighted_principal(
        algorithm,
        stake_principal,
        premium_units,
        max_bps,
        error_days,
    )?;
    if weight > total_weight
        || total_weight - weight
            > max_weight(
                Amount::new(pool_principal.get() - stake_principal.get()),
                max_bps,
            )?
    {
        return Err(DomainError::InvalidAccounting);
    }
    let gross = Weight::from(pool_principal.get()) * weight / total_weight;
    u64::try_from(gross)
        .map(Amount::new)
        .map_err(|_| DomainError::ArithmeticOverflow)
}
