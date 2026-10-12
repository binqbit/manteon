//! Failures in the supplied account graph, separate from transaction errors.

/// An account snapshot cannot be interpreted as the expected Manteon Pool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadError {
    /// An account belongs to a different program.
    WrongOwner,
    /// Account bytes, size, discriminator or executable status are invalid.
    InvalidData,
    /// An Event or vault address is not the canonical PDA for this Pool.
    InvalidAddress,
    /// Account identities or stored relationships disagree.
    InvalidRelationship,
    /// Vault token state, mint or transfer authority is incompatible.
    InvalidVault,
    /// Pool state, schedule or recorded accounting is inconsistent.
    InvalidAccounting,
}

impl core::fmt::Display for ReadError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::WrongOwner => "account belongs to a different program",
            Self::InvalidData => "invalid account data",
            Self::InvalidAddress => "noncanonical account address",
            Self::InvalidRelationship => "inconsistent account relationships",
            Self::InvalidVault => "incompatible collateral vault",
            Self::InvalidAccounting => "inconsistent Pool state or accounting",
        })
    }
}

impl std::error::Error for ReadError {}
