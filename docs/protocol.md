# Protocol specification

The [Anchor interface](../idl/manteon.json) defines instruction and account wire
layouts. The [program](../programs/manteon/src/lib.rs) owns authorization and
atomic changes; [core calculations](../packages/core/src/lib.rs) provide
checked quotes without granting custody or admission rights.

## Accounts and addresses

All seeds below derive PDAs under the Manteon program ID. Public keys are
32-byte seed values; `forecast_day` is little-endian signed `i64`.

| Account              | Identity and purpose                                                                                  |
| -------------------- | ----------------------------------------------------------------------------------------------------- |
| `GlobalConfig`       | `["global_config"]`; deployment admin, fee authority and fee share.                                   |
| `Config`             | Fresh signing keypair; creator-owned reusable settings and `active_pools` reservation count.          |
| `Pool`               | Fresh signing keypair; immutable terms, lifecycle and settlement accounting.                          |
| `Event`              | `["event", pool]`; immutable event definition and latest proposal or verified result.                 |
| `Position`           | `["position", pool, forecast_day]`; principal aggregated for one forecast date and its computed flag. |
| `Stake`              | `["stake", position, owner]`; one owner's principal, allowance and terminal entitlement state.        |
| `PriceMonitor`       | `["price_monitor", pool]`; retained cumulative baseline for a price pool.                             |
| Prediction vault     | `["vault", pool]`; collateral account owned by `["vault_authority", pool]`.                           |
| Platform fee vault   | `["platform_fee_vault", mint]`; collateral account owned by GlobalConfig.                             |
| Tekmerion CPI signer | `["tekmerion", policy]`; authorizes review integration, with no prediction custody rights.            |

Pool retains the Config's collateral mint, audit policy and fee facts at creation.
The Config remains locked while a Pool uses it. Event and Pool are reciprocally
bound. A Position aggregates owners at a date; an owner's Stakes at different
dates remain separate entitlements. Account readers must check owner,
discriminator, full size, canonical address and membership, in addition to
deserializing fields. See the [Rust reader](../packages/client-rust/README.md).

## Instructions and signers

The [strict entrypoint](../programs/manteon/src/entrypoint.rs) rejects unread
payload suffixes before generated Anchor dispatch, for direct calls and CPI.

| Instruction                                   | Required authority and effect                                                              |
| --------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `initialize_global_config`                    | Program upgrade authority; one-time singleton initialization.                              |
| `update_global_config`                        | Global admin; update fee authority and future Config platform share.                       |
| `propose_global_admin`, `accept_global_admin` | Current then nominated admin; two-step role transfer.                                      |
| `withdraw_platform_fees`                      | Fee authority; withdraw from the canonical mint treasury.                                  |
| `create_config`                               | Creator; permissionless settings creation with independent storage payer.                  |
| `update_config`                               | Creator; replace settings only with zero retained Pools.                                   |
| `create_pool`                                 | Config service; create Draft, Event and collateral vault, reserve Config.                  |
| `activate_pool`                               | Config service; validate IPO mint and acquire Tekmerion Policy before opening.             |
| `activate_price_pool`                         | Config service; authenticate initial fixed sources and create monitoring baseline.         |
| `stake`                                       | Owner; deposit a positive amount into an eligible forecast date.                           |
| `unstake`                                     | Owner; withdraw a positive amount allowed by the personal window and entry schedule.       |
| `exit`                                        | Owner; return all remaining Stake capital from an Active Pool at/after horizon.            |
| `propose_company_ipo`                         | Eligible service/public author; validate result and fund a fresh review Case.              |
| `record_price_observation`                    | Any wallet; advance a qualifying noncrossing observation baseline.                         |
| `finalize_price_threshold`                    | Any wallet; verify crossing and freeze or refund atomically.                               |
| `expire_price_observation`                    | Any wallet; enable refunds after horizon or lost monitoring gap.                           |
| `begin_settlement`                            | Eligible service/public caller; apply an accepted IPO Case and freeze or refund.           |
| `process_positions`                           | Authorized settlement caller; process 1–8 distinct writable canonical Positions.           |
| `finalize_settlement`                         | Authorized settlement caller; require complete funded-date processing and enable claims.   |
| `claim`                                       | Owner; consume a competitive allocation or principal refund exactly once.                  |
| `complete_pool`                               | Eligible creator/service/public caller; release an unused or fully consumed Pool's Config. |

