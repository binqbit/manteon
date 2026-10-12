//! Narrow, authenticated Raydium CPMM / Pyth account adapters.
//!
//! Wire layouts pinned to Raydium cp-swap b3187ae and Pyth crosschain 709e742.
//! CLMM, AMM v4 and partial Pyth verification are deliberately rejected.

use anchor_lang::prelude::*;
use manteon_core::math::price::{usd_twap, PriceBounds, USD_SCALE};

use crate::{
    error::ManteonError,
    state::{PriceBaseline, PriceThreshold, UsdQuoteSource},
};

mod pyth;
mod raydium;

pub use pyth::{is_pyth_receiver, SOL_USD_FEED_ID};
pub use raydium::{raydium_accounts, raydium_accounts_on};

/// Trusted deployment selection for host previews; contract instructions use CURRENT.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PriceNetwork {
    Mainnet,
    Devnet,
}

impl PriceNetwork {
    #[cfg(not(feature = "devnet"))]
    pub const CURRENT: Self = Self::Mainnet;
    #[cfg(feature = "devnet")]
    pub const CURRENT: Self = Self::Devnet;
    pub const fn program(self) -> Pubkey {
        match self {
            Self::Mainnet => pubkey!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C"),
            Self::Devnet => pubkey!("DRaycpLY18LhpbydsBWbVJtxpNv9oXPgjRSfpF2bWpYb"),
        }
    }
    pub const fn usdc(self) -> Pubkey {
        match self {
            Self::Mainnet => pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
            Self::Devnet => pubkey!("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU"),
        }
    }
}

pub const RAYDIUM_PROGRAM: Pubkey = PriceNetwork::CURRENT.program();
pub const USDC: Pubkey = PriceNetwork::CURRENT.usdc();
pub const WRAPPED_SOL: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
const DOLLAR: PriceBounds = PriceBounds {
    lower: USD_SCALE,
    upper: USD_SCALE,
};

/// Same borrowed view is usable by the contract and the backend's RPC preview.
#[derive(Clone, Copy)]
pub struct PriceAccount<'a> {
    pub key: Pubkey,
    pub owner: Pubkey,
    pub data: &'a [u8],
}

impl PriceAccount<'_> {
    fn bytes<const N: usize>(&self, offset: usize) -> Result<[u8; N]> {
        self.data
            .get(offset..offset + N)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| error!(ManteonError::InvalidPriceSource))
    }

    fn key_at(&self, offset: usize) -> Result<Pubkey> {
        Ok(Pubkey::new_from_array(self.bytes(offset)?))
    }

    fn u64_at(&self, offset: usize) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(offset)?))
    }

    fn u128_at(&self, offset: usize) -> Result<u128> {
        Ok(u128::from_le_bytes(self.bytes(offset)?))
    }

    fn authenticate(&self, owner: Pubkey, discriminator: &[u8; 8], len: usize) -> Result<()> {
        require!(
            self.owner == owner && self.data.len() == len && self.data.starts_with(discriminator),
            ManteonError::InvalidPriceSource
        );
        Ok(())
    }
}

fn account<'a>(accounts: &[PriceAccount<'a>], key: Pubkey) -> Result<PriceAccount<'a>> {
    let mut matches = accounts.iter().filter(|account| account.key == key);
    let found = *matches.next().ok_or(ManteonError::InvalidPriceSource)?;
    require!(matches.next().is_none(), ManteonError::InvalidPriceSource);
    Ok(found)
}

/// Authenticated quote-token price, before conversion to USD.
#[derive(Clone, Copy, Debug)]
pub struct DexPrice {
    /// Timestamp of the authenticated cumulative endpoint, not a market event time.
    pub observed_at: i64,
    /// Cumulative quote atoms per base atom in Q32.32, integrated over seconds.
    pub cumulative: u128,
    /// Current reserve ratio in Q32.32, before applying mint decimals.
    pub spot_x32: u128,
    pub base_decimals: u8,
    pub quote_decimals: u8,
    pub quote_reserve: u64,
}

impl DexPrice {
    fn spot_usd(&self, quote: PriceBounds) -> Result<PriceBounds> {
        Ok(usd_twap(
            self.spot_x32,
            1,
            self.base_decimals,
            self.quote_decimals,
            quote,
        )
        .map_err(ManteonError::from)?)
    }

    fn has_liquidity(&self, minimum_usd_e9: u64, quote: PriceBounds) -> bool {
        let quote_atoms = u128::from(10_u64.pow(u32::from(self.quote_decimals)));
        u128::from(self.quote_reserve) * u128::from(quote.lower) / quote_atoms
            >= u128::from(minimum_usd_e9)
    }
}

