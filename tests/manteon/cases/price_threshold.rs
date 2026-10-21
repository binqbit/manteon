use crate::support::{
    DAY, OPEN, OWNER_BALANCE, TestContext,
    price::{Sources, settlement},
    settlement::claim_ix,
    setup::{
        config::{address, pubkey},
        pool::{NOW, PoolContext},
    },
};
use anchor_lang::{AccountSerialize, error::ErrorCode};
use manteon::{
    error::ManteonError,
    state::{CrossingDirection, PoolAlgorithm, PoolState, UsdQuoteSource},
};
use solana_instruction_error::InstructionError;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;

fn invalid(error: ManteonError) -> TransactionError {
    TransactionError::InstructionError(0, InstructionError::Custom(error.into()))
}

fn quote_sources() -> [UsdQuoteSource; 3] {
    [
        UsdQuoteSource::Usdc,
        UsdQuoteSource::PythSolUsd {
            price_update_account: anchor_lang::prelude::Pubkey::new_from_array([0x95; 32]),
            receiver_program: anchor_lang::pubkey!("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ"),
            feed_id: manteon::integration::price::SOL_USD_FEED_ID,
            max_age_seconds: 60,
            max_confidence_bps: 100,
        },
        UsdQuoteSource::RaydiumSolUsdc {
            pool_id: anchor_lang::prelude::Pubkey::new_from_array([0xa1; 32]),
        },
    ]
}

#[test]
fn permissionless_price_payouts_cover_both_directions_each_usd_source_and_accuracy_curve() {
    // Independently rounded payouts for 101 atoms at the event and 100 thirty days
    // later. The common one-day error cancels in each curve's relative weights.
    for (algorithm, winner, loser) in [
        (PoolAlgorithm::H30, 134, 66),
        (PoolAlgorithm::H15, 161, 39),
        (PoolAlgorithm::H7, 191, 9),
    ] {
        for quote in quote_sources() {
            for direction in [CrossingDirection::Up, CrossingDirection::Down] {
                let mut rule = Sources::rule(direction);
                rule.usd_quote_source = quote;
                let mut context = TestContext::with_price_threshold_and_algorithm(rule, algorithm);
                let alice = context.alice.insecure_clone();
                let bob = context.bob.insecure_clone();
                let service = context.service.insecure_clone();
                let (before, after) = if direction == CrossingDirection::Up {
                    ((1, 2), (2, 1))
                } else {
                    ((2, 1), (1, 2))
                };
                let sources = Sources::install(&mut context, before.0, before.1);
                context.send(&[sources.ix(&context, &service, "activate")], &[&service]);
                let result_at = NOW + 10_800;
                let day = result_at / DAY;
                context.set_time(OPEN);
                // The forecast midnight must lie at or after entry closes.
                let first = (context.pool_state().lock_at + DAY - 1) / DAY;
                context.send(
                    &[context.stake_ix(&alice, context.alice_token, first, 101)],
                    &[&alice],
                );
                context.send(
                    &[context.stake_ix(&bob, context.bob_token, first + 30, 100)],
                    &[&bob],
                );
                // Both converted sources fix SOL at $100. Token/SOL has 9 quote decimals
                // and 6 base decimals, so its raw atom ratio is ten times the USD price.
                let ratio = if quote == UsdQuoteSource::Usdc { 1 } else { 10 };
                let cumulative =
                    (((u128::from(after.0) * ratio) << 32) / u128::from(after.1)) * 10_800;
                sources.update(&mut context, result_at, cumulative, after.0, after.1);
                context.send(&[sources.ix(&context, &bob, "finalize")], &[&bob]);
                assert_eq!(context.pool_state().state, PoolState::Settling);
                assert_eq!(context.pool_state().accepted_case, None);
                let view = super::reads::read_context(&context).unwrap();
                assert_eq!(view.pool.service_eligible_at, None);
                assert_eq!(
                    super::reads::read_context_monitor(&context)
                        .unwrap()
                        .unwrap()
                        .baseline,
                    None
                );
                let result = view.event.result.unwrap();
                assert_eq!(result.occurred_at, result_at);
                let bounds = result.price_bounds().unwrap();
                let expected = 1_000_000_000 * after.0 / after.1;
                if quote == UsdQuoteSource::Usdc {
                    assert_eq!(bounds.lower, expected);
                    assert_eq!(bounds.upper, expected + 1);
                } else {
                    assert!(bounds.lower <= expected && bounds.upper >= expected);
                    assert!(bounds.upper - bounds.lower < 5_000_000);
                }
                assert!(direction.crossed(bounds, rule.target_price_usd_e9));
                context.send(
                    &settlement(&context, &alice, &[first, first + 30]),
                    &[&alice],
                );
                assert_eq!(context.pool_state().state, PoolState::Claimable);
                assert_eq!(
                    super::reads::read_context(&context).unwrap().event.result,
                    Some(result)
                );
                // Both owners receive the independently specified floor of their share.
                assert!(first == day || first == day + 1);
                for (owner, token, forecast, expected) in [
                    (&alice, context.alice_token, first, winner),
                    (&bob, context.bob_token, first + 30, loser),
                ] {
                    let balance = context.token_balance(token);
                    context.send(
                        &[claim_ix(&context, owner, forecast, pubkey(token))],
                        &[owner],
                    );
                    assert_eq!(context.token_balance(token), balance + expected);
                }
                assert_eq!(context.pool_state().payout_remaining, 1);
                assert_eq!(
                    context.token_balance(address(PoolContext::vault(&context.pool))),
                    1
                );
                for action in ["observe", "finalize"] {
                    context.reject(
                        &[sources.ix(&context, &alice, action)],
                        &[&alice],
                        invalid(ManteonError::InvalidPoolState),
                    );
                    assert_eq!(context.event_state().result, Some(result));
                }
            }
        }
    }
}

