//! Permissionless configuration creation with bounded profit fees.

use anchor_lang::prelude::*;
use anchor_spl::token::{spl_token, Mint, Token, TokenAccount};

use crate::{
    error::ManteonError,
    state::{Config, GlobalConfig},
    validation::configuration,
};

/// Anyone may create an independent Config using the deployment's fee policy.
///
/// The author chooses the configuration; the payer funds account rent.
/// The author retains update authority; the Config keypair only signs creation.
#[derive(Accounts)]
pub struct CreateConfig<'info> {
    // Signers.
    /// Configuration author; no deployment authority is required.
    pub authority: Signer<'info>,
    /// Explicit storage payer, independent of platform and service roles.
    #[account(mut)]
    pub payer: Signer<'info>,

    // Configuration.
    /// Fresh configuration identity; Anchor rejects existing initialized accounts.
    #[account(
        init,
        payer = payer,
        space = Config::DISCRIMINATOR.len() + Config::INIT_SPACE
    )]
    pub config: Account<'info, Config>,

    // Canonical deployment policy, initialized once by the upgrade authority.
    #[account(
        seeds = [GlobalConfig::SEED],
        bump
    )]
    pub global_config: Account<'info, GlobalConfig>,

    // Prediction collateral and fee destination.
    /// Supported legacy SPL mint; native/wrapped SOL is outside this boundary.
    #[account(
        constraint = collateral_mint.key() != spl_token::native_mint::ID
            @ ManteonError::UnsupportedCollateral
    )]
    pub collateral_mint: Account<'info, Mint>,
    /// An initialized, unfrozen destination for this exact collateral mint.
    #[account(
        constraint = fee_recipient.mint == collateral_mint.key()
            @ ManteonError::InvalidFeeRecipient,
        constraint = fee_recipient.state == spl_token::state::AccountState::Initialized
            @ ManteonError::InvalidFeeRecipient
    )]
    pub fee_recipient: Account<'info, TokenAccount>,

    /// One shared collection vault per mint, controlled by GlobalConfig.
    /// A configuration author cannot choose its authority or withdraw fees.
    #[account(
        init_if_needed,
        payer = payer,
        seeds = [GlobalConfig::VAULT_SEED, collateral_mint.key().as_ref()],
        bump,
        token::mint = collateral_mint,
        token::authority = global_config
    )]
    pub platform_fee_vault: Account<'info, TokenAccount>,

    // Programs.
    /// Only the legacy SPL Token program is accepted by this instruction.
    pub token_program: Program<'info, Token>,
    /// System account creation through Anchor's checked initializer.
    pub system_program: Program<'info, System>,
}

/// Store validated creator choices without enabling a Pool or token transfers.
pub(crate) fn handle(
    ctx: Context<CreateConfig>,
    service_authority: Pubkey,
    audit_config: Pubkey,
    profit_fee_bps: u16,
    expected_platform_share_bps: u16,
) -> Result<()> {
    let settings = Config {
        authority: ctx.accounts.authority.key(),
        active_pools: 0,
        service_authority,
        collateral_mint: ctx.accounts.collateral_mint.key(),
        fee_recipient: ctx.accounts.fee_recipient.key(),
        profit_fee_bps,
        platform_share_bps: ctx.accounts.global_config.platform_share_bps,
        audit_config,
    };
    configuration::validate(
        &settings,
        expected_platform_share_bps,
        &ctx.accounts.platform_fee_vault,
        ctx.accounts.global_config.key(),
    )?;
    ctx.accounts.config.set_inner(settings);
    Ok(())
}
