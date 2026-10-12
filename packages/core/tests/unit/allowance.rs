use crate::support::TestContext;
use manteon_core::{
    accounting::allowance::{AllowanceSnapshot, quote_first_deposit},
    constants::WITHDRAWAL_WINDOW_SECONDS,
    error::DomainError,
    types::Amount,
};

#[test]
fn mat_024_first_deposit_quotes_exact_principal_allowance_and_48_hours() {
    let context = TestContext::new(1_000, [0; 3]);
    let quote = quote_first_deposit(Amount::new(100), context.now()).unwrap();

    assert_eq!(quote.principal(), Amount::new(100));
    assert_eq!(quote.withdrawable_principal(), Amount::new(100));
    assert_eq!(quote.withdraw_until(), 173_800);
    assert_eq!(WITHDRAWAL_WINDOW_SECONDS, 172_800);
}

#[test]
fn mat_027_expired_top_up_unlocks_only_new_money_and_live_top_up_keeps_deadline() {
    let mut context = TestContext::new(1_000, [0; 3]);
    let original = quote_first_deposit(Amount::new(100), context.now()).unwrap();

    context.set_time(original.withdraw_until() + 1);
    let fresh = original
        .quote_top_up(Amount::new(20), context.now())
        .unwrap();

    assert_eq!(fresh.principal(), Amount::new(120));
    assert_eq!(fresh.withdrawable_principal(), Amount::new(20));
    assert_eq!(fresh.withdraw_until(), context.now() + 172_800);

    context.set_time(context.now() + 1);
    let topped_up = fresh.quote_top_up(Amount::new(5), context.now()).unwrap();

    assert_eq!(topped_up.principal(), Amount::new(125));
    assert_eq!(topped_up.withdrawable_principal(), Amount::new(25));
    assert_eq!(topped_up.withdraw_until(), fresh.withdraw_until());
    assert_eq!(original.principal(), Amount::new(100));
    assert_eq!(original.withdrawable_principal(), Amount::new(100));
}

#[test]
fn mat_027_top_up_in_first_window_never_extends_deadline() {
    let original = quote_first_deposit(Amount::new(100), 1_000).unwrap();

    for now in [1_001, original.withdraw_until() - 1] {
        let quote = original.quote_top_up(Amount::new(20), now).unwrap();

        assert_eq!(quote.principal(), Amount::new(120));
        assert_eq!(quote.withdrawable_principal(), Amount::new(120));
        assert_eq!(quote.withdraw_until(), original.withdraw_until());
    }
}

#[test]
fn mat_028_partial_withdrawal_reduces_only_remaining_principal_and_allowance() {
    let original = AllowanceSnapshot::new(Amount::new(125), Amount::new(25), 10_000).unwrap();
    let quote = original.quote_withdrawal(Amount::new(7), 9_999).unwrap();

    assert_eq!(quote.principal(), Amount::new(118));
    assert_eq!(quote.withdrawable_principal(), Amount::new(18));
    assert_eq!(quote.withdraw_until(), original.withdraw_until());
    assert_eq!(original.principal(), Amount::new(125));
    assert_eq!(original.withdrawable_principal(), Amount::new(25));
}

#[test]
fn mat_028_mat_029_expired_or_excess_allowance_never_pays_old_locked_money() {
    let original = AllowanceSnapshot::new(Amount::new(125), Amount::new(25), 10_000).unwrap();

    assert_eq!(
        original.quote_withdrawal(Amount::new(26), 9_999),
        Err(DomainError::InsufficientAllowance)
    );
    assert_eq!(
        original.quote_withdrawal(Amount::new(1), 10_001),
        Err(DomainError::AllowanceExpired)
    );

    let exhausted = original.quote_withdrawal(Amount::new(25), 9_999).unwrap();

    assert_eq!(exhausted.principal(), Amount::new(100));
    assert_eq!(exhausted.withdrawable_principal(), Amount::new(0));
    assert_eq!(
        exhausted.quote_withdrawal(Amount::new(1), 9_999),
        Err(DomainError::InsufficientAllowance)
    );
}

#[test]
fn mat_029_deadline_excludes_withdrawal_and_top_up_unlocks_only_new_money() {
    let original = AllowanceSnapshot::new(Amount::new(100), Amount::new(100), 10_000).unwrap();
    let renewed = original.quote_top_up(Amount::new(1), 10_000).unwrap();

    assert_eq!(renewed.principal(), Amount::new(101));
    assert_eq!(renewed.withdrawable_principal(), Amount::new(1));
    assert_eq!(renewed.withdraw_until(), 182_800);
    assert_eq!(
        original.quote_withdrawal(Amount::new(1), 10_000),
        Err(DomainError::AllowanceExpired)
    );
}