#[test]
fn false_crossing_stale_source_substitution_and_erasing_a_crossing_leave_all_accounts_unchanged() {
    let mut context = TestContext::with_price_threshold(Sources::rule(CrossingDirection::Up));
    let service = context.service.insecure_clone();
    let alice = context.alice.insecure_clone();
    let sources = Sources::install(&mut context, 1, 2);
    context.send(&[sources.ix(&context, &service, "activate")], &[&service]);
    sources.update(&mut context, NOW + 3600, (1_u128 << 31) * 3600, 1, 2);
    context.reject(
        &[sources.ix(&context, &alice, "finalize")],
        &[&alice],
        invalid(ManteonError::PriceThresholdNotCrossed),
    );
    context.send(&[sources.ix(&context, &alice, "observe")], &[&alice]);
    sources.update(
        &mut context,
        NOW + 7200,
        (1_u128 << 31) * 3600 + (2_u128 << 32) * 3600,
        2,
        1,
    );
    context.reject(
        &[sources.ix(&context, &alice, "observe")],
        &[&alice],
        invalid(ManteonError::InvalidPriceObservation),
    );
    let mut wrong = sources.ix(&context, &alice, "finalize");
    wrong.accounts[6].pubkey = address(context.token_mint);
    context.reject(
        &[wrong],
        &[&alice],
        invalid(ManteonError::InvalidPriceSource),
    );
    context.set_time(NOW + 7321);
    context.reject(
        &[sources.ix(&context, &alice, "finalize")],
        &[&alice],
        invalid(ManteonError::StalePriceSource),
    );
}

