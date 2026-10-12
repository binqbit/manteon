use anchor_lang::prelude::*;
use manteon_core::{
    math::utc_day::{day_error, utc_day},
    types::Amount,
};

use crate::{
    error::ManteonError,
    state::{Config, Event, Pool, PoolState, Position},
    validation::{admission, resolution},
};

#[derive(Accounts)]
pub struct ProcessPositions<'info> {
    // Signers.
    pub caller: Signer<'info>,

    // Protocol accounts.
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

pub(crate) fn handle<'info>(ctx: Context<'info, ProcessPositions<'info>>) -> Result<()> {
    let pool = &mut ctx.accounts.pool;
    require!(
        !pool.config_released && pool.state == PoolState::Settling,
        ManteonError::InvalidPoolState
    );
    resolution::authorize(
        ctx.accounts.caller.key(),
        &ctx.accounts.config,
        pool,
        &ctx.accounts.event,
        ctx.accounts.case.as_ref().map(|account| account.as_ref()),
        Clock::get()?.unix_timestamp,
    )?;
    require!(
        !ctx.remaining_accounts.is_empty() && ctx.remaining_accounts.len() <= 8,
        ManteonError::InvalidBatch
    );
    let mut progress = pool.settlement_progress()?;
    let result_day = utc_day(
        ctx.accounts
            .event
            .result
            .ok_or(ManteonError::InvalidEventResult)?
            .occurred_at(),
    );
    for (index, info) in ctx.remaining_accounts.iter().enumerate() {
        require!(
            info.is_writable
                && !ctx.remaining_accounts[..index]
                    .iter()
                    .any(|prior| prior.key == info.key),
            ManteonError::InvalidBatch
        );
        let mut position = read_position(info, pool)?;
        let amount = Amount::new(position.principal);
        let error_days = day_error(position.forecast_day, result_day);
        progress
            .process_position(amount, error_days, &mut position.computed)
            .map_err(ManteonError::from)?;
        position.exit(&crate::ID)?;
    }
    pool.processed_dates = progress.processed_dates();
    pool.total_weight = progress.total_weight();
    Ok(())
}

/// Remaining accounts need the same canonical identity checks as declared accounts.
fn read_position<'info>(
    info: &'info AccountInfo<'info>,
    pool: &Account<'_, Pool>,
) -> Result<Account<'info, Position>> {
    require!(
        info.data_len() == Position::DISCRIMINATOR.len() + Position::INIT_SPACE,
        ManteonError::InvalidPosition
    );
    let position = Account::<Position>::try_from(info)?;
    let expected = Pubkey::find_program_address(
        &[
            Position::SEED,
            pool.key().as_ref(),
            &position.forecast_day.to_le_bytes(),
        ],
        &crate::ID,
    )
    .0;
    require!(
        position.pool == pool.key() && position.key() == expected,
        ManteonError::InvalidPosition
    );
    admission::validate_forecast(pool, position.forecast_day)?;
    Ok(position)
}
