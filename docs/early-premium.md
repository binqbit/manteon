# Early commitment weight premium

A pool may reward earlier commitment with an additional calculation weight.
The immutable maximum is selected at creation: **Off (0%), 10%, 20%, or 30%**,
encoded as `early_premium_bps = 0, 1000, 2000, 3000`. Off is the application
creation default; the instruction always carries the explicit value. The policy
is independent of the H7, H15 or H30 accuracy curve and cannot change after
creation.

The premium is not deposited collateral, withdrawable money, insurance or a
guaranteed payout. Real principal and the existing profit fee remain separate.
Earlier weight can outweigh a difference in forecast accuracy: even an exact
late forecast can lose money. There is no promise to preserve a position's
profit/loss classification under the hypothetical 0% policy.

## Choose how new funds enter

`stake(forecast_day, amount, commit_now)` admits only new real funds during the
existing Active entry period. The owner chooses before signing:

| Choice | Ordinary withdrawal | Premium anchor |
|---|---|---|
| `commit_now = false` | The new amount joins the owner's date-specific window | Earlier of that window's actual deadline and global LOCK |
| `commit_now = true` | The new amount is immediately unavailable for ordinary withdrawal | Executing chain Clock time |

An ordinary first deposit opens a **48-hour** window. A top-up during a live
window shares its remaining deadline; it does not restart 48 hours. After
expiry, an ordinary top-up opens a fresh window for only the new amount. Global
LOCK and lifecycle transitions can stop ordinary withdrawals sooner.

Immediate commitment affects only that instruction's new amount. Existing
withdrawable funds and their deadline remain unchanged, including a stored
expired allowance. A first immediate deposit into an empty Stake has no
withdrawal window: its stored allowance and deadline are zero. A later ordinary
deposit then opens its own fresh 48 hours. If some locked principal remains and
a live window has zero remaining allowance, a new ordinary top-up still shares
that live deadline.

Immediate commitment does not cancel protocol refunds or unresolved-event
horizon exits. It does not introduce an instruction to convert existing pending
funds into immediate commitment. Withdrawing and depositing again, when both
are permitted, uses the new operation's actual anchor.

For an entry period of at most 48 hours, a fresh ordinary deposit stays
withdrawable until LOCK and receives zero premium. It still participates with
its baseline accuracy weight. Choosing immediate commitment can receive a
premium because it gives up ordinary withdrawal earlier.

## Exact integer calculation

Times use integer Unix seconds. Let `OPEN < LOCK`, `T = LOCK − OPEN`, and `B` be
the pool's immutable maximum in basis points. A basis point is **0.01 percentage
point**. For each new amount `a`:

```text
anchor = now                              if commit_now
anchor = min(actual_withdraw_until, LOCK)  otherwise
rate_bps = floor(B × (LOCK − anchor) / T)
new_premium_units = a × rate_bps
```

The rate is floored once to a whole basis point at the anchor. It is not rounded
up, recalculated when the window expires, or applied to earlier deposits again.
The instruction's executing Clock, not a browser quote or submission time,
determines NOW. A quote can therefore change before execution.

At a 20% maximum, an ordinary deposit at the beginning of a three-day entry
period gets `floor(2000 × 1/3) = 666` basis points, or **6.66%** additional
weight units. Immediate commitment at opening gets 20%. For a 30-day period,
the opening ordinary rate is **18.66%**, while immediate commitment gets 20%.
These are weight premiums, not promised financial returns.

One owner's date-specific Stake stores real principal `p` and the exact sum
`s = Σ retained_amount × fixed_rate_bps`. The corresponding Position stores
the sums across its owners. For example, 100 atoms at 20% and 100 at 10% give
`p = 200`, `s = 300000`: their derived average is 15%. The protocol never floors
an average rate and multiplies it back into principal.

With existing coefficient-first Q64 accuracy `c`, the raw settlement weight is

```text
c = floor(2^64 × 2^(−absolute_day_error / H))
weight = c × (p × 10000 + s)
gross = floor(real_pool_principal × weight / total_raw_weight)
```

The common `10000` scale is retained in both numerator and denominator,
including Off pools. Thus the 0% policy produces bit-for-bit identical gross
allocations to the baseline accuracy formula. Premium units do not increase
pool principal, collateral backing, token balances, refund amounts or the
profit-fee principal basis. Gross rounding and positive-profit fees occur once
per owner/date, not once per top-up.

