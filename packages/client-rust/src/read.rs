use anchor_lang::prelude::*;
use anchor_spl::token::{spl_token, TokenAccount};
use manteon::state::{Config, Event, EventDefinition, Pool, PoolState};
use manteon_core::{
    math::checked_amounts::backing_excess, types::Amount, validation::time::Schedule,
};

use crate::ReadError;

/// Decoded facts from one validated account graph, not an onchain capability.
pub struct PoolView {
    /// Recorded market configuration; this reader does not authenticate its Policy.
    pub config: Config,
    /// Recorded principal, liabilities, schedule and lifecycle state.
    pub pool: Pool,
    /// The Pool's reciprocally linked, canonical Event.
    pub event: Event,
    /// Observed SPL token balance in base units, separate from Pool principal.
    pub vault_balance: u64,
    /// Balance beyond recorded liabilities; no withdrawal right is implied.
    pub unallocated_balance: u64,
}

/// Read the Pool account graph using supplied account snapshots.
///
/// Checks program ownership, layouts, canonical PDAs, reciprocal relationships
/// and lifecycle accounting invariants. Vault donations remain separate from recorded
/// participation. Config's Policy reference is not authenticated by this read.
///
/// The acquisition layer must obtain all accounts from the intended network and
/// a consistent snapshot at its chosen commitment. These local checks cannot
/// prove RPC honesty or finality. No account is mutated and no signer is required.
pub fn read_pool(
    pool_account: &AccountInfo<'_>,
    config_account: &AccountInfo<'_>,
    event_account: &AccountInfo<'_>,
    vault_account: &AccountInfo<'_>,
) -> core::result::Result<PoolView, ReadError> {
    let keys = [
        pool_account.key,
        config_account.key,
        event_account.key,
        vault_account.key,
    ];
    for (index, key) in keys.iter().enumerate() {
        if keys[index + 1..].contains(key) {
            return Err(ReadError::InvalidRelationship);
        }
    }

    let pool: Pool = decode(pool_account)?;
    let config: Config = decode(config_account)?;
    manteon_core::math::fees::validate_fee_rates(pool.profit_fee_bps, pool.platform_share_bps)
        .map_err(|_| ReadError::InvalidAccounting)?;
    let event: Event = decode(event_account)?;

    if pool.config != *config_account.key
        || pool.event_id != *event_account.key
        || event.pool != *pool_account.key
        || (!pool.config_released
            && (pool.collateral_mint != config.collateral_mint
                || pool.audit_config != config.audit_config
                || pool.fee_recipient != config.fee_recipient
                || pool.profit_fee_bps != config.profit_fee_bps
                || pool.platform_share_bps != config.platform_share_bps
                || config.active_pools == 0))
        || config.authority == Pubkey::default()
        || config.service_authority == Pubkey::default()
        || config.audit_config == Pubkey::default()
        || pool.agreement_hash == [0; 32]
    {
        return Err(ReadError::InvalidRelationship);
    }

    let event_address =
        Pubkey::find_program_address(&[Event::SEED, pool_account.key.as_ref()], &manteon::ID).0;
    let vault_address =
        Pubkey::find_program_address(&[Pool::VAULT_SEED, pool_account.key.as_ref()], &manteon::ID)
            .0;
    if *event_account.key != event_address || *vault_account.key != vault_address {
        return Err(ReadError::InvalidAddress);
    }

    let liability = pool_liability(&pool, &event)?;
    let vault = read_vault(vault_account, pool_account.key, &pool.collateral_mint)?;
    let vault_balance = vault.amount;
    let unallocated_balance = backing_excess(Amount::new(vault_balance), liability)
        .map_err(|_| ReadError::InvalidAccounting)?
        .get();
    Ok(PoolView {
        config,
        pool,
        event,
        vault_balance,
        unallocated_balance,
    })
}

