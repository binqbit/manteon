//! Checked residual-principal and positive-date calculations.

use crate::{error::DomainError, types::Amount};

/// A local mathematical view of a Pool and one of its Positions before freeze.
///
/// The constructor checks necessary consistency between these aggregates; it
/// cannot prove the sum of all Positions, authentic membership, token backing,
/// or whether mutation is permitted. Amount/count widths are calculation
/// representations, not selected account encoding or production capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrincipalBook {
    pool_principal: Amount,
    position_principal: Amount,
    active_dates: u64,
}

impl PrincipalBook {
    /// Builds a snapshot after checking local aggregate/count consistency.
    ///
    /// Every positive date requires at least one token atom. Residual Pool
    /// principal outside this Position must have at least one other active date,
    /// and no other positive date is possible without such residual principal.
    /// These necessary conditions do not prove complete membership discovery.
    pub fn new(
        pool_principal: Amount,
        position_principal: Amount,
        active_dates: u64,
    ) -> Result<Self, DomainError> {
        if position_principal.get() > pool_principal.get() {
            return Err(DomainError::InvalidAccounting);
        }
        let this_date = u64::from(position_principal.get() > 0);
        let other_dates = active_dates
            .checked_sub(this_date)
            .ok_or(DomainError::InvalidAccounting)?;
        let other_principal = pool_principal.get() - position_principal.get();
        if (other_principal == 0) != (other_dates == 0) || other_dates > other_principal {
            return Err(DomainError::InvalidAccounting);
        }
        Ok(Self {
            pool_principal,
            position_principal,
            active_dates,
        })
    }

    /// Returns Pool principal before settlement freeze.
    pub const fn pool_principal(self) -> Amount {
        self.pool_principal
    }

    /// Returns the principal aggregated at this forecast date.
    pub const fn position_principal(self) -> Amount {
        self.position_principal
    }

    /// Returns the number of positive-principal dates in this snapshot.
    pub const fn active_dates(self) -> u64 {
        self.active_dates
    }

    /// Quotes a positive credit and its possible zero-to-positive date crossing.
    ///
    /// The existing value never changes, including on overflow. This does not
    /// establish actual token credit, owner allowance, or permission to reopen
    /// a zero-principal Stake; several Stakes may share this Position.
    pub fn quote_add(self, amount: Amount) -> Result<Self, DomainError> {
        if amount.get() == 0 {
            return Err(DomainError::ZeroAmount);
        }
        let pool_principal = self.pool_principal.checked_add(amount)?;
        let position_principal = self.position_principal.checked_add(amount)?;
        let active_dates = if self.position_principal.get() == 0 {
            self.active_dates
                .checked_add(1)
                .ok_or(DomainError::ArithmeticOverflow)?
        } else {
            self.active_dates
        };
        Self::new(pool_principal, position_principal, active_dates)
    }

    /// Quotes a positive debit and its possible positive-to-zero date crossing.
    ///
    /// This is residual arithmetic for separately authorized pre-freeze owner
    /// actions. It neither authenticates the owner's amount nor creates an exit,
    /// refund, terminal Stake transition, or a post-freeze principal mutation.
    pub fn quote_remove(self, amount: Amount) -> Result<Self, DomainError> {
        if amount.get() == 0 {
            return Err(DomainError::ZeroAmount);
        }
        let position_principal = self.position_principal.checked_sub(amount)?;
        let pool_principal = self.pool_principal.checked_sub(amount)?;
        let active_dates = if position_principal.get() == 0 {
            self.active_dates
                .checked_sub(1)
                .ok_or(DomainError::ArithmeticUnderflow)?
        } else {
            self.active_dates
        };
        Self::new(pool_principal, position_principal, active_dates)
    }
}
