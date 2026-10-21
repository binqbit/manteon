//! Parallel audit custody and the market's single, irreversible acceptance point.

use anchor_lang::{AccountDeserialize, AccountSerialize, InstructionData, prelude::Clock};
use manteon::{
    error::ManteonError,
    state::{Event, PoolState},
};
use solana_instruction_error::InstructionError;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;
use tekmerion_sdk::instructions::{self, ClaimDestination};

use super::support::{
    AUDIT_MINT, DAY, FORECAST, OPEN, TestContext,
    audited::{Answer, AuditFixture},
    result::ResultFixture,
    settlement::CaseFixture,
    setup::{
        config::{address, pubkey},
        pool::PoolContext,
        token,
    },
    tekmerion::case_state,
};

fn invalid_state() -> TransactionError {
    TransactionError::InstructionError(
        0,
        InstructionError::Custom(ManteonError::InvalidPoolState.into()),
    )
}

fn pair(context: &mut TestContext) -> (CaseFixture, CaseFixture, solana_address::Address) {
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    let source = token::create_account(
        &mut context.svm,
        &context.payer,
        0xdf,
        address(AUDIT_MINT),
        bob.pubkey(),
        300,
    );
    context.activate();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );
    context.set_time((FORECAST + 1) * DAY);
    let first = ResultFixture {
        occurred_at: FORECAST * DAY,
    };
    let second = ResultFixture {
        occurred_at: (FORECAST + 1) * DAY,
    };
    let a = CaseFixture::start(context, &first);
    let (b, ix) =
        CaseFixture::instruction_for(context, &second, pubkey(bob.pubkey()), pubkey(source));
    context.send(&[ix], &[&bob]);
    (a, b, source)
}

#[test]
fn either_final_yes_can_win_once_even_if_another_case_was_created_later() {
    for first_author_wins in [true, false] {
        let mut context = TestContext::new();
        let (a, b, _) = pair(&mut context);
        a.finalize_yes(&mut context);
        b.finalize_yes(&mut context);
        let (winner, loser) = if first_author_wins {
            (&a, &b)
        } else {
            (&b, &a)
        };
        // Anyone can accept a validated result; the service cannot withhold it.
        let bob = context.bob.insecure_clone();
        context.send(&[winner.begin_ix(&context, &bob)], &[&bob]);
        let pool = context.svm.get_account(&context.pool.pubkey()).unwrap();
        let event_key = address(PoolContext::event(&context.pool));
        let event = context.svm.get_account(&event_key).unwrap();
        assert_eq!(context.pool_state().accepted_case, Some(winner.key));
        assert_eq!(context.pool_state().state, PoolState::Settling);
        assert_eq!(
            Event::try_deserialize(&mut event.data.as_slice())
                .unwrap()
                .result,
            Some(winner.result)
        );
        context.reject(&[loser.begin_ix(&context, &bob)], &[&bob], invalid_state());
        let candidate = ResultFixture {
            occurred_at: winner.result.occurred_at,
        };
        let alice = context.alice.insecure_clone();
        let (_, ix_alice) = CaseFixture::instruction(&context, &candidate);
        context.reject(&[ix_alice], &[&alice], invalid_state());
        assert_eq!(
            context.svm.get_account(&context.pool.pubkey()).unwrap(),
            pool
        );
        assert_eq!(context.svm.get_account(&event_key).unwrap(), event);
    }
}

#[test]
fn acceptance_checks_the_selected_cases_result_preimage_before_freezing_capital() {
    let mut context = TestContext::new();
    let (a, b, _) = pair(&mut context);
    a.finalize_yes(&mut context);
    b.finalize_yes(&mut context);
    let caller = context.bob.insecure_clone();
    let pool_before = context.svm.get_account(&context.pool.pubkey()).unwrap();
    let event_key = address(PoolContext::event(&context.pool));
    let event_before = context.svm.get_account(&event_key).unwrap();
    let vault_key = address(PoolContext::vault(&context.pool));
    let vault_before = context.svm.get_account(&vault_key).unwrap();

    // Both timestamps have genuine final Yes reviews, but the review selected
    // by the caller must commit to the exact result being applied.
    let mut swapped = a.begin_ix(&context, &caller);
    swapped.data = manteon::instruction::BeginSettlement { data: b.result }.data();
    context.reject(
        &[swapped],
        &[&caller],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ManteonError::InvalidAuditAccount.into()),
        ),
    );
    for (key, expected) in [
        (context.pool.pubkey(), pool_before),
        (event_key, event_before),
        (vault_key, vault_before),
    ] {
        assert_eq!(context.svm.get_account(&key).unwrap(), expected);
    }

    context.send(&[a.begin_ix(&context, &caller)], &[&caller]);
    assert_eq!(context.pool_state().accepted_case, Some(a.key));
    assert_eq!(context.event_state().result, Some(a.result));
}

