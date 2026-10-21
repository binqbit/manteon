use anchor_lang::prelude::*;
use anchor_spl::token::TokenAccount;
use manteon_core::math::utc_day::utc_day;

use crate::{
    custody::token,
    error::ManteonError,
    state::{Config, Event, EventDefinition, EventResult, Pool, PoolState},
    validation::{admission, resolution},
};

#[derive(Accounts)]
pub struct BeginSettlement<'info> {
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
        mut,
        seeds = [Event::SEED, pool.key().as_ref()],
        bump,
        has_one = pool
    )]
    pub event: Account<'info, Event>,
    /// CHECK: fixed owner, schema, PDA and candidate binding checked by adapter.
    pub case: UncheckedAccount<'info>,
    // Token custody.
    #[account(
        seeds = [Pool::VAULT_SEED, pool.key().as_ref()],
        bump
    )]
    pub vault: Account<'info, TokenAccount>,
}

pub(crate) fn handle(ctx: Context<BeginSettlement>, data: EventResult) -> Result<()> {
    let pool = &mut ctx.accounts.pool;
    require!(
        !pool.config_released && pool.state == PoolState::Active && pool.accepted_case.is_none(),
        ManteonError::InvalidPoolState
    );
    require!(
        matches!(
            ctx.accounts.event.definition,
            EventDefinition::ExternalEvent
        ) && ctx.accounts.event.result.is_none(),
        ManteonError::InvalidEventResult
    );
    resolution::accepted(
        pool.key(),
        pool,
        &ctx.accounts.event,
        &ctx.accounts.case,
        &data,
        Clock::get()?.unix_timestamp,
    )?;

    let authority = Pubkey::find_program_address(
        &[Pool::VAULT_AUTHORITY_SEED, pool.key().as_ref()],
        &crate::ID,
    )
    .0;
    token::validate_vault(
        &ctx.accounts.vault,
        &pool.collateral_mint,
        &authority,
        pool.principal,
    )?;
    require!(
        pool.processed_dates == 0 && pool.total_weight == 0 && pool.payout_remaining == 0,
        ManteonError::InvalidAccounting
    );
    let occurred_at = data.occurred_at();
    if occurred_at < pool.lock_at {
        pool.state = PoolState::Refunding;
    } else {
        // Never freeze a book whose admitted dates could all underflow to zero.
        admission::validate_day(pool, utc_day(occurred_at))?;
        pool.payout_remaining = pool.principal;
        pool.state = PoolState::Settling;
    }
    pool.accepted_case = Some(ctx.accounts.case.key());
    ctx.accounts.event.result = Some(data);
    pool.release_if_complete(&mut ctx.accounts.config)?;
    Ok(())
}
