use super::support::{
    DAY, FORECAST, HORIZON, LOCK, OPEN, OWNER_BALANCE, TestContext,
    result::ResultFixture,
    settlement::{CaseFixture, claim_ix},
    setup::config::{address, pubkey},
    setup::pool::PoolContext,
};
use anchor_spl::token::spl_token;
use manteon::{
    error::ManteonError,
    state::{PoolState, StakeState},
};
use solana_instruction_error::InstructionError;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;

fn custom(error: ManteonError) -> TransactionError {
    TransactionError::InstructionError(0, InstructionError::Custom(error.into()))
}

#[test]
fn anchor_030_real_case_freezes_book_and_claims_preserve_basis_and_donations() {
    let mut context = TestContext::new();
    context.activate();
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    let service = context.service.insecure_clone();
    context.set_time(OPEN);
    context.send(
        &[
            context.stake_ix(&alice, context.alice_token, FORECAST, 100),
            context.stake_ix(&bob, context.bob_token, FORECAST + 30, 100),
        ],
        &[&alice, &bob],
    );
    let donate = spl_token::instruction::transfer(
        &spl_token::ID,
        &pubkey(context.alice_token),
        &PoolContext::vault(&context.pool),
        &pubkey(alice.pubkey()),
        &[],
        17,
    )
    .unwrap();
    context.send(&[donate], &[&alice]);
    let mut result = ResultFixture::new(&context);
    context.set_time(FORECAST * DAY);
    result.record_time(&context);
    let case = CaseFixture::start(&mut context, &result);
    // A pending assertion cannot authorize acceptance or payouts.
    context.reject(
        &[case.begin_ix(&context, &service)],
        &[&service],
        custom(ManteonError::InvalidAuditAccount),
    );
    case.finalize_yes(&mut context);
    // The service month uses terminal time, not when someone first invokes this instruction.
    context.send(&[case.begin_ix(&context, &alice)], &[&alice]);
    assert_eq!(context.pool_state().state, PoolState::Settling);
    assert_eq!(
        (
            context.pool_state().principal,
            context.pool_state().payout_remaining
        ),
        (200, 200)
    );
    context.reject(
        &[context.exit_ix(&alice, context.alice_token, FORECAST)],
        &[&alice],
        custom(ManteonError::InvalidPoolState),
    );
    context.reject(
        &[case.process_ix(&context, &service, &[FORECAST, FORECAST])],
        &[&service],
        custom(ManteonError::InvalidBatch),
    );
    assert!(!context.position_state(FORECAST).computed);
    assert_eq!(context.pool_state().processed_dates, 0);
    context.send(
        &[case.process_ix(&context, &service, &[FORECAST])],
        &[&service],
    );
    context.reject(
        &[case.finalize_ix(&context, &service)],
        &[&service],
        custom(ManteonError::IncompleteSettlement),
    );
    context.reject(
        &[case.process_ix(&context, &service, &[FORECAST])],
        &[&service],
        custom(ManteonError::InvalidAccounting),
    );
    context.send(
        &[
            case.process_ix(&context, &service, &[FORECAST + 30]),
            case.finalize_ix(&context, &service),
        ],
        &[&service],
    );
    let weight = 150_u128 << 64;
    assert_eq!(context.pool_state().total_weight, weight);
    let alice_claim = claim_ix(&context, &alice, FORECAST, pubkey(context.alice_token));
    // Actual SPL freeze failure rolls back the claim's liability and terminal flag.
    let freeze = spl_token::instruction::freeze_account(
        &spl_token::ID,
        &pubkey(context.alice_token),
        &pubkey(context.mint),
        &pubkey(context.payer.pubkey()),
        &[],
    )
    .unwrap();
    context.send(&[freeze], &[]);
    context.reject(
        std::slice::from_ref(&alice_claim),
        &[&alice],
        custom(ManteonError::InvalidTokenAccount),
    );
    assert_eq!(context.pool_state().payout_remaining, 200);
    assert_eq!(
        context.stake_state(&alice, FORECAST).state,
        StakeState::Active
    );
    let thaw = spl_token::instruction::thaw_account(
        &spl_token::ID,
        &pubkey(context.alice_token),
        &pubkey(context.mint),
        &pubkey(context.payer.pubkey()),
        &[],
    )
    .unwrap();
    context.send(&[thaw], &[]);
    context.send(std::slice::from_ref(&alice_claim), &[&alice]);
    context.reject(
        &[alice_claim],
        &[&alice],
        custom(ManteonError::InvalidStake),
    );
    context.send(
        &[claim_ix(
            &context,
            &bob,
            FORECAST + 30,
            pubkey(context.bob_token),
        )],
        &[&bob],
    );
    assert_eq!(
        context.token_balance(context.alice_token),
        OWNER_BALANCE - 100 - 17 + 133
    );
    assert_eq!(
        context.token_balance(context.bob_token),
        OWNER_BALANCE - 100 + 66
    );
    let pool = context.pool_state();
    assert_eq!(
        (pool.principal, pool.total_weight, pool.payout_remaining),
        (200, weight, 1)
    );
    assert_eq!(context.position_state(FORECAST).principal, 100);
    assert_eq!(context.stake_state(&alice, FORECAST).principal, 100);
    assert_eq!(
        context.token_balance(address(PoolContext::vault(&context.pool))),
        18
    );
}

