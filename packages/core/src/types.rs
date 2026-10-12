//! Shared calculation types; these have no account serialization contract.

use crate::error::DomainError;

/// Nonnegative token base units in the calculation library.
///
/// The `u64` representation matches a single SPL balance. It does not adopt
/// a production pool cap, mint, decimals, fixed-point format or wire ABI.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Amount(u64);

impl Amount {
    /// Zero token base units.
    pub const ZERO: Self = Self(0);

    /// Construct an amount without applying token or protocol eligibility.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the exact integer token base units.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Add amounts without wrapping or saturating financial values.
    pub fn checked_add(self, other: Self) -> Result<Self, DomainError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(DomainError::ArithmeticOverflow)
    }

    /// Subtract an amount without creating negative principal.
    pub fn checked_sub(self, other: Self) -> Result<Self, DomainError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(DomainError::ArithmeticUnderflow)
    }
}

/// Immutable accuracy curve selected when a Pool is created.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PoolAlgorithm {
    /// Thirty-day half-life, the default for longer-term events.
    #[default]
    H30,
    /// Fifteen-day half-life for medium-term events.
    H15,
    /// Seven-day half-life for short events requiring precise dates.
    H7,
}

impl PoolAlgorithm {
    /// Additional UTC days of absolute error that halve ideal accuracy weight.
    pub const fn half_life_days(self) -> u64 {
        match self {
            Self::H30 => 30,
            Self::H15 => 15,
            Self::H7 => 7,
        }
    }

    /// Largest error whose floored Q64.64 coefficient remains nonzero.
    pub const fn max_error_days(self) -> u64 {
        self.half_life_days() * 64
    }
}
