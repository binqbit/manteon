//! Stable instruction errors and boundary validation failures.

use anchor_lang::prelude::*;

/// Instruction and boundary validation failures.
#[error_code]
pub enum ManteonError {
    /// Only this deployment's current upgrade authority can initialize GlobalConfig.
    #[msg("GlobalConfig initialization requires the deployment upgrade authority")]
    UnauthorizedPlatform,
    /// Supplied deployment metadata does not belong to this program.
    #[msg("ProgramData must belong to this Manteon deployment")]
    InvalidProgramData,
    /// The platform must select a nonzero service identity.
    #[msg("Service authority must not be the default public key")]
    InvalidServiceAuthority,
    /// The intended Policy reference must not be the default public key.
    #[msg("Audit configuration reference must not be the default public key")]
    InvalidAuditConfig,
    /// The selected recipient must be initialized, unfrozen and use the mint.
    #[msg("Fee recipient must be an initialized, unfrozen account of the collateral mint")]
    InvalidFeeRecipient,
    /// Native/wrapped SOL has no approved collateral support.
    #[msg("Native or wrapped SOL is not supported collateral")]
    UnsupportedCollateral,
    #[msg("Pool creation requires the configured service authority")]
    UnauthorizedServiceAuthority,
    #[msg("Pool schedule must satisfy now < opens_at < lock_at <= horizon_at")]
    InvalidPoolSchedule,
    #[msg("Event mint must not be the default public key")]
    InvalidEventMint,
    #[msg("Collateral mint must match the configured mint")]
    InvalidCollateralMint,
    #[msg("Operation is not allowed in this Pool state")]
    InvalidPoolState,
    #[msg("Position identity or membership is invalid")]
    InvalidPosition,
    #[msg("Stake identity or entitlement is invalid")]
    InvalidStake,
    #[msg("Token account must belong to the owner and use the collateral mint")]
    InvalidTokenAccount,
    #[msg("Vault custody configuration is invalid")]
    InvalidVault,
    #[msg("Vault balance is below recorded liabilities")]
    InsufficientBacking,
    #[msg("Token transfer did not produce the exact expected balance changes")]
    UnexpectedTokenBalance,
    #[msg("Recorded principal, allowance or settlement accounting is invalid")]
    InvalidAccounting,
    #[msg("Operation is outside its permitted time window")]
    InvalidTime,
    #[msg("Date is outside the supported Q64.64 settlement domain")]
    UnsupportedDate,
    #[msg("Checked arithmetic failed")]
    Arithmetic,
    #[msg("Amount must be positive and within the available allowance")]
    InvalidAmount,
    #[msg("Authenticated Tekmerion Policy or Case is invalid")]
    InvalidAuditAccount,
    #[msg("Proposed event result does not match this Pool or its valid time domain")]
    InvalidEventResult,
    #[msg("Only the service authority may perform this operation before public fallback")]
    ServiceWindow,
    #[msg("Settlement requires complete unique positive-date coverage")]
    IncompleteSettlement,
    #[msg("Batch must contain between one and eight distinct Positions")]
    InvalidBatch,
    #[msg("Agreement hash must not be zero")]
    InvalidAgreement,
    #[msg("Invalid immutable price threshold rule")]
    InvalidPriceRule,
    #[msg("Price source identity, account layout or liquidity is invalid")]
    InvalidPriceSource,
    #[msg("Price source is stale, future dated or insufficiently verified")]
    StalePriceSource,
    #[msg("The price averaging interval is not ready or monitoring has expired")]
    InvalidPriceObservation,
    #[msg("The verified USD price has not crossed the committed threshold")]
    PriceThresholdNotCrossed,
    #[msg("Profit fee must not exceed 20%; platform share must not exceed 100% of that fee")]
    InvalidFeeRates,
    #[msg("Only the GlobalConfig admin may change deployment settings")]
    UnauthorizedGlobalAdmin,
    #[msg("Only the configured fee authority may withdraw platform fees")]
    UnauthorizedFeeAuthority,
    #[msg("Global authorities must not be the default public key")]
    InvalidGlobalAuthority,
    #[msg("The platform share changed; review the configuration terms again")]
    FeeTermsChanged,
    #[msg("Only the configuration creator may update its settings")]
    UnauthorizedConfigAuthority,
    #[msg("Configuration is retained by unfinished pools")]
    ConfigInUse,
    #[msg("The pool has not completed all participant obligations")]
    PoolNotComplete,
    #[msg("Configuration settings changed; review the pool terms again")]
    ConfigTermsChanged,
}

impl From<manteon_core::DomainError> for ManteonError {
    fn from(value: manteon_core::DomainError) -> Self {
        use manteon_core::DomainError::*;
        match value {
            InvalidFeeRates => Self::InvalidFeeRates,
            ArithmeticOverflow | ArithmeticUnderflow => Self::Arithmetic,
            ZeroAmount | InsufficientAllowance => Self::InvalidAmount,
            InvalidSchedule => Self::InvalidPoolSchedule,
            InvalidForecastDay => Self::UnsupportedDate,
            IncompleteSettlement | ZeroDenominator => Self::IncompleteSettlement,
            InvalidAccounting | PositionAlreadyComputed => Self::InvalidAccounting,
            BeforeOpen | Locked | ActivationTooLate | BeforeHorizon | AllowanceExpired => {
                Self::InvalidTime
            }
        }
    }
}