#[test]
fn anchor_031_pre_lock_result_refunds_remaining_principal_without_fees() {
    let mut context = TestContext::new();
    context.activate();
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    let service = context.service.insecure_clone();
    context.set_time(OPEN);
    context.send(
        &[
            context.stake_ix(&alice, context.alice_token, FORECAST, 100),
            context.stake_ix(&bob, context.bob_token, FORECAST, 50),
            context.stake_ix(&alice, context.alice_token, FORECAST + 1, 30),
            context.stake_ix(&bob, context.bob_token, FORECAST + 2, 70),
        ],
        &[&alice, &bob],
    );
    context.send(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST, 20)],
        &[&alice],
    );
    let vault = PoolContext::vault(&context.pool);
    let donation = spl_token::instruction::transfer_checked(
        &spl_token::ID,
        &pubkey(context.alice_token),
        &pubkey(context.mint),
        &vault,
        &pubkey(alice.pubkey()),
        &[],
        17,
        6,
    )
    .unwrap();
    context.send(&[donation], &[&alice]);
    assert_eq!(context.pool_state().principal, 230);
    assert_eq!(context.token_balance(address(vault)), 247);

    let mut result = ResultFixture::new(&context);
    result.record_time(&context);
    let case = CaseFixture::start(&mut context, &result);
    // Applying the original pre-LOCK result late must not restore funds
    // already returned by ordinary withdrawal or an eligible pending-Case exit.
    context.set_time(HORIZON);
    context.send(
        &[context.exit_ix(&bob, context.bob_token, FORECAST + 2)],
        &[&bob],
    );
    assert_eq!(context.position_state(FORECAST + 2).principal, 0);
    assert_eq!(context.pool_state().active_dates, 2);
    assert_eq!(context.pool_state().principal, 160);
    assert_eq!(context.token_balance(address(vault)), 177);
    case.finalize_yes(&mut context);
    context.send(&[case.begin_ix(&context, &service)], &[&service]);
    assert_eq!(context.pool_state().state, PoolState::Refunding);
    assert_eq!(context.pool_state().principal, 160);
    assert_eq!(context.pool_state().payout_remaining, 0);
    context.reject(
        &[claim_ix(
            &context,
            &alice,
            FORECAST,
            pubkey(context.bob_token),
        )],
        &[&alice],
        custom(ManteonError::InvalidTokenAccount),
    );
    assert_eq!(context.pool_state().principal, 160);
    assert_eq!(context.position_state(FORECAST).principal, 130);
    assert_eq!(context.stake_state(&alice, FORECAST).principal, 80);
    assert_eq!(
        context.stake_state(&alice, FORECAST).state,
        StakeState::Active
    );

    let alice_refund = claim_ix(&context, &alice, FORECAST, pubkey(context.alice_token));
    context.send(std::slice::from_ref(&alice_refund), &[&alice]);
    assert_eq!(context.pool_state().principal, 80);
    assert_eq!(context.pool_state().active_dates, 2);
    assert_eq!(context.position_state(FORECAST).principal, 50);
    assert_eq!(context.token_balance(address(vault)), 97);
    context.reject(
        &[alice_refund],
        &[&alice],
        custom(ManteonError::InvalidStake),
    );
    context.reject(
        &[claim_ix(
            &context,
            &bob,
            FORECAST + 2,
            pubkey(context.bob_token),
        )],
        &[&bob],
        custom(ManteonError::InvalidStake),
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
    assert_eq!(context.pool_state().principal, 30);
    assert_eq!(context.pool_state().active_dates, 1);
    assert_eq!(context.position_state(FORECAST).principal, 0);
    context.send(
        &[claim_ix(
            &context,
            &alice,
            FORECAST + 1,
            pubkey(context.alice_token),
        )],
        &[&alice],
    );
    assert_eq!(
        context.token_balance(context.alice_token),
        OWNER_BALANCE - 17
    );
    assert_eq!(context.token_balance(context.bob_token), OWNER_BALANCE);
    assert_eq!(context.token_balance(address(vault)), 17);
    assert_eq!(
        (
            context.pool_state().principal,
            context.pool_state().active_dates,
            context.pool_state().total_weight,
            context.pool_state().payout_remaining,
        ),
        (0, 0, 0, 0)
    );
    for (owner, day) in [
        (&alice, FORECAST),
        (&bob, FORECAST),
        (&alice, FORECAST + 1),
        (&bob, FORECAST + 2),
    ] {
        let stake = context.stake_state(owner, day);
        assert_eq!(stake.state, StakeState::Refunded);
        assert_eq!(stake.withdrawable_principal, 0);
        assert!(!context.position_state(day).computed);
    }
}

