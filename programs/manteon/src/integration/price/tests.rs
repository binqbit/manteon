use super::*;
use crate::state::{CrossingDirection, PriceProtocol};
use solana_sha256_hasher::hash;

struct Owned {
    key: Pubkey,
    owner: Pubkey,
    data: Vec<u8>,
}

impl Owned {
    fn new(key: Pubkey, owner: Pubkey, name: &str, size: usize) -> Self {
        let mut data = vec![0; size];
        if !name.is_empty() {
            data[..8].copy_from_slice(&hash(format!("account:{name}").as_bytes()).to_bytes()[..8]);
        }
        Self { key, owner, data }
    }
    fn put(&mut self, offset: usize, bytes: &[u8]) {
        self.data[offset..offset + bytes.len()].copy_from_slice(bytes);
    }
    fn view(&self) -> PriceAccount<'_> {
        PriceAccount {
            key: self.key,
            owner: self.owner,
            data: &self.data,
        }
    }
}

fn key(byte: u8) -> Pubkey {
    Pubkey::new_from_array([byte; 32])
}

fn fixture(quote: Pubkey) -> (PriceThreshold, Vec<Owned>) {
    let pool_id = key(1);
    let token_mint = key(2);
    let observation =
        Pubkey::find_program_address(&[b"observation", pool_id.as_ref()], &RAYDIUM_PROGRAM).0;
    let (authority, bump) =
        Pubkey::find_program_address(&[b"vault_and_lp_mint_auth_seed"], &RAYDIUM_PROGRAM);
    let mut pool = Owned::new(pool_id, RAYDIUM_PROGRAM, "PoolState", 637);
    for (offset, address) in [
        (72, key(3)),
        (104, key(4)),
        (168, token_mint),
        (200, quote),
        (232, anchor_spl::token::ID),
        (264, anchor_spl::token::ID),
        (296, observation),
    ] {
        pool.put(offset, address.as_ref());
    }
    pool.data[328] = bump;
    pool.data[331] = 9;
    pool.data[332] = if quote == USDC { 6 } else { 9 };
    let mut history = Owned::new(observation, RAYDIUM_PROGRAM, "ObservationState", 4075);
    history.data[8] = 1;
    history.put(11, pool_id.as_ref());
    history.put(43, &10_000_u64.to_le_bytes());
    history.put(4043, &10_000_u64.to_le_bytes());
    let mut base = Owned::new(key(3), anchor_spl::token::ID, "", 165);
    base.put(0, token_mint.as_ref());
    base.put(32, authority.as_ref());
    base.data[108] = 1;
    base.put(64, &400_000_000_000_000_u64.to_le_bytes());
    let mut counter = Owned::new(key(4), anchor_spl::token::ID, "", 165);
    counter.put(0, quote.as_ref());
    counter.put(32, authority.as_ref());
    counter.data[108] = 1;
    counter.put(64, &200_000_000_000_u64.to_le_bytes());
    let rule = PriceThreshold {
        protocol: PriceProtocol::RaydiumCpmm,
        pool_id,
        token_mint,
        direction: CrossingDirection::Up,
        target_price_usd_e9: USD_SCALE,
        usd_quote_source: UsdQuoteSource::Usdc,
        twap_window_seconds: 3600,
        max_observation_gap_seconds: 14_400,
        minimum_quote_liquidity_usd_e9: 100_000 * USD_SCALE,
    };
    (rule, vec![pool, history, base, counter])
}

fn read(rule: &PriceThreshold, accounts: &[Owned], now: i64) -> Result<PriceSample> {
    sample(
        rule,
        &accounts.iter().map(Owned::view).collect::<Vec<_>>(),
        now,
        50_000,
    )
}

#[test]
fn authenticated_cumulative_endpoint_is_used_even_when_ring_marker_differs() {
    let (rule, mut accounts) = fixture(USDC);
    let first = read(&rule, &accounts, 10_000).unwrap();
    assert!(rule
        .direction
        .opposite(first.spot_usd, rule.target_price_usd_e9));
    let baseline = first.baseline();
    // The ring marker remains old; the current head's cumulative endpoint is fresh.
    accounts[1].put(4043, &13_600_u64.to_le_bytes());
    let raw_two_dollars = (2_u128 << 32) / 1000;
    accounts[1].put(51, &(raw_two_dollars * 3600).to_le_bytes());
    accounts[2].put(64, &100_000_000_000_000_u64.to_le_bytes());
    let current = read(&rule, &accounts, 13_600).unwrap();
    let price = current.interval(&rule, &baseline, 13_600).unwrap();
    assert!(price.lower <= 2 * USD_SCALE && price.upper >= 2 * USD_SCALE);
    assert!(rule.direction.crossed(price, rule.target_price_usd_e9));
    assert!(current.interval(&rule, &baseline, 25_000).is_err());
    // A current reserve price must never be extrapolated into a stale past interval.
    assert!(read(&rule, &accounts, 13_721).is_err());
}