/// Validate the lifecycle and return its recorded liability, never the vault balance.
pub(crate) fn pool_liability(
    pool: &Pool,
    event: &Event,
) -> core::result::Result<Amount, ReadError> {
    Schedule {
        opens_at: pool.opens_at,
        lock_at: pool.lock_at,
        horizon_at: pool.horizon_at,
    }
    .validate_ordering()
    .map_err(|_| ReadError::InvalidAccounting)?;

    if manteon_core::math::utc_day::utc_day(pool.horizon_at)
        - manteon_core::math::utc_day::utc_day(pool.lock_at)
        > pool.algorithm.max_error_days()
    {
        return Err(ReadError::InvalidAccounting);
    }

    if event.definition.token_mint() == Pubkey::default() {
        return Err(ReadError::InvalidRelationship);
    }
    if let Some(result) = event.result {
        result
            .validate(&event.definition, manteon::result::MAX_TIMESTAMP)
            .map_err(|_| ReadError::InvalidRelationship)?;
    }
    validate_event_state(pool, event)?;
    if pool.claimed_principal > pool.principal
        || (pool.state != PoolState::Claimable && pool.claimed_principal != 0)
        || (pool.config_released
            && !(match pool.state {
                PoolState::Draft | PoolState::Active | PoolState::Refunding => pool.principal == 0,
                PoolState::Claimable => pool.claimed_principal == pool.principal,
                PoolState::Settling => false,
            }))
    {
        return Err(ReadError::InvalidAccounting);
    }
    if pool.state == PoolState::Settling && pool.payout_remaining != pool.principal {
        return Err(ReadError::InvalidAccounting);
    }

    // An exhaustive match forces new lifecycle support to select its actual
    // liability basis instead of treating a future phase as an empty Draft.
    match pool.state {
        PoolState::Draft => {
            if pool.principal != 0
                || pool.active_dates != 0
                || pool.processed_dates != 0
                || pool.total_weight != 0
                || pool.payout_remaining != 0
                || pool.next_case_nonce != 0
                || pool.current_case.is_some()
                || event.result.is_some()
            {
                return Err(ReadError::InvalidAccounting);
            }
            Ok(Amount::ZERO)
        }
        PoolState::Active | PoolState::Refunding => {
            if pool.processed_dates != 0
                || pool.total_weight != 0
                || pool.payout_remaining != 0
                || (pool.principal == 0) != (pool.active_dates == 0)
                || pool.active_dates > pool.principal
            {
                return Err(ReadError::InvalidAccounting);
            }
            Ok(Amount::new(pool.principal))
        }
        PoolState::Settling | PoolState::Claimable => {
            let progress = manteon_core::accounting::settlement::SettlementProgress::new(
                pool.algorithm.into(),
                Amount::new(pool.principal),
                pool.active_dates,
                pool.processed_dates,
                pool.total_weight,
            )
            .map_err(|_| ReadError::InvalidAccounting)?;
            if pool.payout_remaining > pool.principal {
                return Err(ReadError::InvalidAccounting);
            }
            if pool.state == PoolState::Claimable && pool.principal != 0 {
                progress
                    .validate_complete()
                    .map_err(|_| ReadError::InvalidAccounting)?;
            }
            Ok(if pool.config_released {
                Amount::ZERO
            } else {
                Amount::new(pool.payout_remaining)
            })
        }
    }
}

/// Validate occurrence facts against the Pool lifecycle without loading monitoring state.
fn validate_event_state(pool: &Pool, event: &Event) -> core::result::Result<(), ReadError> {
    match event.definition {
        EventDefinition::TokenPriceThresholdCrossing { params } => {
            params
                .validate()
                .map_err(|_| ReadError::InvalidRelationship)?;
            if pool.current_case.is_some()
                || pool.next_case_nonce != 0
                || pool.service_eligible_at.is_some()
            {
                return Err(ReadError::InvalidAccounting);
            }
            match (pool.state, event.result) {
                (PoolState::Draft | PoolState::Active | PoolState::Refunding, None) => {}
                (
                    PoolState::Settling | PoolState::Claimable | PoolState::Refunding,
                    Some(result),
                ) => {
                    if result.occurred_at > pool.horizon_at
                        || ((result.occurred_at < pool.lock_at)
                            != (pool.state == PoolState::Refunding))
                    {
                        return Err(ReadError::InvalidAccounting);
                    }
                }
                _ => return Err(ReadError::InvalidAccounting),
            }
        }
        EventDefinition::CompanyIpo { .. } => match (pool.state, event.result) {
            (PoolState::Draft | PoolState::Active, None)
                if pool.current_case.is_none()
                    && pool.next_case_nonce == 0
                    && pool.service_eligible_at.is_none() => {}
            (
                PoolState::Active
                | PoolState::Settling
                | PoolState::Claimable
                | PoolState::Refunding,
                Some(_),
            ) if pool.current_case.is_some()
                && pool.next_case_nonce > 0
                && pool.service_eligible_at.is_some_and(|at| at >= 0) => {}
            _ => return Err(ReadError::InvalidAccounting),
        },
    }
    Ok(())
}

fn read_vault(
    vault_account: &AccountInfo<'_>,
    pool_key: &Pubkey,
    mint: &Pubkey,
) -> core::result::Result<TokenAccount, ReadError> {
    if *vault_account.owner != spl_token::ID {
        return Err(ReadError::WrongOwner);
    }
    let data = vault_account
        .try_borrow_data()
        .map_err(|_| ReadError::InvalidData)?;
    if vault_account.executable || data.len() != TokenAccount::LEN {
        return Err(ReadError::InvalidData);
    }
    let vault =
        TokenAccount::try_deserialize(&mut data.as_ref()).map_err(|_| ReadError::InvalidData)?;
    let vault_authority = Pubkey::find_program_address(
        &[Pool::VAULT_AUTHORITY_SEED, pool_key.as_ref()],
        &manteon::ID,
    )
    .0;
    if vault.mint != *mint
        || vault.owner != vault_authority
        || vault.state != spl_token::state::AccountState::Initialized
        || vault.delegate.is_some()
        || vault.delegated_amount != 0
        || vault.close_authority.is_some()
        || vault.is_native.is_some()
        || vault.mint == spl_token::native_mint::ID
    {
        return Err(ReadError::InvalidVault);
    }
    Ok(vault)
}

pub(crate) fn decode<T>(account: &AccountInfo<'_>) -> core::result::Result<T, ReadError>
where
    T: AccountDeserialize + Owner + Discriminator + Space,
{
    if *account.owner != T::owner() {
        return Err(ReadError::WrongOwner);
    }
    let data = account
        .try_borrow_data()
        .map_err(|_| ReadError::InvalidData)?;
    if account.executable || data.len() != T::DISCRIMINATOR.len() + T::INIT_SPACE {
        return Err(ReadError::InvalidData);
    }
    T::try_deserialize(&mut data.as_ref()).map_err(|_| ReadError::InvalidData)
}
