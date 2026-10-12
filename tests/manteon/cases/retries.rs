use anchor_lang::{AccountDeserialize, prelude::Clock};
use manteon::{
    error::ManteonError,
    state::{Event, EventDefinition},
};
use solana_instruction::AccountMeta;
use solana_instruction_error::InstructionError;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;
use tekmerion_sdk::program::errors::TekmerionError;

use super::support::{
    AUDIT_MINT, DAY, FORECAST, OPEN, OWNER_BALANCE, TestContext,
    result::ResultFixture,
    settlement::CaseFixture,
    setup::config::{address, pubkey},
    setup::pool::PoolContext,
    tekmerion::{ChallengeFixture, case_state},
};

fn invalid_audit() -> TransactionError {
    TransactionError::InstructionError(
        0,
        InstructionError::Custom(ManteonError::InvalidAuditAccount.into()),
    )
}

#[test]
fn final_challenge_funding_and_optimistic_closure_cannot_authorize_conflicting_results() {
    for (fully_funded, closure_delay) in [(false, 0), (false, 1), (true, 0)] {
        let mut context = TestContext::new();
        let challenge = ChallengeFixture::initialize(&mut context, 0xe3, 100);
        context.activate();
        let bob = context.bob.insecure_clone();
        let service = context.service.insecure_clone();
        let mut result = ResultFixture::new(&context);
        context.set_time(FORECAST * DAY);
        result.record_time(&context);
        let case = CaseFixture::start(&mut context, &result);
        let deadline = case_state(&context, &case).funding_deadline;
        let payer = pubkey(context.payer.pubkey());
        let contribution = || tekmerion_sdk::instructions::Contribution {
            payer,
            owner: pubkey(bob.pubkey()),
            source: pubkey(challenge.source),
        };
        context.set_time(deadline - 1);
        context.reject(
            &[tekmerion_sdk::instructions::finalize_unchallenged(
                case.identity,
            )],
            &[],
            TransactionError::InstructionError(
                0,
                InstructionError::Custom(TekmerionError::FundingStillOpen.into()),
            ),
        );
        context.send(
            &[tekmerion_sdk::instructions::contribute(
                case.identity,
                contribution(),
                if fully_funded { 100 } else { 99 },
            )],
            &[&bob],
        );
        context.set_time(deadline);
        if fully_funded {
            context.reject(
                &[tekmerion_sdk::instructions::finalize_unchallenged(
                    case.identity,
                )],
                &[],
                TransactionError::InstructionError(
                    0,
                    InstructionError::Custom(TekmerionError::InvalidPhase.into()),
                ),
            );
            context.reject(
                &[case.begin_ix(&context, &service)],
                &[&service],
                invalid_audit(),
            );
            assert_eq!(
                case_state(&context, &case).phase(),
                tekmerion_sdk::program::CasePhase::Funded
            );
            assert_eq!(
                context.pool_state().state,
                manteon::state::PoolState::Active
            );
            assert_eq!(context.token_balance(address(case.vault)), 200);
        } else {
            context.set_time(deadline + closure_delay);
            context.reject(
                &[tekmerion_sdk::instructions::contribute(
                    case.identity,
                    contribution(),
                    1,
                )],
                &[&bob],
                TransactionError::InstructionError(
                    0,
                    InstructionError::Custom(TekmerionError::FundingClosed.into()),
                ),
            );
            context.send(
                &[tekmerion_sdk::instructions::finalize_unchallenged(
                    case.identity,
                )],
                &[],
            );
            assert_eq!(
                case_state(&context, &case).finalized_at,
                Some(deadline + closure_delay)
            );
            let terminal = context.svm.get_account(&address(case.key)).unwrap();
            context.reject(
                &[tekmerion_sdk::instructions::contribute(
                    case.identity,
                    contribution(),
                    1,
                )],
                &[&bob],
                TransactionError::InstructionError(
                    0,
                    InstructionError::Custom(TekmerionError::InvalidPhase.into()),
                ),
            );
            context.reject(
                &[tekmerion_sdk::instructions::finalize_unchallenged(
                    case.identity,
                )],
                &[],
                TransactionError::InstructionError(
                    0,
                    InstructionError::Custom(TekmerionError::InvalidPhase.into()),
                ),
            );
            context.send(&[case.begin_ix(&context, &service)], &[&service]);
            assert_eq!(
                context.pool_state().state,
                manteon::state::PoolState::Settling
            );
            assert_eq!(context.token_balance(address(case.vault)), 199);
            assert_eq!(context.token_balance(challenge.source), 1);
            assert_eq!(
                context.svm.get_account(&address(case.key)).unwrap(),
                terminal
            );
        }
        assert_eq!(context.pool_state().current_case, Some(case.key));
        assert_eq!(context.pool_state().next_case_nonce, 1);
        assert_eq!(context.policy_uses(), 2);
    }
}