#[test]
fn matching_yes_results_still_require_the_accepted_case_for_every_settlement_step() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    let service = context.service.insecure_clone();
    let bob_source = token::create_account(
        &mut context.svm,
        &context.payer,
        0xe6,
        address(AUDIT_MINT),
        bob.pubkey(),
        100,
    );
    context.activate();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );
    context.set_time(FORECAST * DAY);
    let result = ResultFixture::new(&context);
    let winner = CaseFixture::start(&mut context, &result);
    let (peer, start) =
        CaseFixture::instruction_for(&context, &result, pubkey(bob.pubkey()), pubkey(bob_source));
    context.send(&[start], &[&bob]);
    winner.finalize_yes(&mut context);
    peer.finalize_yes(&mut context);
    assert_eq!(
        case_state(&context, &winner).result_hash,
        case_state(&context, &peer).result_hash
    );
    context.send(&[winner.begin_ix(&context, &service)], &[&service]);
    let frozen = context.svm.get_account(&context.pool.pubkey()).unwrap();
    let position_key = address(context.position(FORECAST));
    let unprocessed = context.svm.get_account(&position_key).unwrap();

    context.reject(
        &[peer.process_ix(&context, &service, &[FORECAST])],
        &[&service],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ManteonError::InvalidAuditAccount.into()),
        ),
    );
    assert_eq!(
        context.svm.get_account(&context.pool.pubkey()).unwrap(),
        frozen
    );
    assert_eq!(context.svm.get_account(&position_key).unwrap(), unprocessed);

    context.send(
        &[winner.process_ix(&context, &service, &[FORECAST])],
        &[&service],
    );
    let processed = context.svm.get_account(&context.pool.pubkey()).unwrap();
    context.reject(
        &[peer.finalize_ix(&context, &service)],
        &[&service],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ManteonError::InvalidAuditAccount.into()),
        ),
    );
    assert_eq!(
        context.svm.get_account(&context.pool.pubkey()).unwrap(),
        processed
    );
    context.send(&[winner.finalize_ix(&context, &service)], &[&service]);
    assert_eq!(context.pool_state().state, PoolState::Claimable);
    assert_eq!(context.pool_state().accepted_case, Some(winner.key));
    assert_eq!(context.event_state().result, Some(winner.result));
}

#[test]
fn another_authors_yes_does_not_cancel_a_funded_case_or_its_claims() {
    let mut context = TestContext::new();
    let audit = AuditFixture::initialize(&mut context);
    let (a, b, source) = pair(&mut context);
    audit.fund(&mut context, &a);
    // Bob's unchallenged Case becomes Yes without waiting for Alice's audit.
    b.finalize_yes(&mut context);
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    context.send(&[b.begin_ix(&context, &alice)], &[&alice]);
    let accepted_pool = context.svm.get_account(&context.pool.pubkey()).unwrap();
    // Alice's case still owns its independent audit deadline and custody.
    assert!(
        context.svm.get_sysvar::<Clock>().unix_timestamp
            < case_state(&context, &a).audit.deadline.unwrap()
    );
    audit.deliver(&mut context, &a, Answer::No);
    audit.finalize(&mut context, &a, Answer::No);
    context.send(
        &[instructions::claim(
            a.identity,
            pubkey(alice.pubkey()),
            ClaimDestination {
                owner: pubkey(alice.pubkey()),
                destination: pubkey(context.audit_source),
            },
        )],
        &[&alice],
    );
    let before = context.token_balance(source);
    context.send(
        &[instructions::claim(
            b.identity,
            pubkey(bob.pubkey()),
            ClaimDestination {
                owner: pubkey(bob.pubkey()),
                destination: pubkey(source),
            },
        )],
        &[&bob],
    );
    assert_eq!(context.token_balance(source), before + 100);
    assert_eq!(
        context.svm.get_account(&context.pool.pubkey()).unwrap(),
        accepted_pool
    );
    assert_eq!(context.pool_state().accepted_case, Some(b.key));
}

