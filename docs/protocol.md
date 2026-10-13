# Protocol specification

Manteon enforces immutable pool terms, authenticated event acceptance and a
single claim per entitlement. This guide defines the exact time and arithmetic
rules. [How it works](how-it-works.md) explains the records and lifecycle;
[event rules](events.md) define each acceptance mechanism.

## Dates and participation

All clocks use Unix seconds and all forecasts use UTC calendar days. A day
number is the number of days since 1 January 1970. For a timestamp $t$,

$$
D(t) = \left\lfloor \frac{t}{86\,400} \right\rfloor.
$$

Let $t_o$ be opening time, $t_l$ entry close and $t_h$ the horizon.
Creation at time $t$ requires

$$
t < t_o < t_l \leq t_h.
$$

The service must activate the pool before $t_o$. Deposits require an Active,
incomplete pool and $t_o \leq t < t_l$. Ordinary withdrawals also require an
Active, incomplete pool and $t < t_l$, together with an owner's live
withdrawal allowance. Each deposit and withdrawal must have a positive amount.

An eligible forecast day $d$ satisfies both

$$
86\,400d \geq t_l,
\qquad
D(t_l) \leq d \leq D(t_l) + E_H,
$$

where $E_H$ is the selected curve's largest supported error. If entry closes
at noon, the earliest forecast is the following UTC day. A midnight close
permits that same day.

| Curve | Half-life $H$ | Largest supported error $E_H = 64H$ |
| ----- | ------------: | ----------------------------------: |
| H30   |       30 days |                           1920 days |
| H15   |       15 days |                            960 days |
| H7    |        7 days |                            448 days |

Creation also requires $D(t_h)-D(t_l) \leq E_H$. Forecast dates may extend
beyond the horizon. Competitive event results must fall within the same date
domain; pre-close occurrences instead enable principal refunds. IPO results
can occur after the horizon within that domain. Price confirmation must execute
no later than the horizon.

## Withdrawals and horizon exits

The personal allowance belongs to an owner's Stake at one forecast date:

- A first deposit gives the full new amount a 48-hour allowance.
- A top-up during a live allowance adds its amount without extending the deadline.
- A top-up after expiry gives only the new amount a fresh 48-hour allowance.
- Withdrawal reduces both remaining principal and the available allowance.
- A full withdrawal leaves an Active, empty Stake. A later eligible deposit
  starts a fresh allowance.

For example, an owner deposits 100 tokens on Monday at noon, then adds 20 on
Tuesday. All 120 are eligible until Wednesday at noon, subject to entry close.
If the owner instead adds 20 after Wednesday noon, only those 20 receive a new
allowance; the earlier 100 remain locked.

Withdrawal deadlines are exclusive. At the exact personal deadline or at entry
close, ordinary withdrawal stops. Event acceptance or monitoring expiry can
end Active participation sooner.

At $t \geq t_h$, an owner may exit with all remaining positive Stake
principal while the pool remains Active. Pending or even finalized IPO review
does not itself stop exit: applying the accepted report does. Transaction
ordering decides whether exit or the transition to settlement/refunds happens
first. Exited capital leaves all subsequent payout calculations and the Stake
is consumed.

## Accuracy and payout arithmetic

All collateral arithmetic uses integer token base units, also called atoms.
For Stake $i$, let $p_i$ be its participating principal, $d_i$ its
forecast day and $d_*$ the accepted event's UTC day. The absolute day error is

$$
e_i = |d_i-d_*|.
$$

With selected half-life $H$, the exact scaled accuracy coefficient and weight
are

$$
c_i = \left\lfloor 2^{64}\,2^{-e_i/H} \right\rfloor,
\qquad
w_i = p_i c_i.
$$

The coefficient is floored **before** multiplying by principal. The scale
retains 64 fractional bits of accuracy without floating-point arithmetic. The
admitted date domain keeps each positive stake's coefficient nonzero.

For frozen pool principal $P$ and total weight $W$,

$$
P = \sum_i p_i,
\qquad
W = \sum_i w_i,
\qquad
g_i = \left\lfloor \frac{P w_i}{W} \right\rfloor.
$$

Here $g_i$ is the gross allocation, including any returned principal. Its
ratio calculation uses checked 256-bit integer arithmetic. Multiplying before
division preserves fractional weight.

Capital at a shared date is additive: the date's aggregate weight equals the
sum of its owners' weights. Splitting same-date principal cannot increase that
weight. Gross payouts and [profit fees](configuration.md#fees) are nevertheless
floored separately per Stake, so splitting can change token-unit rounding.

## Settlement and claims

Acceptance of a competitive result freezes principal and funded date membership.
Each funded Position contributes its weight exactly once, in batches of one to
eight dates. The program authenticates membership and rejects repeated dates,
duplicate accounts in a batch and zero-principal dates.

Finalization enables claims only after every funded date has contributed and a
nonempty pool has a positive denominator. An empty pool needs no date
processing. IPO processing authenticates the accepted current review at every
step and follows its [service window](events.md#company-ipo). Price processing
is public immediately after confirmation.

The owner then claims each positive-principal Stake. Competitive claims consume
the entitlement and reduce outstanding payout liability by the gross allocation.
Frozen principal, date aggregates and total weight remain unchanged. A
zero-token payout still requires a claim to consume the entitlement.

Principal refunds consume the Stake and remove its principal from the remaining
refund book. They preserve the Stake's historical deposited amount. Withdrawals,
horizon exits and refund claims carry no profit fee.

Independent flooring can leave token dust. Donations also remain outside the
participating principal. The protocol provides no dust or donation recovery
action. Completion depends on consumed entitlements, so a nonempty vault does
not by itself prevent [Config release](configuration.md#config-lifecycle).

## Custody and integration boundaries

Prediction collateral must be an initialized legacy SPL token mint. Native or
wrapped SOL and Token-2022 prediction collateral are unsupported. Price-source
vaults can use SPL Token or Token-2022 independently of prediction custody.

Every capital movement checks the expected token mint, authority, initialized
account state, available liability and exact balance changes. Owners sign their
own deposits, withdrawals, exits and claims. No service, Config creator, global
admin or review signer has a discretionary principal-withdrawal action.

Protocol records are linked by authenticated identities. Reading fields alone
is insufficient: integrations must validate program ownership, full account
layout, canonical identities and membership, and acquire snapshots from the
intended network at a consistent checkpoint. The strict entrypoint also rejects
instruction payloads with unread trailing bytes, including calls from other
programs.

Tekmerion review is separate from prediction custody. Manteon authenticates its
policy, current attempt, accepted completion and both document commitments.
Price acceptance authenticates the fixed external sources described in
[event rules](events.md). Network access, program upgrades, external evidence
and source availability remain integration dependencies.
