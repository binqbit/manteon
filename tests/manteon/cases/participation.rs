use anchor_lang::error::ErrorCode;
use anchor_spl::token::spl_token;
use manteon::{
    error::ManteonError,
    state::{PoolState, StakeState},
};
use solana_instruction_error::InstructionError;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;

use super::support::{
    AUDIT_MINT, DAY, FORECAST, HORIZON, LOCK, OPEN, OWNER_BALANCE, TestContext,
    setup::config::{address, pubkey},
    setup::pool::PoolContext,
};

fn rejected(error: impl Into<u32>) -> TransactionError {
    TransactionError::InstructionError(0, InstructionError::Custom(error.into()))
}

#[test]
fn activation_acquires_real_policy_once_and_entry_requires_the_open_window() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();

    assert_eq!(context.pool_state().state, PoolState::Draft);
    assert_ne!(context.pool_state().collateral_mint, AUDIT_MINT);
    assert_eq!(context.policy_uses(), 0);

    let deposit = context.stake_ix(&alice, context.alice_token, FORECAST, 100);
    context.reject(
        &[deposit],
        &[&alice],
        rejected(ManteonError::InvalidPoolState),
    );

    assert!(
        context
            .svm
            .get_account(&address(context.position(FORECAST)))
            .is_none()
    );

    let platform = context.authority.insecure_clone();
    let mut unauthorized = context.activate_ix();
    unauthorized.accounts[0].pubkey = platform.pubkey();
    context.reject(
        &[unauthorized],
        &[&platform],
        rejected(ManteonError::UnauthorizedServiceAuthority),
    );

    context.activate();

    assert_eq!(context.pool_state().state, PoolState::Active);
    assert_eq!(context.policy_uses(), 1);

    let service = context.service.insecure_clone();
    context.reject(
        &[context.activate_ix()],
        &[&service],
        rejected(ManteonError::InvalidPoolState),
    );

    context.set_time(OPEN - 1);
    context.reject(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
        rejected(ManteonError::InvalidTime),
    );

    context.set_time(OPEN);
    for invalid_day in [FORECAST - 1, FORECAST + 1920] {
        context.reject(
            &[context.stake_ix(&alice, context.alice_token, invalid_day, 100)],
            &[&alice],
            rejected(ManteonError::UnsupportedDate),
        );
    }

    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );

    assert_eq!(context.pool_state().principal, 100);
    assert_eq!(
        context.token_balance(context.alice_token),
        OWNER_BALANCE - 100
    );
    assert_eq!(
        context.token_balance(address(PoolContext::vault(&context.pool))),
        100
    );
}

#[test]
fn personal_allowance_expires_exclusively_and_lock_overrides_a_live_window() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    context.activate();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );

    let deadline = OPEN + 2 * DAY;
    assert_eq!(
        context.stake_state(&alice, FORECAST).withdraw_until,
        deadline
    );

    context.set_time(OPEN + 3_600);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 20)],
        &[&alice],
    );
    context.send(
        &[context.stake_ix(&bob, context.bob_token, FORECAST, 50)],
        &[&bob],
    );
    let live = context.stake_state(&alice, FORECAST);

    assert_eq!(
        (
            live.principal,
            live.withdrawable_principal,
            live.withdraw_until
        ),
        (120, 120, deadline)
    );
    assert_eq!(
        (
            context.pool_state().principal,
            context.pool_state().active_dates
        ),
        (170, 1)
    );

    context.send(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST, 20)],
        &[&alice],
    );

    context.set_time(deadline);
    context.reject(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST, 1)],
        &[&alice],
        rejected(ManteonError::InvalidTime),
    );
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 20)],
        &[&alice],
    );
    let renewed = context.stake_state(&alice, FORECAST);

    assert_eq!(
        (
            renewed.principal,
            renewed.withdrawable_principal,
            renewed.withdraw_until
        ),
        (120, 20, deadline + 2 * DAY)
    );

    context.set_time(deadline + 3_600);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 5)],
        &[&alice],
    );
    let topped_up = context.stake_state(&alice, FORECAST);

    assert_eq!(
        (
            topped_up.principal,
            topped_up.withdrawable_principal,
            topped_up.withdraw_until
        ),
        (125, 25, deadline + 2 * DAY)
    );

    context.send(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST, 25)],
        &[&alice],
    );
    context.reject(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST, 1)],
        &[&alice],
        rejected(ManteonError::InvalidAmount),
    );
    assert_eq!(context.pool_state().principal, 150);
    assert_eq!(context.position_state(FORECAST).principal, 150);

    context.set_time(LOCK - 1);
    context.send(
        &[context.stake_ix(&bob, context.bob_token, FORECAST, 1)],
        &[&bob],
    );
    assert!(context.stake_state(&bob, FORECAST).withdraw_until > LOCK);
    context.set_time(LOCK);
    context.reject(
        &[context.stake_ix(&bob, context.bob_token, FORECAST, 1)],
        &[&bob],
        rejected(ManteonError::InvalidTime),
    );
    context.reject(
        &[context.unstake_ix(&bob, context.bob_token, FORECAST, 1)],
        &[&bob],
        rejected(ManteonError::InvalidTime),
    );
    assert_eq!(
        context.token_balance(address(PoolContext::vault(&context.pool))),
        151
    );
}

