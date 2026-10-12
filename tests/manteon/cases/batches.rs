use manteon::{
    error::ManteonError,
    state::{PoolState, StakeState},
};
use solana_instruction::AccountMeta;
use solana_instruction_error::InstructionError;
use solana_transaction_error::TransactionError;

use super::support::{
    DAY, FORECAST, LOCK, OPEN, TestContext,
    result::ResultFixture,
    settlement::{CaseFixture, claim_ix},
    setup::config::{address, pubkey},
};

#[test]
fn maximum_batch_fits_default_budget_and_zero_payout_still_consumes_the_claim() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();
    let service = context.service.insecure_clone();
    context.activate();
    context.set_time(OPEN);
    let days = [
        FORECAST,
        FORECAST + 1,
        FORECAST + 29,
        FORECAST + 30,
        FORECAST + 59,
        FORECAST + 60,
        FORECAST + 61,
        LOCK.div_euclid(DAY) + 1920,
    ];
    for (index, day) in days.iter().copied().enumerate() {
        let principal = if index == days.len() - 1 { 1 } else { 100 };
        context.send(
            &[context.stake_ix(&alice, context.alice_token, day, principal)],
            &[&alice],
        );
    }
    assert_eq!(context.pool_state().active_dates, 8);
    let mut result = ResultFixture::new(&context);
    context.set_time(FORECAST * DAY);
    result.record_time(&context);
    let case = CaseFixture::start(&mut context, &result);
    case.finalize_yes(&mut context);
    context.send(&[case.begin_ix(&context, &service)], &[&service]);

    let mut oversized = days.to_vec();
    oversized.push(FORECAST + 2);
    context.reject(
        &[case.process_ix(&context, &service, &oversized)],
        &[&service],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ManteonError::InvalidBatch.into()),
        ),
    );
    assert_eq!(context.pool_state().processed_dates, 0);
    assert!(
        days.iter()
            .all(|day| !context.position_state(*day).computed)
    );

    let metadata = context.send(&[case.process_ix(&context, &service, &days)], &[&service]);
    println!(
        "Eight-position SBF batch consumed {} compute units",
        metadata.compute_units_consumed
    );
    assert!(metadata.compute_units_consumed <= 200_000);
    assert_eq!(context.pool_state().processed_dates, 8);
    assert!(days.iter().all(|day| context.position_state(*day).computed));
    let weight = context.pool_state().total_weight;
    assert!(weight > 0 && weight <= 701_u128 << 64);
    context.send(&[case.finalize_ix(&context, &service)], &[&service]);
    assert_eq!(context.pool_state().state, PoolState::Claimable);
    assert_eq!(context.pool_state().payout_remaining, 701);

    // A positive position at the admitted far edge has positive Q64.64 weight,
    // but its whole-token entitlement floors to zero against this complete book.
    let distant_day = days[7];
    let balance_before = context.token_balance(context.alice_token);
    let claim = claim_ix(&context, &alice, distant_day, pubkey(context.alice_token));
    context.send(std::slice::from_ref(&claim), &[&alice]);
    assert_eq!(
        context.stake_state(&alice, distant_day).state,
        StakeState::Paid
    );
    assert_eq!(context.token_balance(context.alice_token), balance_before);
    assert_eq!(context.pool_state().payout_remaining, 701);
    assert_eq!(context.pool_state().total_weight, weight);
    context.reject(
        &[claim],
        &[&alice],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ManteonError::InvalidStake.into()),
        ),
    );
}

