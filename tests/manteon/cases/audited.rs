use anchor_lang::AccountDeserialize;
use anchor_spl::token::spl_token;
use manteon::{
    error::ManteonError,
    state::{PoolState, StakeState},
};
use solana_instruction::AccountMeta;
use solana_instruction_error::InstructionError;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;
use tekmerion_sdk::program::errors::TekmerionError;

use super::support::{
    DAY, FORECAST, HORIZON, OPEN, OWNER_BALANCE, TestContext,
    audited::{Answer, AuditFixture},
    result::ResultFixture,
    settlement::{CaseFixture, claim_ix},
    setup::config::{address, pubkey},
    setup::pool::PoolContext,
    tekmerion::case_state,
};

fn custom(code: u32) -> TransactionError {
    TransactionError::InstructionError(0, InstructionError::Custom(code))
}

#[test]
fn pending_funded_audit_preserves_horizon_exit_and_policy_locks_survive_terminal_results() {
    let mut context = TestContext::new();
    let audit = AuditFixture::initialize(&mut context);
    let admin = context.authority.insecure_clone();
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    let service = context.service.insecure_clone();
    let policy_account = context.svm.get_account(&address(context.policy)).unwrap();
    let policy =
        tekmerion_sdk::program::Policy::try_deserialize(&mut policy_account.data.as_slice())
            .unwrap();
    let mut updated_rules = policy.rules;
    updated_rules.audit_timeout += 1;
    let update = tekmerion_sdk::instructions::update_policy(
        pubkey(admin.pubkey()),
        policy.identifier,
        updated_rules,
    );
    // This same valid admin update is permitted only before the first dependency.
    context.send(std::slice::from_ref(&update), &[&admin]);
    context.activate();
    context.reject(
        std::slice::from_ref(&update),
        &[&admin],
        custom(TekmerionError::PolicyInUse.into()),
    );
    context.set_time(OPEN);
    context.send(
        &[
            context.stake_ix(&alice, context.alice_token, FORECAST, 100),
            context.stake_ix(&bob, context.bob_token, FORECAST, 100),
        ],
        &[&alice, &bob],
    );
    let mut result = ResultFixture::new(&context);
    context.set_time(HORIZON - 1);
    result.record_time(&context);
    let case = CaseFixture::start(&mut context, &result);
    audit.fund(&mut context, &case);
    audit.deliver(&mut context, &case, Answer::Yes);
    let pending_case = context.svm.get_account(&address(case.key)).unwrap();
    let pending_vault = context.svm.get_account(&address(case.vault)).unwrap();
    context.reject(
        std::slice::from_ref(&update),
        &[&admin],
        custom(TekmerionError::PolicyInUse.into()),
    );
    context.reject(
        &[case.begin_ix(&context, &service)],
        &[&service],
        custom(ManteonError::InvalidAuditAccount.into()),
    );
    context.set_time(HORIZON);
    context.send(
        &[context.exit_ix(&alice, context.alice_token, FORECAST)],
        &[&alice],
    );
    assert_eq!(context.token_balance(context.alice_token), OWNER_BALANCE);
    assert_eq!(context.pool_state().principal, 100);
    assert_eq!(context.position_state(FORECAST).principal, 100);
    assert_eq!(
        context.stake_state(&alice, FORECAST).state,
        StakeState::Refunded
    );
    assert_eq!(
        context.svm.get_account(&address(case.key)).unwrap(),
        pending_case
    );
    assert_eq!(
        context.svm.get_account(&address(case.vault)).unwrap(),
        pending_vault
    );
    audit.finalize(&mut context, &case, Answer::Yes);
    let terminal_case = context.svm.get_account(&address(case.key)).unwrap();
    assert_eq!(case_state(&context, &case).finalized_at, Some(HORIZON));
    context.reject(
        &[tekmerion_sdk::instructions::finalize_timeout(case.identity)],
        &[],
        custom(TekmerionError::InvalidPhase.into()),
    );
    context.reject(
        std::slice::from_ref(&update),
        &[&admin],
        custom(TekmerionError::PolicyInUse.into()),
    );
    context.send(
        &[
            case.begin_ix(&context, &service),
            case.process_ix(&context, &service, &[FORECAST]),
            case.finalize_ix(&context, &service),
        ],
        &[&service],
    );
    context.reject(
        std::slice::from_ref(&update),
        &[&admin],
        custom(TekmerionError::PolicyInUse.into()),
    );
    context.send(
        &[claim_ix(
            &context,
            &bob,
            FORECAST,
            pubkey(context.bob_token),
        )],
        &[&bob],
    );
    context.reject(
        &[claim_ix(
            &context,
            &alice,
            FORECAST,
            pubkey(context.alice_token),
        )],
        &[&alice],
        custom(ManteonError::InvalidStake.into()),
    );
    context.reject(
        &[update],
        &[&admin],
        custom(TekmerionError::PolicyInUse.into()),
    );
    assert_eq!(context.token_balance(context.bob_token), OWNER_BALANCE);
    assert_eq!(
        context.token_balance(address(PoolContext::vault(&context.pool))),
        0
    );
    assert_eq!(context.pool_state().principal, 100);
    assert_eq!(context.pool_state().payout_remaining, 0);
    assert_eq!(context.policy_uses(), 2);
    assert_eq!(
        context.svm.get_account(&address(case.key)).unwrap(),
        terminal_case
    );
}

