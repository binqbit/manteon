use anchor_lang::{InstructionData, error::ErrorCode};
use manteon::{error::ManteonError, state::PoolState};
use solana_instruction::{AccountMeta, Instruction};
use solana_instruction_error::InstructionError;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;

use super::support::{
    AUDIT_MINT, DAY, FORECAST, OPEN, OWNER_BALANCE, TestContext,
    result::ResultFixture,
    settlement::{CaseFixture, claim_ix},
    setup::{
        config::{address, pubkey},
        pool::PoolContext,
        token,
    },
    tekmerion::case_state,
};

fn custom(code: impl Into<u32>) -> TransactionError {
    TransactionError::InstructionError(0, InstructionError::Custom(code.into()))
}

fn replace_account(
    instruction: &mut Instruction,
    old: anchor_lang::prelude::Pubkey,
    new: anchor_lang::prelude::Pubkey,
) {
    let meta = instruction
        .accounts
        .iter_mut()
        .find(|meta| meta.pubkey == address(old))
        .unwrap();
    meta.pubkey = address(new);
}

#[test]
fn candidate_submission_authenticates_author_asset_client_and_auditor_before_creating_a_case() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    let bob_source = token::create_account(
        &mut context.svm,
        &context.payer,
        0xdc,
        address(AUDIT_MINT),
        bob.pubkey(),
        100,
    );
    context.activate();
    let mut result = ResultFixture::new(&context);
    context.set_time(FORECAST * DAY);
    result.record_time(&context);
    let (case, valid) = CaseFixture::instruction(&context, &result);

    for (account, replacement, error) in [
        (
            pubkey(context.audit_source),
            pubkey(context.alice_token),
            ErrorCode::ConstraintTokenMint,
        ),
        (
            pubkey(context.audit_source),
            pubkey(bob_source),
            ErrorCode::ConstraintTokenOwner,
        ),
        (
            context.client_authority,
            pubkey(context.service.pubkey()),
            ErrorCode::ConstraintSeeds,
        ),
        (tekmerion_sdk::ID, manteon::ID, ErrorCode::InvalidProgramId),
        (
            case.identity.receipt(&pubkey(alice.pubkey())),
            case.identity.receipt(&pubkey(bob.pubkey())),
            ErrorCode::ConstraintSeeds,
        ),
    ] {
        let mut invalid = valid.clone();
        replace_account(&mut invalid, account, replacement);
        context.reject(&[invalid], &[&alice], custom(error));
    }
    let mut unsigned = valid.clone();
    unsigned
        .accounts
        .iter_mut()
        .find(|meta| meta.pubkey == alice.pubkey())
        .unwrap()
        .is_signer = false;
    context.reject(&[unsigned], &[], custom(ErrorCode::AccountNotSigner));
    let mut mismatched_candidate = valid.clone();
    mismatched_candidate.data = manteon::instruction::ProposeExternalEvent {
        data: manteon::state::EventResult {
            extra: Some(manteon::state::EventResultExtra::PriceVerification {
                lower_usd_e9: 2_000_000_000,
                upper_usd_e9: 2_000_000_001,
            }),
            occurred_at: 1_800_000_000,
        },
    }
    .data();
    context.reject(
        &[mismatched_candidate],
        &[&alice],
        custom(ManteonError::InvalidEventResult),
    );
    assert_eq!(context.pool_state().current_case, None);
    assert_eq!(context.pool_state().next_case_nonce, 0);
    assert_eq!(context.policy_uses(), 1);
    assert!(context.svm.get_account(&address(case.key)).is_none());
    context.send(&[valid], &[&alice]);
    assert_eq!(case_state(&context, &case).author, pubkey(alice.pubkey()));
    assert_eq!(context.token_balance(address(case.vault)), 100);
}

