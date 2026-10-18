//! Canonical result facts belong to the event variant that interprets them.

use anchor_lang::prelude::*;
use solana_sha256_hasher::hash;

use super::{EventDefinition, EventResult};
use crate::{error::ManteonError, result};

impl EventResult {
    /// Render validated event facts with the Pool's immutable agreement binding.
    pub fn document(
        &self,
        subject: &EventDefinition,
        pool: Pubkey,
        agreement_hash: &[u8; 32],
    ) -> Result<String> {
        self.validate(subject, result::MAX_TIMESTAMP)?;

        let facts = match subject {
            EventDefinition::ExternalEvent => String::new(),
            EventDefinition::TokenPriceThresholdCrossing { params } => {
                let bounds = self
                    .price_bounds()
                    .ok_or(ManteonError::InvalidEventResult)?;
                format!(
                    concat!(
                        "- Token mint: {mint}\n",
                        "{rule}",
                        "- Verified price lower USD nano: {lower}\n",
                        "- Verified price upper USD nano: {upper}\n",
                    ),
                    mint = params.token_mint,
                    rule = params.document_facts(),
                    lower = bounds.lower,
                    upper = bounds.upper,
                )
            }
        };

        result::render(
            pool,
            agreement_hash,
            subject.kind(),
            self.occurred_at,
            &facts,
        )
    }

    /// SHA-256 of the exact canonical document, derived from typed facts.
    pub fn document_hash(
        &self,
        subject: &EventDefinition,
        pool: Pubkey,
        agreement_hash: &[u8; 32],
    ) -> Result<[u8; 32]> {
        Ok(hash(self.document(subject, pool, agreement_hash)?.as_bytes()).to_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_commitment_binds_time_pool_and_agreement() {
        let pool = Pubkey::new_from_array([1; 32]);
        let subject = EventDefinition::ExternalEvent;
        let result = EventResult {
            extra: None,
            occurred_at: 1_800_000_000,
        };
        let hash = result.document_hash(&subject, pool, &[4; 32]).unwrap();
        assert_ne!(
            hash,
            result
                .document_hash(&subject, Pubkey::new_from_array([9; 32]), &[4; 32])
                .unwrap()
        );
        assert_ne!(
            hash,
            result.document_hash(&subject, pool, &[5; 32]).unwrap()
        );
        assert_ne!(
            hash,
            EventResult {
                extra: None,
                occurred_at: 1_800_000_001
            }
            .document_hash(&subject, pool, &[4; 32])
            .unwrap()
        );
        let text = result.document(&subject, pool, &[4; 32]).unwrap();
        assert!(text.contains("- Occurred at: 2027-01-15T08:00:00Z\n"));
        assert!(!text.contains("Public token") && !text.contains('\r'));
    }
}