#[test]
fn mat_028_mat_032_empty_active_stake_gets_a_fresh_window() {
    let original = AllowanceSnapshot::new(Amount::new(25), Amount::new(25), 10_000).unwrap();
    let zero = original.quote_withdrawal(Amount::new(25), 9_999).unwrap();

    for now in [9_999, 10_000, 10_001] {
        let reopened = zero.quote_top_up(Amount::new(1), now).unwrap();

        assert_eq!(reopened, quote_first_deposit(Amount::new(1), now).unwrap());
    }
}

#[test]
fn mat_026_invalid_allowance_and_zero_actions_are_rejected() {
    assert_eq!(
        AllowanceSnapshot::new(Amount::new(10), Amount::new(11), 10_000),
        Err(DomainError::InvalidAccounting)
    );
    assert_eq!(
        quote_first_deposit(Amount::new(0), 1_000),
        Err(DomainError::ZeroAmount)
    );

    let original = quote_first_deposit(Amount::new(10), 1_000).unwrap();

    assert_eq!(
        original.quote_top_up(Amount::new(0), 1_001),
        Err(DomainError::ZeroAmount)
    );
    assert_eq!(
        original.quote_withdrawal(Amount::new(0), 1_001),
        Err(DomainError::ZeroAmount)
    );
}

#[test]
fn mat_026_top_up_and_deadline_overflow_leave_input_values_unchanged() {
    let original = AllowanceSnapshot::new(Amount::new(u64::MAX), Amount::new(10), 0).unwrap();

    for now in [-1, 1] {
        assert_eq!(
            original.quote_top_up(Amount::new(1), now),
            Err(DomainError::ArithmeticOverflow)
        );
    }

    assert_eq!(original.principal(), Amount::new(u64::MAX));
    assert_eq!(original.withdrawable_principal(), Amount::new(10));

    let near_end = i64::MAX - 172_800;
    let valid = quote_first_deposit(Amount::new(1), near_end).unwrap();

    assert_eq!(valid.withdraw_until(), i64::MAX);
    assert_eq!(
        quote_first_deposit(Amount::new(1), near_end + 1),
        Err(DomainError::ArithmeticOverflow)
    );

    let expired = AllowanceSnapshot::new(Amount::new(1), Amount::new(1), 0).unwrap();

    assert_eq!(
        expired.quote_top_up(Amount::new(1), near_end + 1),
        Err(DomainError::ArithmeticOverflow)
    );
    assert_eq!(expired.principal(), Amount::new(1));
    assert_eq!(expired.withdraw_until(), 0);
}

#[test]
fn mat_026_mat_029_signed_extreme_deadlines_keep_the_same_personal_rules() {
    let start = quote_first_deposit(Amount::new(1), i64::MIN).unwrap();
    assert_eq!(start.withdraw_until(), i64::MIN + 172_800);
    let end = AllowanceSnapshot::new(Amount::new(1), Amount::new(1), i64::MAX).unwrap();
    assert!(end.quote_withdrawal(Amount::new(1), i64::MAX - 1).is_ok());
    assert_eq!(
        end.quote_withdrawal(Amount::new(1), i64::MAX),
        Err(DomainError::AllowanceExpired)
    );
}

#[test]
fn mat_027_mat_028_exhaustive_small_allowances_preserve_locked_principal() {
    for principal in 1..=20 {
        for allowance in 0..=principal {
            let original =
                AllowanceSnapshot::new(Amount::new(principal), Amount::new(allowance), 100)
                    .unwrap();
            for amount in 1..=5 {
                let live = original.quote_top_up(Amount::new(amount), 99).unwrap();
                assert_eq!(
                    live.principal().get() - live.withdrawable_principal().get(),
                    principal - allowance
                );
                let expired = original.quote_top_up(Amount::new(amount), 101).unwrap();
                assert_eq!(
                    expired.principal().get() - expired.withdrawable_principal().get(),
                    principal
                );
                if amount <= allowance {
                    let withdrawal = original.quote_withdrawal(Amount::new(amount), 99).unwrap();
                    assert_eq!(
                        withdrawal.principal().get() - withdrawal.withdrawable_principal().get(),
                        principal - allowance
                    );
                } else {
                    assert_eq!(
                        original.quote_withdrawal(Amount::new(amount), 99),
                        Err(DomainError::InsufficientAllowance)
                    );
                }
            }
        }
    }
}