#[test]
fn owner_pool_observation_vault_mint_authority_and_layout_substitution_fail() {
    let (rule, mut accounts) = fixture(USDC);
    accounts[0].owner = key(99);
    assert!(read(&rule, &accounts, 10_000).is_err());
    accounts[0].owner = RAYDIUM_PROGRAM;
    accounts[1].put(11, key(99).as_ref());
    assert!(read(&rule, &accounts, 10_000).is_err());
    accounts[1].put(11, rule.pool_id.as_ref());
    accounts[2].put(0, key(99).as_ref());
    assert!(read(&rule, &accounts, 10_000).is_err());
    accounts[2].put(0, rule.token_mint.as_ref());
    accounts[3].put(32, key(99).as_ref());
    assert!(read(&rule, &accounts, 10_000).is_err());
    let (rule, mut accounts) = fixture(USDC);
    accounts[1].put(9, &100_u16.to_le_bytes());
    assert!(read(&rule, &accounts, 10_000).is_err());
    accounts[1].put(9, &0_u16.to_le_bytes());
    accounts[0].data.pop();
    assert!(read(&rule, &accounts, 10_000).is_err());
}

#[test]
fn fees_are_excluded_and_liquidity_frozen_tokens_future_data_fail_closed() {
    let (rule, mut accounts) = fixture(USDC);
    accounts[0].put(349, &100_000_000_001_u64.to_le_bytes());
    assert!(read(&rule, &accounts, 10_000).is_err());
    accounts[0].put(349, &0_u64.to_le_bytes());
    accounts[3].data[108] = 2;
    assert!(read(&rule, &accounts, 10_000).is_err());
    accounts[3].data[108] = 1;
    accounts[1].put(4043, &10_001_u64.to_le_bytes());
    assert!(read(&rule, &accounts, 10_000).is_err());
}

#[test]
fn spot_price_and_liquidity_use_reserves_net_of_all_fee_types() {
    let (rule, mut accounts) = fixture(USDC);
    // Gross reserves imply $0.50; spendable reserves imply $1 and $100,000 liquidity.
    for offset in [341, 357, 397] {
        accounts[0].put(offset, &100_000_000_000_000_u64.to_le_bytes());
    }
    for (offset, fees) in [
        (349, 40_000_000_000_u64),
        (365, 30_000_000_000),
        (405, 30_000_000_000),
    ] {
        accounts[0].put(offset, &fees.to_le_bytes());
    }
    let sample = read(&rule, &accounts, 10_000).unwrap();
    assert_eq!(sample.main.quote_reserve, 100_000_000_000);
    assert!(sample.spot_usd.lower <= USD_SCALE && sample.spot_usd.upper >= USD_SCALE);
    assert!(sample.spot_usd.upper - sample.spot_usd.lower < 1000);

    // A single additional fee atom pushes liquidity below the committed minimum.
    accounts[0].put(405, &30_000_000_001_u64.to_le_bytes());
    assert!(read(&rule, &accounts, 10_000).is_err());
    accounts[0].put(341, &u64::MAX.to_le_bytes());
    assert!(read(&rule, &accounts, 10_000).is_err());
}

fn oracle_fixture() -> (PriceThreshold, Vec<Owned>) {
    let (mut rule, mut accounts) = fixture(WRAPPED_SOL);
    let oracle_key = key(5);
    let receiver = pubkey!("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ");
    rule.usd_quote_source = UsdQuoteSource::PythSolUsd {
        price_update_account: oracle_key,
        receiver_program: receiver,
        feed_id: SOL_USD_FEED_ID,
        max_age_seconds: 60,
        max_confidence_bps: 100,
    };
    accounts[3].put(64, &2_000_000_000_000_000_u64.to_le_bytes());
    let mut oracle = Owned::new(oracle_key, receiver, "PriceUpdateV2", 134);
    oracle.data[40] = 1;
    oracle.put(41, &SOL_USD_FEED_ID);
    oracle.put(73, &15_000_000_000_i64.to_le_bytes());
    oracle.put(81, &10_000_000_u64.to_le_bytes());
    oracle.put(89, &(-8_i32).to_le_bytes());
    oracle.put(93, &10_000_i64.to_le_bytes());
    oracle.put(101, &9_999_i64.to_le_bytes());
    oracle.put(125, &49_000_u64.to_le_bytes());
    accounts.push(oracle);
    (rule, accounts)
}

