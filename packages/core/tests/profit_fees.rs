//! Profit-only fee limits, split rounding and atom conservation.
use manteon_core::{
    DomainError,
    math::fees::{MAX_PROFIT_FEE_BPS, quote_profit_fees},
    types::Amount,
};

#[test]
fn profit_only_fees_split_without_an_additional_platform_charge() {
    // 100 invested, 150 gross: 20% of 50 = 10, platform gets 20% of 10.
    let fees = quote_profit_fees(Amount::new(100), Amount::new(150), 2_000, 2_000).unwrap();
    assert_eq!(
        (
            fees.net.get(),
            fees.total.get(),
            fees.platform.get(),
            fees.recipient.get()
        ),
        (140, 10, 2, 8)
    );
    for gross in [0, 1, 99, 100] {
        let fees = quote_profit_fees(Amount::new(100), Amount::new(gross), 2_000, 2_000).unwrap();
        assert_eq!(fees.net.get(), gross);
        assert_eq!(fees.total, Amount::ZERO);
    }
}

#[test]
fn fee_rounding_and_extreme_balances_conserve_the_gross_entitlement() {
    for principal in [0, 1, 100, u64::MAX / 2, u64::MAX] {
        for gross in [0, 1, 104, 105, 149, u64::MAX] {
            for rate in [0, 1, 1_000, MAX_PROFIT_FEE_BPS] {
                for share in [0, 1, 2_000, 10_000] {
                    let fees =
                        quote_profit_fees(Amount::new(principal), Amount::new(gross), rate, share)
                            .unwrap();
                    let profit = gross.saturating_sub(principal);
                    let total = u128::from(profit) * u128::from(rate) / 10_000;
                    let platform = total * u128::from(share) / 10_000;
                    assert_eq!(u128::from(fees.total.get()), total);
                    assert_eq!(u128::from(fees.platform.get()), platform);
                    assert_eq!(
                        fees.net.get() + fees.recipient.get() + fees.platform.get(),
                        gross
                    );
                    assert!(fees.net.get() >= gross.min(principal));
                }
            }
        }
    }
}

#[test]
fn invalid_rates_are_rejected_even_when_the_payment_has_no_profit() {
    for (rate, share) in [(2_001, 2_000), (u16::MAX, 0), (0, 10_001)] {
        assert_eq!(
            quote_profit_fees(Amount::new(100), Amount::new(0), rate, share),
            Err(DomainError::InvalidFeeRates)
        );
    }
}
