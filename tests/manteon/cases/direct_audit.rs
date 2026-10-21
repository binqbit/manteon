//! Author-paid review skips challenge funding, not public admission or audit finality.

use anchor_lang::{InstructionData, error::ErrorCode, prelude::Pubkey};
use manteon::{ProposalMode, error::ManteonError, state::PoolState};
use solana_instruction::{AccountMeta, Instruction};
use solana_instruction_error::InstructionError;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;
use tekmerion_sdk::{
    instructions,
    program::{AuditAnswer, AuditReason, CaseMode, CasePhase, CaseResult, CompletionMethod},
};

use super::support::{
    AUDIT_MINT, DAY, FORECAST, LOCK, OPEN, OWNER_BALANCE, TestContext,
    audited::{Answer, AuditFixture},
    result::ResultFixture,
    settlement::CaseFixture,
    setup::{
        config::{address, pubkey},
        token,
    },
    tekmerion::case_state,
};

const FEE: u64 = 7;

fn custom(code: impl Into<u32>) -> TransactionError {
    TransactionError::InstructionError(0, InstructionError::Custom(code.into()))
}

fn direct(
    context: &TestContext,
    result: &ResultFixture,
    author: Pubkey,
    source: Pubkey,
) -> (CaseFixture, Instruction) {
    let (case, mut ix) = CaseFixture::instruction_for(context, result, author, source);
    ix.data = manteon::instruction::ProposeExternalEvent {
        nonce: case.identity.nonce,
        data: case.result,
        mode: ProposalMode::DirectAudit {
            expected_audit_fee: FEE,
        },
    }
    .data();
    (case, ix)
}

fn decide(context: &mut TestContext, case: &CaseFixture, answer: AuditAnswer) {
    let before = context.auditor_tokens[..2]
        .iter()
        .map(|key| context.token_balance(*key))
        .collect::<Vec<_>>();
    let destinations = context.auditor_tokens[..2]
        .iter()
        .copied()
        .map(pubkey)
        .collect::<Vec<_>>();
    for index in 0..2 {
        let auditor = context.auditors[index].insecure_clone();
        context.send(
            &[instructions::submit_audit(
                case.identity,
                pubkey(auditor.pubkey()),
                answer,
                if answer == AuditAnswer::Unknown {
                    AuditReason::Inconclusive
                } else {
                    AuditReason::Decisive
                },
                if index == 0 { &[] } else { &destinations },
            )],
            &[&auditor],
        );
        if index == 0 {
            assert_eq!(case_state(context, case).phase(), CasePhase::Funded);
            assert_eq!(context.token_balance(address(case.vault)), FEE);
        }
    }
    assert_eq!(
        case_state(context, case).completion_method(),
        Some(CompletionMethod::Audited)
    );
    assert_eq!(context.token_balance(address(case.vault)), 0);
    assert_eq!(
        context.token_balance(context.auditor_tokens[0]),
        before[0] + 4
    );
    assert_eq!(
        context.token_balance(context.auditor_tokens[1]),
        before[1] + 3
    );
}

