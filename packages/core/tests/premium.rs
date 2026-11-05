//! Commitment premium arithmetic, independently referenced and boundary checked.

use manteon_core::{
    DomainError,
    accounting::{allowance::AllowanceSnapshot, settlement::PremiumSettlementProgress},
    math::{
        accuracy, payout,
        premium::{self, Weight},
    },
    types::{Amount, PoolAlgorithm},
};
use std::{path::PathBuf, process::Command};

#[test]
fn premium_quotes_match_independent_full_width_integer_reference() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/reference/premium_vectors.py");
    let output = Command::new("python3")
        .arg(path)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut count = 0;
    let mut wider_than_u128 = false;
    for row in String::from_utf8(output.stdout).unwrap().lines() {
        let cells = row.split('\t').collect::<Vec<_>>();
        let algorithm = match cells[0] {
            "7" => PoolAlgorithm::H7,
            "15" => PoolAlgorithm::H15,
            "30" => PoolAlgorithm::H30,
            _ => unreachable!(),
        };
        let cap = cells[1].parse().unwrap();
        let duration = cells[2].parse::<u64>().unwrap();
        let remaining = cells[3].parse::<u64>().unwrap();
        // Signed endpoints spanning every representable u64 duration.
        let open = i64::MIN;
        let lock = (i128::from(open) + i128::from(duration)) as i64;
        let anchor = (i128::from(lock) - i128::from(remaining)) as i64;
        let rate = premium::rate_bps(cap, anchor, open, lock).unwrap();
        assert_eq!(rate, cells[4].parse::<u16>().unwrap());
        let principal = Amount::new(cells[5].parse().unwrap());
        let error = cells[6].parse().unwrap();
        let units = premium::units(principal, rate).unwrap();
        assert_eq!(units, cells[7].parse::<u128>().unwrap());
        let weight = premium::weighted_principal(algorithm, principal, units, cap, error).unwrap();
        assert_eq!(weight, cells[8].parse::<Weight>().unwrap());
        assert_eq!(Weight::from_le_bytes(weight.to_le_bytes()), weight);
        wider_than_u128 |= weight > Weight::from(u128::MAX);
        let total = cells[9].parse().unwrap();
        let quote = premium::quote_gross(
            algorithm,
            Amount::new(u64::MAX),
            principal,
            units,
            cap,
            error,
            total,
        )
        .unwrap();
        assert_eq!(quote.get(), cells[10].parse::<u64>().unwrap());
        count += 1;
    }
    assert_eq!(count, 612);
    assert!(wider_than_u128);
}

#[test]
fn disabled_premium_is_bit_exact_to_baseline_and_aggregates_do_not_floor_averages() {
    for algorithm in [PoolAlgorithm::H7, PoolAlgorithm::H15, PoolAlgorithm::H30] {
        for error in [0, 1, algorithm.max_error_days()] {
            let p = Amount::new(u64::MAX - 1);
            let old_weight = accuracy::weighted_principal(algorithm, p, error);
            let total = old_weight + accuracy::SCALE;
            assert_eq!(
                premium::weighted_principal(algorithm, p, 0, 0, error).unwrap(),
                Weight::from(old_weight) * Weight::from(10000_u16)
            );
            assert_eq!(
                premium::quote_gross(
                    algorithm,
                    Amount::new(u64::MAX),
                    p,
                    0,
                    0,
                    error,
                    Weight::from(total) * Weight::from(10000_u16)
                )
                .unwrap(),
                payout::quote_gross_payout(algorithm, Amount::new(u64::MAX), p, error, total)
                    .unwrap()
            );
        }
    }
    let a = premium::weighted_principal(PoolAlgorithm::H7, Amount::new(1), 1001, 3000, 1).unwrap();
    let b = premium::weighted_principal(PoolAlgorithm::H7, Amount::new(2), 2001, 3000, 1).unwrap();
    assert_eq!(
        a + b,
        premium::weighted_principal(PoolAlgorithm::H7, Amount::new(3), 3002, 3000, 1).unwrap()
    );
}