#[test]
fn anchor_032_pending_case_does_not_block_horizon_exit_and_empty_settlement_has_no_payouts() {
    let mut context = TestContext::new();
    context.activate();
    let alice = context.alice.insecure_clone();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );
    let mut result = ResultFixture::new(&context);
    context.set_time(HORIZON);
    result.record_time(&context);
    let case = CaseFixture::start(&mut context, &result);
    context.send(
        &[context.exit_ix(&alice, context.alice_token, FORECAST)],
        &[&alice],
    );
    assert_eq!(context.pool_state().principal, 0);
    case.finalize_yes(&mut context);
    assert!(context.pool_state().config_released);
    // An audit may finish independently; an empty completed Pool cannot reacquire
    // its former settings or reenter settlement after the creator edits them.
    context.set_time(HORIZON + 60 + 30 * DAY);
    context.reject(
        &[case.begin_ix(&context, &alice)],
        &[&alice],
        custom(ManteonError::InvalidPoolState),
    );
    assert_eq!(context.pool_state().state, PoolState::Active);
    context.reject(
        &[claim_ix(
            &context,
            &alice,
            FORECAST,
            pubkey(context.alice_token),
        )],
        &[&alice],
        custom(ManteonError::InvalidStake),
    );
    assert_eq!(context.token_balance(context.alice_token), OWNER_BALANCE);
}

#[test]
fn anchor_033_late_out_of_domain_event_keeps_horizon_exit_available() {
    let mut context = TestContext::new();
    context.activate();
    let alice = context.alice.insecure_clone();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );
    let mut result = ResultFixture::new(&context);
    context.set_time(LOCK + 1921 * DAY);
    result.record_time(&context);
    let (_, proposal) = CaseFixture::instruction(&context, &result);
    context.reject(
        &[proposal],
        &[&alice],
        custom(ManteonError::UnsupportedDate),
    );
    assert_eq!(context.pool_state().state, PoolState::Active);
    assert_eq!(context.pool_state().accepted_case, None);
    context.send(
        &[context.exit_ix(&alice, context.alice_token, FORECAST)],
        &[&alice],
    );
    assert_eq!(context.token_balance(context.alice_token), OWNER_BALANCE);
}