#[test]
fn sol_conversion_rejects_substitution_and_stale_updates_before_permissionless_acceptance() {
    for quote in quote_sources().into_iter().skip(1) {
        let mut rule = Sources::rule(CrossingDirection::Up);
        rule.usd_quote_source = quote;
        let mut context = TestContext::with_price_threshold(rule);
        let service = context.service.insecure_clone();
        let alice = context.alice.insecure_clone();
        let sources = Sources::install(&mut context, 1, 2);
        context.send(&[sources.ix(&context, &service, "activate")], &[&service]);
        let result_at = NOW + 10_800;
        sources.update(&mut context, result_at, (20_u128 << 32) * 10_800, 2, 1);
        let instruction = sources.ix(&context, &alice, "finalize");
        let mut substituted = instruction.clone();
        substituted.accounts[10].pubkey = address(context.token_mint);
        context.reject(
            &[substituted],
            &[&alice],
            invalid(ManteonError::InvalidPriceSource),
        );

        // Main pool data remains fresh while only the fixed conversion becomes stale.
        let conversion_key =
            instruction.accounts[if matches!(quote, UsdQuoteSource::PythSolUsd { .. }) {
                10
            } else {
                11
            }]
            .pubkey;
        let original = context.svm.get_account(&conversion_key).unwrap();
        let mut stale = original.clone();
        let (offset, old_time) = match quote {
            UsdQuoteSource::PythSolUsd { .. } => (93, result_at - 61),
            UsdQuoteSource::RaydiumSolUsdc { .. } => (4043, result_at - 31),
            UsdQuoteSource::Usdc => unreachable!(),
        };
        stale.data[offset..offset + 8].copy_from_slice(&old_time.to_le_bytes());
        context.svm.set_account(conversion_key, stale).unwrap();
        context.reject(
            std::slice::from_ref(&instruction),
            &[&alice],
            invalid(ManteonError::StalePriceSource),
        );
        assert_eq!(context.event_state().result, None);
        assert_eq!(context.pool_state().state, PoolState::Active);
        context.svm.set_account(conversion_key, original).unwrap();
        context.send(&[instruction], &[&alice]);
        assert_eq!(context.event_state().result.unwrap().occurred_at, result_at);
    }
}

#[test]
fn early_or_unobservable_crossings_refund_principal_without_an_audit() {
    for expired in [false, true] {
        let mut context = TestContext::with_price_threshold(Sources::rule(CrossingDirection::Up));
        let service = context.service.insecure_clone();
        let alice = context.alice.insecure_clone();
        let sources = Sources::install(&mut context, 1, 2);
        context.send(&[sources.ix(&context, &service, "activate")], &[&service]);
        context.set_time(OPEN);
        let first = (context.pool_state().lock_at + DAY - 1) / DAY;
        context.send(
            &[context.stake_ix(&alice, context.alice_token, first, 101)],
            &[&alice],
        );
        if expired {
            context.set_time(NOW + 14_401);
            context.send(&[sources.ix(&context, &alice, "expire")], &[&alice]);
            assert_eq!(context.event_state().result, None);
        } else {
            sources.update(&mut context, NOW + 3600, (2_u128 << 32) * 3600, 2, 1);
            context.send(&[sources.ix(&context, &alice, "finalize")], &[&alice]);
        }
        assert_eq!(context.pool_state().state, PoolState::Refunding);
        let view = super::reads::read_context(&context).unwrap();
        assert_eq!(
            super::reads::read_context_monitor(&context)
                .unwrap()
                .unwrap()
                .baseline,
            None
        );
        assert_eq!(view.pool.service_eligible_at, None);
        context.send(
            &[claim_ix(
                &context,
                &alice,
                first,
                pubkey(context.alice_token),
            )],
            &[&alice],
        );
        assert_eq!(context.token_balance(context.alice_token), OWNER_BALANCE);
        assert_eq!(
            context.token_balance(address(PoolContext::vault(&context.pool))),
            0
        );
        assert_eq!(context.pool_state().principal, 0);
        assert!(context.pool_state().config_released);
        let account = context.svm.get_account(&context.config_address).unwrap();
        use anchor_lang::AccountDeserialize;
        assert_eq!(
            manteon::state::Config::try_deserialize(&mut account.data.as_slice())
                .unwrap()
                .active_pools,
            0
        );
        super::reads::read_context(&context).unwrap();
        context.reject(
            &[claim_ix(
                &context,
                &alice,
                first,
                pubkey(context.alice_token),
            )],
            &[&alice],
            invalid(ManteonError::InvalidStake),
        );
    }
}

