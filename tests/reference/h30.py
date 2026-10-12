"""High-precision analytical H30 reference, never a production payout engine.

Decimal precision is an oracle setting, not an adopted onchain representation.
Results omit fees and token rounding. No amount/date limits, empty-pool state,
dust beneficiary, claim policy, acceptance authority or settlement ABI is chosen.
"""

from dataclasses import dataclass
from decimal import Decimal, localcontext

PRECISION = 100


@dataclass(frozen=True)
class Forecast:
    """Synthetic eligible principal and its integer UTC forecast day."""

    principal: int
    day: int


@dataclass(frozen=True)
class Allocation:
    """Immutable analytical basis and unrounded no-fee payouts."""

    principal: int
    denominator: Decimal
    payouts: tuple[Decimal, ...]


def accuracy(error_days: int) -> Decimal:
    """Direct 2^(-d/30) at oracle precision, with no nearest-day rescaling."""
    if error_days < 0:
        raise ValueError("absolute error must be nonnegative")
    with localcontext() as context:
        context.prec = PRECISION
        return Decimal(2) ** (-Decimal(error_days) / Decimal(30))


def allocate(forecasts: tuple[Forecast, ...], observed_day: int) -> Allocation:
    """Evaluate the published ideal formula on synthetic residual capital.

    Empty input returns an empty mathematical allocation only; it cannot choose
    a legal onchain completion state. Zero-principal historical positions carry
    no weight. A future onchain numerical implementation must be tested against
    this reference over its separately selected admissible domain.
    """
    if any(forecast.principal < 0 for forecast in forecasts):
        raise ValueError("principal must be nonnegative")
    with localcontext() as context:
        context.prec = PRECISION
        principal = sum(forecast.principal for forecast in forecasts)
        weights = tuple(
            Decimal(forecast.principal) * accuracy(abs(forecast.day - observed_day))
            for forecast in forecasts
        )
        denominator = sum(weights, Decimal(0))
        if principal == 0:
            return Allocation(0, Decimal(0), tuple(Decimal(0) for _ in forecasts))
        if denominator <= 0:
            raise ArithmeticError("positive principal requires a positive denominator")
        payouts = tuple(Decimal(principal) * weight / denominator for weight in weights)
        return Allocation(principal, denominator, payouts)
