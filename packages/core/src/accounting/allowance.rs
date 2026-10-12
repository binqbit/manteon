//! Personal allowance quotes without admission, ownership, or lifecycle authority.

use crate::{constants::WITHDRAWAL_WINDOW_SECONDS, error::DomainError, types::Amount};

/// A mathematical snapshot of one owner's principal and personal allowance.
///
/// This is not serialized Stake state. Constructing it authenticates neither an
/// owner nor a Clock, and does not establish that any deposit actually happened.
/// Pool OPEN/LOCK, freeze, custody, and account guards belong to the runtime
/// adapter and must be checked separately before applying any quote.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AllowanceSnapshot {
    principal: Amount,
    withdrawable_principal: Amount,
    withdraw_until: i64,
}

impl AllowanceSnapshot {
    /// Checks that the stored allowance does not exceed remaining principal.
    ///
    /// An expired stored allowance may be nonzero. A zero-principal snapshot is
    /// allowed for numerical inspection; it grants no reopening or reuse right.
    pub fn new(
        principal: Amount,
        withdrawable_principal: Amount,
        withdraw_until: i64,
    ) -> Result<Self, DomainError> {
        if withdrawable_principal.get() > principal.get() {
            return Err(DomainError::InvalidAccounting);
        }
        Ok(Self {
            principal,
            withdrawable_principal,
            withdraw_until,
        })
    }

    /// Returns remaining principal in token base units.
    pub const fn principal(self) -> Amount {
        self.principal
    }

    /// Returns stored allowance, which is unspendable after its deadline.
    pub const fn withdrawable_principal(self) -> Amount {
        self.withdrawable_principal
    }

    /// Returns the personal deadline in signed Unix seconds.
    pub const fn withdraw_until(self) -> i64 {
        self.withdraw_until
    }

    /// Quotes a positive top-up without changing this snapshot.
    ///
    /// Within a live window, only principal and allowance increase. After
    /// expiry, the new allowance covers the new money only for a fresh 48 hours.
    /// The deadline is exclusive. An empty active Stake receives a fresh window;
    /// the runtime must reject reuse of terminal entitlements.
    pub fn quote_top_up(self, amount: Amount, now: i64) -> Result<Self, DomainError> {
        require_positive(amount)?;
        if self.principal.get() == 0 {
            return quote_first_deposit(amount, now);
        }

        let principal = self.principal.checked_add(amount)?;
        let (withdrawable_principal, withdraw_until) = if now < self.withdraw_until {
            (
                self.withdrawable_principal.checked_add(amount)?,
                self.withdraw_until,
            )
        } else {
            (amount, new_deadline(now)?)
        };
        Self::new(principal, withdrawable_principal, withdraw_until)
    }

    /// Quotes a positive debit from the live personal allowance.
    ///
    /// This calculates amounts only. A quote reaching zero does not select the
    /// terminal Stake label, authorize a full-zero transition, or permit later
    /// reuse. Those decisions remain required before executing that transition.
    /// An expired window cannot debit even a nonzero stored allowance.
    pub fn quote_withdrawal(self, amount: Amount, now: i64) -> Result<Self, DomainError> {
        require_positive(amount)?;
        if now >= self.withdraw_until {
            return Err(DomainError::AllowanceExpired);
        }
        if amount.get() > self.withdrawable_principal.get() {
            return Err(DomainError::InsufficientAllowance);
        }
        Self::new(
            self.principal.checked_sub(amount)?,
            self.withdrawable_principal.checked_sub(amount)?,
            self.withdraw_until,
        )
    }
}

/// Quotes a first positive deposit with its full amount available for 48 hours.
///
/// This function cannot determine whether an account is new. Its caller must
/// establish first-deposit identity and may not use it to reset an existing
/// Stake or restore a consumed entitlement.
pub fn quote_first_deposit(amount: Amount, now: i64) -> Result<AllowanceSnapshot, DomainError> {
    require_positive(amount)?;
    AllowanceSnapshot::new(amount, amount, new_deadline(now)?)
}

fn require_positive(amount: Amount) -> Result<(), DomainError> {
    if amount.get() == 0 {
        return Err(DomainError::ZeroAmount);
    }
    Ok(())
}

fn new_deadline(now: i64) -> Result<i64, DomainError> {
    let deadline = i128::from(now) + i128::from(WITHDRAWAL_WINDOW_SECONDS);
    i64::try_from(deadline).map_err(|_| DomainError::ArithmeticOverflow)
}
