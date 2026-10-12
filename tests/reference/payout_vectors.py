"""Exact integer reference for floored gross allocation, without claim authority.

Columns: pool_principal, stake_principal, error_days, total_weight, gross_atoms.
Synthetic denominators satisfy local bounds; they do not prove membership of an
actual frozen book. Python's unbounded integers independently check wide Rust
multiplication and division.
"""

from random import Random

from accuracy_q64_vectors import MAX_PRINCIPAL, vector

SCALE = 1 << 64


def emit(half_life: int, pool: int, stake: int, error: int, denominator: int, weight: int) -> None:
    assert 0 <= stake <= pool <= MAX_PRINCIPAL
    assert 0 < denominator
    assert weight <= denominator <= weight + (pool - stake) * SCALE
    quotient, remainder = divmod(pool * weight, denominator)
    assert 0 <= quotient <= pool
    assert 0 <= remainder < denominator
    print(half_life, pool, stake, error, denominator, quotient, sep="\t")


def run(half_life: int) -> None:
    pools = (1, 2, 3, (1 << 32) - 1, 1 << 32, (1 << 63) - 1, 1 << 63, MAX_PRINCIPAL)
    errors = (0, 1, half_life - 1, half_life, half_life + 1, 64 * half_life - 1, 64 * half_life, 64 * half_life + 1, half_life * (1 << 32), (1 << 64) - 1)
    coefficients = {error: vector(error, 1, half_life)[2] for error in errors}
    for pool in pools:
        for stake in sorted({0, 1, pool // 2, pool - 1, pool}):
            for error in errors:
                weight = stake * coefficients[error]
                upper = weight + (pool - stake) * SCALE
                lower = max(weight, 1)
                for denominator in sorted({lower, lower + 1, (lower + upper) // 2, upper}):
                    if lower <= denominator <= upper:
                        emit(half_life, pool, stake, error, denominator, weight)

    random = Random(0x4D414E54454F4E)
    for _ in range(256):
        pool = random.randrange(1, MAX_PRINCIPAL + 1)
        stake = random.randrange(pool + 1)
        error = random.randrange(65 * half_life + 1)
        weight = stake * vector(error, 1, half_life)[2]
        upper = weight + (pool - stake) * SCALE
        if upper:
            denominator = random.randrange(max(weight, 1), upper + 1)
            emit(half_life, pool, stake, error, denominator, weight)


if __name__ == "__main__":
    for half_life in (30, 15, 7):
        run(half_life)