#[test]
fn horizon_exit_and_settlement_freeze_serialize_without_reintroducing_returned_principal() {
    for exit_first in [true, false] {
        let mut context = TestContext::new();
        let alice = context.alice.insecure_clone();
        let bob = context.bob.insecure_clone();
        let service = context.service.insecure_clone();
        context.activate();
        context.set_time(OPEN);
        context.send(
            &[
                context.stake_ix(&alice, context.alice_token, FORECAST, 100),
                context.stake_ix(&bob, context.bob_token, FORECAST + 30, 100),
            ],
            &[&alice, &bob],
        );
        let mut result = ResultFixture::new(&context);
        context.set_time(FORECAST * DAY);
        result.record_time(&context);
        let case = CaseFixture::start(&mut context, &result);
        case.finalize_yes(&mut context);
        context.set_time(HORIZON);
        let exit = context.exit_ix(&alice, context.alice_token, FORECAST);
        if exit_first {
            // A terminal Case alone does not freeze Manteon's book.
            context.send(std::slice::from_ref(&exit), &[&alice]);
            assert_eq!(context.pool_state().principal, 100);
        }
        context.send(&[case.begin_ix(&context, &service)], &[&service]);
        context.reject(
            &[exit],
            &[&alice],
            custom(if exit_first {
                ManteonError::InvalidStake
            } else {
                ManteonError::InvalidPoolState
            }),
        );
        let days = if exit_first {
            vec![FORECAST + 30]
        } else {
            vec![FORECAST, FORECAST + 30]
        };
        context.send(
            &[
                case.process_ix(&context, &service, &days),
                case.finalize_ix(&context, &service),
            ],
            &[&service],
        );
        let alice_claim = claim_ix(&context, &alice, FORECAST, pubkey(context.alice_token));
        if exit_first {
            context.reject(
                &[alice_claim],
                &[&alice],
                custom(ManteonError::InvalidStake),
            );
        } else {
            context.send(&[alice_claim], &[&alice]);
        }
        context.send(
            &[claim_ix(
                &context,
                &bob,
                FORECAST + 30,
                pubkey(context.bob_token),
            )],
            &[&bob],
        );
        let pool = context.pool_state();
        let vault_balance = context.token_balance(address(PoolContext::vault(&context.pool)));
        if exit_first {
            assert_eq!(
                (
                    pool.principal,
                    pool.active_dates,
                    pool.processed_dates,
                    pool.total_weight,
                    pool.payout_remaining
                ),
                (100, 1, 1, 50_u128 << 64, 0)
            );
            assert_eq!(context.token_balance(context.alice_token), OWNER_BALANCE);
            assert_eq!(context.token_balance(context.bob_token), OWNER_BALANCE);
            assert!(!context.position_state(FORECAST).computed);
        } else {
            assert_eq!(
                (
                    pool.principal,
                    pool.active_dates,
                    pool.processed_dates,
                    pool.total_weight,
                    pool.payout_remaining
                ),
                (200, 2, 2, 150_u128 << 64, 1)
            );
            assert_eq!(
                context.token_balance(context.alice_token),
                OWNER_BALANCE + 33
            );
            assert_eq!(context.token_balance(context.bob_token), OWNER_BALANCE - 34);
        }
        assert_eq!(vault_balance, pool.payout_remaining);
    }
}

#[test]
fn settled_and_refunding_markets_reject_book_mutation_restart_and_new_candidates() {
    for refund in [false, true] {
        let mut context = TestContext::new();
        let alice = context.alice.insecure_clone();
        let service = context.service.insecure_clone();
        context.activate();
        context.set_time(OPEN);
        context.send(
            &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
            &[&alice],
        );
        let mut result = ResultFixture::new(&context);
        if !refund {
            context.set_time(FORECAST * DAY);
        }
        result.record_time(&context);
        let case = CaseFixture::start(&mut context, &result);
        case.finalize_yes(&mut context);
        context.send(&[case.begin_ix(&context, &service)], &[&service]);
        let phases = if refund {
            vec![PoolState::Refunding]
        } else {
            vec![PoolState::Settling, PoolState::Claimable]
        };
        for phase in phases {
            assert_eq!(context.pool_state().state, phase);
            let (_, candidate) = CaseFixture::instruction(&context, &result);
            let removal_error = || {
                if phase == PoolState::Claimable {
                    ManteonError::InvalidPosition
                } else {
                    ManteonError::InvalidPoolState
                }
            };
            for (instruction, signer, error) in [
                (
                    context.stake_ix(&alice, context.alice_token, FORECAST + 1, 1),
                    &alice,
                    ManteonError::InvalidPoolState,
                ),
                (
                    context.unstake_ix(&alice, context.alice_token, FORECAST, 1),
                    &alice,
                    removal_error(),
                ),
                (
                    context.exit_ix(&alice, context.alice_token, FORECAST),
                    &alice,
                    removal_error(),
                ),
                (
                    context.activate_ix(),
                    &service,
                    ManteonError::InvalidPoolState,
                ),
                (
                    case.begin_ix(&context, &service),
                    &service,
                    ManteonError::InvalidPoolState,
                ),
                (candidate, &alice, ManteonError::InvalidPoolState),
            ] {
                context.reject(&[instruction], &[signer], custom(error));
            }
            assert!(
                context
                    .svm
                    .get_account(&address(context.position(FORECAST + 1)))
                    .is_none()
            );
            assert!(
                context
                    .svm
                    .get_account(&address(context.stake_key(&alice, FORECAST + 1)))
                    .is_none()
            );
            if phase == PoolState::Settling {
                context.send(
                    &[
                        case.process_ix(&context, &service, &[FORECAST]),
                        case.finalize_ix(&context, &service),
                    ],
                    &[&service],
                );
            } else {
                for instruction in [
                    case.process_ix(&context, &service, &[FORECAST]),
                    case.finalize_ix(&context, &service),
                ] {
                    context.reject(
                        &[instruction],
                        &[&service],
                        custom(ManteonError::InvalidPoolState),
                    );
                }
            }
        }
        // Rejected administrative/lifecycle actions leave the authentic claim usable.
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
        assert_eq!(context.pool_state().payout_remaining, 0);
    }
}

