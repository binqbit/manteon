//! Every wallet has identical rights to advance verified observations and accept a crossing.

use anchor_lang::prelude::*;
use anchor_spl::token::TokenAccount;
use manteon_core::math::utc_day::utc_day;

use crate::{
    custody::token,
    error::ManteonError,
    integration::price::{self, PriceAccount, PriceSample},
    state::{
        Config, Event, EventDefinition, EventResult, Pool, PoolState, PriceMonitor, PriceThreshold,
    },
    validation::admission,
};

#[derive(Accounts)]
pub struct ActivatePricePool<'info> {
    #[account(mut)]
    pub caller: Signer<'info>,
    pub config: Account<'info, Config>,
    #[account(mut, has_one = config, constraint = pool.event_id == event.key())]
    pub pool: Account<'info, Pool>,
    #[account(seeds = [Event::SEED, pool.key().as_ref()], bump, has_one = pool)]
    pub event: Account<'info, Event>,
    #[account(seeds = [Pool::VAULT_SEED, pool.key().as_ref()], bump)]
    pub vault: Account<'info, TokenAccount>,
    #[account(
        init,
        payer = caller,
        space = PriceMonitor::DISCRIMINATOR.len() + PriceMonitor::INIT_SPACE,
        seeds = [PriceMonitor::SEED, pool.key().as_ref()],
        bump
    )]
    pub monitor: Account<'info, PriceMonitor>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct PriceThresholdPool<'info> {
    pub caller: Signer<'info>,
    #[account(mut)]
    pub config: Account<'info, Config>,
    #[account(mut, has_one = config, constraint = pool.event_id == event.key())]
    pub pool: Account<'info, Pool>,
    #[account(mut, seeds = [Event::SEED, pool.key().as_ref()], bump, has_one = pool)]
    pub event: Account<'info, Event>,
    #[account(seeds = [Pool::VAULT_SEED, pool.key().as_ref()], bump)]
    pub vault: Account<'info, TokenAccount>,
    #[account(mut, seeds = [PriceMonitor::SEED, pool.key().as_ref()], bump, has_one = pool)]
    pub monitor: Account<'info, PriceMonitor>,
}

fn rule(event: &Event) -> Result<PriceThreshold> {
    let EventDefinition::TokenPriceThresholdCrossing { params } = event.definition else {
        return err!(ManteonError::InvalidPriceRule);
    };
    params.validate()?;
    Ok(params)
}