Equal relative premiums for everyone cancel in the ratio. A 30% weight premium
does not mean a 30% payout increase. Different commitment times at the same
forecast date can produce different returns; this is an explicit part of the
pool's terms.

## Withdrawal, expiry, exit and refund

All currently withdrawable funds in an owner/date window share the same actual
deadline and rate. Ordinary withdrawal of `a` therefore removes exactly

```text
real_principal -= a
withdrawable_principal -= a
premium_units -= a × rate_bps(B, min(withdraw_until, LOCK))
```

No choice or traversal of individual contribution accounts is necessary. Older
locked funds and immediate deposits keep their units. The same updates apply
to date aggregates atomically with the real token transfer.

Expiry itself needs no transaction, worker or clock-driven write. The stored
allowance may remain positive after its deadline, but the time guard makes it
unspendable. Its retained principal and premium units remain in the forecast.
When a later ordinary top-up starts a new window, only its amount becomes
withdrawable; the earlier units remain accumulated.

A horizon exit removes the owner's full real principal and units from the
active date, then clears the Stake's amounts and consumes it. A principal
refund removes the same amounts from the refund book but retains the terminal
Stake's historical principal and premium units. Competitive claims retain all
frozen principal, units and total weight, and consume the entitlement once.
Terminal history cannot be deposited into, withdrawn again or counted twice.

Necessary local checks include `0 ≤ stored_allowance ≤ principal` and
`premium_units ≤ principal × B`. If a stored allowance is positive, its exact
pending units must fit within total units, and the remaining units must fit
within `(principal − stored_allowance) × B`. This check also applies to expired
stored allowances until they are replaced.

## Accounts, ABI and bounded work

The published [Anchor IDL](../idl/manteon.json) is the instruction/account ABI:

- `create_pool` receives `early_premium_bps: u16` immediately after `algorithm`.
- `stake` appends `commit_now: bool` after `forecast_day` and `amount`.
- Pool stores `early_premium_bps` after `algorithm` and retains the three baseline
  algorithm variants only.
- Position and Stake store `premium_units: u128` immediately after principal.
- Pool `total_weight` is **32 bytes, unsigned U256, little endian**. It is a raw
  accuracy-times-basis-point weight; it is not a u128 Q64 weight or token amount.
  Its zero encoding is 32 zero bytes.

There are no contribution, allowance-extension or per-top-up settlement
accounts. The beta account and instruction layout changed; this does not supply
a live migration or permission to change terms for previously funded pools.

Deposit, ordinary withdrawal, exit and claim each update a constant number of
accounts, independent of deposit history. Frozen settlement visits every
positive Position exactly once in authenticated batches of one to eight dates.
Each date's raw weight equals the sum of its owners' raw weights. Stored markers,
date counts and capacity guards prevent duplicate or incomplete processing.
Finalization requires complete coverage and a positive denominator for a
nonempty pool. No off-chain trusted denominator is used.

The Rust core exposes `math::premium::{Weight, PREMIUM_SCALE, rate_bps, units,
validate_units, validate_allowance, max_weight, weighted_principal, quote_gross}`
and `accounting::settlement::PremiumSettlementProgress`. `Weight` is U256 and
provides `from_le_bytes` / `to_le_bytes`. All financial amounts must remain
integer-safe in clients. Never decode u128 values or wide weights through a
JavaScript `number`.

At the full u64 real-principal boundary and the 30% maximum, raw weight needs at
most 142 bits and a gross numerator at most 206 bits. U256 calculations support
that bound without an artificial principal cap or a restricted timestamp span.
The linear rate calculation widens timestamp differences before multiplication.

## Local verification

Run the maintained [build checks](build.md). The suite includes all baseline
lifecycle scenarios, both deposit modes and mixed funds, shared deadlines,
cutoff and expiry, empty-Stake stale deadlines, exact premium removal,
refund/exit behavior, fee-once claims and invalid premium ledgers. Independent
Python arbitrary-integer vectors certify the Rust rate, unit, weight and gross
results, including full-width time differences and principal bounds.

Actual local SBF execution of eight-date premium batches at near-u64 pool
principal across all twelve H7/H15/H30 × 0/10/20/30 configurations measured a
maximum **73,467 compute units**, within the standard 200,000-unit instruction
budget. This is local execution evidence, not a universal performance guarantee
for another program build or future runtime. There is no deployment, economic
behavioral validation or independent security audit implied by these checks.
