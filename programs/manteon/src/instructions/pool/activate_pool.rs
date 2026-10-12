use anchor_lang::prelude::*;
use anchor_spl::token_interface::Mint;
use tekmerion_sdk::{cpi, program::program::Tekmerion};

use crate::{
    error::ManteonError,
    integration::tekmerion,
    state::{Config, Event, Pool, PoolState},
    validation::admission,
};

#[derive(Accounts)]
pub struct ActivatePool<'info> {
    // Signer.
    pub service_authority: Signer<'info>,

    // Market accounts.
    #[account(has_one = service_authority @ ManteonError::UnauthorizedServiceAuthority)]
    pub config: Account<'info, Config>,
    #[account(
        mut,
        has_one = config,
        constraint = pool.event_id == event.key()
    )]
    pub pool: Account<'info, Pool>,
    #[account(
        seeds = [Event::SEED, pool.key().as_ref()],
        bump,
        has_one = pool
    )]
    pub event: Account<'info, Event>,

    // Private asset.
    pub token_mint: InterfaceAccount<'info, Mint>,

    // Audit policy and CPI authority.
    /// CHECK: strict authenticated Tekmerion Policy decoder in handler.
    #[account(
        mut,
        address = pool.audit_config
    )]
    pub policy: UncheckedAccount<'info>,
    /// CHECK: canonical CPI signer; carries no data or custody rights.
    #[account(
        seeds = [tekmerion::CLIENT_SEED, policy.key().as_ref()],
        bump
    )]
    pub client_authority: UncheckedAccount<'info>,

    // Programs.
    pub tekmerion_program: Program<'info, Tekmerion>,
}

pub(crate) fn handle(ctx: Context<ActivatePool>) -> Result<()> {
    let pool = &ctx.accounts.pool;
    require!(
        !matches!(
            ctx.accounts.event.definition,
            crate::state::EventDefinition::TokenPriceThresholdCrossing { .. }
        ),
        ManteonError::InvalidPriceRule
    );
    require!(
        !pool.config_released && pool.state == PoolState::Draft,
        ManteonError::InvalidPoolState
    );
    pool.schedule()
        .validate_activation_time(Clock::get()?.unix_timestamp)
        .map_err(ManteonError::from)?;
    admission::validate_day(pool, manteon_core::math::utc_day::utc_day(pool.horizon_at))?;
    require!(
        ctx.accounts.event.definition.token_mint() == ctx.accounts.token_mint.key()
            && ctx.accounts.event.result.is_none(),
        ManteonError::InvalidEventMint
    );
    require!(
        pool.audit_config == ctx.accounts.config.audit_config
            && pool.collateral_mint == ctx.accounts.config.collateral_mint,
        ManteonError::InvalidAuditConfig
    );

    // Retain the authenticated Policy before enabling participation.
    let policy = tekmerion::policy(&ctx.accounts.policy)?;
    let policy_key = ctx.accounts.policy.key();
    let bump = [ctx.bumps.client_authority];
    let seeds = [tekmerion::CLIENT_SEED, policy_key.as_ref(), bump.as_ref()];
    let signer_seeds = [&seeds[..]];
    cpi::acquire_policy(CpiContext::new_with_signer(
        ctx.accounts.tekmerion_program.key(),
        cpi::accounts::AcquirePolicy {
            policy: ctx.accounts.policy.to_account_info(),
            client_authority: ctx.accounts.client_authority.to_account_info(),
            subject: pool.to_account_info(),
        },
        &signer_seeds,
    ))?;

    let acquired = tekmerion::policy(&ctx.accounts.policy)?;
    require!(
        Some(acquired.active_uses) == policy.active_uses.checked_add(1),
        ManteonError::InvalidAuditAccount
    );

    ctx.accounts.pool.state = PoolState::Active;
    Ok(())
}