#[test]
fn anchor_034_funded_timeout_allows_one_bound_paid_retry_without_reusing_the_old_case() {
    let mut context = TestContext::new();
    context.activate();
    let alice = context.alice.insecure_clone();
    assert_ne!(pubkey(context.mint), AUDIT_MINT);
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );
    // Fund the test wallet before loading the immutable mainnet USDC snapshot.
    let challenge = ChallengeFixture::initialize(&mut context, 0xc0, 100);
    let mut result = ResultFixture::new(&context);
    context.set_time(FORECAST * DAY);
    result.record_time(&context);
    let first = CaseFixture::start(&mut context, &result);
    let first_state = case_state(&context, &first);
    assert_eq!((first_state.nonce, first_state.previous_case), (0, None));
    challenge.fund(&mut context, &first);
    assert_eq!(context.token_balance(challenge.source), 0);

    let (second, missing_previous) = CaseFixture::instruction(&context, &result);
    let mut retry = missing_previous.clone();
    retry
        .accounts
        .push(AccountMeta::new_readonly(address(first.key), false));
    // A funded audit is still the sole active attempt; funding closure is not Yes.
    context.reject(std::slice::from_ref(&retry), &[&alice], invalid_audit());

    let now = context.svm.get_sysvar::<Clock>().unix_timestamp;
    context.set_time(now + 300); // The fixture's genuine Policy audit timeout.
    context.send(
        &[tekmerion_sdk::instructions::finalize_timeout(
            first.identity,
        )],
        &[],
    );
    let old_case = context.svm.get_account(&address(first.key)).unwrap();
    let old_vault = context.svm.get_account(&address(first.vault)).unwrap();
    context.reject(&[missing_previous], &[&alice], invalid_audit());
    let mut wrong_previous = retry.clone();
    wrong_previous.accounts.last_mut().unwrap().pubkey = address(context.policy);
    context.reject(&[wrong_previous], &[&alice], invalid_audit());
    assert!(context.svm.get_account(&address(second.key)).is_none());
    assert!(context.svm.get_account(&address(second.vault)).is_none());

    context.send(&[retry], &[&alice]);
    let state = case_state(&context, &second);
    assert_eq!(
        (state.policy, state.subject, state.author),
        (
            context.policy,
            pubkey(context.pool.pubkey()),
            pubkey(alice.pubkey())
        )
    );
    assert_eq!((state.nonce, state.previous_case), (1, Some(first.key)));
    assert_eq!(state.result_hash, first_state.result_hash);
    let pool = context.pool_state();
    assert_eq!(
        (pool.current_case, pool.next_case_nonce),
        (Some(second.key), 2)
    );
    assert_eq!((pool.principal, context.policy_uses()), (100, 3));
    let event_account = context
        .svm
        .get_account(&address(PoolContext::event(&context.pool)))
        .unwrap();
    let event = Event::try_deserialize(&mut event_account.data.as_slice()).unwrap();
    assert_eq!(event.pool, pubkey(context.pool.pubkey()));
    assert_eq!(event.result.unwrap().occurred_at(), result.occurred_at);
    assert_eq!(
        event.definition,
        EventDefinition::CompanyIpo {
            token_mint: context.token_mint,
        }
    );
    assert_eq!(
        context.token_balance(context.audit_source),
        OWNER_BALANCE - 200
    );
    assert_eq!(context.token_balance(address(second.vault)), 100);
    assert_eq!(
        context.svm.get_account(&address(first.key)).unwrap(),
        old_case
    );
    assert_eq!(
        context.svm.get_account(&address(first.vault)).unwrap(),
        old_vault
    );
    assert_eq!(context.token_balance(challenge.source), 0);
    assert_eq!(
        context.token_balance(context.alice_token),
        OWNER_BALANCE - 100
    );
    assert_eq!(
        context.token_balance(address(PoolContext::vault(&context.pool))),
        100
    );

    second.finalize_yes(&mut context);
    let (_, mut replace_yes) = CaseFixture::instruction(&context, &result);
    replace_yes
        .accounts
        .push(AccountMeta::new_readonly(address(second.key), false));
    context.reject(&[replace_yes], &[&alice], invalid_audit());
    assert_eq!(
        (
            context.pool_state().current_case,
            context.pool_state().next_case_nonce
        ),
        (Some(second.key), 2)
    );
}

