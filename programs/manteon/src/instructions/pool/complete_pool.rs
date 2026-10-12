//! Release unused or fully consumed Pools without touching custody accounts.
use anchor_lang::prelude::*;

use crate::{
    error::ManteonError,
    state::{Config, Pool, PoolState},
};

#[derive(Accounts)]
pub struct CompletePool<'info> {
    pub caller: Signer<'info>,
    #[account(mut)]
    pub config: Account<'info, Config>,
    #[account(mut, has_one = config)]
    pub pool: Account<'info, Pool>,
}

pub(crate) fn handle(ctx: Context<CompletePool>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let pool = &mut ctx.accounts.pool;
    require!(!pool.config_released, ManteonError::InvalidPoolState);
    let complete = match pool.state {
        PoolState::Draft => {
            now >= pool.lock_at
                || ctx.accounts.caller.key() == ctx.accounts.config.authority
                || ctx.accounts.caller.key() == ctx.accounts.config.service_authority
        }
        PoolState::Active => now >= pool.horizon_at && pool.principal == 0,
        PoolState::Claimable => pool.claimed_principal == pool.principal,
        PoolState::Refunding => pool.principal == 0,
        PoolState::Settling => false,
    };
    require!(complete, ManteonError::PoolNotComplete);
    pool.release_config(&mut ctx.accounts.config)
}
