//! Local settlement accounting; account authenticity and freeze are not simulated.

use manteon_core::{
    DomainError,
    accounting::{principal::PrincipalBook, settlement::SettlementProgress},
    math::{accuracy::SCALE, payout::quote_gross_payout},
    types::{Amount, PoolAlgorithm},
};

#[test]
fn mat_060_mat_062_unequal_dates_resume_with_the_same_unique_denominator_in_every_order() {
    for (positions, expected_weight) in [
        ([(100, 0), (200, 30), (300, 60)], 275 * SCALE),
        (
            [(u64::MAX - 2, 0), (1, 30), (1, 1920)],
            u128::from(u64::MAX - 2) * SCALE + SCALE / 2 + 1,
        ),
    ] {
        let pool_principal = Amount::new(positions.iter().map(|(principal, _)| principal).sum());
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let mut progress =
                SettlementProgress::new(PoolAlgorithm::H30, pool_principal, 3, 0, 0).unwrap();
            let mut computed = [false; 3];
            for (processed, index) in order.into_iter().enumerate() {
                let (principal, error) = positions[index];
                progress
                    .process_position(Amount::new(principal), error, &mut computed[index])
                    .unwrap();
                assert!(computed[index]);
                assert_eq!(progress.processed_dates(), processed as u64 + 1);
                if processed < 2 {
                    assert_eq!(
                        progress.validate_complete(),
                        Err(DomainError::IncompleteSettlement)
                    );
                }
                if processed == 0 {
                    // Reconstruct numerical progress after a pause, retaining the
                    // existing markers. This is not an authenticated runtime reload.
                    let resumed = SettlementProgress::new(
                        PoolAlgorithm::H30,
                        pool_principal,
                        3,
                        progress.processed_dates(),
                        progress.total_weight(),
                    )
                    .unwrap();
                    assert_eq!(resumed, progress);
                    progress = resumed;
                }
            }
            assert_eq!(computed, [true; 3]);
            assert_eq!(progress.total_weight(), expected_weight);
            assert_eq!(progress.validate_complete(), Ok(()));
        }
    }
}

#[test]
fn mat_061_mat_062_invalid_progress_or_positions_cannot_partially_advance_markers() {
    for (principal, active, processed, weight) in [
        (0, 1, 0, 0),
        (1, 0, 0, 0),
        (2, 3, 0, 0),
        (3, 2, 3, 0),
        (3, 2, 0, 1),
        (3, 2, 1, 3 * SCALE),
    ] {
        assert_eq!(
            SettlementProgress::new(
                PoolAlgorithm::H30,
                Amount::new(principal),
                active,
                processed,
                weight
            ),
            Err(DomainError::InvalidAccounting)
        );
    }

    let initial = SettlementProgress::new(PoolAlgorithm::H30, Amount::new(10), 3, 0, 0).unwrap();
    let mut replay = initial;
    let mut replay_marker = false;
    replay
        .process_position(Amount::new(3), 0, &mut replay_marker)
        .unwrap();
    let full =
        SettlementProgress::new(PoolAlgorithm::H30, Amount::new(10), 1, 1, 10 * SCALE).unwrap();
    let sole_date = SettlementProgress::new(PoolAlgorithm::H30, Amount::new(10), 1, 0, 0).unwrap();
    let near_capacity =
        SettlementProgress::new(PoolAlgorithm::H30, Amount::new(10), 2, 1, 9 * SCALE).unwrap();
    let near_overflow = SettlementProgress::new(
        PoolAlgorithm::H30,
        Amount::new(u64::MAX),
        2,
        1,
        u128::from(u64::MAX - 1) * SCALE,
    )
    .unwrap();
    for (mut progress, principal, error, mut computed, expected) in [
        (
            replay,
            3,
            0,
            replay_marker,
            DomainError::PositionAlreadyComputed,
        ),
        (initial, 0, 0, false, DomainError::ZeroAmount),
        (full, 1, 0, false, DomainError::InvalidAccounting),
        (sole_date, 9, 0, false, DomainError::InvalidAccounting),
        (initial, 9, 0, false, DomainError::InvalidAccounting),
        (near_capacity, 2, 0, false, DomainError::InvalidAccounting),
        // A zero coefficient still reserves the Position's full principal.
        (
            near_capacity,
            9,
            1921,
            false,
            DomainError::InvalidAccounting,
        ),
        (
            near_overflow,
            u64::MAX - 1,
            0,
            false,
            DomainError::InvalidAccounting,
        ),
    ] {
        let before = progress;
        let marker_before = computed;
        assert_eq!(
            progress.process_position(Amount::new(principal), error, &mut computed),
            Err(expected)
        );
        assert_eq!(progress, before);
        assert_eq!(computed, marker_before);
    }
}