#[test]
fn invalid_batch_tails_roll_back_every_marker_and_the_denominator() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();
    let service = context.service.insecure_clone();
    let other_pool = context.create_pool(0xe1);
    context.activate();
    let original_pool = std::mem::replace(&mut context.pool, other_pool);
    context.activate();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 17)],
        &[&alice],
    );
    let foreign_position = address(context.position(FORECAST));
    let foreign_before = context.svm.get_account(&foreign_position).unwrap();
    context.pool = original_pool;
    for day in [FORECAST, FORECAST + 30, FORECAST + 60, FORECAST + 90] {
        context.send(
            &[context.stake_ix(&alice, context.alice_token, day, 100)],
            &[&alice],
        );
    }
    context.send(
        &[context.unstake_ix(&alice, context.alice_token, FORECAST + 90, 100)],
        &[&alice],
    );
    let mut result = ResultFixture::new(&context);
    context.set_time(FORECAST * DAY);
    result.record_time(&context);
    let case = CaseFixture::start(&mut context, &result);
    case.finalize_yes(&mut context);
    context.send(&[case.begin_ix(&context, &service)], &[&service]);

    let empty = case.process_ix(&context, &service, &[]);
    context.reject(
        &[empty],
        &[&service],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ManteonError::InvalidBatch.into()),
        ),
    );
    for (name, tail, error) in [
        (
            "foreign Pool Position",
            AccountMeta::new(foreign_position, false),
            ManteonError::InvalidPosition,
        ),
        (
            "missing Position",
            AccountMeta::new(address(context.position(FORECAST + 1)), false),
            ManteonError::InvalidPosition,
        ),
        (
            "read-only Position",
            AccountMeta::new_readonly(address(context.position(FORECAST + 30)), false),
            ManteonError::InvalidBatch,
        ),
        (
            "duplicate Position",
            AccountMeta::new(address(context.position(FORECAST)), false),
            ManteonError::InvalidBatch,
        ),
        (
            "zero-principal tombstone",
            AccountMeta::new(address(context.position(FORECAST + 90)), false),
            ManteonError::InvalidAmount,
        ),
    ] {
        println!("Reject batch tail: {name}");
        let mut batch = case.process_ix(&context, &service, &[FORECAST]);
        batch.accounts.push(tail);
        context.reject(
            &[batch],
            &[&service],
            TransactionError::InstructionError(0, InstructionError::Custom(error.into())),
        );
        assert!(!context.position_state(FORECAST).computed);
        assert_eq!(
            (
                context.pool_state().processed_dates,
                context.pool_state().total_weight
            ),
            (0, 0)
        );
        assert_eq!(
            context.svm.get_account(&foreign_position).unwrap(),
            foreign_before
        );
    }

    context.send(
        &[case.process_ix(&context, &service, &[FORECAST])],
        &[&service],
    );
    // The first instruction computes a new date; the second encounters an old
    // marker after another valid date. Both instructions must be rolled back.
    context.reject(
        &[
            case.process_ix(&context, &service, &[FORECAST + 30]),
            case.process_ix(&context, &service, &[FORECAST + 60, FORECAST]),
        ],
        &[&service],
        TransactionError::InstructionError(
            1,
            InstructionError::Custom(ManteonError::InvalidAccounting.into()),
        ),
    );
    assert_eq!(
        (
            context.pool_state().processed_dates,
            context.pool_state().total_weight
        ),
        (1, 100_u128 << 64)
    );
    assert!(!context.position_state(FORECAST + 30).computed);
    assert!(!context.position_state(FORECAST + 60).computed);
    context.send(
        &[
            case.process_ix(&context, &service, &[FORECAST + 60, FORECAST + 30]),
            case.finalize_ix(&context, &service),
        ],
        &[&service],
    );
    let pool = context.pool_state();
    assert_eq!(
        (
            pool.principal,
            pool.active_dates,
            pool.processed_dates,
            pool.total_weight
        ),
        (300, 3, 3, 175_u128 << 64)
    );
    assert!(!context.position_state(FORECAST + 90).computed);
    assert_eq!(pool.state, PoolState::Claimable);
}

#[test]
fn legal_batch_orders_and_partitions_produce_the_same_frozen_book() {
    let orders = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for (index, order) in orders.into_iter().enumerate() {
        let mut context = TestContext::new();
        let alice = context.alice.insecure_clone();
        let service = context.service.insecure_clone();
        context.activate();
        context.set_time(OPEN);
        let entries = [(FORECAST, 101), (FORECAST + 30, 53), (FORECAST + 60, 27)];
        for (day, principal) in entries {
            context.send(
                &[context.stake_ix(&alice, context.alice_token, day, principal)],
                &[&alice],
            );
        }
        let mut result = ResultFixture::new(&context);
        context.set_time(FORECAST * DAY);
        result.record_time(&context);
        let case = CaseFixture::start(&mut context, &result);
        case.finalize_yes(&mut context);
        context.send(&[case.begin_ix(&context, &service)], &[&service]);
        let days = order.map(|entry| entries[entry].0);
        // Exercise one batch, singleton batches, and a partial final batch.
        for batch in days.chunks(index % 3 + 1) {
            context.send(&[case.process_ix(&context, &service, batch)], &[&service]);
        }
        let pool = context.pool_state();
        assert_eq!(
            pool.state,
            PoolState::Settling,
            "the last batch must not finalize implicitly"
        );
        // Exact H30 errors 0/30/60 give 101 + 53/2 + 27/4 = 537/4.
        assert_eq!(
            (
                pool.principal,
                pool.payout_remaining,
                pool.active_dates,
                pool.processed_dates,
                pool.total_weight
            ),
            (181, 181, 3, 3, 537_u128 << 62)
        );
        context.send(&[case.finalize_ix(&context, &service)], &[&service]);
        assert_eq!(context.pool_state().state, PoolState::Claimable);
    }
}
