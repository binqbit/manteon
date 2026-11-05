//! Direct commitment premium scenarios executed against the actual SBF program.

use crate::support::{
    DAY, FORECAST, HORIZON, LOCK, OPEN, TestContext,
    result::ResultFixture,
    settlement::{CaseFixture, claim_ix},
};
use manteon::{
    error::ManteonError,
    state::{PoolAlgorithm, PoolState, StakeState},
};
use manteon_core::{
    math::{
        premium::{self, Weight},
        utc_day::day_error,
    },
    types::Amount,
};
use solana_instruction_error::InstructionError;
use solana_transaction_error::TransactionError;

fn custom(error: ManteonError) -> TransactionError {
    TransactionError::InstructionError(0, InstructionError::Custom(error.into()))
}

#[test]
fn premium_modes_keep_shared_window_and_remove_only_pending_units() {
    let mut c = TestContext::with_premium(PoolAlgorithm::H7, 3000);
    let alice = c.alice.insecure_clone();
    c.activate();
    c.set_time(OPEN);
    c.send(
        &[c.stake_ix(&alice, c.alice_token, FORECAST, 100)],
        &[&alice],
    );
    let first = c.stake_state(&alice, FORECAST);
    assert_eq!(first.premium_units, 100 * 2000);
    assert_eq!(first.withdraw_until, OPEN + 2 * DAY);
    c.set_time(OPEN + DAY);
    c.send(
        &[c.stake_committed_ix(&alice, c.alice_token, FORECAST, 100)],
        &[&alice],
    );
    c.send(
        &[c.stake_ix(&alice, c.alice_token, FORECAST, 100)],
        &[&alice],
    );
    let mixed = c.stake_state(&alice, FORECAST);
    assert_eq!(mixed.principal, 300);
    assert_eq!(mixed.withdrawable_principal, 200);
    assert_eq!(mixed.withdraw_until, first.withdraw_until);
    assert_eq!(mixed.premium_units, 100 * 2000 + 100 * 2500 + 100 * 2000);
    c.send(
        &[c.unstake_ix(&alice, c.alice_token, FORECAST, 150)],
        &[&alice],
    );
    let remaining = c.stake_state(&alice, FORECAST);
    assert_eq!(remaining.principal, 150);
    assert_eq!(remaining.withdrawable_principal, 50);
    assert_eq!(remaining.premium_units, 100 * 2500 + 50 * 2000);
    assert_eq!(
        c.position_state(FORECAST).premium_units,
        remaining.premium_units
    );
    c.set_time(first.withdraw_until);
    c.reject(
        &[c.unstake_ix(&alice, c.alice_token, FORECAST, 1)],
        &[&alice],
        custom(ManteonError::InvalidTime),
    );
    c.send(
        &[c.stake_committed_ix(&alice, c.alice_token, FORECAST, 1)],
        &[&alice],
    );
    let expired = c.stake_state(&alice, FORECAST);
    assert_eq!(expired.withdrawable_principal, 50);
    assert_eq!(expired.withdraw_until, first.withdraw_until);
    c.send(
        &[c.stake_ix(&alice, c.alice_token, FORECAST, 10)],
        &[&alice],
    );
    let new = c.stake_state(&alice, FORECAST);
    assert_eq!(new.withdrawable_principal, 10);
    assert_eq!(new.withdraw_until, OPEN + 4 * DAY);
    assert_eq!(
        new.premium_units,
        remaining.premium_units + 2000 + 10 * 1000
    );
}

