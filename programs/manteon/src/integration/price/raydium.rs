//! Authenticated Raydium CPMM reserves and cumulative prices (cp-swap b3187ae).

use anchor_lang::prelude::*;
use anchor_spl::token_interface::TokenAccount;

use super::{account, DexPrice, PriceAccount, PriceNetwork};
use crate::error::ManteonError;

// Offsets include the Anchor discriminator and follow the packed PoolState layout.
const POOL_SIZE: usize = 637;
const POOL_DISCRIMINATOR: [u8; 8] = [247, 237, 227, 245, 215, 195, 222, 70];
const TOKEN_VAULTS: [usize; 2] = [72, 104];
const TOKEN_MINTS: [usize; 2] = [168, 200];
const TOKEN_PROGRAMS: [usize; 2] = [232, 264];
const OBSERVATION_KEY: usize = 296;
const AUTHORITY_BUMP: usize = 328;
const STATUS: usize = 329;
const MINT_DECIMALS: [usize; 2] = [331, 332];
const PROTOCOL_FEES: [usize; 2] = [341, 349];
const FUND_FEES: [usize; 2] = [357, 365];
const OPEN_TIME: usize = 373;
const CREATOR_FEES: [usize; 2] = [397, 405];
const SWAPS_DISABLED: u8 = 1 << 2;
const AUTHORITY_SEED: &[u8] = b"vault_and_lp_mint_auth_seed";

// ObservationState contains a ring of cumulative Q32.32 prices for both tokens.
const OBSERVATION_STATE_SIZE: usize = 4075;
const OBSERVATION_DISCRIMINATOR: [u8; 8] = [122, 174, 197, 53, 129, 9, 165, 132];
const INITIALIZED: usize = 8;
const OBSERVATION_INDEX: usize = 9;
const OBSERVATION_POOL_ID: usize = 11;
const OBSERVATIONS: usize = 43;
const OBSERVATION_SIZE: usize = 40;
const OBSERVATION_COUNT: usize = 100;
const CUMULATIVE_PRICES: [usize; 2] = [8, 24];
const LAST_UPDATE_TIMESTAMP: usize = OBSERVATIONS + OBSERVATION_SIZE * OBSERVATION_COUNT;
const OBSERVATION_SEED: &[u8] = b"observation";
const MAX_OBSERVATION_AGE_SECONDS: i64 = 120;

/// Source bindings can be derived without trusting catalog metadata.
/// Returns the pool, observation account, token-0 vault and token-1 vault, in order.
pub fn raydium_accounts(pool: PriceAccount<'_>) -> Result<[Pubkey; 4]> {
    raydium_accounts_on(pool, PriceNetwork::CURRENT)
}

/// Discover source accounts using the backend's verified network deployment.
pub fn raydium_accounts_on(pool: PriceAccount<'_>, network: PriceNetwork) -> Result<[Pubkey; 4]> {
    pool.authenticate(network.program(), &POOL_DISCRIMINATOR, POOL_SIZE)?;
    Ok([
        pool.key,
        pool.key_at(OBSERVATION_KEY)?,
        pool.key_at(TOKEN_VAULTS[0])?,
        pool.key_at(TOKEN_VAULTS[1])?,
    ])
}