#[test]
fn case_creation_failures_roll_back_audit_custody_and_market_attempt() {
    use anchor_spl::token::spl_token;

    let mut context = TestContext::new();
    context.activate();
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    let reserve = ChallengeFixture::initialize(&mut context, 0xc4, 1);
    let mut result = ResultFixture::new(&context);
    context.set_time(FORECAST * DAY);
    result.record_time(&context);
    context.send(
        &[spl_token::instruction::transfer(
            &spl_token::ID,
            &pubkey(context.audit_source),
            &pubkey(reserve.source),
            &pubkey(alice.pubkey()),
            &[],
            OWNER_BALANCE - 99,
        )
        .unwrap()],
        &[&alice],
    );
    let (case, start) = CaseFixture::instruction(&context, &result);
    context.reject(
        std::slice::from_ref(&start),
        &[&alice],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(spl_token::error::TokenError::InsufficientFunds as u32),
        ),
    );
    context.send(
        &[spl_token::instruction::transfer(
            &spl_token::ID,
            &pubkey(reserve.source),
            &pubkey(context.audit_source),
            &pubkey(bob.pubkey()),
            &[],
            1,
        )
        .unwrap()],
        &[&bob],
    );
    // This failure occurs after the valid CPI has created all three audit accounts.
    let failing_transfer = spl_token::instruction::transfer(
        &spl_token::ID,
        &pubkey(context.alice_token),
        &pubkey(context.bob_token),
        &pubkey(alice.pubkey()),
        &[],
        OWNER_BALANCE + 1,
    )
    .unwrap();
    context.reject(
        &[start.clone(), failing_transfer],
        &[&alice],
        TransactionError::InstructionError(
            1,
            InstructionError::Custom(spl_token::error::TokenError::InsufficientFunds as u32),
        ),
    );
    for created in [
        case.key,
        case.vault,
        case.identity.receipt(&pubkey(alice.pubkey())),
    ] {
        assert!(context.svm.get_account(&address(created)).is_none());
    }
    assert_eq!(context.pool_state().current_case, None);
    assert_eq!(context.pool_state().next_case_nonce, 0);
    assert_eq!(context.policy_uses(), 1);
    assert_eq!(context.token_balance(context.audit_source), 100);

    context.send(&[start], &[&alice]);
    assert_eq!(context.pool_state().current_case, Some(case.key));
    assert_eq!(context.pool_state().next_case_nonce, 1);
    assert_eq!(context.policy_uses(), 2);
    assert_eq!(context.token_balance(context.audit_source), 0);
    assert_eq!(context.token_balance(address(case.vault)), 100);
    let receipt_account = context
        .svm
        .get_account(&address(case.identity.receipt(&pubkey(alice.pubkey()))))
        .unwrap();
    assert_eq!(receipt_account.owner, address(tekmerion_sdk::ID));
    let receipt =
        tekmerion_sdk::program::StakeReceipt::try_deserialize(&mut receipt_account.data.as_slice())
            .unwrap();
    assert_eq!(
        (receipt.case, receipt.owner, receipt.principal),
        (case.key, pubkey(alice.pubkey()), 100)
    );
    assert_eq!(receipt.side, tekmerion_sdk::program::AUTHOR_SIDE);
    assert!(!receipt.claimed);
}
