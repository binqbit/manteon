use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use manteon_core::{math::premium, types::Amount, DomainError};

use crate::{
    custody::token::{transfer_exact, validate_owner_account, validate_vault},
    error::ManteonError,
    state::{Config, Pool, PoolState, Position, Stake as StakeAccount, StakeState},
};

#[derive(Accounts)]
pub struct Unstake<'info> {
    // Signers.
    pub owner: Signer<'info>,

    // Protocol accounts.
    #[account(has_one = collateral_mint @ ManteonError::InvalidCollateralMint)]
    pub config: Account<'info, Config>,
    #[account(
        mut,
        has_one = config,
        has_one = collateral_mint @ ManteonError::InvalidCollateralMint,
        constraint = pool.audit_config == config.audit_config @ ManteonError::InvalidAuditConfig
    )]
    pub pool: Box<Account<'info, Pool>>,
    #[account(
        mut,
        seeds = [Position::SEED, pool.key().as_ref(), &position.forecast_day.to_le_bytes()],
        bump,
        has_one = pool @ ManteonError::InvalidPosition,
        constraint = !position.computed @ ManteonError::InvalidPosition
    )]
    pub position: Account<'info, Position>,
    #[account(
        mut,
        seeds = [StakeAccount::SEED, position.key().as_ref(), owner.key().as_ref()],
        bump,
        has_one = owner @ ManteonError::InvalidStake,
        has_one = position @ ManteonError::InvalidStake,
        constraint = stake.state == StakeState::Active @ ManteonError::InvalidStake
    )]
    pub stake: Account<'info, StakeAccount>,

    // Token custody.
    pub collateral_mint: Account<'info, Mint>,
    #[account(
        mut,
        token::mint = collateral_mint,
        token::authority = owner,
        token::token_program = token_program
    )]
    pub destination: Account<'info, TokenAccount>,
    /// CHECK: Canonical transfer-authority PDA; only its address is used.
    #[account(
        seeds = [Pool::VAULT_AUTHORITY_SEED, pool.key().as_ref()],
        bump
    )]
    pub vault_authority: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [Pool::VAULT_SEED, pool.key().as_ref()],
        bump,
        token::mint = collateral_mint,
        token::authority = vault_authority,
        token::token_program = token_program
    )]
    pub vault: Account<'info, TokenAccount>,

    // Programs.
    pub token_program: Program<'info, Token>,
}

pub(crate) fn handle(ctx: Context<Unstake>, amount: u64) -> Result<()> {
    require!(
        !ctx.accounts.pool.config_released && ctx.accounts.pool.state == PoolState::Active,
        ManteonError::InvalidPoolState
    );
    let now = Clock::get()?.unix_timestamp;
    if now >= ctx.accounts.pool.lock_at {
        return Err(ManteonError::from(DomainError::Locked).into());
    }
    require!(
        ctx.accounts.stake.principal <= ctx.accounts.position.principal
            && ctx.accounts.stake.premium_units <= ctx.accounts.position.premium_units,
        ManteonError::InvalidAccounting
    );
    let allowance = ctx
        .accounts
        .stake
        .allowance()?
        .quote_withdrawal(Amount::new(amount), now)
        .map_err(ManteonError::from)?;
    ctx.accounts.stake.validate_premium(&ctx.accounts.pool)?;
    let book = ctx
        .accounts
        .pool
        .principal_book(&ctx.accounts.position)?
        .quote_remove(Amount::new(amount))
        .map_err(ManteonError::from)?;
    let rate = premium::rate_bps(
        ctx.accounts.pool.early_premium_bps,
        ctx.accounts
            .stake
            .withdraw_until
            .min(ctx.accounts.pool.lock_at),
        ctx.accounts.pool.opens_at,
        ctx.accounts.pool.lock_at,
    )
    .map_err(ManteonError::from)?;
    let removed_units = premium::units(Amount::new(amount), rate).map_err(ManteonError::from)?;
    let stake_units = ctx
        .accounts
        .stake
        .premium_units
        .checked_sub(removed_units)
        .ok_or(ManteonError::InvalidAccounting)?;
    let position_units = ctx
        .accounts
        .position
        .premium_units
        .checked_sub(removed_units)
        .ok_or(ManteonError::InvalidAccounting)?;
    premium::validate_units(
        allowance.principal(),
        stake_units,
        ctx.accounts.pool.early_premium_bps,
    )
    .map_err(ManteonError::from)?;
    premium::validate_allowance(
        allowance.principal(),
        allowance.withdrawable_principal(),
        allowance.withdraw_until(),
        stake_units,
        ctx.accounts.pool.early_premium_bps,
        ctx.accounts.pool.opens_at,
        ctx.accounts.pool.lock_at,
    )
    .map_err(ManteonError::from)?;
    premium::validate_units(
        book.position_principal(),
        position_units,
        ctx.accounts.pool.early_premium_bps,
    )
    .map_err(ManteonError::from)?;

    validate_owner_account(
        &ctx.accounts.destination,
        &ctx.accounts.owner.key(),
        &ctx.accounts.collateral_mint.key(),
    )?;
    validate_vault(
        &ctx.accounts.vault,
        &ctx.accounts.collateral_mint.key(),
        &ctx.accounts.vault_authority.key(),
        ctx.accounts.pool.principal,
    )?;
    let pool_key = ctx.accounts.pool.key();
    let bump = [ctx.bumps.vault_authority];
    let authority_seeds: &[&[u8]] = &[Pool::VAULT_AUTHORITY_SEED, pool_key.as_ref(), &bump];
    transfer_exact(
        &ctx.accounts.collateral_mint,
        &mut ctx.accounts.vault,
        &mut ctx.accounts.destination,
        ctx.accounts.vault_authority.to_account_info(),
        &[authority_seeds],
        amount,
    )?;

    ctx.accounts.pool.principal = book.pool_principal().get();
    ctx.accounts.pool.active_dates = book.active_dates();
    ctx.accounts.position.principal = book.position_principal().get();
    ctx.accounts.position.premium_units = position_units;
    ctx.accounts.stake.principal = allowance.principal().get();
    ctx.accounts.stake.premium_units = stake_units;
    ctx.accounts.stake.withdrawable_principal = allowance.withdrawable_principal().get();
    Ok(())
}
