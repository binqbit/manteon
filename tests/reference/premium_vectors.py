"""Independent arbitrary-integer reference for basis-point commitment premiums.

Q64 coefficients are independently certified by the existing integer-power
oracle. The reference never invokes Rust or uses a floating-point tolerance.
"""

from accuracy_q64_vectors import vector


def main():
    maximum = (1 << 64) - 1
    for half_life in (7, 15, 30):
        for cap in (0, 1000, 2000, 3000):
            for duration in (1, 3, 259200, 2592000, maximum):
                for remaining in sorted({0, 1, duration // 2, duration}):
                    rate = cap * remaining // duration
                    for principal in (1, 1000003, maximum - 1):
                        error = half_life + 1
                        coefficient = vector(error, principal, half_life)[2]
                        units = principal * rate
                        weight = coefficient * (principal * 10000 + units)
                        other = maximum - principal
                        other_coefficient = vector(0, other, half_life)[2]
                        denominator = weight + other_coefficient * other * 10000
                        gross = maximum * weight // denominator
                        print(half_life, cap, duration, remaining, rate, principal,
                              error, units, weight, denominator, gross, sep="\t")


if __name__ == "__main__":
    main()