#[test]
fn pool_accuracy_algorithms_change_real_claims_and_preserve_frozen_weights() {
    use super::support::audited::{Answer, AuditFixture};
    use manteon::state::PoolAlgorithm;
    // Equal 100-atom stakes, at the event and 30 days later. These independently
    // rounded amounts distinguish all three curves, including H7's fractional weight.
    for (algorithm, alice_payout, bob_payout) in [
        (PoolAlgorithm::H30, 133, 66),
        (PoolAlgorithm::H15, 160, 40),
        (PoolAlgorithm::H7, 190, 9),
    ] {
        for challenged in [false, true] {
            let mut context = TestContext::with_algorithm(algorithm);
            let audit = challenged.then(|| AuditFixture::initialize(&mut context));
            assert_eq!(context.pool_state().algorithm, algorithm);
            context.activate();
            let alice = context.alice.insecure_clone();
            let bob = context.bob.insecure_clone();
            let service = context.service.insecure_clone();
            context.set_time(OPEN);
            context.send(
                &[
                    context.stake_ix(&alice, context.alice_token, FORECAST, 100),
                    context.stake_ix(&bob, context.bob_token, FORECAST + 30, 100),
                ],
                &[&alice, &bob],
            );
            let mut result = ResultFixture::new(&context);
            context.set_time(FORECAST * DAY);
            result.record_time(&context);
            let case = CaseFixture::start(&mut context, &result);
            if let Some(audit) = audit {
                audit.fund(&mut context, &case);
                audit.deliver(&mut context, &case, Answer::Yes);
                audit.finalize(&mut context, &case, Answer::Yes);
            } else {
                case.finalize_yes(&mut context);
            }
            context.send(&[case.begin_ix(&context, &service)], &[&service]);
            context.send(
                &[
                    case.process_ix(&context, &service, &[FORECAST, FORECAST + 30]),
                    case.finalize_ix(&context, &service),
                ],
                &[&service],
            );
            let frozen_weight = context.pool_state().total_weight;
            context.send(
                &[claim_ix(
                    &context,
                    &alice,
                    FORECAST,
                    pubkey(context.alice_token),
                )],
                &[&alice],
            );
            context.send(
                &[claim_ix(
                    &context,
                    &bob,
                    FORECAST + 30,
                    pubkey(context.bob_token),
                )],
                &[&bob],
            );
            assert_eq!(
                context.token_balance(context.alice_token),
                OWNER_BALANCE - 100 + alice_payout
            );
            assert_eq!(
                context.token_balance(context.bob_token),
                OWNER_BALANCE - 100 + bob_payout
            );
            let pool = context.pool_state();
            assert_eq!(pool.algorithm, algorithm);
            assert_eq!(pool.principal, 200);
            assert_eq!(pool.total_weight, frozen_weight);
            assert_eq!(pool.payout_remaining, 200 - alice_payout - bob_payout);
        }
    }
}

#[test]
fn each_accuracy_curve_admits_its_last_nonzero_forecast_and_rejects_the_next_day() {
    use manteon::state::PoolAlgorithm;
    for algorithm in [PoolAlgorithm::H30, PoolAlgorithm::H15, PoolAlgorithm::H7] {
        let mut context = TestContext::with_algorithm(algorithm);
        context.activate();
        context.set_time(OPEN);
        let alice = context.alice.insecure_clone();
        let last = LOCK.div_euclid(DAY) + algorithm.max_error_days();
        context.send(
            &[context.stake_ix(&alice, context.alice_token, last, 1)],
            &[&alice],
        );
        context.reject(
            &[context.stake_ix(&alice, context.alice_token, last + 1, 1)],
            &[&alice],
            custom(ManteonError::UnsupportedDate),
        );
        assert_eq!(context.pool_state().principal, 1);
    }
}
