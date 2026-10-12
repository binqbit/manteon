//! Public Tekmerion Case decoding and real challenge funding shared by audit paths.

use anchor_lang::AccountDeserialize;
use solana_address::Address;
use solana_signer::Signer;
use tekmerion_sdk::{instructions, program::CasePhase};

use super::{
    context::{AUDIT_MINT, TEKMERION, TestContext},
    settlement::CaseFixture,
    setup::config::{address, pubkey},
};

/// Decode the account using the same versioned SDK as the integrated program.
pub fn case_state(context: &TestContext, case: &CaseFixture) -> tekmerion_sdk::program::Case {
    let account = context.svm.get_account(&address(case.key)).unwrap();
    assert_eq!(account.owner, address(TEKMERION));
    assert_eq!(account.data.len(), tekmerion_sdk::program::Case::SPACE);
    tekmerion_sdk::program::Case::try_deserialize(&mut account.data.as_slice()).unwrap()
}

pub struct ChallengeFixture {
    pub source: Address,
}

impl ChallengeFixture {
    pub fn initialize(context: &mut TestContext, seed: u8, amount: u64) -> Self {
        Self {
            source: super::setup::token::create_account(
                &mut context.svm,
                &context.payer,
                seed,
                address(AUDIT_MINT),
                context.bob.pubkey(),
                amount,
            ),
        }
    }

    /// Meet the real fixture Policy's challenge target without supplying an answer.
    pub fn fund(&self, context: &mut TestContext, case: &CaseFixture) {
        let bob = context.bob.insecure_clone();
        let before = context.token_balance(self.source);
        context.send(
            &[instructions::contribute(
                case.identity,
                instructions::Contribution {
                    payer: pubkey(context.payer.pubkey()),
                    owner: pubkey(bob.pubkey()),
                    source: pubkey(self.source),
                },
                100,
            )],
            &[&bob],
        );
        assert_eq!(context.token_balance(self.source), before - 100);
        assert_eq!(context.token_balance(address(case.vault)), 200);
        assert_eq!(case_state(context, case).phase(), CasePhase::Funded); // Funded, no answer yet.
    }
}
