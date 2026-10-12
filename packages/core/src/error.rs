//! Semantic failures, without production instruction error codes.

/// Checked calculation error; not a numeric onchain ABI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainError {
    /// Profit fee exceeds its hard cap or the platform share exceeds the total fee.
    InvalidFeeRates,
    /// An addition or multiplication exceeded the calculation representation.
    ArithmeticOverflow,
    /// A subtraction would produce a negative amount or count.
    ArithmeticUnderflow,
    /// A proportional calculation requires a positive total weight.
    ZeroDenominator,
    /// The operation requires positive principal.
    ZeroAmount,
    /// The schedule does not satisfy its strict creation ordering.
    InvalidSchedule,
    /// Entry is not yet open.
    BeforeOpen,
    /// Clock has reached the global participation cutoff.
    Locked,
    /// Activation must precede the committed opening time.
    ActivationTooLate,
    /// A forecast's UTC day starts before LOCK.
    InvalidForecastDay,
    /// Horizon exit is not yet temporally eligible.
    BeforeHorizon,
    /// Local ledger amounts or positive-date counts disagree.
    InvalidAccounting,
    /// This Position has already contributed to the settlement totals.
    PositionAlreadyComputed,
    /// Some positive-principal dates remain unprocessed.
    IncompleteSettlement,
    /// The stored personal allowance has expired.
    AllowanceExpired,
    /// The requested withdrawal exceeds the live allowance.
    InsufficientAllowance,
}