#[test]
fn premium_short_market_pending_has_no_bonus_and_immediate_keeps_no_window() {
    let mut c = TestContext::with_premium_schedule(
        OPEN,
        OPEN + DAY,
        OPEN + 2 * DAY,
        None,
        PoolAlgorithm::H30,
        (0, false),
        3000,
    );
    let alice = c.alice.insecure_clone();
    let bob = c.bob.insecure_clone();
    c.activate();
    c.set_time(OPEN);
    c.send(
        &[
            c.stake_ix(&alice, c.alice_token, FORECAST, 100),
            c.stake_committed_ix(&bob, c.bob_token, FORECAST, 100),
        ],
        &[&alice, &bob],
    );
    let a = c.stake_state(&alice, FORECAST);
    let b = c.stake_state(&bob, FORECAST);
    assert_eq!(a.premium_units, 0);
    assert_eq!(a.withdrawable_principal, 100);
    assert_eq!(b.premium_units, 300000);
    assert_eq!(b.withdrawable_principal, 0);
    assert_eq!(b.withdraw_until, 0);
    c.reject(
        &[c.unstake_ix(&bob, c.bob_token, FORECAST, 1)],
        &[&bob],
        custom(ManteonError::InvalidTime),
    );
    c.set_time(OPEN + DAY);
    c.reject(
        &[c.unstake_ix(&alice, c.alice_token, FORECAST, 1)],
        &[&alice],
        custom(ManteonError::InvalidTime),
    );
}

#[test]
fn premium_zero_principal_immediate_clears_stale_deadline_before_new_regular_window() {
    let mut c = TestContext::with_premium(PoolAlgorithm::H15, 2000);
    let a = c.alice.insecure_clone();
    c.activate();
    c.set_time(OPEN);
    c.send(
        &[
            c.stake_ix(&a, c.alice_token, FORECAST, 100),
            c.unstake_ix(&a, c.alice_token, FORECAST, 100),
        ],
        &[&a],
    );
    assert_eq!(c.stake_state(&a, FORECAST).principal, 0);
    c.set_time(OPEN + DAY);
    c.send(
        &[c.stake_committed_ix(&a, c.alice_token, FORECAST, 100)],
        &[&a],
    );
    assert_eq!(c.stake_state(&a, FORECAST).withdraw_until, 0);
    c.set_time(OPEN + DAY + 1);
    c.send(&[c.stake_ix(&a, c.alice_token, FORECAST, 10)], &[&a]);
    assert_eq!(
        c.stake_state(&a, FORECAST).withdraw_until,
        OPEN + 3 * DAY + 1
    );
}

#[test]
fn premium_date_settlement_and_claims_match_wide_formula_for_every_cap_and_curve() {
    for algorithm in [PoolAlgorithm::H7, PoolAlgorithm::H15, PoolAlgorithm::H30] {
        for cap in [0, 1000, 2000, 3000] {
            let mut c = TestContext::with_premium(algorithm, cap);
            let a = c.alice.insecure_clone();
            let b = c.bob.insecure_clone();
            let service = c.service.insecure_clone();
            c.activate();
            c.set_time(OPEN);
            c.send(
                &[
                    c.stake_committed_ix(&a, c.alice_token, FORECAST, 100),
                    c.stake_ix(&a, c.alice_token, FORECAST, 100),
                    c.stake_ix(&b, c.bob_token, FORECAST, 100),
                ],
                &[&a, &b],
            );
            c.set_time(OPEN + 5 * DAY);
            c.send(
                &[c.stake_committed_ix(&b, c.bob_token, FORECAST + 1, 100)],
                &[&b],
            );
            let mut r = ResultFixture::new(&c);
            c.set_time(FORECAST * DAY);
            r.record_time(&c);
            let case = CaseFixture::start(&mut c, &r);
            case.finalize_yes(&mut c);
            c.send(&[case.begin_ix(&c, &service)], &[&service]);
            c.reject(
                &[case.process_ix(&c, &service, &[FORECAST, FORECAST])],
                &[&service],
                custom(ManteonError::InvalidBatch),
            );
            c.send(
                &[
                    case.process_ix(&c, &service, &[FORECAST, FORECAST + 1]),
                    case.finalize_ix(&c, &service),
                ],
                &[&service],
            );
            assert_eq!(c.pool_state().state, PoolState::Claimable);
            let pool = c.pool_state();
            let mut expected = Weight::ZERO;
            for day in [FORECAST, FORECAST + 1] {
                let p = c.position_state(day);
                expected += premium::weighted_principal(
                    algorithm.into(),
                    Amount::new(p.principal),
                    p.premium_units,
                    cap,
                    day_error(day, FORECAST),
                )
                .unwrap();
            }
            assert_eq!(Weight::from_le_bytes(pool.total_weight), expected);
            let mut paid = 0;
            for (owner, token, day) in [
                (&a, c.alice_token, FORECAST),
                (&b, c.bob_token, FORECAST),
                (&b, c.bob_token, FORECAST + 1),
            ] {
                let stake = c.stake_state(owner, day);
                let quote = premium::quote_gross(
                    algorithm.into(),
                    Amount::new(pool.principal),
                    Amount::new(stake.principal),
                    stake.premium_units,
                    cap,
                    day_error(day, FORECAST),
                    expected,
                )
                .unwrap()
                .get();
                let before = c.token_balance(token);
                c.send(&[claim_ix(&c, owner, day, token)], &[owner]);
                assert_eq!(c.token_balance(token) - before, quote);
                assert_eq!(c.stake_state(owner, day).premium_units, stake.premium_units);
                assert_eq!(c.stake_state(owner, day).state, StakeState::Paid);
                paid += quote;
            }
            assert_eq!(c.pool_state().payout_remaining, pool.principal - paid);
            assert!(c.pool_state().config_released);
            super::reads::read_context(&c).unwrap();
        }
    }
}