#[test]
fn premium_policy_boundaries_capacity_and_failed_markers_are_checked() {
    for cap in [1, 999, 1001, 2999, 3001, u16::MAX] {
        assert_eq!(
            premium::validate_max_bps(cap),
            Err(DomainError::InvalidPremiumPolicy)
        );
    }
    assert_eq!(premium::rate_bps(3000, 0, 0, 3).unwrap(), 3000);
    assert_eq!(premium::rate_bps(3000, 1, 0, 3).unwrap(), 2000);
    assert_eq!(premium::rate_bps(3000, 2, 0, 3).unwrap(), 1000);
    assert_eq!(premium::rate_bps(3000, 3, 0, 3).unwrap(), 0);
    assert_eq!(
        premium::rate_bps(3000, 4, 0, 3),
        Err(DomainError::InvalidSchedule)
    );
    assert_eq!(
        premium::validate_units(Amount::new(1), 3001, 3000),
        Err(DomainError::InvalidAccounting)
    );
    let maximum = premium::max_weight(Amount::new(u64::MAX), 3000).unwrap();
    assert!(maximum > Weight::from(u128::MAX));
    assert!(
        PremiumSettlementProgress::new(PoolAlgorithm::H30, 3000, Amount::new(2), 2, 1, maximum)
            .is_err()
    );
    let mut progress = PremiumSettlementProgress::new(
        PoolAlgorithm::H30,
        3000,
        Amount::new(2),
        2,
        0,
        Weight::ZERO,
    )
    .unwrap();
    let before = progress;
    let mut computed = false;
    assert!(
        progress
            .process_position(Amount::new(2), 0, 0, &mut computed)
            .is_err()
    );
    assert_eq!(progress, before);
    assert!(!computed);
    progress
        .process_position(Amount::new(1), 3000, 0, &mut computed)
        .unwrap();
    assert!(computed);
    assert_eq!(
        progress.validate_complete(),
        Err(DomainError::IncompleteSettlement)
    );
    let mut second = false;
    progress
        .process_position(Amount::new(1), 0, 30, &mut second)
        .unwrap();
    progress.validate_complete().unwrap();
    let before = progress;
    assert_eq!(
        progress.process_position(Amount::new(1), 0, 0, &mut computed),
        Err(DomainError::PositionAlreadyComputed)
    );
    assert_eq!(before, progress);
}

#[test]
fn immediate_top_up_preserves_old_rights_and_clears_zero_principal_stale_window() {
    let pending = AllowanceSnapshot::new(Amount::new(100), Amount::new(40), 999).unwrap();
    let next = pending.quote_committed_top_up(Amount::new(50)).unwrap();
    assert_eq!(next.principal().get(), 150);
    assert_eq!(next.withdrawable_principal().get(), 40);
    assert_eq!(next.withdraw_until(), 999);
    let empty = AllowanceSnapshot::new(Amount::ZERO, Amount::ZERO, 999)
        .unwrap()
        .quote_committed_top_up(Amount::new(1))
        .unwrap();
    assert_eq!(empty.withdrawable_principal(), Amount::ZERO);
    assert_eq!(empty.withdraw_until(), 0);
    let fresh = empty.quote_top_up(Amount::new(2), 100).unwrap();
    assert_eq!(fresh.withdrawable_principal().get(), 2);
    assert_eq!(fresh.withdraw_until(), 100 + 172800);
}

#[test]
fn retained_pending_and_locked_units_each_have_independent_capacity() {
    let valid = |units| {
        premium::validate_allowance(
            Amount::new(150),
            Amount::new(100),
            2 * 86400,
            units,
            3000,
            0,
            6 * 86400,
        )
    };
    assert_eq!(valid(350000), Ok(()));
    assert_eq!(valid(199999), Err(DomainError::InvalidAccounting));
    assert_eq!(valid(350001), Err(DomainError::InvalidAccounting));
    assert_eq!(
        premium::validate_allowance(Amount::new(1), Amount::ZERO, 0, 3000, 3000, 100, 200),
        Ok(())
    );
}

#[test]
fn direct_weight_premium_can_change_financial_sign_without_changing_accuracy() {
    let p = Amount::new(100);
    let early = premium::weighted_principal(PoolAlgorithm::H30, p, 300000, 3000, 1).unwrap();
    let late = premium::weighted_principal(PoolAlgorithm::H30, p, 0, 3000, 0).unwrap();
    assert!(early > late);
    let total = early + late;
    assert!(
        premium::quote_gross(
            PoolAlgorithm::H30,
            Amount::new(200),
            p,
            300000,
            3000,
            1,
            total
        )
        .unwrap()
            > p
    );
    assert!(
        premium::quote_gross(PoolAlgorithm::H30, Amount::new(200), p, 0, 3000, 0, total).unwrap()
            < p
    );
    let boosted = premium::weighted_principal(PoolAlgorithm::H30, p, 300000, 3000, 0).unwrap();
    assert_eq!(
        premium::quote_gross(
            PoolAlgorithm::H30,
            Amount::new(200),
            p,
            300000,
            3000,
            0,
            boosted * Weight::from(2_u8)
        )
        .unwrap(),
        p
    );
}
