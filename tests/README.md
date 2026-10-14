# Protocol tests

The standalone tests check numerical rules and execute the protocol locally
with controlled clocks and funds. They require no application, wallet,
validator or RPC endpoint.

## Run the checks

From the Manteon repository root:

```sh
git submodule update --init --recursive
bash scripts/check.sh --restore
```

Later runs use `bash scripts/check.sh` with offline, locked inputs.
`bash scripts/check-core.sh` runs the calculation checks;
`bash scripts/check-anchor.sh` builds both protocols and the Manteon interface
and runs the contract checks. See [build requirements and tool pins](../docs/build.md).

The independent Python numerical references can run without Rust:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/reference -p 'test_*.py' -v
```

## What is verified

| Test layer                       | Purpose                                                                                                                    |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Core calculations                | Checked amounts, UTC boundaries, H30/H15/H7 accuracy, withdrawal allowances, principal accounting, fees and payout quotes. |
| Independent numerical references | Certify exact coefficient floors and token-unit allocations using high-precision and integer models.                       |
| Local contract execution         | Run the real Manteon and pinned Tekmerion SBF programs in LiteSVM, including review integration and token transfers.       |
| Account-reader checks            | Accept authentic related snapshots and reject invalid identities, layouts, relationships and accounting.                   |
| Interface compatibility          | Check that maintained and generated interfaces agree and that consumer instruction encoding is compatible.                 |

Contract scenarios cover configuration reuse, governance, withdrawals and
horizon exits, both event mechanisms, service fallback, review retries,
settlement batches, profit fees, zero-payout claims and duplicate-payment
protection. Failed transactions also verify that state changes and transfers
roll back together.

Fee tests verify that closing or changing a recipient account cannot bind a
winner's claim to that address; a replacement can be created and paid atomically.

IPO tests verify time validation and authenticated review outcomes. External
company facts remain an evidence-review responsibility. Price tests cover both
crossing directions, supported USD conversions, stale or substituted sources,
failed confirmations and expiry refunds.

Controlled source data and local auditor signatures establish execution
behavior. They do not prove live provider availability, factual IPO truth,
auditor independence, economic fair value or public-network deployment.