#[test]
fn direct_review_charges_only_the_fee_and_requires_a_real_quorum_for_every_answer() {
    for answer in [AuditAnswer::Yes, AuditAnswer::No, AuditAnswer::Unknown] {
        let mut context = TestContext::new();
        context.activate();
        let alice = context.alice.insecure_clone();
        let caller = context.bob.insecure_clone();
        context.set_time(OPEN);
        context.send(
            &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
            &[&alice],
        );
        context.set_time(FORECAST * DAY);
        let result = ResultFixture::new(&context);
        let (case, ix) = direct(
            &context,
            &result,
            pubkey(alice.pubkey()),
            pubkey(context.audit_source),
        );
        context.send(&[ix], &[&alice]);
        let created = case_state(&context, &case);
        assert_eq!(created.mode, CaseMode::DirectAudit);
        assert_eq!(created.author_principal, FEE);
        assert_eq!(
            (created.challenge_target, created.challenge_principal),
            (0, 0)
        );
        assert_eq!(created.phase(), CasePhase::Funded);
        assert_eq!(created.funding_deadline, FORECAST * DAY);
        assert_eq!(created.audit.deadline, Some(FORECAST * DAY + 300));
        assert_eq!(
            context.token_balance(context.audit_source),
            OWNER_BALANCE - FEE
        );
        assert_eq!(context.token_balance(address(case.vault)), FEE);
        assert!(context.event_state().result.is_none());
        context.reject(
            &[case.begin_ix(&context, &caller)],
            &[&caller],
            custom(ManteonError::InvalidAuditAccount),
        );
        context.reject(
            &[instructions::finalize_unchallenged(case.identity)],
            &[],
            custom(tekmerion_sdk::program::errors::TekmerionError::InvalidPhase),
        );

        decide(&mut context, &case, answer);
        assert_eq!(case_state(&context, &case).author_entitlement, 0);
        assert_eq!(case_state(&context, &case).contributor_entitlement, 0);
        assert_eq!(context.pool_state().principal, 100);
        if answer == AuditAnswer::Yes {
            context.send(&[case.begin_ix(&context, &caller)], &[&caller]);
            assert_eq!(context.pool_state().accepted_case, Some(case.key));
            assert_eq!(context.pool_state().state, PoolState::Settling);
            assert_eq!(context.event_state().result, Some(case.result));
        } else {
            context.reject(
                &[case.begin_ix(&context, &caller)],
                &[&caller],
                custom(ManteonError::InvalidAuditAccount),
            );
            let (retry, mut ix) = direct(
                &context,
                &result,
                pubkey(alice.pubkey()),
                pubkey(context.audit_source),
            );
            ix.accounts
                .push(AccountMeta::new_readonly(address(case.key), false));
            context.send(&[ix], &[&alice]);
            assert_eq!(case_state(&context, &retry).previous_case, Some(case.key));
            assert_eq!(
                context.token_balance(context.audit_source),
                OWNER_BALANCE - 2 * FEE
            );
            assert_eq!(context.pool_state().accepted_case, None);
        }
    }
}

#[test]
fn direct_fee_is_refunded_only_on_technical_timeout_and_retry_keeps_its_own_custody() {
    let mut context = TestContext::new();
    context.activate();
    let alice = context.alice.insecure_clone();
    context.set_time(FORECAST * DAY);
    let result = ResultFixture::new(&context);
    let (case, ix) = direct(
        &context,
        &result,
        pubkey(alice.pubkey()),
        pubkey(context.audit_source),
    );
    context.send(&[ix], &[&alice]);
    context.set_time(case_state(&context, &case).audit.deadline.unwrap());
    context.send(&[instructions::finalize_timeout(case.identity)], &[]);
    let completed = case_state(&context, &case);
    assert_eq!(
        completed.completion_method(),
        Some(CompletionMethod::TechnicalFailure)
    );
    assert_eq!(completed.result(), None);
    assert_eq!(completed.author_entitlement, FEE);
    let (retry, mut ix) = direct(
        &context,
        &result,
        pubkey(alice.pubkey()),
        pubkey(context.audit_source),
    );
    ix.accounts
        .push(AccountMeta::new_readonly(address(case.key), false));
    context.send(&[ix], &[&alice]);
    let claim = instructions::claim(
        case.identity,
        case.identity.author,
        instructions::ClaimDestination {
            owner: pubkey(alice.pubkey()),
            destination: pubkey(context.audit_source),
        },
    );
    context.send(std::slice::from_ref(&claim), &[&alice]);
    assert_eq!(
        context.token_balance(context.audit_source),
        OWNER_BALANCE - FEE
    );
    assert_eq!(context.token_balance(address(case.vault)), 0);
    assert_eq!(context.token_balance(address(retry.vault)), FEE);
    context.reject(
        &[claim],
        &[&alice],
        custom(tekmerion_sdk::program::errors::TekmerionError::AlreadyClaimed),
    );
}