#[test]
fn premium_exit_clears_exact_units_without_touching_other_owner() {
    let mut c = TestContext::with_premium(PoolAlgorithm::H30, 3000);
    let a = c.alice.insecure_clone();
    let b = c.bob.insecure_clone();
    c.activate();
    c.set_time(OPEN);
    c.send(
        &[
            c.stake_committed_ix(&a, c.alice_token, FORECAST, 100),
            c.stake_ix(&b, c.bob_token, FORECAST, 100),
        ],
        &[&a, &b],
    );
    let b_units = c.stake_state(&b, FORECAST).premium_units;
    c.set_time(HORIZON);
    c.send(&[c.exit_ix(&a, c.alice_token, FORECAST)], &[&a]);
    let a = c.stake_state(&a, FORECAST);
    assert_eq!(
        (a.principal, a.withdrawable_principal, a.premium_units),
        (0, 0, 0)
    );
    assert_eq!(c.position_state(FORECAST).premium_units, b_units);
    assert_eq!(c.pool_state().principal, 100);
    super::reads::read_context(&c).unwrap();
}

#[test]
fn premium_refund_returns_real_principal_and_excludes_units_once() {
    let mut c = TestContext::with_premium(PoolAlgorithm::H30, 3000);
    let a = c.alice.insecure_clone();
    let b = c.bob.insecure_clone();
    let service = c.service.insecure_clone();
    c.activate();
    c.set_time(OPEN);
    c.send(
        &[
            c.stake_committed_ix(&a, c.alice_token, FORECAST, 100),
            c.stake_ix(&b, c.bob_token, FORECAST, 100),
        ],
        &[&a, &b],
    );
    let a_units = c.stake_state(&a, FORECAST).premium_units;
    let b_units = c.stake_state(&b, FORECAST).premium_units;
    let mut result = ResultFixture::new(&c);
    c.set_time(LOCK - 1);
    result.record_time(&c);
    let case = CaseFixture::start(&mut c, &result);
    case.finalize_yes(&mut c);
    c.send(&[case.begin_ix(&c, &service)], &[&service]);
    assert_eq!(c.pool_state().state, PoolState::Refunding);
    for (owner, token, remaining) in [(&a, c.alice_token, b_units), (&b, c.bob_token, 0)] {
        let before = c.token_balance(token);
        let ix = claim_ix(&c, owner, FORECAST, token);
        c.send(std::slice::from_ref(&ix), &[owner]);
        assert_eq!(c.token_balance(token) - before, 100);
        assert_eq!(c.position_state(FORECAST).premium_units, remaining);
        assert_eq!(c.stake_state(owner, FORECAST).state, StakeState::Refunded);
        c.reject(&[ix], &[owner], custom(ManteonError::InvalidStake));
    }
    assert_eq!(c.stake_state(&a, FORECAST).premium_units, a_units);
    assert_eq!(c.pool_state().principal, 0);
    assert!(c.pool_state().config_released);
    super::reads::read_context(&c).unwrap();
}

