//! Shared settings validation for configuration creation and unused updates.

use anchor_lang::prelude::*;
use anchor_spl::token::TokenAccount;
use manteon_core::math::fees::validate_fee_rates;

use crate::{custody::token, error::ManteonError, state::Config};

/// Account constraints retain each instruction's distinct creation/update authority.
pub(crate) fn validate(
    settings: &Config,
    expected_platform_share_bps: u16,
    platform_fee_vault: &Account<'_, TokenAccount>,
    global_config: Pubkey,
) -> Result<()> {
    require_eq!(
        settings.platform_share_bps,
        expected_platform_share_bps,
        ManteonError::FeeTermsChanged
    );
    validate_fee_rates(settings.profit_fee_bps, settings.platform_share_bps)
        .map_err(ManteonError::from)?;
    require_keys_neq!(
        settings.fee_recipient,
        Pubkey::default(),
        ManteonError::InvalidFeeRecipient
    );
    token::validate_vault(
        platform_fee_vault,
        &settings.collateral_mint,
        &global_config,
        0,
    )?;
    require_keys_neq!(
        settings.service_authority,
        Pubkey::default(),
        ManteonError::InvalidServiceAuthority
    );
    require_keys_neq!(
        settings.audit_config,
        Pubkey::default(),
        ManteonError::InvalidAuditConfig
    );
    Ok(())
}
