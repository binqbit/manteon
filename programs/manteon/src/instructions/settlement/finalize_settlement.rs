use anchor_lang::prelude::*;

use crate::{
    error::ManteonError,
    state::{Config, Event, Pool, PoolState},
    validation::resolution,
};

#[derive(Accounts)]
pub struct FinalizeSettlement<'info> {
    // Signers.
    pub caller: Signer<'info>,

    // Protocol accounts.
    #[account(mut)]
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
    /// CHECK: authenticated terminal current Case verified by resolution adapter.
    pub case: Option<UncheckedAccount<'info>>,
}

pub(crate) fn handle(ctx: Context<FinalizeSettlement>) -> Result<()> {
    let pool = &mut ctx.accounts.pool;
    require!(
        !pool.config_released && pool.state == PoolState::Settling,
        ManteonError::InvalidPoolState
    );
    require!(
        ctx.remaining_accounts.is_empty(),
        ManteonError::InvalidBatch
    );
    resolution::authorize(
        ctx.accounts.caller.key(),
        &ctx.accounts.config,
        pool,
        &ctx.accounts.event,
        ctx.accounts.case.as_ref().map(|account| account.as_ref()),
        Clock::get()?.unix_timestamp,
    )?;

    let progress = pool.settlement_progress()?;
    if pool.principal != 0 {
        progress.validate_complete().map_err(ManteonError::from)?;
    }
    pool.state = PoolState::Claimable;
    pool.release_if_complete(&mut ctx.accounts.config)?;
    Ok(())
}