#[test]
fn premium_actual_profit_fee_is_once_per_owner_date_after_mixed_topups() {
    use crate::support::setup::config::{address, pubkey};
    use manteon_core::math::fees::quote_profit_fees;
    let mut c = TestContext::with_premium_schedule(
        OPEN,
        LOCK,
        HORIZON,
        None,
        PoolAlgorithm::H30,
        (2000, false),
        3000,
    );
    let a = c.alice.insecure_clone();
    let b = c.bob.insecure_clone();
    let service = c.service.insecure_clone();
    c.activate();
    c.set_time(OPEN);
    c.send(
        &[
            c.stake_committed_ix(&a, c.alice_token, FORECAST, 500),
            c.stake_ix(&a, c.alice_token, FORECAST, 500),
            c.stake_committed_ix(&b, c.bob_token, FORECAST + 30, 1000),
        ],
        &[&a, &b],
    );
    let mut result = ResultFixture::new(&c);
    c.set_time(FORECAST * DAY);
    result.record_time(&c);
    let case = CaseFixture::start(&mut c, &result);
    case.finalize_yes(&mut c);
    c.send(
        &[
            case.begin_ix(&c, &service),
            case.process_ix(&c, &service, &[FORECAST, FORECAST + 30]),
            case.finalize_ix(&c, &service),
        ],
        &[&service],
    );
    let p = c.pool_state();
    let stake = c.stake_state(&a, FORECAST);
    let gross = premium::quote_gross(
        p.algorithm.into(),
        Amount::new(p.principal),
        Amount::new(stake.principal),
        stake.premium_units,
        p.early_premium_bps,
        0,
        Weight::from_le_bytes(p.total_weight),
    )
    .unwrap();
    let fees = quote_profit_fees(
        Amount::new(stake.principal),
        gross,
        p.profit_fee_bps,
        p.platform_share_bps,
    )
    .unwrap();
    assert!(fees.total.get() > 0);
    let platform = address(manteon::state::GlobalConfig::vault_address(&pubkey(c.mint)));
    let owner_before = c.token_balance(c.alice_token);
    let recipient_before = c.token_balance(c.fee_recipient);
    c.send(&[claim_ix(&c, &a, FORECAST, c.alice_token)], &[&a]);
    assert_eq!(
        c.token_balance(c.alice_token) - owner_before,
        fees.net.get()
    );
    assert_eq!(
        c.token_balance(c.fee_recipient) - recipient_before,
        fees.recipient.get()
    );
    assert_eq!(c.token_balance(platform), fees.platform.get());
}

