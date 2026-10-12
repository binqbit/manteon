"""Independent, integer-certified Decimal vectors for all coefficient-first accuracy curves.

Output columns: half_life_days, error_days, principal, coefficient_floor, ideal_weighted_floor.
The last column floors the ideal product, not the production coefficient-first
product; comparing both exposes the quantization error rather than hiding it.
"""

from decimal import Decimal, localcontext
from functools import cache

MAX_PRINCIPAL = (1 << 64) - 1


def certify_floor(value: int, numerator: int, denominator: int, half_life: int) -> None:
    """Prove value <= (numerator/denominator)**(1/half_life) < value + 1."""
    assert value**half_life * denominator <= numerator < (value + 1) ** half_life * denominator


@cache
def scaled_accuracy(error_days: int, half_life: int) -> Decimal:
    """Reuse the principal-independent exponent across independently certified stakes."""
    with localcontext() as context:
        context.prec = 100
        return Decimal(2) ** (Decimal(64) - Decimal(error_days) / Decimal(half_life))


def vector(error_days: int, principal: int, half_life: int) -> tuple[int, int, int, int]:
    assert half_life in (30, 15, 7)
    assert 0 <= error_days <= (1 << 64) - 1
    assert 0 <= principal <= MAX_PRINCIPAL
    if error_days >= 128 * half_life:
        # principal < 2**64, hence principal * 2**(64-d/H) < 1.
        # This numerical fact avoids exponent limits for full-width day errors;
        # it does not choose a market date limit or a settlement zero policy.
        return error_days, principal, 0, 0

    with localcontext() as context:
        context.prec = 100
        scaled = scaled_accuracy(error_days, half_life)
        coefficient = int(scaled)
        ideal_weighted = int(Decimal(principal) * scaled)

    exponent = 64 * half_life - error_days
    numerator = 1 << max(exponent, 0)
    denominator = 1 << max(-exponent, 0)
    certify_floor(coefficient, numerator, denominator, half_life)
    certify_floor(ideal_weighted, principal**half_life * numerator, denominator, half_life)
    return error_days, principal, coefficient, ideal_weighted


def main() -> None:
    for half_life in (30, 15, 7):
        errors = [*range(65 * half_life + 1), 66 * half_life, 100 * half_life,
                  128 * half_life - 1, 128 * half_life, 10_000, 1 << 63, (1 << 64) - 1]
        for error_days in errors:
            for principal in (1, MAX_PRINCIPAL):
                print(half_life, *vector(error_days, principal, half_life), sep="\t")
        for error_days in (0, 1, half_life - 1, half_life, half_life + 1,
                           63 * half_life, 64 * half_life - 1, 64 * half_life,
                           64 * half_life + 1, 128 * half_life):
            for principal in (0, 3, 1_000_003):
                print(half_life, *vector(error_days, principal, half_life), sep="\t")


if __name__ == "__main__":
    main()