#[test]
fn direct_proposals_require_public_opening_even_when_the_service_pays() {
    let horizon = OPEN + 100 * DAY;
    let public_at = horizon - 30 * DAY;
    for service_pays in [false, true] {
        let mut context = TestContext::with_schedule(OPEN, LOCK, horizon);
        let author = if service_pays {
            context.service.insecure_clone()
        } else {
            context.alice.insecure_clone()
        };
        let source = token::create_account(
            &mut context.svm,
            &context.payer,
            0xee,
            address(AUDIT_MINT),
            author.pubkey(),
            FEE,
        );
        context.activate();
        context.set_time(public_at - 1);
        let result = ResultFixture::new(&context);
        let (case, ix) = direct(&context, &result, pubkey(author.pubkey()), pubkey(source));
        context.reject(
            std::slice::from_ref(&ix),
            &[&author],
            custom(ManteonError::ServiceWindow),
        );
        assert!(context.svm.get_account(&address(case.key)).is_none());
        assert_eq!(context.token_balance(source), FEE);
        context.set_time(public_at);
        context.send(&[ix], &[&author]);
        assert!(context.pool_state().public_proposals);
        assert_eq!(context.pool_state().service_eligible_at, Some(public_at));
        assert_eq!(context.token_balance(source), 0);
        assert_eq!(context.token_balance(address(case.vault)), FEE);
    }
}

#[test]
fn failed_service_proof_opens_direct_review_and_a_paid_yes_cannot_replace_an_accepted_peer() {
    for failure in [Some(Answer::No), Some(Answer::Unknown), None] {
        let mut context = TestContext::with_schedule(OPEN, LOCK, OPEN + 100 * DAY);
        let audit = AuditFixture::initialize(&mut context);
        let service = context.service.insecure_clone();
        let alice = context.alice.insecure_clone();
        let service_source = token::create_account(
            &mut context.svm,
            &context.payer,
            0xed,
            address(AUDIT_MINT),
            service.pubkey(),
            100,
        );
        context.activate();
        context.set_time(FORECAST * DAY);
        let result = ResultFixture::new(&context);
        let (failed, ix) = CaseFixture::instruction_for(
            &context,
            &result,
            pubkey(service.pubkey()),
            pubkey(service_source),
        );
        context.send(&[ix], &[&service]);
        audit.fund(&mut context, &failed);
        if let Some(answer) = failure {
            audit.deliver(&mut context, &failed, answer);
            audit.finalize(&mut context, &failed, answer);
        } else {
            context.set_time(case_state(&context, &failed).audit.deadline.unwrap());
            context.send(&[instructions::finalize_timeout(failed.identity)], &[]);
        }
        let (paid, mut start) = direct(
            &context,
            &result,
            pubkey(alice.pubkey()),
            pubkey(context.audit_source),
        );
        start.accounts[7] = AccountMeta::new_readonly(address(failed.key), false);
        context.send(&[start], &[&alice]);
        assert!(context.pool_state().public_proposals);
        assert_eq!(
            context.pool_state().service_eligible_at,
            Some(FORECAST * DAY)
        );
        let (peer, mut start) = CaseFixture::instruction_for(
            &context,
            &result,
            pubkey(service.pubkey()),
            pubkey(service_source),
        );
        // Refill the service author's independent source after its rejected bond.
        context.send(
            &[anchor_spl::token::spl_token::instruction::transfer(
                &anchor_spl::token::spl_token::ID,
                &pubkey(context.audit_source),
                &pubkey(service_source),
                &pubkey(alice.pubkey()),
                &[],
                100,
            )
            .unwrap()],
            &[&alice],
        );
        start
            .accounts
            .push(AccountMeta::new_readonly(address(failed.key), false));
        context.send(&[start], &[&service]);
        peer.finalize_yes(&mut context);
        context.send(&[peer.begin_ix(&context, &alice)], &[&alice]);
        decide(&mut context, &paid, AuditAnswer::Yes);
        context.reject(
            &[paid.begin_ix(&context, &alice)],
            &[&alice],
            custom(ManteonError::InvalidPoolState),
        );
        assert_eq!(context.pool_state().accepted_case, Some(peer.key));
        assert_eq!(case_state(&context, &paid).result(), Some(CaseResult::Yes));
    }
}

