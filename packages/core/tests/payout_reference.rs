//! Gross-allocation arithmetic only: no claim consumption, fee or dust ownership.

use std::{path::PathBuf, process::Command};

use manteon_core::{
    DomainError,
    math::{
        accuracy::{SCALE, weighted_principal},
        payout::quote_gross_payout,
    },
    types::{Amount, PoolAlgorithm},
};

#[test]
fn mat_066_mat_071_gross_floor_matches_python_wide_integer_division() {
    let generator =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/reference/payout_vectors.py");
    let output = Command::new("python3")
        .arg(generator)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Python 3 is required for the independent payout reference");
    assert!(
        output.status.success(),
        "payout reference failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let vectors = String::from_utf8(output.stdout).unwrap();
    let mut rows = 0;
    let mut wide_numerator = false;
    for line in vectors.lines() {
        let values = line
            .split('\t')
            .map(|value| value.parse::<u128>().unwrap())
            .collect::<Vec<_>>();
        let [half_life, pool, stake, error, denominator, expected] = values.as_slice() else {
            panic!("invalid payout reference row: {line}")
        };
        let algorithm = match half_life {
            30 => PoolAlgorithm::H30,
            15 => PoolAlgorithm::H15,
            7 => PoolAlgorithm::H7,
            _ => panic!("unsupported reference curve"),
        };
        let pool = u64::try_from(*pool).unwrap();
        let stake = u64::try_from(*stake).unwrap();
        let error = u64::try_from(*error).unwrap();
        let actual = quote_gross_payout(
            algorithm,
            Amount::new(pool),
            Amount::new(stake),
            error,
            *denominator,
        )
        .unwrap();
        assert_eq!(u128::from(actual.get()), *expected, "payout vector {line}");
        wide_numerator |= u128::from(pool)
            .checked_mul(weighted_principal(algorithm, Amount::new(stake), error))
            .is_none();
        rows += 1;
    }
    assert_eq!(rows, 4056, "consume the complete reference corpus");
    assert!(
        wide_numerator,
        "reference must exercise products exceeding u128"
    );
}

#[test]
fn mat_068_mat_071_complete_books_conserve_principal_in_all_orders_and_splits_cannot_gain() {
    const ORDERS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for book in [
        [(1_u64, 0_u64), (1, 30), (1, 60)],
        [(u64::MAX - 2, 0), (1, 30), (1, 1920)],
        [(1, 1921), (u64::MAX / 2, 1), (u64::MAX / 2 - 1, 29)],
    ] {
        let pool = book.iter().map(|(principal, _)| principal).sum::<u64>();
        let denominator = book
            .iter()
            .map(|(principal, error)| {
                weighted_principal(PoolAlgorithm::H30, Amount::new(*principal), *error)
            })
            .sum::<u128>();
        let expected = book.map(|(principal, error)| {
            quote_gross_payout(
                PoolAlgorithm::H30,
                Amount::new(pool),
                Amount::new(principal),
                error,
                denominator,
            )
            .unwrap()
            .get()
        });
        if pool == 3 {
            assert_eq!(
                expected,
                [1, 0, 0],
                "fractional entitlements floor independently"
            );
        }
        for order in ORDERS {
            let mut remaining = pool;
            for owner in order {
                let (principal, error) = book[owner];
                let gross = quote_gross_payout(
                    PoolAlgorithm::H30,
                    Amount::new(pool),
                    Amount::new(principal),
                    error,
                    denominator,
                )
                .unwrap()
                .get();
                assert_eq!(gross, expected[owner]);
                remaining = remaining
                    .checked_sub(gross)
                    .expect("gross allocations cannot overpay the frozen principal");
            }
            assert!(
                remaining < book.len() as u64,
                "flooring residue is less than the entitlement count"
            );
        }
    }

    let mut rounding_loss = false;
    for (pool, parts, error, other_error) in [
        (3, [1, 1], 30, 0),
        (4000, [400, 600], 7, 30),
        (u64::MAX, [1, u64::MAX - 2], 1, 0),
    ] {
        let stake = parts.into_iter().sum::<u64>();
        let denominator = weighted_principal(PoolAlgorithm::H30, Amount::new(stake), error)
            + weighted_principal(PoolAlgorithm::H30, Amount::new(pool - stake), other_error);
        let merged = quote_gross_payout(
            PoolAlgorithm::H30,
            Amount::new(pool),
            Amount::new(stake),
            error,
            denominator,
        )
        .unwrap()
        .get();
        let split = parts
            .into_iter()
            .map(|part| {
                quote_gross_payout(
                    PoolAlgorithm::H30,
                    Amount::new(pool),
                    Amount::new(part),
                    error,
                    denominator,
                )
                .unwrap()
                .get()
            })
            .sum::<u64>();
        assert!(
            split <= merged,
            "same-date splitting cannot increase gross atoms"
        );
        assert!(merged - split < parts.len() as u64);
        rounding_loss |= split < merged;
    }
    assert!(
        rounding_loss,
        "the fixtures must distinguish ideal additivity from floored token payouts"
    );
}

#[test]
fn mat_064_mat_071_gross_quote_rejects_zero_or_inconsistent_denominators() {
    for (pool, stake, error, denominator, expected) in [
        (0, 0, 0, 0, DomainError::ZeroDenominator),
        (10, 5, 0, 0, DomainError::ZeroDenominator),
        (10, 10, 1921, 0, DomainError::ZeroDenominator),
        (10, 11, 0, 11 * SCALE, DomainError::InvalidAccounting),
        (10, 5, 0, 5 * SCALE - 1, DomainError::InvalidAccounting),
        (
            10,
            5,
            30,
            5 * SCALE / 2 + 5 * SCALE + 1,
            DomainError::InvalidAccounting,
        ),
        (10, 10, 0, 10 * SCALE + 1, DomainError::InvalidAccounting),
        (0, 0, 0, 1, DomainError::InvalidAccounting),
        (10, 10, 1921, 1, DomainError::InvalidAccounting),
        (10, 0, 0, 10 * SCALE + 1, DomainError::InvalidAccounting),
        (10, 5, 1921, 5 * SCALE + 1, DomainError::InvalidAccounting),
    ] {
        assert_eq!(
            quote_gross_payout(
                PoolAlgorithm::H30,
                Amount::new(pool),
                Amount::new(stake),
                error,
                denominator
            ),
            Err(expected),
            "pool={pool}, stake={stake}, error={error}, denominator={denominator}"
        );
    }
}
