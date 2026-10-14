use anchor_lang::prelude::*;
use manteon_core::{
    accounting::{principal::PrincipalBook, settlement::SettlementProgress},
    types::Amount,
    validation::time::Schedule,
};

use crate::{
    error::ManteonError,
    state::{Config, Position},
};

/// A scheduled market and its audit, participation and settlement accounting.
#[account]
#[derive(InitSpace)]
pub struct Pool {
    // Identity and retained configuration.
    pub config: Pubkey,
    pub event_id: Pubkey,
    pub collateral_mint: Pubkey,
    pub audit_config: Pubkey,
    /// Fee recipient authority; retained even after the Config is reused.
    pub fee_recipient: Pubkey,
    pub profit_fee_bps: u16,
    pub platform_share_bps: u16,
    /// Prevents repeated release or reactivation of a completed Pool.
    pub config_released: bool,
    /// SHA-256 of the immutable canonical agreement document.
    pub agreement_hash: [u8; 32],
    /// Immutable payout accuracy curve.
    pub algorithm: PoolAlgorithm,

    // Immutable entry and horizon schedule.
    pub opens_at: i64,
    pub lock_at: i64,
    pub horizon_at: i64,

    // Lifecycle and current audit attempt.
    pub state: PoolState,
    pub next_case_nonce: u64,
    pub current_case: Option<Pubkey>,

    // Participation basis, frozen for competitive settlement.
    /// Participating principal; becomes the immutable basis at settlement freeze.
    pub principal: u64,
    pub active_dates: u64,

    // Settlement progress and outstanding payout liability.
    pub processed_dates: u64,
    /// Unsigned Q64.64 accumulator: 64 integer bits and 64 fractional bits.
    pub total_weight: u128,
    /// Unpaid allocations; after completion, any remainder is rounding dust.
    pub payout_remaining: u64,
    /// Principal of consumed competitive Stakes, including zero-value payouts.
    pub claimed_principal: u64,

    // Retained candidate-submission fallback anchor.
    /// Clock time of the first successfully funded proposal.
    /// Preserves public submission eligibility across candidate changes/retries.
    /// Settlement service eligibility is derived separately from audit finality.
    pub service_eligible_at: Option<i64>,
}

impl Pool {
    pub(crate) fn release_config(&mut self, config: &mut Config) -> Result<()> {
        require!(!self.config_released, ManteonError::InvalidPoolState);
        config.release()?;
        self.config_released = true;
        Ok(())
    }

    pub(crate) fn release_if_complete(&mut self, config: &mut Config) -> Result<()> {
        if !self.config_released
            && ((self.state == PoolState::Claimable && self.claimed_principal == self.principal)
                || (self.state == PoolState::Refunding && self.principal == 0))
        {
            self.release_config(config)?;
        }
        Ok(())
    }

    /// One prediction token vault per Pool; its address is not stored redundantly.
    pub const VAULT_SEED: &'static [u8] = b"vault";
    /// Separate transfer authority, never used as the auditor client signer.
    pub const VAULT_AUTHORITY_SEED: &'static [u8] = b"vault_authority";

    pub(crate) fn schedule(&self) -> Schedule {
        Schedule {
            opens_at: self.opens_at,
            lock_at: self.lock_at,
            horizon_at: self.horizon_at,
        }
    }

    /// Quote bookkeeping after the instruction authenticates Position membership.
    pub(crate) fn principal_book(&self, position: &Position) -> Result<PrincipalBook> {
        Ok(PrincipalBook::new(
            Amount::new(self.principal),
            Amount::new(position.principal),
            self.active_dates,
        )
        .map_err(ManteonError::from)?)
    }

    /// Read checked progress without granting freeze or processing authority.
    pub(crate) fn settlement_progress(&self) -> Result<SettlementProgress> {
        Ok(SettlementProgress::new(
            self.algorithm.into(),
            Amount::new(self.principal),
            self.active_dates,
            self.processed_dates,
            self.total_weight,
        )
        .map_err(ManteonError::from)?)
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoolState {
    /// Created but not activated for participation.
    Draft,
    /// Participation and, after horizon, owner exits remain possible.
    Active,
    /// Accepted competitive event; principal and date membership are frozen.
    Settling,
    /// Every positive Position has contributed to the immutable denominator.
    Claimable,
    /// Accepted pre-LOCK event; owners can reclaim remaining principal.
    Refunding,
}

/// Pool-specific accuracy half-life. Variant indices are part of the wire ABI.
#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoolAlgorithm {
    H30,
    H15,
    H7,
}

impl From<PoolAlgorithm> for manteon_core::types::PoolAlgorithm {
    fn from(value: PoolAlgorithm) -> Self {
        match value {
            PoolAlgorithm::H30 => Self::H30,
            PoolAlgorithm::H15 => Self::H15,
            PoolAlgorithm::H7 => Self::H7,
        }
    }
}

impl PoolAlgorithm {
    /// Upper bound that keeps every admissible pair's Q64.64 weight nonzero.
    pub fn max_error_days(self) -> i64 {
        let algorithm: manteon_core::types::PoolAlgorithm = self.into();
        algorithm.max_error_days() as i64
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::H30 => "H30",
            Self::H15 => "H15",
            Self::H7 => "H7",
        }
    }
}