#[test]
fn sol_oracle_requires_fixed_feed_full_verification_recent_past_time_and_small_confidence() {
    let (rule, mut accounts) = oracle_fixture();
    assert!(read(&rule, &accounts, 10_000).is_ok());
    accounts[4].data[40] = 0;
    assert!(read(&rule, &accounts, 10_000).is_err());
    accounts[4].data[40] = 1;
    accounts[4].data[41] ^= 1;
    assert!(read(&rule, &accounts, 10_000).is_err());
    accounts[4].data[41] ^= 1;
    accounts[4].put(93, &9_939_i64.to_le_bytes());
    assert!(read(&rule, &accounts, 10_000).is_err());
    accounts[4].put(93, &10_001_i64.to_le_bytes());
    assert!(read(&rule, &accounts, 10_000).is_err());
    accounts[4].put(93, &10_000_i64.to_le_bytes());
    accounts[4].put(81, &151_000_000_u64.to_le_bytes());
    assert!(read(&rule, &accounts, 10_000).is_err());
}

#[test]
fn uncertain_bounds_cannot_trigger_either_direction() {
    let price = PriceBounds {
        lower: 99,
        upper: 101,
    };
    assert!(!CrossingDirection::Up.crossed(price, 100));
    assert!(!CrossingDirection::Down.crossed(price, 100));
    assert!(!CrossingDirection::Up.opposite(price, 100));
    assert!(!CrossingDirection::Down.opposite(price, 100));
    assert!(CrossingDirection::Up.crossed(
        PriceBounds {
            lower: 100,
            upper: 101
        },
        100
    ));
    assert!(CrossingDirection::Down.crossed(
        PriceBounds {
            lower: 99,
            upper: 100
        },
        100
    ));
}

#[test]
fn token_2022_vaults_and_reverse_pair_orientation_use_the_authenticated_counter() {
    let (rule, mut accounts) = fixture(USDC);
    accounts[0].put(232, anchor_spl::token_2022::ID.as_ref());
    accounts[2].owner = anchor_spl::token_2022::ID;
    assert!(read(&rule, &accounts, 10_000).is_ok());
    // Reverse all mint/program/vault/decimal/fee orientation together.
    for (left, right, len) in [
        (72, 104, 32),
        (168, 200, 32),
        (232, 264, 32),
        (331, 332, 1),
        (341, 349, 8),
        (357, 365, 8),
        (397, 405, 8),
    ] {
        let old_left = accounts[0].data[left..left + len].to_vec();
        let old_right = accounts[0].data[right..right + len].to_vec();
        accounts[0].put(left, &old_right);
        accounts[0].put(right, &old_left);
    }
    accounts[1].put(51, &999_u128.to_le_bytes());
    accounts[1].put(67, &123_u128.to_le_bytes());
    let sample = read(&rule, &accounts, 10_000).unwrap();
    assert_eq!(sample.main.cumulative, 123);
    assert!(sample.spot_usd.lower < USD_SCALE);
    accounts[2].owner = anchor_spl::token::ID;
    assert!(read(&rule, &accounts, 10_000).is_err());
}

#[test]
fn fixed_sol_usdc_reference_uses_its_own_interval_and_rejects_misaligned_endpoints() {
    let (mut rule, mut accounts) = fixture(WRAPPED_SOL);
    let (_, mut reference) = fixture(USDC);
    let reference_pool = key(6);
    let observation =
        Pubkey::find_program_address(&[b"observation", reference_pool.as_ref()], &RAYDIUM_PROGRAM)
            .0;
    reference[0].key = reference_pool;
    reference[0].put(72, key(7).as_ref());
    reference[0].put(104, key(8).as_ref());
    reference[0].put(168, WRAPPED_SOL.as_ref());
    reference[0].put(296, observation.as_ref());
    reference[1].key = observation;
    reference[1].put(11, reference_pool.as_ref());
    reference[2].key = key(7);
    reference[2].put(0, WRAPPED_SOL.as_ref());
    reference[2].put(64, &2_000_000_000_000_u64.to_le_bytes());
    reference[3].key = key(8);
    reference[3].put(64, &300_000_000_000_u64.to_le_bytes());
    accounts[3].put(64, &2_000_000_000_000_000_u64.to_le_bytes());
    rule.usd_quote_source = UsdQuoteSource::RaydiumSolUsdc {
        pool_id: reference_pool,
    };
    accounts.extend(reference);
    let baseline = read(&rule, &accounts, 10_000).unwrap().baseline();
    for index in [1, 5] {
        accounts[index].put(4043, &13_600_u64.to_le_bytes());
    }
    accounts[1].put(51, &((5_u128 << 32) * 3600).to_le_bytes());
    accounts[5].put(51, &(((150_u128 << 32) / 1000) * 3600).to_le_bytes());
    let sample = read(&rule, &accounts, 13_600).unwrap();
    let usd = sample.interval(&rule, &baseline, 13_600).unwrap();
    assert!(usd.lower <= 750 * USD_SCALE && usd.upper >= 750 * USD_SCALE);
    accounts[5].put(4043, &13_569_u64.to_le_bytes());
    assert!(read(&rule, &accounts, 13_600).is_err());
}