#[test]
fn direct_proposal_cannot_spend_another_authors_token_account() {
    let mut context = TestContext::new();
    context.activate();
    context.set_time(FORECAST * DAY);
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    let source = token::create_account(
        &mut context.svm,
        &context.payer,
        0xec,
        address(AUDIT_MINT),
        bob.pubkey(),
        FEE,
    );
    let result = ResultFixture::new(&context);
    let (case, ix) = direct(&context, &result, pubkey(alice.pubkey()), pubkey(source));
    context.reject(&[ix], &[&alice], custom(ErrorCode::ConstraintTokenOwner));
    assert_eq!(context.token_balance(source), FEE);
    assert!(context.svm.get_account(&address(case.key)).is_none());
}

#[test]
fn direct_proposal_opens_at_the_first_service_proposal_deadline_even_while_it_is_pending() {
    let mut context = TestContext::with_schedule(OPEN, LOCK, OPEN + 100 * DAY);
    let service = context.service.insecure_clone();
    let alice = context.alice.insecure_clone();
    let source = token::create_account(
        &mut context.svm,
        &context.payer,
        0xeb,
        address(AUDIT_MINT),
        service.pubkey(),
        100,
    );
    context.activate();
    context.set_time(FORECAST * DAY);
    let result = ResultFixture::new(&context);
    let (service_case, ix) =
        CaseFixture::instruction_for(&context, &result, pubkey(service.pubkey()), pubkey(source));
    context.send(&[ix], &[&service]);
    let public_at = FORECAST * DAY + 30 * DAY;
    let (case, ix) = direct(
        &context,
        &result,
        pubkey(alice.pubkey()),
        pubkey(context.audit_source),
    );
    context.set_time(public_at - 1);
    context.reject(
        std::slice::from_ref(&ix),
        &[&alice],
        custom(ManteonError::ServiceWindow),
    );
    assert!(context.svm.get_account(&address(case.key)).is_none());
    context.set_time(public_at);
    context.send(&[ix], &[&alice]);
    assert!(context.pool_state().public_proposals);
    assert_eq!(
        context.pool_state().service_eligible_at,
        Some(FORECAST * DAY)
    );
    assert_eq!(
        case_state(&context, &service_case).phase(),
        CasePhase::Funding
    );
    assert_eq!(context.token_balance(address(service_case.vault)), 100);
    assert_eq!(context.token_balance(address(case.vault)), FEE);
}

#[test]
fn direct_fee_quote_and_available_funds_are_checked_atomically() {
    let mut context = TestContext::new();
    context.activate();
    context.set_time(FORECAST * DAY);
    let alice = context.alice.insecure_clone();
    let result = ResultFixture::new(&context);
    let (case, mut ix) = direct(
        &context,
        &result,
        pubkey(alice.pubkey()),
        pubkey(context.audit_source),
    );
    ix.data = manteon::instruction::ProposeExternalEvent {
        nonce: case.identity.nonce,
        data: case.result,
        mode: ProposalMode::DirectAudit {
            expected_audit_fee: FEE + 1,
        },
    }
    .data();
    context.reject(
        &[ix],
        &[&alice],
        custom(tekmerion_sdk::program::errors::TekmerionError::InvalidAuditFee),
    );
    assert!(context.svm.get_account(&address(case.key)).is_none());
    assert_eq!(context.token_balance(context.audit_source), OWNER_BALANCE);
    let source = token::create_account(
        &mut context.svm,
        &context.payer,
        0xea,
        address(AUDIT_MINT),
        alice.pubkey(),
        FEE - 1,
    );
    let (_, ix) = direct(&context, &result, pubkey(alice.pubkey()), pubkey(source));
    context.reject(&[ix], &[&alice], custom(1u32)); // SPL Token InsufficientFunds.
    assert!(context.svm.get_account(&address(case.key)).is_none());
    assert_eq!(context.token_balance(source), FEE - 1);
    assert_eq!(context.pool_state().service_eligible_at, None);
    assert!(!context.pool_state().public_proposals);
}
