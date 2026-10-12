use anchor_lang::AccountDeserialize;
use manteon::{
    error::ManteonError,
    state::{Event, EventDefinition},
};
use solana_instruction::AccountMeta;
use solana_instruction_error::InstructionError;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;

use super::support::{
    DAY, FORECAST, LOCK, OPEN, OWNER_BALANCE, TestContext,
    audited::{Answer, AuditFixture},
    result::ResultFixture,
    settlement::CaseFixture,
    setup::config::{address, pubkey},
    setup::pool::PoolContext,
    setup::token,
    tekmerion::case_state,
};

fn service_window() -> TransactionError {
    TransactionError::InstructionError(
        0,
        InstructionError::Custom(ManteonError::ServiceWindow.into()),
    )
}

#[test]
fn horizon_submission_opens_at_its_own_deadline_while_service_can_submit_earlier() {
    let horizon = OPEN + 100 * DAY;
    let public_at = horizon - 30 * DAY;
    for service_submits in [false, true] {
        let mut context = TestContext::with_schedule(OPEN, LOCK, horizon);
        let service = context.service.insecure_clone();
        let alice = context.alice.insecure_clone();
        let service_source = token::create_account(
            &mut context.svm,
            &context.payer,
            0xda,
            address(super::support::AUDIT_MINT),
            service.pubkey(),
            100,
        );
        context.activate();
        let mut result = ResultFixture::new(&context);
        context.set_time(public_at - 1);
        result.record_time(&context);
        let (case, start) = if service_submits {
            CaseFixture::instruction_for(
                &context,
                &result,
                pubkey(service.pubkey()),
                pubkey(service_source),
            )
        } else {
            CaseFixture::instruction(&context, &result)
        };
        if !service_submits {
            context.reject(std::slice::from_ref(&start), &[&alice], service_window());
            assert_eq!(context.pool_state().service_eligible_at, None);
            context.set_time(public_at);
        }
        context.send(&[start], &[if service_submits { &service } else { &alice }]);
        let pool = context.pool_state();
        assert_eq!(
            (pool.opens_at, pool.lock_at, pool.horizon_at),
            (OPEN, LOCK, horizon)
        );
        assert_eq!(
            pool.service_eligible_at,
            Some(if service_submits {
                public_at - 1
            } else {
                public_at
            })
        );
        assert_eq!(
            (pool.current_case, pool.next_case_nonce),
            (Some(case.key), 1)
        );
        assert_eq!(context.policy_uses(), 2);
        assert_eq!(context.token_balance(address(case.vault)), 100);
        assert_eq!(
            case_state(&context, &case).author,
            pubkey(if service_submits {
                service.pubkey()
            } else {
                alice.pubkey()
            }),
        );
    }
}

#[test]
fn settlement_public_handoff_uses_finality_time_even_after_delayed_partial_service_work() {
    for service_begins in [false, true] {
        let mut context = TestContext::new();
        context.activate();
        let alice = context.alice.insecure_clone();
        let service = context.service.insecure_clone();
        context.set_time(OPEN);
        context.send(
            &[
                context.stake_ix(&alice, context.alice_token, FORECAST, 100),
                context.stake_ix(&alice, context.alice_token, FORECAST + 30, 100),
            ],
            &[&alice],
        );
        let mut result = ResultFixture::new(&context);
        context.set_time(FORECAST * DAY);
        result.record_time(&context);
        let case = CaseFixture::start(&mut context, &result);
        case.finalize_yes(&mut context);
        let finality = case_state(&context, &case).finalized_at.unwrap();
        let original_case = context.svm.get_account(&address(case.key)).unwrap();
        if service_begins {
            context.set_time(finality + 10 * DAY);
            context.send(&[case.begin_ix(&context, &service)], &[&service]);
        }
        context.set_time(finality + 30 * DAY - 1);
        if service_begins {
            context.reject(
                &[case.process_ix(&context, &alice, &[FORECAST])],
                &[&alice],
                service_window(),
            );
            context.reject(
                &[case.finalize_ix(&context, &alice)],
                &[&alice],
                service_window(),
            );
            context.send(
                &[case.process_ix(&context, &service, &[FORECAST])],
                &[&service],
            );
            assert_eq!(context.pool_state().processed_dates, 1);
        } else {
            context.reject(
                &[case.begin_ix(&context, &alice)],
                &[&alice],
                service_window(),
            );
        }
        context.set_time(finality + 30 * DAY);
        if !service_begins {
            context.send(&[case.begin_ix(&context, &alice)], &[&alice]);
            context.send(&[case.process_ix(&context, &alice, &[FORECAST])], &[&alice]);
        }
        context.send(
            &[
                case.process_ix(&context, &alice, &[FORECAST + 30]),
                case.finalize_ix(&context, &alice),
            ],
            &[&alice],
        );
        let pool = context.pool_state();
        assert_eq!(pool.state, manteon::state::PoolState::Claimable);
        assert_eq!(
            (pool.principal, pool.payout_remaining, pool.processed_dates),
            (200, 200, 2)
        );
        assert_eq!(pool.total_weight, 150_u128 << 64);
        assert_eq!(
            context.svm.get_account(&address(case.key)).unwrap(),
            original_case
        );
    }
}

