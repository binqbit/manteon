use anchor_lang::{error::ErrorCode, prelude::Pubkey};
use anchor_spl::token::spl_token;
use manteon::{error::ManteonError, state::StakeState};
use solana_address::Address;
use solana_instruction::Instruction;
use solana_instruction_error::InstructionError;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;

use crate::support::{
    DAY, FORECAST, HORIZON, LOCK, OPEN, OWNER_BALANCE, TestContext,
    setup::{
        config::{address, pubkey},
        pool::PoolContext,
    },
};

fn rejected(error: impl Into<u32>) -> TransactionError {
    TransactionError::InstructionError(0, InstructionError::Custom(error.into()))
}

fn replace(instruction: &mut Instruction, from: Address, to: Address) {
    instruction
        .accounts
        .iter_mut()
        .find(|meta| meta.pubkey == from)
        .expect("instruction contains the role being substituted")
        .pubkey = to;
}

#[test]
fn owner_operations_reject_foreign_pool_stakes_tokens_and_unsigned_authority() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    let sibling = context.create_pool(0xe1);
    context.activate();
    let original = std::mem::replace(&mut context.pool, sibling);
    context.activate();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 70)],
        &[&alice],
    );
    let foreign_pool = context.pool.pubkey();
    let foreign_position = address(context.position(FORECAST));
    let foreign_stake = address(context.stake_key(&alice, FORECAST));
    let foreign_vault = address(PoolContext::vault(&context.pool));
    let foreign_authority = address(PoolContext::vault_authority(&context.pool));
    let sibling_accounts = [foreign_pool, foreign_position, foreign_stake, foreign_vault]
        .map(|key| (key, context.svm.get_account(&key).unwrap()));
    context.pool = original;
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );
    context.send(
        &[context.stake_ix(&bob, context.bob_token, FORECAST, 25)],
        &[&bob],
    );

    for (operation, timestamp, valid) in [
        (
            "stake",
            OPEN,
            context.stake_ix(&alice, context.alice_token, FORECAST, 10),
        ),
        (
            "unstake",
            OPEN,
            context.unstake_ix(&alice, context.alice_token, FORECAST, 10),
        ),
        (
            "exit",
            HORIZON,
            context.exit_ix(&alice, context.alice_token, FORECAST),
        ),
    ] {
        context.set_time(timestamp);
        for (role, from, to, code) in [
            (
                "foreign Position",
                address(context.position(FORECAST)),
                foreign_position,
                u32::from(ErrorCode::ConstraintSeeds),
            ),
            (
                "foreign Stake",
                address(context.stake_key(&alice, FORECAST)),
                foreign_stake,
                u32::from(ErrorCode::ConstraintSeeds),
            ),
            (
                "another owner's Stake",
                address(context.stake_key(&alice, FORECAST)),
                address(context.stake_key(&bob, FORECAST)),
                u32::from(ErrorCode::ConstraintSeeds),
            ),
            (
                "foreign vault",
                address(PoolContext::vault(&context.pool)),
                foreign_vault,
                u32::from(ErrorCode::ConstraintSeeds),
            ),
            (
                "foreign vault authority",
                address(PoolContext::vault_authority(&context.pool)),
                foreign_authority,
                u32::from(ErrorCode::ConstraintSeeds),
            ),
            (
                "another owner's token account",
                context.alice_token,
                context.bob_token,
                u32::from(ErrorCode::ConstraintTokenOwner),
            ),
            (
                "owner's token account with another mint",
                context.alice_token,
                context.audit_source,
                u32::from(ErrorCode::ConstraintTokenMint),
            ),
            (
                "unsupported token program",
                address(spl_token::ID),
                address(Pubkey::from_str_const(
                    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
                )),
                u32::from(ErrorCode::InvalidProgramId),
            ),
        ] {
            println!("{operation} rejects {role}");
            let mut instruction = valid.clone();
            replace(&mut instruction, from, to);
            context.reject(&[instruction], &[&alice], rejected(code));
        }
        let mut wrong_owner = valid.clone();
        replace(&mut wrong_owner, alice.pubkey(), bob.pubkey());
        context.reject(
            &[wrong_owner],
            &[&bob],
            rejected(ErrorCode::ConstraintSeeds),
        );
        let mut unsigned = valid.clone();
        unsigned
            .accounts
            .iter_mut()
            .find(|meta| meta.pubkey == alice.pubkey())
            .unwrap()
            .is_signer = false;
        context.reject(&[unsigned], &[], rejected(ErrorCode::AccountNotSigner));
        context.send(&[valid], &[&alice]);
        for (key, snapshot) in &sibling_accounts {
            assert_eq!(
                context.svm.get_account(key).as_ref(),
                Some(snapshot),
                "independent sibling account {key}"
            );
        }
    }
    assert_eq!(
        context.stake_state(&alice, FORECAST).state,
        StakeState::Refunded
    );
    assert_eq!(context.stake_state(&bob, FORECAST).principal, 25);
    assert_eq!(
        (
            context.pool_state().principal,
            context.pool_state().active_dates
        ),
        (25, 1)
    );
    assert_eq!(
        context.token_balance(address(PoolContext::vault(&context.pool))),
        25
    );
    assert_eq!(context.token_balance(foreign_vault), 70);
    assert_eq!(
        context.token_balance(context.alice_token),
        OWNER_BALANCE - 70
    );
    assert_eq!(context.policy_uses(), 2);
}