#[test]
fn anchor_authenticated_audited_yes_requires_finality_and_pays_only_market_principal() {
    let mut context = TestContext::new();
    let audit = AuditFixture::initialize(&mut context);
    context.activate();
    let alice = context.alice.insecure_clone();
    let service = context.service.insecure_clone();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );
    let mut result = ResultFixture::new(&context);
    context.set_time(FORECAST * DAY);
    result.record_time(&context);
    let case = CaseFixture::start(&mut context, &result);
    audit.fund(&mut context, &case);

    // An ordinary market author cannot manufacture an authenticated audit vote.
    context.reject(
        &[audit.vote_ix(&case, &alice, Answer::Yes, &[])],
        &[&alice],
        custom(TekmerionError::UnauthorizedAuditor.into()),
    );
    audit.deliver(&mut context, &case, Answer::Yes);
    context.reject(
        &[case.begin_ix(&context, &service)],
        &[&service],
        custom(ManteonError::InvalidAuditAccount.into()),
    );
    let (_, mut premature_retry) = CaseFixture::instruction(&context, &result);
    premature_retry
        .accounts
        .push(AccountMeta::new_readonly(address(case.key), false));
    context.reject(
        &[premature_retry],
        &[&alice],
        custom(ManteonError::InvalidAuditAccount.into()),
    );
    let first_auditor = context.auditors[0].insecure_clone();
    context.reject(
        &[audit.vote_ix(&case, &first_auditor, Answer::Yes, &[])],
        &[&first_auditor],
        custom(TekmerionError::DuplicateAuditVote.into()),
    );
    // A failed quorum fee transfer must leave the pending decision and market intact.
    let second_auditor = context.auditors[1].insecure_clone();
    context.reject(
        &[audit.vote_ix(
            &case,
            &second_auditor,
            Answer::Yes,
            &[
                pubkey(context.auditor_tokens[0]),
                pubkey(context.audit_source),
            ],
        )],
        &[&second_auditor],
        custom(TekmerionError::InvalidFeeRecipient.into()),
    );
    audit.finalize(&mut context, &case, Answer::Yes);
    let finalized_case = context.svm.get_account(&address(case.key)).unwrap();

    // A genuine donation gives no prediction rights; audit custody stays separate.
    let donation = spl_token::instruction::transfer(
        &spl_token::ID,
        &pubkey(context.alice_token),
        &PoolContext::vault(&context.pool),
        &pubkey(alice.pubkey()),
        &[],
        17,
    )
    .unwrap();
    context.send(&[donation], &[&alice]);
    context.send(
        &[
            case.begin_ix(&context, &service),
            case.process_ix(&context, &service, &[FORECAST]),
            case.finalize_ix(&context, &service),
        ],
        &[&service],
    );
    assert_eq!(context.pool_state().state, PoolState::Claimable);
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
        OWNER_BALANCE - 17
    );
    assert_eq!(
        context.token_balance(address(PoolContext::vault(&context.pool))),
        17
    );
    assert_eq!(context.token_balance(address(case.vault)), 193);
    assert_eq!(context.pool_state().principal, 100);
    assert_eq!(context.pool_state().payout_remaining, 0);
    assert_eq!(
        context.stake_state(&alice, FORECAST).state,
        StakeState::Paid
    );
    assert_eq!(
        context.svm.get_account(&address(case.key)).unwrap(),
        finalized_case,
        "Manteon consumes the terminal decision without mutating audit entitlements"
    );
}