#[test]
fn service_failure_opens_competition_early_and_the_public_latch_survives_retries() {
    for answer in [Some(Answer::No), Some(Answer::Unknown), None] {
        let mut context = TestContext::with_schedule(OPEN, super::support::LOCK, OPEN + 200 * DAY);
        let audit = AuditFixture::initialize(&mut context);
        let service = context.service.insecure_clone();
        let alice = context.alice.insecure_clone();
        let bob = context.bob.insecure_clone();
        let service_source = token::create_account(
            &mut context.svm,
            &context.payer,
            0xe1,
            address(AUDIT_MINT),
            service.pubkey(),
            300,
        );
        let bob_source = token::create_account(
            &mut context.svm,
            &context.payer,
            0xe2,
            address(AUDIT_MINT),
            bob.pubkey(),
            100,
        );
        context.activate();
        context.set_time(OPEN + DAY);
        let result = ResultFixture::new(&context);
        let (service_case, start) = CaseFixture::instruction_for(
            &context,
            &result,
            pubkey(service.pubkey()),
            pubkey(service_source),
        );
        context.send(&[start], &[&service]);
        let (_, denied) = CaseFixture::instruction(&context, &result);
        context.reject(
            std::slice::from_ref(&denied),
            &[&alice],
            TransactionError::InstructionError(
                0,
                InstructionError::Custom(ManteonError::ServiceWindow.into()),
            ),
        );
        let mut proof = denied.clone();
        assert_eq!(proof.accounts[7].pubkey, address(manteon::ID));
        proof.accounts[7].pubkey = address(service_case.key);
        context.reject(
            &[proof.clone()],
            &[&alice],
            TransactionError::InstructionError(
                0,
                InstructionError::Custom(ManteonError::InvalidAuditAccount.into()),
            ),
        );
        audit.fund(&mut context, &service_case);
        if let Some(answer) = answer {
            audit.deliver(&mut context, &service_case, answer);
            audit.finalize(&mut context, &service_case, answer);
        } else {
            context.set_time(case_state(&context, &service_case).audit.deadline.unwrap());
            context.send(
                &[instructions::finalize_timeout(service_case.identity)],
                &[],
            );
        }
        // Finalized failure alone does not mutate Manteon. It is authenticated
        // when supplied to the first public proposal, and the opening persists.
        assert!(!context.pool_state().public_proposals);
        context.send(&[proof], &[&alice]);
        assert!(context.pool_state().public_proposals);
        assert_eq!(context.pool_state().service_eligible_at, Some(OPEN + DAY));
        let (_, bob_start) = CaseFixture::instruction_for(
            &context,
            &result,
            pubkey(bob.pubkey()),
            pubkey(bob_source),
        );
        context.send(&[bob_start], &[&bob]);
        let (_, mut service_retry) = CaseFixture::instruction_for(
            &context,
            &result,
            pubkey(service.pubkey()),
            pubkey(service_source),
        );
        service_retry
            .accounts
            .push(solana_instruction::AccountMeta::new_readonly(
                address(service_case.key),
                false,
            ));
        context.send(&[service_retry], &[&service]);
        assert!(context.pool_state().public_proposals);
        assert_eq!(context.pool_state().accepted_case, None);
        assert_eq!(context.pool_state().service_eligible_at, Some(OPEN + DAY));
    }
}

#[test]
fn service_yes_keeps_proposal_priority_but_any_wallet_can_apply_its_result() {
    let mut context = TestContext::with_schedule(OPEN, super::support::LOCK, OPEN + 200 * DAY);
    let service = context.service.insecure_clone();
    let alice = context.alice.insecure_clone();
    let source = token::create_account(
        &mut context.svm,
        &context.payer,
        0xe4,
        address(AUDIT_MINT),
        service.pubkey(),
        100,
    );
    context.activate();
    context.set_time(OPEN + DAY);
    let result = ResultFixture::new(&context);
    let (case, ix) =
        CaseFixture::instruction_for(&context, &result, pubkey(service.pubkey()), pubkey(source));
    context.send(&[ix], &[&service]);
    case.finalize_yes(&mut context);
    let (_, mut proposal) = CaseFixture::instruction(&context, &result);
    proposal.accounts[7].pubkey = address(case.key);
    context.reject(
        &[proposal],
        &[&alice],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ManteonError::InvalidAuditAccount.into()),
        ),
    );
    assert!(!context.pool_state().public_proposals);
    context.send(&[case.begin_ix(&context, &alice)], &[&alice]);
    assert_eq!(context.pool_state().accepted_case, Some(case.key));
    assert_eq!(context.pool_state().state, PoolState::Refunding);
}