#[test]
fn forecast_admission_distinguishes_midnight_from_midday_lock() {
    let midnight = FORECAST * DAY;
    for lock in [midnight, midnight + 1] {
        let mut context = TestContext::with_schedule(OPEN, lock, lock + 2 * DAY);
        let alice = context.alice.insecure_clone();
        context.activate();
        context.set_time(OPEN);
        let same_day = context.stake_ix(&alice, context.alice_token, FORECAST, 10);
        if lock == midnight {
            context.send(&[same_day], &[&alice]);
            assert_eq!(context.position_state(FORECAST).principal, 10);
        } else {
            context.reject(
                &[same_day],
                &[&alice],
                rejected(ManteonError::UnsupportedDate),
            );
            assert!(
                context
                    .svm
                    .get_account(&address(context.position(FORECAST)))
                    .is_none()
            );
        }
        context.send(
            &[context.stake_ix(&alice, context.alice_token, FORECAST + 1, 20)],
            &[&alice],
        );
        assert_eq!(context.position_state(FORECAST + 1).principal, 20);
    }
}

#[test]
fn zero_and_excess_withdrawals_and_failed_topups_preserve_the_allowance() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();
    context.activate();
    context.set_time(OPEN);
    context.reject(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 0)],
        &[&alice],
        rejected(ManteonError::InvalidAmount),
    );
    assert!(
        context
            .svm
            .get_account(&address(context.position(FORECAST)))
            .is_none()
    );
    assert!(
        context
            .svm
            .get_account(&address(context.stake_key(&alice, FORECAST)))
            .is_none()
    );
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );
    for amount in [0, 101] {
        context.reject(
            &[context.unstake_ix(&alice, context.alice_token, FORECAST, amount)],
            &[&alice],
            rejected(ManteonError::InvalidAmount),
        );
    }
    let deadline = OPEN + 2 * DAY;
    context.set_time(deadline - 1);
    context.send(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST, 1)],
        &[&alice],
    );
    context.set_time(deadline + 1);
    context.reject(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST, 1)],
        &[&alice],
        rejected(ManteonError::InvalidTime),
    );
    context.reject(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, OWNER_BALANCE)],
        &[&alice],
        rejected(spl_token::error::TokenError::InsufficientFunds as u32),
    );
    let preserved = context.stake_state(&alice, FORECAST);
    assert_eq!(
        (
            preserved.principal,
            preserved.withdrawable_principal,
            preserved.withdraw_until
        ),
        (99, 99, deadline)
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
        (119, 20, deadline + 1 + 2 * DAY)
    );
    context.reject(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST, 21)],
        &[&alice],
        rejected(ManteonError::InvalidAmount),
    );
    context.send(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST, 20)],
        &[&alice],
    );
    assert_eq!(context.pool_state().principal, 99);
}

