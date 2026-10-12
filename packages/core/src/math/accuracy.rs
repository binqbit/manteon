//! Selectable accuracy weights with coefficient-first flooring in Q64.64.
//!
//! These calculations do not authorize dates or settlement. A coefficient can
//! round to zero; positive-principal settlement still requires a valid nonzero
//! denominator and a separately selected numerical domain.

use crate::types::{Amount, PoolAlgorithm};

const FRACTIONAL_BITS: u32 = 64;

/// The Q64.64 encoding of an exact coefficient of one.
pub const SCALE: u128 = 1_u128 << FRACTIONAL_BITS;

// floor(2^64 * 2^(-r/30)) for each remainder r. The independent reference test
// certifies the exact floor with integer powers, not floating-point tolerance.
const H30_COEFFICIENTS: [u128; 30] = [
    SCALE,
    18_025_419_856_780_639_094,
    17_613_718_698_265_799_538,
    17_211_420_807_207_096_429,
    16_818_311_412_679_010_380,
    16_434_180_649_130_740_277,
    16_058_823_444_347_289_866,
    15_692_039_409_969_526_668,
    15_333_632_734_514_766_198,
    14_983_412_078_840_769_423,
    14_641_190_473_997_345_813,
    14_306_785_221_411_029_005,
    13_980_017_795_349_537_628,
    13_660_713_747_613_950_915,
    13_348_702_614_407_718_039,
    13_043_817_825_332_782_212,
    12_745_896_614_464_236_183,
    12_454_779_933_456_035_413,
    12_170_312_366_631_379_499,
    11_892_342_048_012_431_974,
    11_620_720_580_245_083_921,
    11_355_302_955_375_478_541,
    11_095_947_477_436_002_415,
    10_842_515_686_799_415_150,
    10_594_872_286_260_733_109,
    10_352_885_068_807_405_237,
    10_116_424_847_039_220_363,
    9_885_365_384_200_266_033,
    9_659_583_326_786_119_593,
    9_438_958_138_690_293_151,
];

const H7_COEFFICIENTS: [u128; 7] = [
    18_446_744_073_709_551_616,
    16_707_652_636_178_719_991,
    15_132_516_366_888_310_227,
    13_705_878_173_351_620_644,
    12_413_738_201_122_729_700,
    11_243_416_450_733_715_988,
    10_183_428_346_603_629_346,
];

/// Return exactly floor(2^64 * 2^(-error_days / half_life_days)).
///
/// The last nonzero error is 64 half-lives: 1920, 960 or 448 UTC days.
/// The protocol separately admits forecasts and competitive results in that domain.
pub fn coefficient(algorithm: PoolAlgorithm, error_days: u64) -> u128 {
    let half_lives = error_days / algorithm.half_life_days();
    if half_lives > u64::from(FRACTIONAL_BITS) {
        return 0;
    }
    let remainder = (error_days % algorithm.half_life_days()) as usize;
    let base = match algorithm {
        PoolAlgorithm::H30 => H30_COEFFICIENTS[remainder],
        // 2^(-r/15) equals 2^(-2r/30), including the exact integer floor.
        PoolAlgorithm::H15 => H30_COEFFICIENTS[2 * remainder],
        PoolAlgorithm::H7 => H7_COEFFICIENTS[remainder],
    };
    // floor(floor(x) / 2^q) = floor(x / 2^q): shifting introduces no extra error.
    base >> half_lives
}

/// Multiply principal by the floored coefficient, retaining all 64 fraction bits.
///
/// This preserves same-date additivity: splitting principal cannot change its
/// total weight. It differs from flooring only after multiplying the ideal accuracy
/// coefficient by principal. No token-unit rounding or payout occurs here.
pub fn weighted_principal(algorithm: PoolAlgorithm, principal: Amount, error_days: u64) -> u128 {
    // principal <= 2^64 - 1 and coefficient <= 2^64, so the product fits u128.
    u128::from(principal.get()) * coefficient(algorithm, error_days)
}
