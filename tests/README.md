# Protocol tests

Run from the standalone Manteon repository root. Tests use controlled inputs
and local temporary funds; they establish supported execution behavior without
an application, wallet, validator or RPC endpoint.

| Layer                                                 | Scope                                                                                                              |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| [Core](../packages/core/tests)                        | Checked amounts, UTC guards, H30/H15/H7, allowances, principal accounting, fees and payout quotes.                 |
| [References](reference)                               | Independent high-precision Decimal and integer models for coefficients and floored allocations.                    |
| [Anchor and LiteSVM](manteon/contract.rs)             | Real Manteon and pinned Tekmerion SBF execution, authenticated CPI, token custody, lifecycle, replay and rollback. |
| [Rust reader](../packages/client-rust/src/read.rs)    | Authentic account-graph reads and rejection cases exercised in the VM suite.                                       |
| [Shared vectors](fixtures/participation-vectors.json) | Canonical account layouts and participation cases for consumers.                                                   |
| [IDL checks](manteon/cases/idl.rs)                    | Maintained/generated interface compatibility and client wire encoding.                                             |

First initialize the pinned dependency and restore tools:

```sh
git submodule update --init --recursive
bash scripts/check.sh --restore
```

Later `bash scripts/check.sh` combines core and contract checks offline.
`bash scripts/check-core.sh` runs pure checks; `bash scripts/check-anchor.sh`
builds both programs and IDL, checks formatting/Clippy, and executes host and VM
tests. [Build requirements and tool pins](../docs/build.md) are separate from
consumer application tests.

Python references can run without Rust:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/reference -p 'test_*.py' -v
```

The [VM fixture](manteon/support/context.rs) shares actors, Clock control,
canonical account setup and exact token-balance assertions. Scenarios cover
Config reservation/reuse, governance, withdrawals and horizon exits, both event
types, service fallback, review retries, settlement batches, profit fees, zero
payout consumption, failed CPI rollback and duplicate claims.

IPO fixtures verify nonfuture occurrence facts and authenticated review outcomes;
the contract does not establish the truth of external company facts. Price
fixtures execute both directions, source conversion, stale/substituted sources,
failed crossings and expiry refunds. Controlled Raydium/Pyth account bytes do
not prove live provider availability or economic fair value. Local committee
signatures test the actual review rules, without proving live auditors' factual
judgment or deployed program identity.