pub(super) fn read(
    accounts: &[PriceAccount<'_>],
    pool_id: Pubkey,
    base: Pubkey,
    quote: Pubkey,
    now: i64,
    network: PriceNetwork,
) -> Result<DexPrice> {
    let pool = account(accounts, pool_id)?;
    let keys = raydium_accounts_on(pool, network)?;
    let mints = [pool.key_at(TOKEN_MINTS[0])?, pool.key_at(TOKEN_MINTS[1])?];
    let base_index = if mints == [base, quote] {
        0
    } else if mints == [quote, base] {
        1
    } else {
        return err!(ManteonError::InvalidPriceSource);
    };
    let quote_index = 1 - base_index;

    require!(
        pool.data[STATUS] & SWAPS_DISABLED == 0 && pool.u64_at(OPEN_TIME)? <= now as u64,
        ManteonError::InvalidPriceSource
    );
    let (authority, bump) = Pubkey::find_program_address(&[AUTHORITY_SEED], &network.program());
    require!(
        pool.data[AUTHORITY_BUMP] == bump,
        ManteonError::InvalidPriceSource
    );
    let reserves = [
        vault_reserve(pool, accounts, 0, authority)?,
        vault_reserve(pool, accounts, 1, authority)?,
    ];
    let observation = account(accounts, keys[1])?;
    let (observed_at, cumulative) =
        cumulative_price(observation, pool_id, base_index, now, network)?;

    let base_decimals = pool.data[MINT_DECIMALS[base_index]];
    let quote_decimals = pool.data[MINT_DECIMALS[quote_index]];
    require!(
        base_decimals <= 18 && quote_decimals == if quote == network.usdc() { 6 } else { 9 },
        ManteonError::InvalidPriceSource
    );
    Ok(DexPrice {
        observed_at,
        cumulative,
        spot_x32: (u128::from(reserves[quote_index]) << 32) / u128::from(reserves[base_index]),
        base_decimals,
        quote_decimals,
        quote_reserve: reserves[quote_index],
    })
}

/// Accrued protocol, fund and creator fees are held in vaults but are not reserves.
fn vault_reserve(
    pool: PriceAccount<'_>,
    accounts: &[PriceAccount<'_>],
    token_index: usize,
    authority: Pubkey,
) -> Result<u64> {
    let vault = account(accounts, pool.key_at(TOKEN_VAULTS[token_index])?)?;
    let token_program = pool.key_at(TOKEN_PROGRAMS[token_index])?;
    require!(
        (token_program == anchor_spl::token::ID || token_program == anchor_spl::token_2022::ID)
            && vault.owner == token_program
            && (token_program != anchor_spl::token::ID
                || vault.data.len() == anchor_spl::token::TokenAccount::LEN),
        ManteonError::InvalidPriceSource
    );
    let mut token_data = vault.data;
    let token = TokenAccount::try_deserialize(&mut token_data)
        .map_err(|_| error!(ManteonError::InvalidPriceSource))?;
    require!(
        token.mint == pool.key_at(TOKEN_MINTS[token_index])?
            && token.owner == authority
            && token.state
                == anchor_spl::token_2022::spl_token_2022::state::AccountState::Initialized,
        ManteonError::InvalidPriceSource
    );
    let protocol_fees = pool.u64_at(PROTOCOL_FEES[token_index])?;
    let fund_fees = pool.u64_at(FUND_FEES[token_index])?;
    let creator_fees = pool.u64_at(CREATOR_FEES[token_index])?;
    let fees = protocol_fees
        .checked_add(fund_fees)
        .and_then(|sum| sum.checked_add(creator_fees))
        .ok_or(ManteonError::Arithmetic)?;
    token
        .amount
        .checked_sub(fees)
        .filter(|reserve| *reserve > 0)
        .ok_or_else(|| error!(ManteonError::InvalidPriceSource))
}

/// Read the current cumulative endpoint; never extend it using today's reserves.
fn cumulative_price(
    observation: PriceAccount<'_>,
    pool_id: Pubkey,
    token_index: usize,
    now: i64,
    network: PriceNetwork,
) -> Result<(i64, u128)> {
    observation.authenticate(
        network.program(),
        &OBSERVATION_DISCRIMINATOR,
        OBSERVATION_STATE_SIZE,
    )?;
    let expected =
        Pubkey::find_program_address(&[OBSERVATION_SEED, pool_id.as_ref()], &network.program()).0;
    require!(
        observation.key == expected
            && observation.data[INITIALIZED] == 1
            && observation.key_at(OBSERVATION_POOL_ID)? == pool_id,
        ManteonError::InvalidPriceSource
    );
    let index = usize::from(u16::from_le_bytes(observation.bytes(OBSERVATION_INDEX)?));
    require!(index < OBSERVATION_COUNT, ManteonError::InvalidPriceSource);
    let entry = OBSERVATIONS + index * OBSERVATION_SIZE;
    let observed_at = i64::try_from(observation.u64_at(LAST_UPDATE_TIMESTAMP)?)
        .map_err(|_| error!(ManteonError::StalePriceSource))?;
    require!(
        observed_at > 0
            && observed_at <= now
            && now - observed_at <= MAX_OBSERVATION_AGE_SECONDS
            && observation.u64_at(entry)? <= observed_at as u64,
        ManteonError::StalePriceSource
    );
    // Raydium can update a head's cumulative value without advancing its ring timestamp.
    let cumulative = observation.u128_at(entry + CUMULATIVE_PRICES[token_index])?;
    Ok((observed_at, cumulative))
}
