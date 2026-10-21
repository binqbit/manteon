use anchor_lang::prelude::*;
use manteon_core::math::utc_day::{day_start, utc_day};

use crate::{error::ManteonError, state::Pool};

// Operational service month: exactly thirty UTC days, independently of the accuracy half-life.
const SERVICE_MONTH: i64 = 30 * 86_400;
// Forecasts and competitive results share the selected curve's nonzero domain.

pub(crate) fn validate_forecast(pool: &Pool, day: i64) -> Result<()> {
    pool.schedule()
        .validate_forecast(day)
        .map_err(ManteonError::from)?;
    validate_day(pool, day)
}

pub(crate) fn validate_day(pool: &Pool, day: i64) -> Result<()> {
    let first = utc_day(pool.lock_at);
    require!(
        first >= 0 && day >= first && day <= first + pool.algorithm.max_error_days(),
        ManteonError::UnsupportedDate
    );
    day_start(day).map_err(ManteonError::from)?;
    Ok(())
}

pub(crate) fn service_access(
    caller: Pubkey,
    service: Pubkey,
    eligible_at: i64,
    now: i64,
) -> Result<()> {
    let public_at = eligible_at
        .checked_add(SERVICE_MONTH)
        .ok_or(ManteonError::Arithmetic)?;
    require!(
        caller == service || now >= public_at,
        ManteonError::ServiceWindow
    );
    Ok(())
}

/// External result timestamps never authorize early public submission.
pub(crate) fn candidate_access(
    pool: &Pool,
    author: Pubkey,
    service: Pubkey,
    now: i64,
    service_failed: bool,
) -> Result<(i64, bool)> {
    let public_at = pool
        .horizon_at
        .checked_sub(SERVICE_MONTH)
        .ok_or(ManteonError::Arithmetic)?;
    // The executing Clock anchors the first paid attempt; retries cannot reset it.
    let eligible_at = pool.service_eligible_at.unwrap_or(now);
    let proposal_fallback = eligible_at
        .checked_add(SERVICE_MONTH)
        .ok_or(ManteonError::Arithmetic)?;
    let public = pool.public_proposals || service_failed || now >= public_at.min(proposal_fallback);
    require!(author == service || public, ManteonError::ServiceWindow);
    Ok((eligible_at, public))
}
