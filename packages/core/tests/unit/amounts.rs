use manteon_core::{DomainError, math::checked_amounts::backing_excess, types::Amount};

#[test]
fn mat_026_amounts_do_not_wrap_at_balance_limits() {
    assert_eq!(
        Amount::new(u64::MAX).checked_add(Amount::new(1)),
        Err(DomainError::ArithmeticOverflow)
    );
    assert_eq!(
        Amount::ZERO.checked_sub(Amount::new(1)),
        Err(DomainError::ArithmeticUnderflow)
    );
}

#[test]
fn mat_092_donation_is_excess_not_new_principal() {
    let liability = Amount::new(1_000);
    assert_eq!(
        backing_excess(Amount::new(1_020), liability),
        Ok(Amount::new(20))
    );
    assert_eq!(backing_excess(liability, liability), Ok(Amount::ZERO));
    assert_eq!(
        backing_excess(Amount::new(999), liability),
        Err(DomainError::InvalidAccounting)
    );
    assert_eq!(liability.get(), 1_000);
}
