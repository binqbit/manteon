use super::*;

#[test]
fn common_time_requires_only_the_facts_needed_by_the_definition() {
    let external = EventDefinition::ExternalEvent;
    let result = EventResult {
        occurred_at: 100,
        extra: None,
    };
    assert!(result.validate(&external, 100).is_ok());
    assert!(result.validate(&external, 99).is_err());
    assert!(EventResult {
        occurred_at: -1,
        ..result
    }
    .validate(&external, 100)
    .is_err());

    for direction in [CrossingDirection::Up, CrossingDirection::Down] {
        let price = EventDefinition::TokenPriceThresholdCrossing {
            params: PriceThreshold {
                protocol: PriceProtocol::RaydiumCpmm,
                pool_id: Pubkey::new_from_array([2; 32]),
                token_mint: Pubkey::new_from_array([1; 32]),
                direction,
                target_price_usd_e9: 1_000_000_000,
                usd_quote_source: UsdQuoteSource::Usdc,
                twap_window_seconds: 3_600,
                max_observation_gap_seconds: 14_400,
                minimum_quote_liquidity_usd_e9: 100_000_000_000_000,
            },
        };
        assert!(result.validate(&price, 100).is_err());
        let confirmed = EventResult {
            extra: Some(EventResultExtra::PriceVerification {
                lower_usd_e9: 1_000_000_000,
                upper_usd_e9: 1_000_000_000,
            }),
            ..result
        };
        assert!(confirmed.validate(&price, 100).is_ok());
        assert!(confirmed.validate(&external, 100).is_err());
        assert!(confirmed.validate(&price, 99).is_err());
        let straddles = EventResult {
            extra: Some(EventResultExtra::PriceVerification {
                lower_usd_e9: 999_999_999,
                upper_usd_e9: 1_000_000_001,
            }),
            ..result
        };
        assert!(straddles.validate(&price, 100).is_err());
    }
}