#[test]
fn mat_062_mat_064_zero_weights_count_but_cannot_make_a_zero_denominator_complete() {
    let empty = SettlementProgress::new(PoolAlgorithm::H30, Amount::ZERO, 0, 0, 0).unwrap();
    assert_eq!(empty.validate_complete(), Err(DomainError::ZeroDenominator));
    let mut progress =
        SettlementProgress::new(PoolAlgorithm::H30, Amount::new(3), 2, 0, 0).unwrap();
    let mut first = false;
    progress
        .process_position(Amount::new(1), 1921, &mut first)
        .unwrap();
    assert!(first);
    assert_eq!(progress.processed_dates(), 1);
    assert_eq!(progress.total_weight(), 0);
    assert_eq!(
        progress.validate_complete(),
        Err(DomainError::IncompleteSettlement)
    );

    let mut all_zero = progress;
    let mut zero_second = false;
    all_zero
        .process_position(Amount::new(2), u64::MAX, &mut zero_second)
        .unwrap();
    assert!(zero_second);
    assert_eq!(all_zero.processed_dates(), 2);
    assert_eq!(
        all_zero.validate_complete(),
        Err(DomainError::ZeroDenominator)
    );

    let mut positive_second = false;
    progress
        .process_position(Amount::new(2), 0, &mut positive_second)
        .unwrap();
    assert_eq!(progress.processed_dates(), 2);
    assert_eq!(progress.total_weight(), 2 * SCALE);
    assert_eq!(progress.validate_complete(), Ok(()));
}

#[test]
fn mat_036_mat_060_mat_066_residual_principal_is_the_basis_for_processing_and_gross_quotes() {
    // A mathematical exit removes the entire 200-atom date from a 600-atom book.
    // Neither this quote nor the subsequent calculation establishes owner rights.
    let residual = PrincipalBook::new(Amount::new(600), Amount::new(200), 3)
        .unwrap()
        .quote_remove(Amount::new(200))
        .unwrap();
    assert_eq!(residual.pool_principal(), Amount::new(400));
    assert_eq!(residual.position_principal(), Amount::ZERO);
    assert_eq!(residual.active_dates(), 2);
    let mut progress = SettlementProgress::new(
        PoolAlgorithm::H30,
        residual.pool_principal(),
        residual.active_dates(),
        0,
        0,
    )
    .unwrap();
    for (principal, error) in [(100, 0), (300, 60)] {
        let mut computed = false;
        progress
            .process_position(Amount::new(principal), error, &mut computed)
            .unwrap();
        assert!(computed);
    }
    assert_eq!(progress.total_weight(), 175 * SCALE);
    progress.validate_complete().unwrap();
    let gross = [(100, 0), (300, 60)].map(|(principal, error)| {
        quote_gross_payout(
            PoolAlgorithm::H30,
            residual.pool_principal(),
            Amount::new(principal),
            error,
            progress.total_weight(),
        )
        .unwrap()
        .get()
    });
    assert_eq!(gross, [228, 171]);
    assert_eq!(
        residual.pool_principal().get() - gross.into_iter().sum::<u64>(),
        1
    );
    assert_eq!(200 + residual.pool_principal().get(), 600);
    assert_eq!(
        progress.total_weight(),
        175 * SCALE,
        "gross quotes retain the frozen basis"
    );
}

#[test]
fn every_algorithm_settles_one_frozen_basis_and_same_date_splits_cannot_gain() {
    use manteon_core::math::accuracy::weighted_principal;
    for algorithm in [PoolAlgorithm::H30, PoolAlgorithm::H15, PoolAlgorithm::H7] {
        let mut progress = SettlementProgress::new(algorithm, Amount::new(200), 2, 0, 0).unwrap();
        for error in [0, 30] {
            progress
                .process_position(Amount::new(100), error, &mut false)
                .unwrap();
        }
        progress.validate_complete().unwrap();
        let expected = weighted_principal(algorithm, Amount::new(100), 0)
            + weighted_principal(algorithm, Amount::new(100), 30);
        assert_eq!(progress.total_weight(), expected);
        let quote = |amount, error| {
            quote_gross_payout(
                algorithm,
                Amount::new(200),
                Amount::new(amount),
                error,
                expected,
            )
            .unwrap()
            .get()
        };
        assert!(quote(100, 0) + quote(100, 30) <= 200);
        assert!(quote(33, 30) + quote(67, 30) <= quote(100, 30));
        let boundary = algorithm.max_error_days();
        assert_eq!(
            manteon_core::math::accuracy::coefficient(algorithm, boundary),
            1
        );
        assert_eq!(
            manteon_core::math::accuracy::coefficient(algorithm, boundary + 1),
            0
        );
    }
}