#[test]
fn premium_split_ledger_guard_rejects_invalid_pending_and_locked_units_atomically() {
    use crate::support::setup::config::address;
    use anchor_lang::AccountSerialize;
    let mut c = TestContext::with_premium(PoolAlgorithm::H30, 3000);
    let a = c.alice.insecure_clone();
    c.activate();
    c.set_time(OPEN);
    c.send(
        &[
            c.stake_ix(&a, c.alice_token, FORECAST, 100),
            c.stake_committed_ix(&a, c.alice_token, FORECAST, 50),
        ],
        &[&a],
    );
    let key = address(c.stake_key(&a, FORECAST));
    let original = c.svm.get_account(&key).unwrap();
    let position_key = address(c.position(FORECAST));
    let original_position = c.svm.get_account(&position_key).unwrap();
    for units in [199999, 350001] {
        let mut corrupted = original.clone();
        let mut stake = c.stake_state(&a, FORECAST);
        stake.premium_units = units;
        stake
            .try_serialize(&mut corrupted.data.as_mut_slice())
            .unwrap();
        c.svm.set_account(key, corrupted.clone()).unwrap();
        let mut corrupted_position = original_position.clone();
        let mut position = c.position_state(FORECAST);
        position.premium_units = units;
        position
            .try_serialize(&mut corrupted_position.data.as_mut_slice())
            .unwrap();
        c.svm
            .set_account(position_key, corrupted_position.clone())
            .unwrap();
        c.reject(
            &[c.stake_committed_ix(&a, c.alice_token, FORECAST, 1)],
            &[&a],
            custom(ManteonError::InvalidAccounting),
        );
        c.reject(
            &[c.unstake_ix(&a, c.alice_token, FORECAST, 1)],
            &[&a],
            custom(ManteonError::InvalidAccounting),
        );
        assert_eq!(c.svm.get_account(&key).unwrap(), corrupted);
        assert_eq!(
            c.svm.get_account(&position_key).unwrap(),
            corrupted_position
        );
        c.svm.set_account(key, original.clone()).unwrap();
        c.svm
            .set_account(position_key, original_position.clone())
            .unwrap();
    }
}

#[test]
fn premium_creation_rejects_unlisted_maxima() {
    use crate::support::setup::pool::{NOW, PoolContext};
    use anchor_lang::InstructionData;
    use solana_keypair::Keypair;
    let mut c = PoolContext::new();
    let pool = Keypair::new_from_array([0x91; 32]);
    for cap in [1, 999, 1001, 3001, u16::MAX] {
        let mut ix = c.instruction(&pool);
        ix.data = manteon::instruction::CreatePool {
            event: manteon::state::EventDefinition::ExternalEvent,
            algorithm: PoolAlgorithm::H30,
            early_premium_bps: cap,
            agreement_hash: [0xa1; 32],
            expected_config_hash: c.config_terms_hash(),
            opens_at: NOW + 10,
            lock_at: NOW + 20,
            horizon_at: NOW + 30,
        }
        .data();
        c.assert_rejected(
            &[ix],
            &pool,
            None,
            custom(ManteonError::InvalidPremiumPolicy),
        );
    }
}

