//! Gross accuracy allocation from a frozen principal and weight basis.

use ethnum::U256;

use crate::{
    error::DomainError,
    math::accuracy,
    types::{Amount, PoolAlgorithm},
};

/// Quote floor(pool_principal * stake_weight / total_weight) in token atoms.
///
/// Derives the Stake's weight using the coefficient-first Q64.64 accuracy rule.
/// Both Pool principal and total weight must stay fixed across all quotes;
/// gross already includes any returned principal. No fee is deducted here.
///
/// Checks necessary local principal/weight bounds, not complete settlement,
/// authentic membership or custody. A zero-weight share can quote zero when
/// the denominator is positive. This calculation does not authorize a claim,
/// select a date domain or assign rounding residue.
pub fn quote_gross_payout(
    algorithm: PoolAlgorithm,
    pool_principal: Amount,
    stake_principal: Amount,
    error_days: u64,
    total_weight: u128,
) -> Result<Amount, DomainError> {
    if total_weight == 0 {
        return Err(DomainError::ZeroDenominator);
    }
    if stake_principal > pool_principal {
        return Err(DomainError::InvalidAccounting);
    }
    let stake_weight = accuracy::weighted_principal(algorithm, stake_principal, error_days);
    if stake_weight > total_weight {
        return Err(DomainError::InvalidAccounting);
    }
    let other_principal = pool_principal.get() - stake_principal.get();
    // Each remaining token atom contributes at most SCALE; this fits u128.
    if total_weight - stake_weight > u128::from(other_principal) * accuracy::SCALE {
        return Err(DomainError::InvalidAccounting);
    }

    // The exact u64 * u128 numerator needs at most 192 bits. Divide only once;
    // cancelling the Q64.64 scale beforehand would discard fractional weight.
    let numerator = U256::from(pool_principal.get()) * U256::from(stake_weight);
    let gross = numerator / U256::from(total_weight);
    // stake_weight <= total_weight also proves gross <= pool_principal.
    u64::try_from(gross)
        .map(Amount::new)
        .map_err(|_| DomainError::ArithmeticOverflow)
}
