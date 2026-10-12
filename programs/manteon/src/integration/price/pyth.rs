//! Fully verified Pyth SOL/USD updates (pyth-crosschain 709e742).

use anchor_lang::prelude::*;
use manteon_core::math::price::{oracle_usd, PriceBounds};

use super::{account, PriceAccount};
use crate::{error::ManteonError, state::UsdQuoteSource};

pub const SOL_USD_FEED_ID: [u8; 32] = [
    0xef, 0x0d, 0x8b, 0x6f, 0xda, 0x2c, 0xeb, 0xa4, 0x1d, 0xa1, 0x5d, 0x40, 0x95, 0xd1, 0xda, 0x39,
    0x2a, 0x0d, 0x2f, 0x8e, 0xd0, 0xc6, 0xc7, 0xbc, 0x0f, 0x4c, 0xfa, 0xc8, 0xc2, 0x80, 0xb5, 0x6d,
];

// Full verification uses a one-byte enum tag; the allocation still reserves 134 bytes.
const PRICE_UPDATE_SIZE: usize = 134;
const PRICE_UPDATE_DISCRIMINATOR: [u8; 8] = [34, 241, 35, 99, 157, 126, 244, 205];
const VERIFICATION_LEVEL: usize = 40;
const FULL_VERIFICATION: u8 = 1;
const FEED_ID: usize = 41;
const PRICE: usize = 73;
const CONFIDENCE: usize = 81;
const EXPONENT: usize = 89;
const PUBLISH_TIME: usize = 93;
const PREVIOUS_PUBLISH_TIME: usize = 101;
const POSTED_SLOT: usize = 125;

pub fn is_pyth_receiver(key: Pubkey) -> bool {
    key == pubkey!("rec2HHDDnjLfj4kE7VyEtFA1HPGQLK33259532cRyHp")
        || key == pubkey!("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ")
}

pub(super) fn read(
    rule: &UsdQuoteSource,
    accounts: &[PriceAccount<'_>],
    now: i64,
    slot: u64,
) -> Result<PriceBounds> {
    let UsdQuoteSource::PythSolUsd {
        price_update_account,
        receiver_program,
        feed_id,
        max_age_seconds,
        max_confidence_bps,
    } = *rule
    else {
        return err!(ManteonError::InvalidPriceRule);
    };
    let oracle = account(accounts, price_update_account)?;
    oracle.authenticate(
        receiver_program,
        &PRICE_UPDATE_DISCRIMINATOR,
        PRICE_UPDATE_SIZE,
    )?;
    require!(
        is_pyth_receiver(receiver_program)
            && oracle.data[VERIFICATION_LEVEL] == FULL_VERIFICATION
            && oracle.bytes::<32>(FEED_ID)? == feed_id,
        ManteonError::InvalidPriceSource
    );
    let price = i64::from_le_bytes(oracle.bytes(PRICE)?);
    let confidence = oracle.u64_at(CONFIDENCE)?;
    let exponent = i32::from_le_bytes(oracle.bytes(EXPONENT)?);
    let published_at = i64::from_le_bytes(oracle.bytes(PUBLISH_TIME)?);
    let previous_at = i64::from_le_bytes(oracle.bytes(PREVIOUS_PUBLISH_TIME)?);
    require!(
        price > 0
            && published_at > 0
            && published_at <= now
            && previous_at <= published_at
            && now - published_at <= i64::from(max_age_seconds)
            && oracle.u64_at(POSTED_SLOT)? <= slot
            && u128::from(confidence) * 10_000 <= price as u128 * u128::from(max_confidence_bps),
        ManteonError::StalePriceSource
    );
    oracle_usd(price, confidence, exponent).map_err(|_| error!(ManteonError::InvalidPriceSource))
}