#[test]
fn shared_result_never_authorizes_a_foreign_pool_case_or_event_and_acceptance_is_final() {
    let mut context = TestContext::new();
    let sibling = context.create_pool(0xdd);
    context.activate();
    let primary = std::mem::replace(&mut context.pool, sibling);
    context.activate();
    let sibling = std::mem::replace(&mut context.pool, primary);
    assert_eq!(context.policy_uses(), 2);
    let alice = context.alice.insecure_clone();
    let service = context.service.insecure_clone();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );
    let primary = std::mem::replace(&mut context.pool, sibling);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 300)],
        &[&alice],
    );
    let sibling = std::mem::replace(&mut context.pool, primary);
    assert_eq!(
        context.token_balance(context.alice_token),
        OWNER_BALANCE - 400
    );
    let mut result = ResultFixture::new(&context);
    let mut other_result = ResultFixture::new(&context);
    context.set_time(FORECAST * DAY);
    result.record_time(&context);
    other_result.record_time(&context);
    other_result.occurred_at -= 1;
    let primary_case = CaseFixture::start(&mut context, &result);
    let primary = std::mem::replace(&mut context.pool, sibling);
    let sibling_case = CaseFixture::start(&mut context, &result);
    sibling_case.finalize_yes(&mut context);
    let sibling = std::mem::replace(&mut context.pool, primary);
    primary_case.finalize_yes(&mut context);
    assert_ne!(primary_case.key, sibling_case.key);
    let foreign_pool = context.svm.get_account(&sibling.pubkey()).unwrap();
    let foreign_event = context
        .svm
        .get_account(&address(PoolContext::event(&sibling)))
        .unwrap();
    let foreign_case = context.svm.get_account(&address(sibling_case.key)).unwrap();
    let foreign_vault = context
        .svm
        .get_account(&address(PoolContext::vault(&sibling)))
        .unwrap();
    let mut foreign_result = primary_case.begin_ix(&context, &service);
    replace_account(&mut foreign_result, primary_case.key, sibling_case.key);
    context.reject(
        &[foreign_result],
        &[&service],
        custom(ManteonError::InvalidAuditAccount),
    );
    let mut foreign_event_ix = primary_case.begin_ix(&context, &service);
    replace_account(
        &mut foreign_event_ix,
        PoolContext::event(&context.pool),
        PoolContext::event(&sibling),
    );
    context.reject(
        &[foreign_event_ix],
        &[&service],
        custom(ErrorCode::ConstraintRaw),
    );
    context.send(&[primary_case.begin_ix(&context, &service)], &[&service]);
    assert_eq!(context.pool_state().state, PoolState::Settling);
    context.reject(
        &[primary_case.begin_ix(&context, &service)],
        &[&service],
        custom(ManteonError::InvalidPoolState),
    );
    let (_, mut replacement) = CaseFixture::instruction(&context, &other_result);
    replacement
        .accounts
        .push(AccountMeta::new_readonly(address(primary_case.key), false));
    context.reject(
        &[replacement],
        &[&alice],
        custom(ManteonError::InvalidPoolState),
    );
    context.send(
        &[
            primary_case.process_ix(&context, &service, &[FORECAST]),
            primary_case.finalize_ix(&context, &service),
        ],
        &[&service],
    );
    context.send(
        &[claim_ix(
            &context,
            &alice,
            FORECAST,
            pubkey(context.alice_token),
        )],
        &[&alice],
    );
    assert_eq!(
        context.token_balance(context.alice_token),
        OWNER_BALANCE - 300
    );
    assert_eq!(context.pool_state().principal, 100);
    assert_eq!(context.pool_state().payout_remaining, 0);
    assert_eq!(
        context.token_balance(address(PoolContext::vault(&context.pool))),
        0
    );
    for (key, expected) in [
        (sibling.pubkey(), foreign_pool),
        (address(PoolContext::event(&sibling)), foreign_event),
        (address(sibling_case.key), foreign_case),
        (address(PoolContext::vault(&sibling)), foreign_vault),
    ] {
        assert_eq!(context.svm.get_account(&key).unwrap(), expected);
    }
    let _primary = std::mem::replace(&mut context.pool, sibling);
    context.send(
        &[
            sibling_case.begin_ix(&context, &service),
            sibling_case.process_ix(&context, &service, &[FORECAST]),
            sibling_case.finalize_ix(&context, &service),
        ],
        &[&service],
    );
    context.send(
        &[claim_ix(
            &context,
            &alice,
            FORECAST,
            pubkey(context.alice_token),
        )],
        &[&alice],
    );
    assert_eq!(context.token_balance(context.alice_token), OWNER_BALANCE);
    assert_eq!(context.pool_state().state, PoolState::Claimable);
    assert_eq!(context.pool_state().principal, 300);
    assert_eq!(context.pool_state().payout_remaining, 0);
    assert_eq!(
        context.token_balance(address(PoolContext::vault(&context.pool))),
        0
    );
    assert_eq!(context.policy_uses(), 4);
}

