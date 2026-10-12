//! Compiled integer accuracy curves checked against an independent exact-certified oracle.

use std::{path::PathBuf, process::Command};

use manteon_core::{
    math::{
        accuracy::{SCALE, coefficient, weighted_principal},
        utc_day::day_error,
    },
    types::{Amount, PoolAlgorithm},
};

#[test]
fn mat_060_mat_071_q64_weights_match_integer_certified_decimal_reference() {
    let generator = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/reference/accuracy_q64_vectors.py");
    let output = Command::new("python3")
        .arg(generator)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Python 3 is required for the independent accuracy reference");
    assert!(
        output.status.success(),
        "reference generation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let vectors = String::from_utf8(output.stdout).unwrap();
    let mut rows = 0;
    let mut distinct_rounding = false;
    for line in vectors.lines() {
        let values = line
            .split('\t')
            .map(|value| value.parse::<u128>().unwrap())
            .collect::<Vec<_>>();
        let [
            half_life,
            error_days,
            principal,
            expected_coefficient,
            ideal_weighted,
        ] = values.as_slice()
        else {
            panic!("invalid reference row: {line}")
        };
        let algorithm = match half_life {
            30 => PoolAlgorithm::H30,
            15 => PoolAlgorithm::H15,
            7 => PoolAlgorithm::H7,
            _ => panic!("unsupported reference curve"),
        };
        let error_days = u64::try_from(*error_days).unwrap();
        let principal = u64::try_from(*principal).unwrap();
        let actual_coefficient = coefficient(algorithm, error_days);
        assert_eq!(
            actual_coefficient, *expected_coefficient,
            "coefficient at error {error_days}"
        );
        let actual = weighted_principal(algorithm, Amount::new(principal), error_days);
        assert_eq!(
            actual,
            u128::from(principal) * expected_coefficient,
            "coefficient-first product at error {error_days}, principal {principal}"
        );
        assert!(actual <= *ideal_weighted);
        if principal > 0 {
            assert!(
                ideal_weighted - actual < u128::from(principal),
                "coefficient quantization error at {line}"
            );
        } else {
            assert_eq!(actual, 0);
        }
        distinct_rounding |= actual != *ideal_weighted;
        rows += 1;
    }
    assert_eq!(rows, 6898, "full domain of reference vectors was consumed");
    assert!(
        distinct_rounding,
        "coefficient-first flooring must not be confused with flooring the ideal product"
    );
}

#[test]
fn mat_071_signed_day_extremes_and_same_date_splits_preserve_weighted_influence() {
    // Large absolute calendar values with a small error must retain their
    // accuracy. Opposite extremes must not overflow signed subtraction.
    for (forecast, observed, error) in [
        (i64::MIN, i64::MIN, 0),
        (i64::MAX, i64::MAX, 0),
        (i64::MIN, i64::MIN + 29, 29),
        (i64::MAX - 30, i64::MAX, 30),
        (-30, 30, 60),
        (0, 10_000, 10_000),
        (i64::MIN, i64::MAX, u64::MAX),
    ] {
        assert_eq!(day_error(forecast, observed), error);
        let weight = weighted_principal(
            PoolAlgorithm::H30,
            Amount::new(u64::MAX),
            day_error(forecast, observed),
        );
        assert_eq!(
            weight,
            weighted_principal(
                PoolAlgorithm::H30,
                Amount::new(u64::MAX),
                day_error(observed, forecast)
            )
        );
        assert_eq!(
            weight,
            weighted_principal(PoolAlgorithm::H30, Amount::new(u64::MAX), error)
        );
    }

    let parts = [1, 1_000_003, u64::MAX - 1_000_004];
    let total_principal = parts.into_iter().map(u128::from).sum::<u128>();
    assert_eq!(total_principal, u128::from(u64::MAX));
    for error in [0, 1, 29, 30, 31, 1919, 1920, 1921, 10_000, u64::MAX] {
        let split = parts
            .into_iter()
            .map(|principal| weighted_principal(PoolAlgorithm::H30, Amount::new(principal), error))
            .sum::<u128>();
        let merged = weighted_principal(PoolAlgorithm::H30, Amount::new(u64::MAX), error);
        assert_eq!(split, merged, "same-date partition at error {error}");
        assert!(merged <= total_principal * SCALE);
    }
    let mixed = parts
        .into_iter()
        .zip([0, 29, 30])
        .map(|(principal, error)| {
            weighted_principal(PoolAlgorithm::H30, Amount::new(principal), error)
        })
        .sum::<u128>();
    assert!(mixed <= total_principal * SCALE);
}