#[test]
fn premium_near_u64_principal_eight_date_batches_fit_default_sbf_budget() {
    use crate::support::setup::config::pubkey;
    use anchor_lang::solana_program::program_pack::Pack;
    use anchor_spl::token::spl_token;
    use solana_signer::Signer;
    let mut maximum_cu = 0;
    for algorithm in [PoolAlgorithm::H7, PoolAlgorithm::H15, PoolAlgorithm::H30] {
        for cap in [0, 1000, 2000, 3000] {
            let mut c = TestContext::with_premium(algorithm, cap);
            let owner = c.alice.insecure_clone();
            let service = c.service.insecure_clone();
            let mint = c.svm.get_account(&c.mint).unwrap();
            let supply = spl_token::state::Mint::unpack(&mint.data).unwrap().supply;
            let extra = u64::MAX - supply;
            let mint_ix = spl_token::instruction::mint_to(
                &spl_token::ID,
                &pubkey(c.mint),
                &pubkey(c.alice_token),
                &pubkey(c.payer.pubkey()),
                &[],
                extra,
            )
            .unwrap();
            c.send(&[mint_ix], &[]);
            let total = c.token_balance(c.alice_token);
            assert!(total > u64::MAX / 2);
            c.activate();
            c.set_time(OPEN);
            let half_life = match algorithm {
                PoolAlgorithm::H7 => 7,
                PoolAlgorithm::H15 => 15,
                PoolAlgorithm::H30 => 30,
            };
            let days = [
                FORECAST,
                FORECAST + 1,
                FORECAST + half_life - 1,
                FORECAST + half_life,
                FORECAST + half_life + 1,
                FORECAST + 2 * half_life,
                LOCK.div_euclid(DAY) + 64 * half_life - 1,
                LOCK.div_euclid(DAY) + 64 * half_life,
            ];
            for (index, day) in days.into_iter().enumerate() {
                let amount = if index == 7 {
                    total - (total / 8) * 7
                } else {
                    total / 8
                };
                c.send(
                    &[c.stake_committed_ix(&owner, c.alice_token, day, amount)],
                    &[&owner],
                );
            }
            assert_eq!(c.pool_state().principal, total);
            let mut r = ResultFixture::new(&c);
            c.set_time(FORECAST * DAY);
            r.record_time(&c);
            let case = CaseFixture::start(&mut c, &r);
            case.finalize_yes(&mut c);
            c.send(&[case.begin_ix(&c, &service)], &[&service]);
            let metadata = c.send(&[case.process_ix(&c, &service, &days)], &[&service]);
            maximum_cu = maximum_cu.max(metadata.compute_units_consumed);
            assert!(metadata.compute_units_consumed <= 200000);
            c.send(&[case.finalize_ix(&c, &service)], &[&service]);
            assert!(Weight::from_le_bytes(c.pool_state().total_weight) > Weight::from(u128::MAX));
            let mut paid = 0_u64;
            for day in days {
                let before = c.token_balance(c.alice_token);
                c.send(&[claim_ix(&c, &owner, day, c.alice_token)], &[&owner]);
                paid = paid
                    .checked_add(c.token_balance(c.alice_token) - before)
                    .unwrap();
            }
            assert_eq!(c.pool_state().payout_remaining, total - paid);
            assert!(c.pool_state().config_released);
            super::reads::read_context(&c).unwrap();
        }
    }
    println!(
        "Near-u64 principal, eight-date premium batch maximum: {maximum_cu} CU (12 H/cap combinations)"
    );
}

#[test]
fn premium_price_market_uses_same_raw_weights_and_preserves_permissionless_settlement() {
    use crate::support::{
        price::{Sources, settlement},
        setup::pool::NOW,
    };
    use manteon::state::CrossingDirection;
    let rule = Sources::rule(CrossingDirection::Up);
    let mut c = TestContext::with_premium_schedule(
        OPEN,
        NOW + 7200,
        NOW + 86400,
        Some(rule),
        PoolAlgorithm::H7,
        (0, false),
        3000,
    );
    let a = c.alice.insecure_clone();
    let b = c.bob.insecure_clone();
    let service = c.service.insecure_clone();
    let sources = Sources::install(&mut c, 1, 2);
    c.send(&[sources.ix(&c, &service, "activate")], &[&service]);
    let first = (c.pool_state().lock_at + DAY - 1) / DAY;
    c.set_time(OPEN);
    c.send(
        &[
            c.stake_committed_ix(&a, c.alice_token, first, 100),
            c.stake_ix(&b, c.bob_token, first, 100),
        ],
        &[&a, &b],
    );
    assert_eq!(c.stake_state(&a, first).premium_units, 300000);
    assert_eq!(c.stake_state(&b, first).premium_units, 0);
    let at = NOW + 10800;
    sources.update(&mut c, at, (2_u128 << 32) * 10800, 2, 1);
    c.send(&[sources.ix(&c, &b, "finalize")], &[&b]);
    c.send(&settlement(&c, &b, &[first]), &[&b]);
    for (owner, token, expected) in [(&a, c.alice_token, 113), (&b, c.bob_token, 86)] {
        let before = c.token_balance(token);
        c.send(&[claim_ix(&c, owner, first, token)], &[owner]);
        assert_eq!(c.token_balance(token) - before, expected);
    }
    assert_eq!(c.pool_state().payout_remaining, 1);
    assert!(c.pool_state().config_released);
    super::reads::read_context(&c).unwrap();
}