#[test]
fn public_access_proof_requires_the_exact_service_market_policy_and_agreement() {
    for mismatch in ["author", "policy", "subject", "agreement"] {
        let mut context = TestContext::with_schedule(OPEN, super::support::LOCK, OPEN + 200 * DAY);
        let audit = AuditFixture::initialize(&mut context);
        let service = context.service.insecure_clone();
        let alice = context.alice.insecure_clone();
        let source = token::create_account(
            &mut context.svm,
            &context.payer,
            0xe5,
            address(AUDIT_MINT),
            service.pubkey(),
            100,
        );
        context.activate();
        context.set_time(OPEN + DAY);
        let result = ResultFixture::new(&context);
        let (service_case, start) = CaseFixture::instruction_for(
            &context,
            &result,
            pubkey(service.pubkey()),
            pubkey(source),
        );
        context.send(&[start], &[&service]);
        audit.fund(&mut context, &service_case);
        audit.deliver(&mut context, &service_case, Answer::No);
        audit.finalize(&mut context, &service_case, Answer::No);

        let original = context.svm.get_account(&address(service_case.key)).unwrap();
        let (_, mut proposal) = CaseFixture::instruction(&context, &result);
        let mut foreign = case_state(&context, &service_case);
        match mismatch {
            "author" => foreign.author = pubkey(alice.pubkey()),
            "policy" => foreign.policy = anchor_lang::prelude::Pubkey::new_from_array([0xf1; 32]),
            "subject" => foreign.subject = anchor_lang::prelude::Pubkey::new_from_array([0xf2; 32]),
            "agreement" => foreign.agreement_hash[0] ^= 1,
            _ => unreachable!(),
        }
        // Keep owner, schema and PDA internally valid. This exercises the new
        // access proof's market bindings, rather than only a bad-address guard.
        let (foreign_key, bump) = tekmerion_sdk::program::Case::address(
            &foreign.policy,
            &foreign.subject,
            &foreign.author,
            foreign.nonce,
        );
        foreign.bump = bump;
        foreign.vault_bump = tekmerion_sdk::program::Case::vault_address(&foreign_key).1;
        let mut foreign_account = original.clone();
        foreign_account.data.fill(0);
        foreign
            .try_serialize(&mut foreign_account.data.as_mut_slice())
            .unwrap();
        context
            .svm
            .set_account(address(foreign_key), foreign_account)
            .unwrap();
        proposal.accounts[7].pubkey = address(foreign_key);

        let pool = context.svm.get_account(&context.pool.pubkey()).unwrap();
        let event_key = address(PoolContext::event(&context.pool));
        let event = context.svm.get_account(&event_key).unwrap();
        let funds = context.svm.get_account(&context.audit_source).unwrap();
        let uses = context.policy_uses();
        context.reject(
            &[proposal.clone()],
            &[&alice],
            TransactionError::InstructionError(
                0,
                InstructionError::Custom(ManteonError::InvalidAuditAccount.into()),
            ),
        );
        assert_eq!(
            context.svm.get_account(&context.pool.pubkey()).unwrap(),
            pool,
            "{mismatch}"
        );
        assert_eq!(
            context.svm.get_account(&event_key).unwrap(),
            event,
            "{mismatch}"
        );
        assert_eq!(
            context.svm.get_account(&context.audit_source).unwrap(),
            funds,
            "{mismatch}"
        );
        assert_eq!(context.policy_uses(), uses, "{mismatch}");

        // The real service failure still opens access. The rejected proof did
        // not consume a bond, open competition or reset the first proposal time.
        context
            .svm
            .set_account(address(service_case.key), original)
            .unwrap();
        if foreign_key != service_case.key {
            context
                .svm
                .set_account(address(foreign_key), solana_account::Account::default())
                .unwrap();
        }
        proposal.accounts[7].pubkey = address(service_case.key);
        context.send(&[proposal], &[&alice]);
        assert!(context.pool_state().public_proposals);
        assert_eq!(context.pool_state().service_eligible_at, Some(OPEN + DAY));
    }
}