#[test]
fn empty_active_stakes_reopen_but_horizon_exits_consume_only_recorded_principal() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    context.activate();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 10)],
        &[&alice],
    );
    context.send(
        &[context.stake_ix(&bob, context.bob_token, FORECAST, 20)],
        &[&bob],
    );
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST + 1, 30)],
        &[&alice],
    );
    assert_eq!(
        (
            context.pool_state().principal,
            context.pool_state().active_dates
        ),
        (60, 2)
    );

    context.send(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST, 10)],
        &[&alice],
    );
    let empty = context.stake_state(&alice, FORECAST);
    assert_eq!(
        (empty.principal, empty.withdrawable_principal, empty.state),
        (0, 0, StakeState::Active)
    );
    assert_eq!(context.position_state(FORECAST).principal, 20);
    assert_eq!(context.pool_state().active_dates, 2);
    context.set_time(OPEN + 1);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 7)],
        &[&alice],
    );
    assert_eq!(
        context.stake_state(&alice, FORECAST).withdraw_until,
        OPEN + 1 + 2 * DAY
    );
    context.send(
        &[context.unstake_ix(&bob, context.bob_token, FORECAST, 20)],
        &[&bob],
    );
    context.send(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST, 7)],
        &[&alice],
    );
    assert_eq!(
        (
            context.pool_state().principal,
            context.pool_state().active_dates
        ),
        (30, 1)
    );
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 3)],
        &[&alice],
    );
    assert_eq!(
        (
            context.pool_state().principal,
            context.pool_state().active_dates
        ),
        (33, 2)
    );

    let vault = PoolContext::vault(&context.pool);
    let donation = spl_token::instruction::transfer_checked(
        &spl_token::ID,
        &pubkey(context.fee_recipient),
        &pubkey(context.mint),
        &vault,
        &pubkey(context.payer.pubkey()),
        &[],
        17,
        6,
    )
    .unwrap();
    context.send(&[donation], &[]);
    assert_eq!(context.token_balance(address(vault)), 50);
    assert_eq!(context.pool_state().principal, 33);

    context.set_time(HORIZON - 1);
    context.reject(
        &[context.exit_ix(&alice, context.alice_token, FORECAST)],
        &[&alice],
        rejected(ManteonError::InvalidTime),
    );
    context.set_time(HORIZON);
    context.send(
        &[context.exit_ix(&alice, context.alice_token, FORECAST)],
        &[&alice],
    );
    let exited = context.stake_state(&alice, FORECAST);
    assert_eq!(
        (
            exited.principal,
            exited.withdrawable_principal,
            exited.state
        ),
        (0, 0, StakeState::Refunded)
    );
    assert_eq!(
        (
            context.pool_state().principal,
            context.pool_state().active_dates
        ),
        (30, 1)
    );
    context.reject(
        &[context.exit_ix(&alice, context.alice_token, FORECAST)],
        &[&alice],
        rejected(ManteonError::InvalidStake),
    );
    context.send(
        &[context.exit_ix(&alice, context.alice_token, FORECAST + 1)],
        &[&alice],
    );
    assert_eq!(
        (
            context.pool_state().principal,
            context.pool_state().active_dates
        ),
        (0, 0)
    );
    assert_eq!(context.token_balance(address(vault)), 17);
    assert_eq!(context.token_balance(context.alice_token), OWNER_BALANCE);
    assert_eq!(context.token_balance(context.bob_token), OWNER_BALANCE);
}

#[test]
fn foreign_source_and_failed_transfers_roll_back_new_accounts_and_aggregates() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    context.activate();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 50)],
        &[&alice],
    );
    context.reject(
        &[context.stake_ix(&alice, context.bob_token, FORECAST, 1)],
        &[&alice],
        rejected(ErrorCode::ConstraintTokenOwner),
    );

    let new_day = FORECAST + 1;
    context.reject(
        &[context.stake_ix(&bob, context.bob_token, new_day, OWNER_BALANCE + 1)],
        &[&bob],
        rejected(spl_token::error::TokenError::InsufficientFunds as u32),
    );
    assert!(
        context
            .svm
            .get_account(&address(context.position(new_day)))
            .is_none()
    );
    assert!(
        context
            .svm
            .get_account(&address(context.stake_key(&bob, new_day)))
            .is_none()
    );

    let deposit = context.stake_ix(&bob, context.bob_token, new_day, 10);
    let failing_transfer = spl_token::instruction::transfer_checked(
        &spl_token::ID,
        &pubkey(context.bob_token),
        &pubkey(context.mint),
        &pubkey(context.alice_token),
        &pubkey(bob.pubkey()),
        &[],
        OWNER_BALANCE + 1,
        6,
    )
    .unwrap();
    context.reject(
        &[deposit, failing_transfer],
        &[&bob],
        TransactionError::InstructionError(
            1,
            InstructionError::Custom(spl_token::error::TokenError::InsufficientFunds as u32),
        ),
    );
    assert!(
        context
            .svm
            .get_account(&address(context.position(new_day)))
            .is_none()
    );
    assert!(
        context
            .svm
            .get_account(&address(context.stake_key(&bob, new_day)))
            .is_none()
    );
    assert_eq!(
        (
            context.pool_state().principal,
            context.pool_state().active_dates
        ),
        (50, 1)
    );

    context.send(
        &[context.stake_ix(&bob, context.bob_token, new_day, 10)],
        &[&bob],
    );
    assert_eq!(
        (
            context.pool_state().principal,
            context.pool_state().active_dates
        ),
        (60, 2)
    );
    assert_eq!(context.position_state(new_day).principal, 10);
    assert_eq!(context.token_balance(context.bob_token), OWNER_BALANCE - 10);
    assert_eq!(
        context.token_balance(address(PoolContext::vault(&context.pool))),
        60
    );
}
