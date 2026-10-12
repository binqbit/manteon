use anchor_lang::prelude::*;
use manteon::state::{Event, EventDefinition, Pool, PoolState, PriceMonitor, UsdQuoteSource};

use crate::{
    read::{decode, pool_liability},
    ReadError,
};

/// Read the dedicated monitoring account for a validated price Pool/Event pair.
///
/// Acquire this account at the same bank as the graph passed to `read_pool`.
/// A Draft may have no account (or a prefunded, uninitialized System account).
/// Activated price pools retain the canonical monitor, with a baseline only
/// while Active. General market and claim reads do not need this account.
pub fn read_price_monitor(
    pool_key: &Pubkey,
    pool: &Pool,
    event: &Event,
    monitor_account: Option<&AccountInfo<'_>>,
) -> core::result::Result<Option<PriceMonitor>, ReadError> {
    let EventDefinition::TokenPriceThresholdCrossing { params } = event.definition else {
        return Err(ReadError::InvalidRelationship);
    };
    let event_key = Pubkey::find_program_address(&[Event::SEED, pool_key.as_ref()], &manteon::ID).0;
    if event.pool != *pool_key || pool.event_id != event_key {
        return Err(ReadError::InvalidRelationship);
    }
    pool_liability(pool, event)?;
    let Some(account) = monitor_account else {
        return if pool.state == PoolState::Draft {
            Ok(None)
        } else {
            Err(ReadError::InvalidData)
        };
    };
    let monitor_key =
        Pubkey::find_program_address(&[PriceMonitor::SEED, pool_key.as_ref()], &manteon::ID).0;
    if *account.key != monitor_key {
        return Err(ReadError::InvalidAddress);
    }
    if pool.state == PoolState::Draft {
        return if *account.owner == anchor_lang::system_program::ID
            && !account.executable
            && account
                .try_borrow_data()
                .map_err(|_| ReadError::InvalidData)?
                .is_empty()
        {
            Ok(None)
        } else {
            Err(ReadError::InvalidAccounting)
        };
    }
    let monitor: PriceMonitor = decode(account)?;
    if monitor.pool != *pool_key {
        return Err(ReadError::InvalidRelationship);
    }
    match (pool.state, monitor.baseline) {
        (PoolState::Active, Some(baseline)) => {
            let reference = matches!(
                params.usd_quote_source,
                UsdQuoteSource::RaydiumSolUsdc { .. }
            );
            if baseline.observed_at <= 0
                || baseline.observed_at > pool.horizon_at
                || reference != (baseline.reference_observed_at > 0)
                || (!reference && baseline.reference_cumulative_price_x32 != 0)
            {
                return Err(ReadError::InvalidAccounting);
            }
        }
        (PoolState::Settling | PoolState::Claimable | PoolState::Refunding, None) => {}
        _ => return Err(ReadError::InvalidAccounting),
    }
    Ok(Some(monitor))
}
