# Configuration and fees

Deployment governance, reusable market settings and prediction custody have
separate authorities. The [account types](../programs/manteon/src/state/config.rs)
and [global instructions](../programs/manteon/src/instructions/global_config.rs)
define these boundaries.

## Deployment roles

`GlobalConfig` is the singleton PDA derived from `global_config`. Initialization
requires the deployed program's authenticated upgrade authority and explicitly
sets nonzero `admin`, `fee_authority` and a platform share from 0 to 10,000 basis
points. Initialize it before revoking upgrade authority or creating Configs.
The program supplies no default share or role addresses.

| Role           | Rights                                                          |
| -------------- | --------------------------------------------------------------- |
| Global admin   | Update fee authority/share and nominate a new admin.            |
| Pending admin  | Sign acceptance; only then replace the current admin.           |
| Fee authority  | Withdraw collected platform fees for a mint.                    |
| Config creator | Update unused Config settings and complete its unused Drafts.   |
| Config service | Create/activate Pools and use the defined IPO service windows.  |
| Stake owner    | Authorize own deposits, eligible withdrawals, exits and claims. |

Nominating the current admin clears a pending transfer. Changing admin leaves
the fee authority unchanged. The initializer has no special administrative or
withdrawal privilege afterward unless assigned that role.

Roles may be wallets or multisig signing PDAs. A multisig program must supply
its PDA signature by CPI; its quorum and member management remain its own
responsibility. Manteon applies the same signer and role checks.

## Config lifecycle

Anyone may create a Config with a separate storage payer. It stores creator,
service, collateral mint, fee recipient, bounded profit fee, platform share
and Tekmerion policy. Saving settings snapshots the GlobalConfig share and
requires `expected_platform_share_bps` to match at execution. Config creation
checks a nonzero policy identity; IPO activation authenticates that Policy.

`create_pool` binds `expected_config_hash` to `Config::terms_hash()`, SHA-256
over creator, service, mint, recipient, little-endian fee rates and policy in
that order. The reservation count is excluded, allowing concurrent Pool
creation with the same reviewed settings. A successful creation increments
`active_pools`, including Drafts. Only the creator may update a Config, and
only when its count is zero.

Each Pool snapshots its critical collateral, policy and fee facts so later
Config reuse preserves historical claims and receipts. Each reservation is
released exactly once:

- Last competitive claim, including zero payouts: `claimed_principal == principal`.
- Last refund claim or horizon exit: no participating principal remains.
- Empty finalized/refunded Pool: automatic release at the transition.
- `complete_pool`: creator/service may abandon a Draft immediately; anyone may
  do so at/after lock, complete an empty Active Pool at/after horizon, or release
  a fully consumed Claimable/Refunding Pool.

A Settling Pool cannot complete, and unpaid entitlements cannot be bypassed.
`config_released` prevents reactivation and duplicate release. Token-vault dust
or donations do not prevent completion. Releasing this Config reservation does
not release Tekmerion's Policy; the pinned review protocol has no policy release
instruction, so acquired review terms remain immutable.

## Fees

`profit_fee_bps` ranges from 0 to 2000: at most 20% of positive gross profit.
`platform_share_bps` ranges from 0 to 10,000: a share of that fee. With amounts
in collateral atoms and `B = 10,000`:

```text
profit   = max(gross − stake_principal, 0)
fee      = floor(profit × profit_fee_bps / B)
platform = floor(fee × platform_share_bps / B)
recipient = fee − platform
owner_net = gross − fee
```

The platform share is included in the total fee. Losses, break-even allocations,
principal refunds, ordinary withdrawals and exits have no profit fee. For a
100 USDC Stake with a 150 USDC gross allocation, a 20% fee and 20% platform
share pay 140 USDC to its owner, 8 to the recipient and 2 to the platform.

Claims use Pool snapshots. Updating the global share affects Configs when
created or edited while unused; it cannot rewrite an existing Pool's rates.
The fee recipient is an initialized collateral account, distinct from the
platform vault. Claims require it only when its calculated share is positive.

## Platform treasury

Each mint has one PDA at `["platform_fee_vault", mint]`, controlled by
GlobalConfig and created by the first Config using that mint. Config authors
cannot substitute its address or authority. The fee authority can withdraw a
positive amount up to its available balance into an initialized, unfrozen,
distinct token account of the same mint. This operation accepts no prediction
Pool vault. Administration and service roles cannot withdraw principal.

The [state diagrams](how-it-works.md#pool-states) and
[payout specification](protocol.md#accuracy-and-payout-arithmetic) explain when
claim rights are created and consumed.
