use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use manteon_core::types::Amount;

use crate::{
    custody::token::{transfer_exact, validate_owner_account, validate_vault},
    error::ManteonError,
    state::{Config, Pool, PoolState, Position, Stake as StakeAccount, StakeState},
    validation::admission::validate_forecast,
};

#[derive(Accounts)]
#[instruction(forecast_day: i64)]
pub struct Stake<'info> {
    // Signers.
    pub owner: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,

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
        init_if_needed,
        payer = payer,
        space = Position::DISCRIMINATOR.len() + Position::INIT_SPACE,
        seeds = [Position::SEED, pool.key().as_ref(), &forecast_day.to_le_bytes()],
        bump
    )]
    pub position: Account<'info, Position>,
    #[account(
        init_if_needed,
        payer = payer,
        space = StakeAccount::DISCRIMINATOR.len() + StakeAccount::INIT_SPACE,
        seeds = [StakeAccount::SEED, position.key().as_ref(), owner.key().as_ref()],
        bump
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
    pub source: Account<'info, TokenAccount>,
    /// CHECK: Only the canonical PDA address is used; no account data is read.
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
    pub system_program: Program<'info, System>,
}

pub(crate) fn handle(ctx: Context<Stake>, forecast_day: i64, amount: u64) -> Result<()> {
    require!(
        !ctx.accounts.pool.config_released && ctx.accounts.pool.state == PoolState::Active,
        ManteonError::InvalidPoolState
    );
    let now = Clock::get()?.unix_timestamp;
    ctx.accounts
        .pool
        .schedule()
        .validate_entry_time(now)
        .map_err(ManteonError::from)?;
    validate_forecast(&ctx.accounts.pool, forecast_day)?;

    let pool_key = ctx.accounts.pool.key();
    let position_key = ctx.accounts.position.key();
    let owner_key = ctx.accounts.owner.key();
    ctx.accounts
        .position
        .validate_deposit(pool_key, forecast_day)?;
    ctx.accounts
        .stake
        .validate_deposit(owner_key, position_key)?;
    require!(
        ctx.accounts.stake.principal <= ctx.accounts.position.principal,
        ManteonError::InvalidAccounting
    );
    let allowance = ctx
        .accounts
        .stake
        .allowance()?
        .quote_top_up(Amount::new(amount), now)
        .map_err(ManteonError::from)?;
    let book = ctx
        .accounts
        .pool
        .principal_book(&ctx.accounts.position)?
        .quote_add(Amount::new(amount))
        .map_err(ManteonError::from)?;

    validate_owner_account(
        &ctx.accounts.source,
        &owner_key,
        &ctx.accounts.collateral_mint.key(),
    )?;
    validate_vault(
        &ctx.accounts.vault,
        &ctx.accounts.collateral_mint.key(),
        &ctx.accounts.vault_authority.key(),
        ctx.accounts.pool.principal,
    )?;
    transfer_exact(
        &ctx.accounts.collateral_mint,
        &mut ctx.accounts.source,
        &mut ctx.accounts.vault,
        ctx.accounts.owner.to_account_info(),
        &[],
        amount,
    )?;

    ctx.accounts.pool.principal = book.pool_principal().get();
    ctx.accounts.pool.active_dates = book.active_dates();
    ctx.accounts.position.set_inner(Position {
        pool: pool_key,
        forecast_day,
        principal: book.position_principal().get(),
        computed: false,
    });
    ctx.accounts.stake.set_inner(StakeAccount {
        owner: owner_key,
        position: position_key,
        principal: allowance.principal().get(),
        withdrawable_principal: allowance.withdrawable_principal().get(),
        withdraw_until: allowance.withdraw_until(),
        state: StakeState::Active,
    });
    Ok(())
}
