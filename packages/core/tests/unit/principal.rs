use crate::support::TestContext;
use manteon_core::{accounting::principal::PrincipalBook, error::DomainError, types::Amount};

#[test]
fn mat_024_mat_032_first_positive_date_increases_count_once() {
    let context = TestContext::new(1_000, [0; 3]);
    let empty = context.book(0);
    let first_owner = empty.quote_add(Amount::new(100)).unwrap();

    TestContext::assert_book(first_owner, 0, [100, 0, 0]);

    let second_owner = first_owner.quote_add(Amount::new(20)).unwrap();

    TestContext::assert_book(second_owner, 0, [120, 0, 0]);
    TestContext::assert_book(empty, 0, [0; 3]);
}

#[test]
fn mat_032_mat_036_one_owner_removal_keeps_other_owners_date_active() {
    let context = TestContext::new(1_000, [120, 30, 0]);
    let initial = context.book(0);
    let after_first_owner = initial.quote_remove(Amount::new(100)).unwrap();

    TestContext::assert_book(after_first_owner, 0, [20, 30, 0]);

    let after_second_owner = after_first_owner.quote_remove(Amount::new(20)).unwrap();

    TestContext::assert_book(after_second_owner, 0, [0, 30, 0]);
    // Values only: no selected owner eligibility or full-zero lifecycle transition.
    TestContext::assert_book(initial, 0, [120, 30, 0]);
}

#[test]
fn mat_032_mat_092_distinct_zero_crossings_preserve_exact_pool_sum() {
    let context = TestContext::new(1_000, [100, 0, 30]);
    let existing = context.book(0).quote_add(Amount::new(20)).unwrap();

    TestContext::assert_book(existing, 0, [120, 0, 30]);

    let new_date = context.book(1).quote_add(Amount::new(20)).unwrap();

    TestContext::assert_book(new_date, 1, [100, 20, 30]);

    let removed_date = context.book(2).quote_remove(Amount::new(30)).unwrap();

    TestContext::assert_book(removed_date, 2, [100, 0, 0]);

    let all_removed = PrincipalBook::new(Amount::new(30), Amount::new(30), 1)
        .unwrap()
        .quote_remove(Amount::new(30))
        .unwrap();

    TestContext::assert_book(all_removed, 0, [0; 3]);
}

#[test]
fn mat_026_mat_092_inconsistent_local_aggregates_reject() {
    for (pool, position, active) in [
        (0, 1, 1),
        (10, 11, 1),
        (10, 10, 0),
        (0, 0, 1),
        (10, 0, 0),
        (10, 5, 1),
        (10, 10, 2),
        (2, 0, 3),
        (2, 1, 3),
    ] {
        assert_eq!(
            PrincipalBook::new(Amount::new(pool), Amount::new(position), active),
            Err(DomainError::InvalidAccounting),
            "pool={pool}, position={position}, active={active}"
        );
    }
}

#[test]
fn mat_026_arithmetic_failures_and_zero_actions_do_not_change_the_input() {
    let original = PrincipalBook::new(Amount::new(u64::MAX), Amount::new(10), 2).unwrap();

    assert_eq!(
        original.quote_add(Amount::new(1)),
        Err(DomainError::ArithmeticOverflow)
    );
    assert_eq!(
        original.quote_remove(Amount::new(11)),
        Err(DomainError::ArithmeticUnderflow)
    );
    assert_eq!(
        original.quote_add(Amount::new(0)),
        Err(DomainError::ZeroAmount)
    );
    assert_eq!(
        original.quote_remove(Amount::new(0)),
        Err(DomainError::ZeroAmount)
    );
    assert_eq!(original.pool_principal(), Amount::new(u64::MAX));
    assert_eq!(original.position_principal(), Amount::new(10));
    assert_eq!(original.active_dates(), 2);

    let largest_count =
        PrincipalBook::new(Amount::new(u64::MAX), Amount::new(1), u64::MAX).unwrap();
    let removed = largest_count.quote_remove(Amount::new(1)).unwrap();

    assert_eq!(removed.pool_principal(), Amount::new(u64::MAX - 1));
    assert_eq!(removed.position_principal(), Amount::new(0));
    assert_eq!(removed.active_dates(), u64::MAX - 1);
}

#[test]
fn mat_032_mat_092_exhaustive_small_books_match_independent_position_sums() {
    for first in 0..=4 {
        for second in 0..=4 {
            for third in 0..=4 {
                let positions = [first, second, third];
                let context = TestContext::new(1_000, positions);
                for position in 0..3 {
                    let original = context.book(position);
                    for amount in 1..=5 {
                        let mut expected = positions;
                        expected[position] += amount;
                        let added = original.quote_add(Amount::new(amount)).unwrap();

                        TestContext::assert_book(added, position, expected);
                        assert_eq!(added.quote_remove(Amount::new(amount)), Ok(original));

                        if amount <= positions[position] {
                            expected = positions;
                            expected[position] -= amount;
                            let removed = original.quote_remove(Amount::new(amount)).unwrap();

                            TestContext::assert_book(removed, position, expected);
                        } else {
                            assert_eq!(
                                original.quote_remove(Amount::new(amount)),
                                Err(DomainError::ArithmeticUnderflow)
                            );
                        }
                    }
                }
            }
        }
    }
}
