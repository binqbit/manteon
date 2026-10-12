//! Independent dimensional and outward-rounding checks for threshold prices.

use manteon_core::math::price::{PriceBounds, USD_SCALE, oracle_usd, usd_twap};

#[test]
fn token_usdc_and_token_sol_convert_complete_tokens_without_decimal_bias() {
    let dollar = PriceBounds {
        lower: USD_SCALE,
        upper: USD_SCALE,
    };
    // One 9-decimal token costs two 6-decimal USDC: 0.002 quote atoms/base atom.
    // Bounds cover both integer conversion and CPMM's Q32.32 quantization.
    let ratio = (2_u128 * (1_u128 << 32)) / 1000;
    let price = usd_twap(ratio * 3600, 3600, 9, 6, dollar).unwrap();
    assert!(price.lower <= 2 * USD_SCALE && price.upper >= 2 * USD_SCALE);
    assert!(2 * USD_SCALE - price.lower < 250);
    let sol = oracle_usd(15_000_000_000, 10_000_000, -8).unwrap();
    let converted = usd_twap((1_u128 << 31) * 3600, 3600, 9, 9, sol).unwrap();
    assert_eq!(converted.lower, 74_950_000_000);
    assert!(converted.upper >= 75_050_000_000 && converted.upper <= 75_050_000_035);
}

#[test]
fn uncertainty_rounds_outward_and_invalid_or_overflowing_prices_fail_closed() {
    assert_eq!(
        oracle_usd(1001, 1, -12).unwrap(),
        PriceBounds { lower: 1, upper: 2 }
    );
    assert!(oracle_usd(-1, 0, -8).is_err());
    assert!(oracle_usd(10, 10, -8).is_err());
    assert!(oracle_usd(10, 0, 10).is_err());
    let dollar = PriceBounds {
        lower: USD_SCALE,
        upper: USD_SCALE,
    };
    assert!(usd_twap(u128::MAX, 1, 18, 0, dollar).is_err());
    assert!(usd_twap(1, 0, 9, 6, dollar).is_err());
    assert!(usd_twap(1, 1, 19, 6, dollar).is_err());
    let price = usd_twap(1, 3, 0, 0, dollar).unwrap();
    assert_eq!(price, PriceBounds { lower: 0, upper: 1 });
}
