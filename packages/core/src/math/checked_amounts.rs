//! Custody-backing calculations.

use crate::{DomainError, types::Amount};

/// Check that a known liability is backed, returning only the excess balance.
///
/// Excess may include unsolicited donations. This does not add it to principal
/// or grant anyone a right to withdraw it. It is not a fee or dust policy.
pub fn backing_excess(balance: Amount, liability: Amount) -> Result<Amount, DomainError> {
    balance
        .checked_sub(liability)
        .map_err(|_| DomainError::InvalidAccounting)
}
