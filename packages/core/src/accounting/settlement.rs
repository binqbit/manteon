//! Single-pass weight accumulation, separate from freeze and claim authority.

use crate::{
    accounting::principal::PrincipalBook,
    error::DomainError,
    math::accuracy,
    types::{Amount, PoolAlgorithm},
};

/// Local arithmetic view of the frozen Pool basis and processing totals.
///
/// The caller must authenticate frozen membership, each Position and its marker,
/// and the accepted result used to derive day errors. Local count/weight bounds do
/// not prove complete coverage. Constructing this value grants no reset or freeze
/// authority and adds no fields to the onchain Pool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettlementProgress {
    algorithm: PoolAlgorithm,
    pool_principal: Amount,
    active_dates: u64,
    processed_dates: u64,
    total_weight: u128,
}

impl SettlementProgress {
    /// Read a snapshot after checking necessary local count and weight bounds.
    ///
    /// Every unprocessed positive date reserves at least one principal atom.
    /// Processed dates can contribute zero weight after coefficient rounding.
    pub fn new(
        algorithm: PoolAlgorithm,
        pool_principal: Amount,
        active_dates: u64,
        processed_dates: u64,
        total_weight: u128,
    ) -> Result<Self, DomainError> {
        let principal = pool_principal.get();
        if (principal == 0) != (active_dates == 0)
            || active_dates > principal
            || processed_dates > active_dates
        {
            return Err(DomainError::InvalidAccounting);
        }
        let unprocessed_dates = active_dates - processed_dates;
        let processed_capacity = u128::from(principal - unprocessed_dates) * accuracy::SCALE;
        if (processed_dates == 0 && total_weight != 0) || total_weight > processed_capacity {
            return Err(DomainError::InvalidAccounting);
        }
        Ok(Self {
            algorithm,
            pool_principal,
            active_dates,
            processed_dates,
            total_weight,
        })
    }

    /// Number of positive-principal dates already included, even at zero weight.
    pub const fn processed_dates(&self) -> u64 {
        self.processed_dates
    }

    /// Accumulated coefficient-first Q64.64 weight.
    pub const fn total_weight(&self) -> u128 {
        self.total_weight
    }

    /// Add one positive Position and set its supplied marker only on success.
    ///
    /// Any error leaves both inputs unchanged. Reusing a true marker fails;
    /// the runtime must ensure it is the canonical Position's actual marker.
    /// This operation neither finalizes settlement nor changes frozen principal.
    pub fn process_position(
        &mut self,
        principal: Amount,
        error_days: u64,
        computed: &mut bool,
    ) -> Result<(), DomainError> {
        if *computed {
            return Err(DomainError::PositionAlreadyComputed);
        }
        if principal == Amount::ZERO {
            return Err(DomainError::ZeroAmount);
        }
        if self.processed_dates == self.active_dates {
            return Err(DomainError::InvalidAccounting);
        }
        PrincipalBook::new(self.pool_principal, principal, self.active_dates)?;

        let remaining_dates = self.active_dates - self.processed_dates - 1;
        let previous_capacity = self.pool_principal.get() - principal.get() - remaining_dates;
        // Reserve this Position's full principal, even if its weight is zero,
        // plus one atom for every other unprocessed positive date.
        if self.total_weight > u128::from(previous_capacity) * accuracy::SCALE {
            return Err(DomainError::InvalidAccounting);
        }
        let total_weight = self
            .total_weight
            .checked_add(accuracy::weighted_principal(
                self.algorithm,
                principal,
                error_days,
            ))
            .ok_or(DomainError::ArithmeticOverflow)?;
        let next = Self::new(
            self.algorithm,
            self.pool_principal,
            self.active_dates,
            self.processed_dates + 1,
            total_weight,
        )?;
        *self = next;
        *computed = true;
        Ok(())
    }

    /// Require full counted coverage and a positive payout denominator.
    ///
    /// This read-only check does not open claims or prove custody and membership.
    /// Empty snapshots have no payout denominator; their lifecycle stays separate.
    pub fn validate_complete(&self) -> Result<(), DomainError> {
        if self.processed_dates != self.active_dates {
            return Err(DomainError::IncompleteSettlement);
        }
        if self.total_weight == 0 {
            return Err(DomainError::ZeroDenominator);
        }
        Ok(())
    }
}