#[test]
fn competing_candidate_authors_serialize_one_case_and_one_bond_in_either_order() {
    for alice_first in [true, false] {
        let mut context = TestContext::new();
        let alice = context.alice.insecure_clone();
        let bob = context.bob.insecure_clone();
        let bob_source = token::create_account(
            &mut context.svm,
            &context.payer,
            0xde,
            address(AUDIT_MINT),
            bob.pubkey(),
            100,
        );
        context.activate();
        let mut first_result = ResultFixture::new(&context);
        let mut second_result = ResultFixture::new(&context);
        context.set_time(FORECAST * DAY);
        first_result.record_time(&context);
        second_result.record_time(&context);
        second_result.occurred_at -= 1;
        let (alice_case, alice_start) = CaseFixture::instruction(&context, &first_result);
        let (bob_case, bob_start) = CaseFixture::instruction_for(
            &context,
            &second_result,
            pubkey(bob.pubkey()),
            pubkey(bob_source),
        );
        assert_eq!(alice_case.key, bob_case.key);
        let (winner, winner_start, loser, loser_start, winning_result) = if alice_first {
            (&alice, alice_start, &bob, bob_start, &first_result)
        } else {
            (&bob, bob_start, &alice, alice_start, &second_result)
        };
        context.send(&[winner_start], &[winner]);
        let accepted_attempt = context.svm.get_account(&address(alice_case.key)).unwrap();
        context.reject(
            &[loser_start],
            &[loser],
            custom(ManteonError::InvalidAuditAccount),
        );
        let (next_case, mut concurrent_retry) = CaseFixture::instruction(&context, winning_result);
        concurrent_retry
            .accounts
            .push(AccountMeta::new_readonly(address(alice_case.key), false));
        context.reject(
            &[concurrent_retry],
            &[&alice],
            custom(ManteonError::InvalidAuditAccount),
        );
        assert!(context.svm.get_account(&address(next_case.key)).is_none());
        assert!(
            context
                .svm
                .get_account(&address(
                    alice_case.identity.receipt(&pubkey(loser.pubkey()))
                ))
                .is_none()
        );
        assert_eq!(
            context.svm.get_account(&address(alice_case.key)).unwrap(),
            accepted_attempt
        );
        assert_eq!(
            case_state(&context, &alice_case).author,
            pubkey(winner.pubkey())
        );
        assert_eq!(context.pool_state().current_case, Some(alice_case.key));
        assert_eq!(context.pool_state().next_case_nonce, 1);
        assert_eq!(context.token_balance(address(alice_case.vault)), 100);
        assert_eq!(context.policy_uses(), 2);
        assert_eq!(
            context.token_balance(context.audit_source),
            super::support::OWNER_BALANCE - if alice_first { 100 } else { 0 }
        );
        assert_eq!(
            context.token_balance(bob_source),
            if alice_first { 100 } else { 0 }
        );
    }
}
