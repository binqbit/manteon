//! Real committee signatures; only the quorum-completing vote finalizes a Case.

use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_signer::Signer;
use tekmerion_sdk::{
    instructions,
    program::{AuditReason, CasePhase, CaseResult, CompletionMethod},
};

pub use tekmerion_sdk::program::AuditAnswer as Answer;

use super::{
    context::TestContext,
    settlement::CaseFixture,
    setup::config::{address, pubkey},
    tekmerion::{ChallengeFixture, case_state},
};

pub struct AuditFixture {
    challenge: ChallengeFixture,
}

impl AuditFixture {
    pub fn initialize(context: &mut TestContext) -> Self {
        Self {
            challenge: ChallengeFixture::initialize(context, 0xc5, 1_000),
        }
    }

    pub fn fund(&self, context: &mut TestContext, case: &CaseFixture) {
        self.challenge.fund(context, case);
    }

    pub fn vote_ix(
        &self,
        case: &CaseFixture,
        auditor: &Keypair,
        answer: Answer,
        fee_destinations: &[anchor_lang::prelude::Pubkey],
    ) -> Instruction {
        instructions::submit_audit(
            case.identity,
            pubkey(auditor.pubkey()),
            answer,
            if answer == Answer::Unknown {
                AuditReason::Inconclusive
            } else {
                AuditReason::Decisive
            },
            fee_destinations,
        )
    }

    /// One authentic vote is insufficient for the fixture's two-vote quorum.
    pub fn deliver(&self, context: &mut TestContext, case: &CaseFixture, answer: Answer) {
        let auditor = context.auditors[0].insecure_clone();
        let before = context.pool_state().principal;
        context.send(&[self.vote_ix(case, &auditor, answer, &[])], &[&auditor]);
        let state = case_state(context, case);
        assert_eq!(state.phase(), CasePhase::Funded);
        assert_eq!(state.audit.votes.len(), 1);
        assert_eq!(state.audit.votes[0].response.answer, answer);
        assert_eq!(state.audit.response, None);
        assert_eq!((state.result(), state.completion_method()), (None, None));
        assert_eq!(context.pool_state().principal, before);
    }

    /// The second signature atomically settles the audit and pays its fee.
    pub fn finalize(&self, context: &mut TestContext, case: &CaseFixture, answer: Answer) {
        let auditor = context.auditors[1].insecure_clone();
        let destinations = context.auditor_tokens[..2]
            .iter()
            .copied()
            .map(pubkey)
            .collect::<Vec<_>>();
        context.send(
            &[self.vote_ix(case, &auditor, answer, &destinations)],
            &[&auditor],
        );
        let result = match answer {
            Answer::Yes => CaseResult::Yes,
            Answer::No => CaseResult::No,
            Answer::Unknown => CaseResult::Unknown,
        };
        let state = case_state(context, case);
        assert_eq!(
            (state.phase(), state.result(), state.completion_method()),
            (
                CasePhase::Finalized,
                Some(result),
                Some(CompletionMethod::Audited)
            ),
        );
        assert_eq!(context.token_balance(address(case.vault)), 193);
    }
}