#[test]
fn public_retry_keeps_earlier_eligibility_when_the_candidate_changes() {
    let mut context = TestContext::with_schedule(OPEN, LOCK, OPEN + 200 * DAY);
    let audit = AuditFixture::initialize(&mut context);
    assert_eq!(context.pool_state().service_eligible_at, None);
    context.activate();
    assert_eq!(context.pool_state().service_eligible_at, None);
    let alice = context.alice.insecure_clone();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );

    // An unverified backdated result cannot open public submission.
    let mut first_result = ResultFixture::new(&context);
    context.set_time(OPEN + DAY);
    first_result.record_time(&context);
    context.set_time(OPEN + 31 * DAY - 1);
    let (_, premature) = CaseFixture::instruction(&context, &first_result);
    context.reject(
        &[premature],
        &[&alice],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ManteonError::ServiceWindow.into()),
        ),
    );
    assert_eq!(context.pool_state().service_eligible_at, None);
    context.set_time(OPEN + DAY);
    let service = context.service.insecure_clone();
    let source = token::create_account(
        &mut context.svm,
        &context.payer,
        0xd9,
        address(super::support::AUDIT_MINT),
        service.pubkey(),
        100,
    );
    let (first_case, first_start) = CaseFixture::instruction_for(
        &context,
        &first_result,
        pubkey(service.pubkey()),
        pubkey(source),
    );
    context.send(&[first_start], &[&service]);
    assert_eq!(context.pool_state().service_eligible_at, Some(OPEN + DAY));
    audit.fund(&mut context, &first_case);
    audit.deliver(&mut context, &first_case, Answer::No);
    audit.finalize(&mut context, &first_case, Answer::No);
    let first_case_account = context.svm.get_account(&address(first_case.key)).unwrap();

    let mut second_result = ResultFixture::new(&context);
    context.set_time(OPEN + 40 * DAY);
    second_result.record_time(&context);
    let (second_case, mut retry) = CaseFixture::instruction(&context, &second_result);
    retry
        .accounts
        .push(AccountMeta::new_readonly(address(first_case.key), false));
    // The event duty became public on day 31. A fresh candidate on day 40
    // must not postpone it to day 70; the separate horizon opening is day 170.
    context.send(&[retry], &[&alice]);
    let pool = context.pool_state();
    assert_eq!(pool.current_case, Some(second_case.key));
    assert_eq!(pool.next_case_nonce, 2);
    assert_eq!(pool.service_eligible_at, Some(OPEN + DAY));
    assert_eq!(pool.principal, 100);
    assert_eq!(context.policy_uses(), 3);
    assert_eq!(
        context.token_balance(context.audit_source),
        OWNER_BALANCE - 100
    );
    let event_account = context
        .svm
        .get_account(&address(PoolContext::event(&context.pool)))
        .unwrap();
    let event = Event::try_deserialize(&mut event_account.data.as_slice()).unwrap();
    assert_eq!(
        event.definition,
        EventDefinition::CompanyIpo {
            token_mint: context.token_mint,
        }
    );
    assert_eq!(
        event.result.unwrap().occurred_at(),
        second_result.occurred_at
    );
    assert_eq!(
        context.svm.get_account(&address(first_case.key)).unwrap(),
        first_case_account
    );
}