#[test]
fn coincident_allowance_and_global_lock_close_both_owner_operations() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();
    context.activate();
    context.set_time(LOCK - 2 * DAY);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 10)],
        &[&alice],
    );
    assert_eq!(context.stake_state(&alice, FORECAST).withdraw_until, LOCK);
    context.set_time(LOCK - 1);
    context.send(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST, 1)],
        &[&alice],
    );
    for now in [LOCK, LOCK + 1] {
        context.set_time(now);
        context.reject(
            &[context.unstake_ix(&alice, context.alice_token, FORECAST, 1)],
            &[&alice],
            rejected(ManteonError::InvalidTime),
        );
        context.reject(
            &[context.stake_ix(&alice, context.alice_token, FORECAST, 1)],
            &[&alice],
            rejected(ManteonError::InvalidTime),
        );
    }
    assert_eq!(context.pool_state().principal, 9);
    assert_eq!(
        context.token_balance(context.alice_token),
        OWNER_BALANCE - 9
    );
}

#[test]
fn frozen_custody_and_failed_exit_transactions_preserve_owner_rights() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();
    context.activate();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );
    for token in [
        context.alice_token,
        address(PoolContext::vault(&context.pool)),
    ] {
        let freeze = spl_token::instruction::freeze_account(
            &spl_token::ID,
            &pubkey(token),
            &pubkey(context.mint),
            &pubkey(context.payer.pubkey()),
            &[],
        )
        .unwrap();
        context.send(&[freeze], &[]);
        let code = if token == context.alice_token {
            u32::from(ManteonError::InvalidTokenAccount)
        } else {
            u32::from(ManteonError::InvalidVault)
        };
        context.reject(
            &[context.stake_ix(&alice, context.alice_token, FORECAST + 1, 10)],
            &[&alice],
            rejected(code),
        );
        context.reject(
            &[context.unstake_ix(&alice, context.alice_token, FORECAST, 10)],
            &[&alice],
            rejected(code),
        );
        assert!(
            context
                .svm
                .get_account(&address(context.position(FORECAST + 1)))
                .is_none()
        );
        let thaw = spl_token::instruction::thaw_account(
            &spl_token::ID,
            &pubkey(token),
            &pubkey(context.mint),
            &pubkey(context.payer.pubkey()),
            &[],
        )
        .unwrap();
        context.send(&[thaw], &[]);
    }
    context.send(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST, 10)],
        &[&alice],
    );
    context.set_time(HORIZON);
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
        &[context.exit_ix(&alice, context.alice_token, FORECAST)],
        &[&alice],
        rejected(ManteonError::InvalidTokenAccount),
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
    let insufficient_transfer = spl_token::instruction::transfer_checked(
        &spl_token::ID,
        &pubkey(context.alice_token),
        &pubkey(context.mint),
        &pubkey(context.bob_token),
        &pubkey(alice.pubkey()),
        &[],
        OWNER_BALANCE + 1,
        6,
    )
    .unwrap();
    context.reject(
        &[
            context.exit_ix(&alice, context.alice_token, FORECAST),
            insufficient_transfer,
        ],
        &[&alice],
        TransactionError::InstructionError(
            1,
            InstructionError::Custom(spl_token::error::TokenError::InsufficientFunds as u32),
        ),
    );
    assert_eq!(
        context.stake_state(&alice, FORECAST).state,
        StakeState::Active
    );
    assert_eq!(context.pool_state().principal, 90);
    context.send(
        &[context.exit_ix(&alice, context.alice_token, FORECAST)],
        &[&alice],
    );
    assert_eq!(
        context.stake_state(&alice, FORECAST).state,
        StakeState::Refunded
    );
    assert_eq!(
        (
            context.pool_state().principal,
            context.pool_state().active_dates
        ),
        (0, 0)
    );
    assert_eq!(context.token_balance(context.alice_token), OWNER_BALANCE);
}
