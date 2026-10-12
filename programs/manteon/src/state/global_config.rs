//! Deployment governance and canonical platform fee collection addresses.

use anchor_lang::prelude::*;

/// Initialized once by the upgrade authority. Roles may be wallets or multisig
/// signing PDAs. Market configurations snapshot the share at creation.
#[account]
#[derive(InitSpace)]
pub struct GlobalConfig {
    /// Authority allowed to update deployment settings and nominate its successor.
    pub admin: Pubkey,
    /// A nominated successor must sign acceptance before gaining administrative rights.
    pub pending_admin: Option<Pubkey>,
    /// Authority allowed to withdraw collected platform fees, without pool access.
    pub fee_authority: Pubkey,
    /// Basis points of the configuration's collected fee, not an extra user charge.
    pub platform_share_bps: u16,
}

impl GlobalConfig {
    pub const SEED: &'static [u8] = b"global_config";
    pub const VAULT_SEED: &'static [u8] = b"platform_fee_vault";

    pub fn address() -> Pubkey {
        Pubkey::find_program_address(&[Self::SEED], &crate::ID).0
    }

    pub fn vault_address(mint: &Pubkey) -> Pubkey {
        Pubkey::find_program_address(&[Self::VAULT_SEED, mint.as_ref()], &crate::ID).0
    }
}