#[test]
fn activation_creates_only_the_price_monitor_and_failed_checks_leave_no_account() {
    let mut context = TestContext::with_price_threshold(Sources::rule(CrossingDirection::Up));
    let service = context.service.insecure_clone();
    let payer = context.payer.insecure_clone();
    let monitor_key = address(context.price_monitor());
    let event_key = address(PoolContext::event(&context.pool));
    let event_before = context.svm.get_account(&event_key).unwrap();
    let sources = Sources::install(&mut context, 2, 1);
    context.reject(
        &[sources.ix(&context, &payer, "activate")],
        &[],
        invalid(ManteonError::UnauthorizedServiceAuthority),
    );
    context.reject(
        &[sources.ix(&context, &service, "activate")],
        &[&service],
        invalid(ManteonError::InvalidPriceObservation),
    );
    assert!(context.svm.get_account(&monitor_key).is_none());
    assert_eq!(context.pool_state().state, PoolState::Draft);

    // A third party cannot prevent activation by prefunding the deterministic PDA.
    let prefunded = 1_000_000;
    context.send(
        &[solana_system_interface::instruction::transfer(
            &pubkey(context.payer.pubkey()),
            &context.price_monitor(),
            prefunded,
        )],
        &[],
    );
    assert!(
        super::reads::read_context_monitor(&context)
            .unwrap()
            .is_none()
    );
    sources.update(&mut context, NOW, 0, 1, 2);
    let payer_before = context.svm.get_account(&service.pubkey()).unwrap().lamports;
    context.send(&[sources.ix(&context, &service, "activate")], &[&service]);
    let monitor = context.svm.get_account(&monitor_key).unwrap();
    assert_eq!(monitor.data.len(), 89);
    assert_eq!(monitor.owner, address(manteon::ID));
    assert_eq!(
        monitor.lamports,
        context.svm.minimum_balance_for_rent_exemption(89)
    );
    assert_eq!(
        context.svm.get_account(&service.pubkey()).unwrap().lamports,
        payer_before - (monitor.lamports - prefunded)
    );
    assert_eq!(context.svm.get_account(&event_key).unwrap(), event_before);
    assert_eq!(
        context.price_monitor_state().pool,
        pubkey(context.pool.pubkey())
    );
    assert_eq!(
        super::reads::read_context_monitor(&context)
            .unwrap()
            .unwrap()
            .baseline
            .unwrap()
            .observed_at,
        NOW
    );
    context.reject(
        &[sources.ix(&context, &service, "activate")],
        &[&service],
        TransactionError::InstructionError(0, InstructionError::Custom(0)),
    );
}

#[test]
fn observations_require_the_writable_bound_monitor_and_exact_price_sources() {
    let mut context = TestContext::with_price_threshold(Sources::rule(CrossingDirection::Up));
    let service = context.service.insecure_clone();
    let alice = context.alice.insecure_clone();
    let sources = Sources::install(&mut context, 1, 2);
    context.send(&[sources.ix(&context, &service, "activate")], &[&service]);
    sources.update(&mut context, NOW + 3600, (1_u128 << 31) * 3600, 1, 2);
    let monitor_key = address(context.price_monitor());
    let original = context.svm.get_account(&monitor_key).unwrap();
    let mut foreign = context.price_monitor_state();
    foreign.pool = pubkey(context.config_address);
    let mut changed = original.clone();
    foreign
        .try_serialize(&mut changed.data.as_mut_slice())
        .unwrap();
    context.svm.set_account(monitor_key, changed).unwrap();
    context.reject(
        &[sources.ix(&context, &alice, "observe")],
        &[&alice],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ErrorCode::ConstraintHasOne.into()),
        ),
    );
    context.svm.set_account(monitor_key, original).unwrap();

    let mut readonly = sources.ix(&context, &alice, "observe");
    readonly.accounts[5].is_writable = false;
    context.reject(
        &[readonly],
        &[&alice],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ErrorCode::ConstraintMut.into()),
        ),
    );
    let mut extra = sources.ix(&context, &alice, "observe");
    extra.accounts.push(extra.accounts[6].clone());
    context.reject(
        &[extra],
        &[&alice],
        invalid(ManteonError::InvalidPriceSource),
    );
    context.send(&[sources.ix(&context, &alice, "observe")], &[&alice]);
    assert_eq!(
        context.price_monitor_state().baseline.unwrap().observed_at,
        NOW + 3600
    );
}
