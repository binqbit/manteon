# Event rules

An immutable [EventDefinition](../programs/manteon/src/state/event.rs) selects
one of two acceptance mechanisms. `EventResult` contains `occurred_at` and
optional verified price bounds. An IPO value in Event is a candidate until its
current Case is accepted; a price result is produced by verified source reads.

## Company IPO

`CompanyIpo` identifies the token mint representing the company. Applications
commit the issuer/evidence sources in the agreement. The agreed question asks
when the company actually completed its public IPO. Announcements, estimates,
token mint creation and a token's first exchange trade do not establish that
fact. If evidence confirms the date without a time, use 00:00:00 UTC on that
date. If completion and date cannot be established, the review answer is Unknown.

The contract checks mint identity, nonfuture occurrence time, its supported
timestamp range and the competitive date domain. It renders the result
document and opens a Tekmerion Case with the separate USDC author bond, bound
to the Pool, policy, attempt nonce, agreement hash and result hash. It does not
read websites or establish external IPO facts itself.

Under the acquired Policy, challengers can fund a challenge. An underfunded
Case can finalize with optimistic Yes after its funding window. A funded Case
requires the appointed auditors' signed matching verdicts; failure to reach
quorum before timeout closes without accepting the candidate. See
[Tekmerion's process](../vendor/tekmerion/docs/how-it-works.md).

Manteon accepts only its current authenticated finalized Yes, completed
optimistically or by audit. An audited No/Unknown or technical failure permits
a fresh paid attempt; previous Case claims remain available. Every retry keeps
the first successful proposal's Clock time, so it cannot reset public access.

The service may propose immediately. Other authors become eligible at the
earlier of `horizon_at − 30 days` and `first_paid_attempt_at + 30 days`. Applying
accepted Yes and processing/finalizing settlement are available immediately to
the service, and to the public at `case.finalized_at + 30 days`. These windows use
exactly thirty UTC days, independently of the selected accuracy half-life.

An accepted occurrence before lock enables principal refunds; otherwise it
freezes the remaining book for settlement. The horizon allows exits while the
Pool remains Active, including pending review, and is not an IPO deadline.

## Token price threshold crossing

`TokenPriceThresholdCrossing` fixes a Raydium CPMM pool, token mint, direction,
positive USD target, conversion source, averaging interval, maximum gap and
minimum quote liquidity. CLMM and AMM v4 are unsupported.

| Rule                     | On-chain bounds                                             |
| ------------------------ | ----------------------------------------------------------- |
| Target                   | USD per whole token, integer billionths of a dollar (`e9`). |
| Minimum averaging window | 300–3600 seconds.                                           |
| Maximum observation gap  | At least twice the minimum window, at most 86,400 seconds.  |
| Minimum quote liquidity  | At least $10,000 in fee-adjusted quote reserves.            |
| Pyth maximum age         | 1–120 seconds.                                              |
| Pyth confidence ratio    | 1–100 basis points.                                         |

These are validation limits, not program defaults. All source parameters are
immutable. Supported conversion choices are:

- **Token/USDC:** canonical USDC valued at exactly $1; no extra oracle.
- **Token/SOL plus Pyth SOL/USD:** fixed maintained update account, supported
  receiver and SOL/USD feed. Fully verified, positive, nonfuture updates must
  satisfy age/confidence bounds. Token/SOL interval mean is multiplied by
  current SOL/USD confidence bounds.
- **Token/SOL plus Raydium SOL/USDC:** a separate fixed CPMM pool. The product
  of interval means defines conversion; it is not the mean of instantaneous
  USD products. Source endpoints must be within 30 seconds.

Activation requires current spot bounds entirely opposite the target:
`upper < target` for up, `lower > target` for down. It creates a PriceMonitor
with authenticated cumulative endpoints. Source readers check owners, supported
layouts, pool/mint identities, observation PDA, vaults, trading availability and
liquidity. Raydium fees are excluded from reserves; cumulative endpoints may be
at most 120 seconds old. Supported source vaults may use SPL Token or Token-2022.

The baseline survives Raydium ring rewrites. An interval runs from it to a new
authenticated endpoint within the fixed minimum window and maximum gap; it is
not a rolling chart average. Integer calculations handle decimals, cumulative
wrapping, confidence and outward rounding. No unobserved interval is filled
with a present spot value.

Any wallet may advance observations, confirm a crossing or expire monitoring:

1. `record_price_observation` advances the baseline only when neither interval
   nor current spot satisfies the target, so it cannot erase a qualifying price.
2. `finalize_price_threshold` requires both to qualify: `lower >= target` for
   up, `upper <= target` for down. A straddling uncertainty interval fails.
   Executing Clock becomes occurrence time, with `now <= horizon_at`. It clears
   the baseline and atomically freezes principal or enables pre-lock refunds.
3. Settlement processing and finalization are public immediately; no Case,
   bond, audit or service waiting window is involved.
4. `expire_price_observation` requires `now > horizon_at` or
   `now − baseline.observed_at > max_gap`. It accepts no source accounts,
   clears the baseline and enables refunds without inventing a result.

Confirmation records verification time, not the first historical price move.
Brief spikes may fail the average/spot combination; stale sources, inactivity
or lost monitoring can prevent confirmation. The rule does not compensate for
a USDC depeg, prove fair value or exclude sustained manipulation. External
program upgrades remain a trust dependency.

The [adapter](../programs/manteon/src/integration/price.rs),
[Raydium reader](../programs/manteon/src/integration/price/raydium.rs),
[Pyth reader](../programs/manteon/src/integration/price/pyth.rs) and
[instructions](../programs/manteon/src/instructions/price_threshold.rs) define
supported layouts and exact checks. [Local tests](../tests/README.md) verify
execution with controlled sources; they do not prove live source availability.
