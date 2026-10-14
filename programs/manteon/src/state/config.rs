//! Reusable market settings, locked while any Pool retains them.

use anchor_lang::prelude::*;

/// Creator-selected identities and bounded fees; no discretionary principal access.
///
/// Only the creator may update unused settings. Drafts retain them at creation;
/// each Pool releases its dependency exactly once after completing its lifecycle.
#[account]
#[derive(InitSpace)]
pub struct Config {
    /// Creator authorized to update settings when active_pools is zero.
    pub authority: Pubkey,
    /// Pools that have not yet released their dependency, including Drafts.
    pub active_pools: u64,
    // Service authorization.
    /// Creator-selected service executor for instructions that authorize it.
    pub service_authority: Pubkey,

    // Prediction collateral and configured fee destination.
    /// Initialized legacy SPL mint selected for prediction collateral.
    pub collateral_mint: Pubkey,
    /// Authority (wallet or PDA) owning accounts that receive the fee share.
    /// A destination may be created atomically with claim; it need not exist now.
    pub fee_recipient: Pubkey,
    /// Fee on positive gross profit, at most MAX_PROFIT_FEE_BPS.
    pub profit_fee_bps: u16,
    /// Deployment share copied when settings are saved; locked while in use.
    pub platform_share_bps: u16,

    // External audit dependency.
    /// Tekmerion Policy address, authenticated by the adapter before activation or use.
    pub audit_config: Pubkey,
}

impl Config {
    /// Bind reviewed settings without invalidating concurrent Pool reservations.
    pub fn terms_hash(&self) -> [u8; 32] {
        solana_sha256_hasher::hashv(&[
            self.authority.as_ref(),
            self.service_authority.as_ref(),
            self.collateral_mint.as_ref(),
            self.fee_recipient.as_ref(),
            &self.profit_fee_bps.to_le_bytes(),
            &self.platform_share_bps.to_le_bytes(),
            self.audit_config.as_ref(),
        ])
        .to_bytes()
    }

    pub(crate) fn retain(&mut self) -> Result<()> {
        self.active_pools = self
            .active_pools
            .checked_add(1)
            .ok_or(crate::error::ManteonError::InvalidAccounting)?;
        Ok(())
    }

    pub(crate) fn release(&mut self) -> Result<()> {
        self.active_pools = self
            .active_pools
            .checked_sub(1)
            .ok_or(crate::error::ManteonError::InvalidAccounting)?;
        Ok(())
    }
}