#[test]
fn anchor_authenticated_no_and_unknown_allow_paid_bound_retries_without_settlement() {
    let mut context = TestContext::new();
    let audit = AuditFixture::initialize(&mut context);
    context.activate();
    let alice = context.alice.insecure_clone();
    let service = context.service.insecure_clone();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );
    let mut result = ResultFixture::new(&context);
    context.set_time(FORECAST * DAY);
    result.record_time(&context);
    let mut previous: Option<CaseFixture> = None;
    for (nonce, answer) in [Answer::No, Answer::Unknown, Answer::Yes]
        .into_iter()
        .enumerate()
    {
        let (case, mut start) = CaseFixture::instruction(&context, &result);
        let previous_account = previous.as_ref().map(|previous| {
            start
                .accounts
                .push(AccountMeta::new_readonly(address(previous.key), false));
            context.svm.get_account(&address(previous.key)).unwrap()
        });
        let audit_balance = context.token_balance(context.audit_source);
        context.send(&[start], &[&alice]);
        assert_eq!(
            context.token_balance(context.audit_source),
            audit_balance - 100
        );
        let pool = context.pool_state();
        assert_eq!(
            (pool.current_case, pool.next_case_nonce),
            (Some(case.key), nonce as u64 + 1)
        );
        if let Some(previous) = previous.as_ref() {
            context.reject(
                &[previous.begin_ix(&context, &service)],
                &[&service],
                custom(ManteonError::InvalidAuditAccount.into()),
            );
            assert_eq!(
                context.svm.get_account(&address(previous.key)),
                previous_account
            );
        }
        audit.fund(&mut context, &case);
        audit.deliver(&mut context, &case, answer);
        audit.finalize(&mut context, &case, answer);
        if answer == Answer::Yes {
            context.send(&[case.begin_ix(&context, &service)], &[&service]);
            assert_eq!(context.pool_state().state, PoolState::Settling);
        } else {
            context.reject(
                &[case.begin_ix(&context, &service)],
                &[&service],
                custom(ManteonError::InvalidAuditAccount.into()),
            );
            assert_eq!(context.pool_state().state, PoolState::Active);
            assert_eq!(context.pool_state().payout_remaining, 0);
            assert!(!context.position_state(FORECAST).computed);
            assert_eq!(
                context.stake_state(&alice, FORECAST).state,
                StakeState::Active
            );
        }
        assert_eq!(context.pool_state().principal, 100);
        assert_eq!(
            context.token_balance(address(PoolContext::vault(&context.pool))),
            100
        );
        assert_eq!(context.token_balance(address(case.vault)), 193);
        previous = Some(case);
    }
    assert_eq!(
        context.token_balance(context.audit_source),
        OWNER_BALANCE - 300
    );
}