/// Main DEX price and the authenticated source used to value its quote token in USD.
#[derive(Clone, Copy, Debug)]
pub struct PriceSample {
    pub main: DexPrice,
    pub reference: Option<DexPrice>,
    pub quote_usd: PriceBounds,
    pub spot_usd: PriceBounds,
}

impl PriceSample {
    pub fn baseline(&self) -> PriceBaseline {
        PriceBaseline {
            observed_at: self.main.observed_at,
            cumulative_price_x32: self.main.cumulative,
            reference_observed_at: self.reference.map_or(0, |source| source.observed_at),
            reference_cumulative_price_x32: self.reference.map_or(0, |source| source.cumulative),
        }
    }

    pub fn interval(
        &self,
        rule: &PriceThreshold,
        baseline: &PriceBaseline,
        now: i64,
    ) -> Result<PriceBounds> {
        let elapsed = interval_seconds(self.main.observed_at, baseline.observed_at, rule, now)?;
        let quote = if let Some(reference) = self.reference {
            let elapsed = interval_seconds(
                reference.observed_at,
                baseline.reference_observed_at,
                rule,
                now,
            )?;
            usd_twap(
                reference
                    .cumulative
                    .wrapping_sub(baseline.reference_cumulative_price_x32),
                elapsed,
                reference.base_decimals,
                reference.quote_decimals,
                DOLLAR,
            )
            .map_err(ManteonError::from)?
        } else {
            self.quote_usd
        };
        usd_twap(
            self.main
                .cumulative
                .wrapping_sub(baseline.cumulative_price_x32),
            elapsed,
            self.main.base_decimals,
            self.main.quote_decimals,
            quote,
        )
        .map_err(|_| error!(ManteonError::InvalidPriceObservation))
    }
}

fn interval_seconds(end: i64, start: i64, rule: &PriceThreshold, now: i64) -> Result<u64> {
    require!(
        start > 0
            && end > start
            && end <= now
            && end - start >= i64::from(rule.twap_window_seconds)
            && now - start <= i64::from(rule.max_observation_gap_seconds),
        ManteonError::InvalidPriceObservation
    );
    Ok((end - start) as u64)
}

pub fn sample(
    rule: &PriceThreshold,
    accounts: &[PriceAccount<'_>],
    now: i64,
    slot: u64,
) -> Result<PriceSample> {
    sample_on(rule, accounts, now, slot, PriceNetwork::CURRENT)
}

/// Backend previews select the trusted network matching their verified genesis.
pub fn sample_on(
    rule: &PriceThreshold,
    accounts: &[PriceAccount<'_>],
    now: i64,
    slot: u64,
    network: PriceNetwork,
) -> Result<PriceSample> {
    rule.validate()?;
    require!(now > 0, ManteonError::InvalidTime);
    let quote_mint = match rule.usd_quote_source {
        UsdQuoteSource::Usdc => network.usdc(),
        _ => WRAPPED_SOL,
    };
    let main = raydium::read(
        accounts,
        rule.pool_id,
        rule.token_mint,
        quote_mint,
        now,
        network,
    )?;
    let reference = if let UsdQuoteSource::RaydiumSolUsdc { pool_id } = rule.usd_quote_source {
        let source = raydium::read(accounts, pool_id, WRAPPED_SOL, network.usdc(), now, network)?;
        require!(
            (source.observed_at - main.observed_at).abs() <= 30,
            ManteonError::StalePriceSource
        );
        Some(source)
    } else {
        None
    };
    let quote_usd = match rule.usd_quote_source {
        UsdQuoteSource::Usdc => DOLLAR,
        UsdQuoteSource::PythSolUsd { .. } => {
            pyth::read(&rule.usd_quote_source, accounts, now, slot)?
        }
        UsdQuoteSource::RaydiumSolUsdc { .. } => {
            let source = reference.ok_or(ManteonError::InvalidPriceSource)?;
            source.spot_usd(DOLLAR)?
        }
    };
    let expected_accounts = match rule.usd_quote_source {
        UsdQuoteSource::Usdc => 4,
        UsdQuoteSource::PythSolUsd { .. } => 5,
        UsdQuoteSource::RaydiumSolUsdc { .. } => 8,
    };
    require!(
        accounts.len() == expected_accounts && quote_usd.lower > 0,
        ManteonError::InvalidPriceSource
    );
    require!(
        main.has_liquidity(rule.minimum_quote_liquidity_usd_e9, quote_usd)
            && reference.is_none_or(
                |source| source.has_liquidity(rule.minimum_quote_liquidity_usd_e9, DOLLAR)
            ),
        ManteonError::InvalidPriceSource
    );
    let spot_usd = main.spot_usd(quote_usd)?;
    Ok(PriceSample {
        main,
        reference,
        quote_usd,
        spot_usd,
    })
}

#[cfg(test)]
mod tests;
