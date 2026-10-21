# Event rules

Each pool selects an immutable event definition and acceptance mechanism.
External events use evidence review through Tekmerion. Token price thresholds
use fixed on-chain sources. Both supply the UTC date used in [settlement](protocol.md#accuracy-and-payout-arithmetic).

## External event

`ExternalEvent` registers an external fact through the pool's Event account.
The pool commits to its Agreement hash. The Agreement defines the subject,
occurrence criteria, sources and rule for selecting the occurrence timestamp.
Any relevant token, company or instrument identity belongs in that document;
the contract does not require a token mint for an external event.

An IPO and a bridge opening use the same event type and proposal instruction.
For example, an IPO Agreement can require a completed public offering and reject
announcements; a bridge Agreement can require the first public opening. These
are authored conditions, not separate contract branches. Insufficient evidence
can produce an Unknown review answer.

A proposer supplies occurrence facts and funds a fresh Tekmerion review with a
separate USDC author bond. Manteon checks the event identity, a nonfuture time,
the supported calendar range and, for a competitive result, the allowed date
domain. It generates a canonical result commitment bound to the pool and its
agreement. It does not fetch websites or establish the external facts.

### Review and acceptance

The pool retains its review policy when activated. Challengers may fund a
challenge under those locked terms:

- An underfunded challenge can close with **optimistic Yes** after its funding
  window. Auditors have not verified the report in this path.
- A fully funded challenge requires the appointed auditors to reach the
  threshold of matching signed answers: Yes, No or Unknown.
- Failure to reach agreement before the audit deadline closes as a technical
  failure and leaves the report unaccepted.

See [Tekmerion's process](../vendor/tekmerion/docs/how-it-works.md) for review
funding, voting, fees and withdrawals.

Manteon accepts the first authenticated, finalized Yes successfully applied to
the pool, whether optimistic or audited. Any wallet can apply it. That single
transaction fixes the accepted Case and occurrence facts and freezes prediction
capital or enables early-event refunds. Two competing Yes results cannot
replace one another: transaction ordering selects the first successful acceptance.
A review becoming final alone does not freeze the market.

### Parallel cases prevent blocked results

Each author has a separate sequence of Cases for the same market. One author
can have only one unresolved Case; different eligible authors can open and
resolve their Cases concurrently. A pending or repeatedly rejected proposal
therefore cannot monopolize the market once public submission is available.

An audited No, audited Unknown or technical failure allows that author to
retry with a fresh bond. All Cases keep their own funding, audit deadlines and
claims. When one Yes is applied, new Cases are disabled, while existing Cases
finish their own review and withdrawals without changing the accepted market
result. A latest-completed Case displayed by an application is informational;
it has no authority to replace the accepted Case.

### Occurrence time and access windows

The service has the initial proposal opportunity while the pool is Active.
Other authors can challenge that proposal. Public proposals become available
when either of these conditions holds:

- A service-authored Case ends in audited No, audited Unknown or technical
  failure. A proposer supplies that authenticated Case as proof to open access.
- The earlier of thirty days before the horizon and thirty days after the first
  paid proposal is reached. With no proposal, the horizon-based opening applies.

Once a public proposal succeeds, the opening is retained permanently. Neither
a service retry nor another author's new Case can close or restart it. The
reported occurrence date cannot accelerate these access windows.

Applying a finalized Yes is permissionless immediately. Processing funded dates
and completing settlement retain their existing service priority: the service
can act immediately, and other callers can act thirty days after the accepted
Case's finalization. Every service window means exactly thirty UTC days,
independently of H30, H15 or H7. Owner claims require no service signature.

An accepted occurrence before entry close enables principal refunds. An
occurrence at or after entry close freezes the remaining capital for
competitive settlement. The horizon enables owner exits while the pool stays
Active, including during pending review; it does not impose an event occurrence
or review deadline.

## Token price threshold crossing

The price rule fixes a Raydium CPMM pool, tracked token, direction, positive USD
target, conversion source, minimum averaging interval, maximum monitoring gap
and minimum quote liquidity. CLMM and AMM v4 are unsupported. The sources and
parameters cannot be replaced after pool creation.

Prices represent USD per whole token with nine decimal places. An encoded
integer $x$ means $x/10^9$ dollars. Targets and calculations use integer
arithmetic, with lower and upper bounds where conversion or rounding creates
uncertainty.

### Supported USD conversion

| Tracked market and conversion                | Meaning of the interval price                                                                        |
| -------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| Token/USDC                                   | Average token price in canonical USDC, with USDC valued at exactly $1.                               |
| Token/SOL with Pyth SOL/USD                  | Average token/SOL price multiplied by the current verified SOL/USD confidence bounds.                |
| Token/SOL with a fixed Raydium SOL/USDC pool | Product of the two interval averages, using authenticated endpoints within 30 seconds of each other. |

The last path computes a product of averages; it does not reconstruct an
average of simultaneous USD products. Pyth conversion fixes the update
account, supported receiver and SOL/USD feed. Fully verified updates must be
positive, nonfuture and within the selected age and confidence limits.

### Baseline, average and spot

Activation must happen before opening and requires the current spot bounds to
be entirely opposite the target. For an upward crossing, the upper bound must
be below the target. For a downward crossing, the lower bound must be above it.
Activation saves an authenticated cumulative-price baseline.

A cumulative price records price integrated over time. If $C_0$ and $C_1$ are
its values at endpoints $t_0$ and $t_1$, the interval average follows

$$
\overline{p} = \frac{C_1-C_0}{t_1-t_0},
$$

with the source's fixed-point scale, token decimals and USD conversion applied.
The implementation handles cumulative-counter wrapping and rounds converted
bounds outward. It does not extend an old endpoint with today's spot price or
fill an unobserved interval.

```mermaid
flowchart LR
    Start["Retained cumulative baseline"] --> Interval["Later authenticated endpoint: interval average"]
    Interval --> Both{"Average and current spot both beyond target?"}
    Both -->|Yes| Confirm["Record executing Clock as event time"]
    Both -->|"Neither beyond target"| Advance["Allow a new observation baseline"]
    Both -->|"Only one beyond target"| Wait["Keep the baseline; no confirmation"]
```

The baseline belongs to Manteon, so Raydium observation-ring rewrites do not
erase it. The interval begins at that retained baseline, rather than at a
rolling chart window. A successful noncrossing observation advances it only
when neither the interval nor the current spot satisfies the target.

Let $q$ be the target and $[L,U]$ a verified price interval. A crossing requires

$$
\begin{aligned}
\text{upward:}&\quad L \geq q,\\
\text{downward:}&\quad U \leq q.
\end{aligned}
$$

**Both** the interval average and the current spot bounds must qualify. A bound
that straddles the target cannot confirm a crossing.

### Safeguards and expiry

| Requirement                    | Enforced boundary                                                                                          |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------- |
| Minimum averaging interval     | 300–3600 seconds, selected by the pool.                                                                    |
| Maximum monitoring gap         | At least twice the minimum interval and at most 86,400 seconds.                                            |
| Quote liquidity                | At least $10,000 in fee-adjusted quote reserves; applies to both CPMM pools when using Raydium conversion. |
| Raydium endpoint freshness     | No more than 120 seconds old.                                                                              |
| Pyth update age                | Pool-selected limit of 1–120 seconds.                                                                      |
| Pyth confidence-to-price ratio | Pool-selected limit of 1–100 basis points.                                                                 |

These public-build bounds remain unchanged. An explicitly selected `localnet`
build permits a 60-second minimum for disposable demonstrations; the selected
window is still committed in the agreement. See [build profiles](build.md).

These are validation bounds, not defaults. Source reads also authenticate
account owners, complete supported layouts, pool and mint identities,
observation identity, vaults and trading availability. Protocol, fund and
creator fees held in Raydium vaults are excluded from reserves. Source vaults
may use SPL Token or Token-2022.

Any wallet may submit observations, confirm a threshold or expire monitoring.
Confirmation must execute at or before the horizon and within the maximum gap
from the retained baseline. It records the executing on-chain Clock, clears
the baseline and atomically freezes principal or enables pre-close refunds.
Settlement processing is public immediately, with no Case, bond or auditors.

After the horizon, or once the retained baseline exceeds the maximum gap, any
wallet can expire monitoring. Expiry accepts no price-source accounts, clears
the baseline and enables principal refunds without creating an event time.
The expiry comparisons are strict: exactly at the horizon, confirmation and
owner exit can still be eligible; exactly at the maximum gap, an otherwise
valid observation or confirmation can still qualify.

Confirmation establishes verification time, not the first historical market
move. Average-plus-spot checks can reject brief spikes. Sustained manipulation,
USDC depegging, stale or unavailable sources and external program upgrades
remain limits of the rule. Controlled local tests verify its execution, not
live source availability or economic fair value.