fn sample(
    rule: &PriceThreshold,
    accounts: &[AccountInfo<'_>],
    clock: &Clock,
) -> Result<PriceSample> {
    let data = accounts
        .iter()
        .map(|account| account.try_borrow_data().map_err(Into::into))
        .collect::<Result<Vec<_>>>()?;
    let views = accounts
        .iter()
        .zip(&data)
        .map(|(account, data)| PriceAccount {
            key: *account.key,
            owner: *account.owner,
            data: &data[..],
        })
        .collect::<Vec<_>>();
    price::sample(rule, &views, clock.unix_timestamp, clock.slot)
}

fn validate_active(accounts: &PriceThresholdPool<'_>, now: i64) -> Result<PriceThreshold> {
    require!(
        !accounts.pool.config_released
            && accounts.pool.state == PoolState::Active
            && accounts.event.result.is_none()
            && accounts.monitor.baseline.is_some()
            && accounts.pool.accepted_case.is_none()
            && accounts.pool.service_eligible_at.is_none()
            && !accounts.pool.public_proposals,
        ManteonError::InvalidPoolState
    );
    require!(now <= accounts.pool.horizon_at, ManteonError::InvalidTime);
    let authority = Pubkey::find_program_address(
        &[Pool::VAULT_AUTHORITY_SEED, accounts.pool.key().as_ref()],
        &crate::ID,
    )
    .0;
    token::validate_vault(
        &accounts.vault,
        &accounts.pool.collateral_mint,
        &authority,
        accounts.pool.principal,
    )?;
    rule(&accounts.event)
}

pub(crate) fn activate(ctx: Context<ActivatePricePool>) -> Result<()> {
    let clock = Clock::get()?;
    require_keys_eq!(
        ctx.accounts.caller.key(),
        ctx.accounts.config.service_authority,
        ManteonError::UnauthorizedServiceAuthority
    );
    require!(
        !ctx.accounts.pool.config_released
            && ctx.accounts.pool.state == PoolState::Draft
            && ctx.accounts.event.result.is_none()
            && ctx.accounts.pool.accepted_case.is_none()
            && ctx.accounts.pool.service_eligible_at.is_none()
            && !ctx.accounts.pool.public_proposals,
        ManteonError::InvalidPoolState
    );
    ctx.accounts
        .pool
        .schedule()
        .validate_activation_time(clock.unix_timestamp)
        .map_err(ManteonError::from)?;
    let rule = rule(&ctx.accounts.event)?;
    let initial = sample(&rule, ctx.remaining_accounts, &clock)?;
    // A market cannot start after its defined crossing already happened.
    require!(
        rule.direction
            .opposite(initial.spot_usd, rule.target_price_usd_e9),
        ManteonError::InvalidPriceObservation
    );
    ctx.accounts.monitor.set_inner(PriceMonitor {
        pool: ctx.accounts.pool.key(),
        baseline: Some(initial.baseline()),
    });
    ctx.accounts.pool.state = PoolState::Active;
    Ok(())
}

/// Advance the baseline only when neither the interval nor current spot has crossed.
/// A crossing cannot be erased by this instruction; it must be finalized instead.
pub(crate) fn observe(ctx: Context<PriceThresholdPool>) -> Result<()> {
    let clock = Clock::get()?;
    let rule = validate_active(ctx.accounts, clock.unix_timestamp)?;
    let baseline = ctx
        .accounts
        .monitor
        .baseline
        .ok_or(ManteonError::InvalidPriceObservation)?;
    let current = sample(&rule, ctx.remaining_accounts, &clock)?;
    let interval = current.interval(&rule, &baseline, clock.unix_timestamp)?;
    require!(
        !rule.direction.crossed(interval, rule.target_price_usd_e9)
            && !rule
                .direction
                .crossed(current.spot_usd, rule.target_price_usd_e9),
        ManteonError::InvalidPriceObservation
    );
    ctx.accounts.monitor.baseline = Some(current.baseline());
    Ok(())
}

/// Accept a crossing and freeze principal in one transaction. No caller timestamp or audit.
pub(crate) fn finalize(ctx: Context<PriceThresholdPool>) -> Result<()> {
    let clock = Clock::get()?;
    let rule = validate_active(ctx.accounts, clock.unix_timestamp)?;
    let baseline = ctx
        .accounts
        .monitor
        .baseline
        .ok_or(ManteonError::InvalidPriceObservation)?;
    let current = sample(&rule, ctx.remaining_accounts, &clock)?;
    let interval = current.interval(&rule, &baseline, clock.unix_timestamp)?;
    require!(
        rule.direction.crossed(interval, rule.target_price_usd_e9)
            && rule
                .direction
                .crossed(current.spot_usd, rule.target_price_usd_e9),
        ManteonError::PriceThresholdNotCrossed
    );
    let occurred_at = clock.unix_timestamp;
    if occurred_at >= ctx.accounts.pool.lock_at {
        admission::validate_day(&ctx.accounts.pool, utc_day(occurred_at))?;
    }
    require!(
        ctx.accounts.pool.processed_dates == 0
            && ctx.accounts.pool.total_weight == 0
            && ctx.accounts.pool.payout_remaining == 0,
        ManteonError::InvalidAccounting
    );
    ctx.accounts.event.result = Some(EventResult {
        occurred_at,
        extra: Some(crate::state::EventResultExtra::PriceVerification {
            lower_usd_e9: interval.lower,
            upper_usd_e9: interval.upper,
        }),
    });
    ctx.accounts.monitor.baseline = None;
    if occurred_at < ctx.accounts.pool.lock_at {
        ctx.accounts.pool.state = PoolState::Refunding;
    } else {
        ctx.accounts.pool.payout_remaining = ctx.accounts.pool.principal;
        ctx.accounts.pool.state = PoolState::Settling;
    }
    ctx.accounts
        .pool
        .release_if_complete(&mut ctx.accounts.config)?;
    Ok(())
}

/// A lost monitoring interval cannot be reconstructed or given an invented event date.
pub(crate) fn expire(ctx: Context<PriceThresholdPool>) -> Result<()> {
    require!(
        ctx.remaining_accounts.is_empty(),
        ManteonError::InvalidPriceSource
    );
    let now = Clock::get()?.unix_timestamp;
    let rule = rule(&ctx.accounts.event)?;
    let baseline = ctx
        .accounts
        .monitor
        .baseline
        .ok_or(ManteonError::InvalidPriceObservation)?;
    require!(
        !ctx.accounts.pool.config_released
            && ctx.accounts.pool.state == PoolState::Active
            && ctx.accounts.event.result.is_none()
            && ctx.accounts.pool.accepted_case.is_none()
            && ctx.accounts.pool.service_eligible_at.is_none()
            && !ctx.accounts.pool.public_proposals
            && (now > ctx.accounts.pool.horizon_at
                || now - baseline.observed_at > i64::from(rule.max_observation_gap_seconds)),
        ManteonError::InvalidPriceObservation
    );
    ctx.accounts.monitor.baseline = None;
    ctx.accounts.pool.state = PoolState::Refunding;
    ctx.accounts
        .pool
        .release_if_complete(&mut ctx.accounts.config)?;
    Ok(())
}
