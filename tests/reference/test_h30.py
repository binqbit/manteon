"""Reference economics checks, not evidence of production H30 or token claims."""

import unittest
from decimal import Decimal, localcontext
from itertools import permutations

from h30 import PRECISION, Forecast, accuracy, allocate


class H30ReferenceTests(unittest.TestCase):
    def assert_close(self, actual, expected):
        with localcontext() as context:
            context.prec = PRECISION
            tolerance = Decimal("1e-90") * max(Decimal(1), abs(expected))
            self.assertLessEqual(abs(actual - expected), tolerance)

    def test_mat_060_exact_and_half_life_weights(self):
        for error, expected in [(0, "1"), (30, "0.5"), (60, "0.25"), (90, "0.125")]:
            self.assertEqual(accuracy(error), Decimal(expected))
        self.assertGreater(accuracy(7), accuracy(15))
        self.assertGreater(accuracy(15), accuracy(30))

    def test_mat_060_symmetry_around_observed_day(self):
        result = allocate((Forecast(100, 10), Forecast(100, 30)), 20)
        self.assertEqual(result.payouts[0], result.payouts[1])

    def test_mat_066_documented_no_fee_worked_example(self):
        result = allocate(tuple(Forecast(p, day) for p, day in [(2000, 2), (3000, 7), (100, 20), (4000, 10), (900, 60)]), 0)
        self.assertEqual(result.principal, 10_000)
        self.assertEqual(
            tuple(p.quantize(Decimal("0.01")) for p in result.payouts),
            tuple(map(Decimal, ["2409.85", "3220.40", "79.50", "4006.32", "283.93"])),
        )
        self.assertEqual(result.denominator.quantize(Decimal("0.000001")), Decimal("7924.482847"))
        with localcontext() as context:
            context.prec = PRECISION
            self.assert_close(sum(result.payouts), Decimal(10_000))

    def test_mat_036_exited_capital_never_returns_to_reference_pool(self):
        before = allocate((Forecast(100, 0), Forecast(100, 30)), 0)
        after = allocate((Forecast(100, 0),), 0)
        self.assertEqual(before.denominator, Decimal(150))
        self.assertEqual(after.principal, 100)
        self.assertEqual(after.denominator, Decimal(100))
        self.assertEqual(after.payouts, (Decimal(100),))

    def test_empty_reference_has_no_division_or_fabricated_payout(self):
        self.assertEqual(allocate((), 500).payouts, ())
        zeros = allocate((Forecast(0, 0), Forecast(0, 30)), 0)
        self.assertEqual(zeros.principal, 0)
        self.assertEqual(zeros.denominator, 0)
        self.assertEqual(zeros.payouts, (Decimal(0), Decimal(0)))

    def test_mat_071_same_date_splitting_does_not_increase_ideal_influence(self):
        merged = allocate((Forecast(1000, 7), Forecast(3000, 30)), 0)
        split = allocate((Forecast(400, 7), Forecast(600, 7), Forecast(3000, 30)), 0)
        self.assert_close(merged.denominator, split.denominator)
        with localcontext() as context:
            context.prec = PRECISION
            self.assert_close(merged.payouts[0], split.payouts[0] + split.payouts[1])

    def test_mha_012_enumeration_order_leaves_ideal_entitlements_fixed(self):
        forecasts = (Forecast(100, 0), Forecast(200, 15), Forecast(300, 60))
        original = allocate(forecasts, 0)
        for order in permutations(range(3)):
            permuted = allocate(tuple(forecasts[i] for i in order), 0)
            for index, source in enumerate(order):
                self.assert_close(permuted.payouts[index], original.payouts[source])
        # This is mathematical order independence, not executed claim replay.

    def test_mat_071_late_event_keeps_remote_nearby_weights_distinct(self):
        result = allocate((Forecast(1, 0), Forecast(1, 1)), 10_000)
        self.assertGreater(result.denominator, 0)
        self.assertGreater(result.payouts[1], result.payouts[0])
        self.assertGreater(accuracy(10_000), 0)
        # No 1920/3000/10000-day production limit follows from this fixture.

    def test_equal_accuracy_only_returns_principal(self):
        result = allocate((Forecast(1, 30), Forecast(10**18, -30)), 0)
        self.assertEqual(result.payouts, (Decimal(1), Decimal(10**18)))

    def test_negative_principal_and_negative_absolute_error_rejected(self):
        with self.assertRaises(ValueError):
            allocate((Forecast(-1, 0),), 0)
        with self.assertRaises(ValueError):
            accuracy(-1)


if __name__ == "__main__":
    unittest.main()
