use anchor_lang::prelude::*;

use crate::error::ManteonError;

mod document;
mod price;
#[cfg(test)]
mod tests;
pub use price::{CrossingDirection, PriceProtocol, PriceThreshold, UsdQuoteSource};

/// Immutable event rule and its occurrence facts.
// Event v2 rejects the former IPO layout, including accounts padded to the same size.
#[account(discriminator = [93, 52, 253, 229, 28, 199, 196, 160])]
#[derive(InitSpace)]
pub struct Event {
    // Reciprocal Pool identity and immutable subject.
    pub pool: Pubkey,
    pub definition: EventDefinition,

    /// Latest external proposal or verified price result. External acceptance requires its Case.
    pub result: Option<EventResult>,
}

impl Event {
    pub const SEED: &'static [u8] = b"event";
}

#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventDefinition {
    /// The subject and criteria are defined by the Pool's committed Agreement.
    ExternalEvent,
    TokenPriceThresholdCrossing {
        params: PriceThreshold,
    },
}

impl EventDefinition {
    pub fn token_mint(&self) -> Option<Pubkey> {
        match *self {
            Self::ExternalEvent => None,
            Self::TokenPriceThresholdCrossing { params } => Some(params.token_mint),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::ExternalEvent => "external_event",
            Self::TokenPriceThresholdCrossing { .. } => "token_price_threshold_crossing",
        }
    }
}

/// Occurrence facts: proposed for external review or verified directly from price sources.
#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventResult {
    pub occurred_at: i64,
    pub extra: Option<EventResultExtra>,
}

/// Additional verified facts; the event definition supplies the event type.
#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventResultExtra {
    PriceVerification {
        lower_usd_e9: u64,
        upper_usd_e9: u64,
    },
}

impl EventResult {
    pub fn occurred_at(&self) -> i64 {
        self.occurred_at
    }

    pub fn price_bounds(&self) -> Option<manteon_core::math::price::PriceBounds> {
        self.extra.map(|extra| match extra {
            EventResultExtra::PriceVerification {
                lower_usd_e9,
                upper_usd_e9,
            } => manteon_core::math::price::PriceBounds {
                lower: lower_usd_e9,
                upper: upper_usd_e9,
            },
        })
    }

    /// Verify the common time and event-specific facts. Instructions authenticate
    /// either the external audit or the fixed on-chain price sources.
    pub fn validate(&self, subject: &EventDefinition, now: i64) -> Result<()> {
        match (subject, self.extra) {
            (EventDefinition::ExternalEvent, None) => {}
            (
                EventDefinition::TokenPriceThresholdCrossing { params },
                Some(EventResultExtra::PriceVerification {
                    lower_usd_e9,
                    upper_usd_e9,
                }),
            ) => {
                params.validate()?;
                require!(
                    lower_usd_e9 <= upper_usd_e9
                        && params.direction.crossed(
                            manteon_core::math::price::PriceBounds {
                                lower: lower_usd_e9,
                                upper: upper_usd_e9
                            },
                            params.target_price_usd_e9,
                        ),
                    ManteonError::InvalidEventResult
                );
            }
            _ => return err!(ManteonError::InvalidEventResult),
        }

        let at = self.occurred_at();
        require!(
            (0..=crate::result::MAX_TIMESTAMP).contains(&at) && at <= now,
            ManteonError::InvalidEventResult
        );
        Ok(())
    }
}