Price settlement processing and finalization are immediately public. IPO
settlement steps authenticate the current accepted Case each time and follow
the [service window](events.md#company-ipo). Claiming requires no service
signature or document archive.

## Dates and participation

Unix timestamps are signed seconds; forecast dates are integer UTC days since
the Unix epoch. Creation requires `now < opens_at < lock_at <= horizon_at`;
activation requires `now < opens_at`. Deposits require an unreleased Active Pool
and `opens_at <= now < lock_at`. Ordinary withdrawal requires an unreleased
Active Pool, `now < lock_at` and a live personal allowance; its capital can only
have entered after opening.

An eligible forecast has `day_start(forecast_day) >= lock_at` and
`utc_day(lock_at) <= forecast_day <= utc_day(lock_at) + max_error_days`.
Consequently a lock within a day makes the next UTC day the earliest forecast.
H30, H15 and H7 support maximum errors of 1920, 960 and 448 days respectively.
Creation bounds the horizon within that same interval. Forecasts may extend
past the horizon; accepted competitive results must remain in the supported
domain. Pre-lock results instead enable principal refunds.

New capital is withdrawable for 48 hours. Top-ups during a live allowance add
withdrawable capital without extending its deadline. After expiry, only the
new deposit receives a fresh allowance; earlier capital stays locked. The
personal deadline is exclusive, and lock ends ordinary withdrawals even if it
has not expired. Full ordinary withdrawal leaves a zero-principal Active Stake
that can receive a later eligible deposit. At/after horizon, an owner may exit
an Active Pool regardless of a pending review, until freeze or refund transition
wins transaction ordering. Exit consumes the Stake.

## Accuracy and payout arithmetic

All collateral amounts use integer token atoms. Let `p_i` be Stake principal,
`P` the frozen pool principal, `d_i` absolute UTC-day forecast error and `H` the
selected 30-, 15- or 7-day half-life:

```text
c_i = floor(2^64 × 2^(−d_i/H))
w_i = p_i × c_i
W   = sum of the weights of all frozen Positions
gross_i = floor(P × w_i / W)
```

The coefficient is floored before multiplying principal. Q64.64 weights retain
64 fractional bits, and the payout numerator uses checked 256-bit arithmetic.
Same-date principal is additive, so splitting it does not increase weight.
Token-atom rounding occurs independently for each Stake's payout; splitting can
change the resulting rounding and per-Stake fee calculation.

Settlement freezes principal, funded date membership and the eventual
denominator. Processing checks canonical Position identity, excludes duplicate
accounts in a batch and records each positive date exactly once. Finalization
requires every funded date to have contributed and a positive denominator for
a nonempty pool. An empty pool can finalize without processing.

Gross allocations already include returned principal. Claims reduce
`payout_remaining` by gross, consume the Stake and accumulate its principal in
`claimed_principal`. Pool/Position principal and total weight remain frozen.
A zero-value allocation still requires a claim and consumes its entitlement.
Independent flooring can leave dust; the program exposes no dust withdrawal
or vault donation recovery instruction. Completion depends on consumed
principal, not an empty token vault. See [fee formulas](configuration.md#fees).

## Custody and integration boundaries

Prediction collateral is an initialized legacy SPL mint; native/wrapped SOL
and Token-2022 prediction collateral are unsupported. Price-source vault reads
can support SPL Token and Token-2022 independently of prediction custody.
Token transfers check expected mint, owner, initialized state, liability and
exact balance changes. A service, Config creator, global admin or audit signer
has no discretionary prediction-principal withdrawal instruction.

Tekmerion owns separate USDC review custody. Its Policy/Case accounts are
authenticated by [the adapter](../programs/manteon/src/integration/tekmerion.rs),
including owner, full size, discriminator, canonical address, signer binding,
attempt chain and both document hashes. Price readers authenticate the fixed
external sources described in [event rules](events.md).

Upgrades, network access, external program behavior and off-chain evidence
remain trust dependencies. Synthetic tests establish local execution behavior,
not live deployment or the factual truth of an IPO verdict.
