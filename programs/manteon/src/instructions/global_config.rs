//! Administrative settings, accepted authority transfers and platform fee withdrawals.

use anchor_lang::prelude::*;
use anchor_spl::token::{spl_token, Mint, Token, TokenAccount};
use manteon_core::math::fees::BASIS_POINTS;

use crate::{custody::token, error::ManteonError, state::GlobalConfig};

pub(crate) fn validate_settings(admin: Pubkey, fee_authority: Pubkey, share: u16) -> Result<()> {
    require!(
        admin != Pubkey::default() && fee_authority != Pubkey::default(),
        ManteonError::InvalidGlobalAuthority
    );
    require!(share <= BASIS_POINTS, ManteonError::InvalidFeeRates);
    Ok(())
}

#[derive(Accounts)]
pub struct ManageGlobalConfig<'info> {
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [GlobalConfig::SEED],
        bump,
        constraint = global_config.admin == authority.key() @ ManteonError::UnauthorizedGlobalAdmin
    )]
    pub global_config: Account<'info, GlobalConfig>,
}

pub(crate) fn update(
    ctx: Context<ManageGlobalConfig>,
    fee_authority: Pubkey,
    platform_share_bps: u16,
) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    validate_settings(config.admin, fee_authority, platform_share_bps)?;
    config.fee_authority = fee_authority;
    config.platform_share_bps = platform_share_bps;
    Ok(())
}

pub(crate) fn propose_admin(ctx: Context<ManageGlobalConfig>, new_admin: Pubkey) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    // Nominating the current admin clears a pending transfer without changing roles.
    validate_settings(new_admin, config.fee_authority, config.platform_share_bps)?;
    config.pending_admin = (new_admin != config.admin).then_some(new_admin);
    Ok(())
}

#[derive(Accounts)]
pub struct AcceptGlobalAdmin<'info> {
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [GlobalConfig::SEED],
        bump,
        constraint = global_config.pending_admin == Some(authority.key())
            @ ManteonError::UnauthorizedGlobalAdmin
    )]
    pub global_config: Account<'info, GlobalConfig>,
}

pub(crate) fn accept_admin(ctx: Context<AcceptGlobalAdmin>) -> Result<()> {
    let config = &mut ctx.accounts.global_config;
    config.admin = ctx.accounts.authority.key();
    config.pending_admin = None;
    Ok(())
}

#[derive(Accounts)]
pub struct WithdrawPlatformFees<'info> {
    pub authority: Signer<'info>,
    #[account(
        seeds = [GlobalConfig::SEED],
        bump,
        constraint = global_config.fee_authority == authority.key()
            @ ManteonError::UnauthorizedFeeAuthority
    )]
    pub global_config: Account<'info, GlobalConfig>,
    pub collateral_mint: Account<'info, Mint>,
    #[account(
        mut,
        seeds = [GlobalConfig::VAULT_SEED, collateral_mint.key().as_ref()],
        bump
    )]
    pub platform_fee_vault: Account<'info, TokenAccount>,
    #[account(mut)]
    pub destination: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

pub(crate) fn withdraw(ctx: Context<WithdrawPlatformFees>, amount: u64) -> Result<()> {
    require!(amount > 0, ManteonError::InvalidAmount);
    token::validate_vault(
        &ctx.accounts.platform_fee_vault,
        &ctx.accounts.collateral_mint.key(),
        &ctx.accounts.global_config.key(),
        amount,
    )?;
    require!(
        ctx.accounts.destination.mint == ctx.accounts.collateral_mint.key()
            && ctx.accounts.destination.state == spl_token::state::AccountState::Initialized
            && ctx.accounts.destination.is_native.is_none(),
        ManteonError::InvalidTokenAccount
    );
    let bump = [ctx.bumps.global_config];
    let seeds: &[&[u8]] = &[GlobalConfig::SEED, &bump];
    token::transfer_exact(
        &ctx.accounts.collateral_mint,
        &mut ctx.accounts.platform_fee_vault,
        &mut ctx.accounts.destination,
        ctx.accounts.global_config.to_account_info(),
        &[seeds],
        amount,
    )
}
