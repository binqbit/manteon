//! Immutable price rule and the contract's own authenticated observation baseline.

use anchor_lang::prelude::*;
use manteon_core::math::price::PriceBounds;

use crate::error::ManteonError;

#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, Debug, Eq, PartialEq)]
pub enum PriceProtocol {
    RaydiumCpmm,
}

#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, Debug, Eq, PartialEq)]
pub enum CrossingDirection {
    Up,
    Down,
}

impl CrossingDirection {
    /// The complete uncertainty interval must lie beyond the target.
    pub fn crossed(self, price: PriceBounds, target: u64) -> bool {
        match self {
            Self::Up => price.lower >= target,
            Self::Down => price.upper <= target,
        }
    }

    pub fn opposite(self, price: PriceBounds, target: u64) -> bool {
        match self {
            Self::Up => price.upper < target,
            Self::Down => price.lower > target,
        }
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsdQuoteSource {
    /// The tracked pool quotes in the canonical USDC mint.
    Usdc,
    /// The tracked pool quotes in wrapped SOL. This exact verified account/feed is fixed.
    PythSolUsd {
        price_update_account: Pubkey,
        receiver_program: Pubkey,
        feed_id: [u8; 32],
        max_age_seconds: u32,
        max_confidence_bps: u16,
    },
    /// Wrapped SOL converted using this exact Raydium SOL/USDC pool's interval TWAP.
    RaydiumSolUsdc { pool_id: Pubkey },
}

#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, Debug, Eq, PartialEq)]
pub struct PriceThreshold {
    pub protocol: PriceProtocol,
    pub pool_id: Pubkey,
    pub token_mint: Pubkey,
    pub direction: CrossingDirection,
    /// USD per whole token, with nine fractional digits. No binary floats.
    pub target_price_usd_e9: u64,
    pub usd_quote_source: UsdQuoteSource,
    pub twap_window_seconds: u32,
    pub max_observation_gap_seconds: u32,
    pub minimum_quote_liquidity_usd_e9: u64,
}

impl PriceThreshold {
    /// The same event-owned rule facts appear in agreements and accepted result documents.
    pub fn document_facts(&self) -> String {
        let conversion = match self.usd_quote_source {
            UsdQuoteSource::Usdc => "- USD quote source: usdc\n".to_owned(),
            UsdQuoteSource::PythSolUsd {
                price_update_account,
                receiver_program,
                feed_id,
                max_age_seconds,
                max_confidence_bps,
            } => format!(
                concat!(
                    "- USD quote source: pyth_sol_usd\n",
                    "- SOL/USD oracle account: {price_update_account}\n",
                    "- Oracle receiver program: {receiver_program}\n",
                    "- Oracle feed ID: {feed_id}\n",
                    "- Oracle maximum age seconds: {max_age_seconds}\n",
                    "- Oracle maximum confidence bps: {max_confidence_bps}\n",
                ),
                price_update_account = price_update_account,
                receiver_program = receiver_program,
                feed_id = feed_id
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                max_age_seconds = max_age_seconds,
                max_confidence_bps = max_confidence_bps,
            ),
            UsdQuoteSource::RaydiumSolUsdc { pool_id } => format!(
                "- USD quote source: raydium_sol_usdc\n- SOL/USDC reference pool: {pool_id}\n"
            ),
        };
        let direction = match self.direction {
            CrossingDirection::Up => "up",
            CrossingDirection::Down => "down",
        };
        format!(
            concat!(
                "- Protocol: raydium_cpmm\n",
                "- Raydium CPMM pool: {pool_id}\n",
                "- Direction: {direction}\n",
                "- Target price USD nano-units: {target_price}\n",
                "{conversion}",
                "- TWAP minimum window seconds: {twap_window}\n",
                "- Maximum observation gap seconds: {max_observation_gap}\n",
                "- Minimum quote liquidity USD nano-units: {minimum_quote_liquidity}\n",
            ),
            pool_id = self.pool_id,
            direction = direction,
            target_price = self.target_price_usd_e9,
            conversion = conversion,
            twap_window = self.twap_window_seconds,
            max_observation_gap = self.max_observation_gap_seconds,
            minimum_quote_liquidity = self.minimum_quote_liquidity_usd_e9,
        )
    }

    pub fn validate(&self) -> Result<()> {
        let minimum_window = if cfg!(feature = "localnet") { 60 } else { 300 };
        require!(
            self.pool_id != Pubkey::default()
                && self.token_mint != Pubkey::default()
                && self.target_price_usd_e9 > 0
                && (minimum_window..=3_600).contains(&self.twap_window_seconds)
                && self.max_observation_gap_seconds >= self.twap_window_seconds * 2
                && self.max_observation_gap_seconds <= 86_400
                && self.minimum_quote_liquidity_usd_e9 >= 10_000_000_000_000,
            ManteonError::InvalidPriceRule
        );
        match self.usd_quote_source {
            UsdQuoteSource::Usdc => (),
            UsdQuoteSource::PythSolUsd {
                price_update_account,
                receiver_program,
                feed_id,
                max_age_seconds,
                max_confidence_bps,
            } => require!(
                price_update_account != Pubkey::default()
                    && crate::integration::price::is_pyth_receiver(receiver_program)
                    && feed_id == crate::integration::price::SOL_USD_FEED_ID
                    && (1..=120).contains(&max_age_seconds)
                    && (1..=100).contains(&max_confidence_bps),
                ManteonError::InvalidPriceRule
            ),
            UsdQuoteSource::RaydiumSolUsdc { pool_id } => require!(
                pool_id != Pubkey::default() && pool_id != self.pool_id,
                ManteonError::InvalidPriceRule
            ),
        }
        Ok(())
    }
}
