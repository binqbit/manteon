# Build and checks

Run the standalone checks from the Manteon repository root:

```sh
git submodule update --init --recursive
bash scripts/check.sh --restore
```

Use Bash, Python 3.11+, rustup and a C build toolchain, including `pkg-config`
and OpenSSL development headers for host tooling. The first restoration
downloads pinned compilers/tools and locked dependencies. Later runs reuse them
and build offline. These checks require no application checkout, wallet,
validator, RPC endpoint or funded account.

## Commands

| Command                                  | Work                                                                                                                         |
| ---------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `bash scripts/check.sh --restore`        | Restore pinned inputs, then run both suites.                                                                                 |
| `bash scripts/check.sh`                  | Run both suites with offline, locked inputs.                                                                                 |
| `bash scripts/check-core.sh`             | Check core formatting, Clippy, Rust tests/example and independent Python references.                                         |
| `bash scripts/check-anchor.sh --restore` | Restore contract tools/dependencies, then run the contract suite.                                                            |
| `bash scripts/check-anchor.sh`           | Check formatting/Clippy, build both SBF programs and the Manteon IDL, verify interface compatibility and run local VM tests. |

The Python references can also run independently of Rust:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/reference -p 'test_*.py' -v
```

## Reproducible tools

The scripts keep the pure calculation, Anchor and VM compiler environments
separate and select their pinned versions automatically.

| Tool                      | Version                |
| ------------------------- | ---------------------- |
| Core Rust                 | 1.85.1                 |
| Anchor host Rust          | 1.89.0                 |
| LiteSVM test Rust         | 1.93.0                 |
| CLI bootstrap Rust        | 1.95.0                 |
| Anchor CLI                | 1.2.1                  |
| cargo-build-sbf           | 4.0.0                  |
| Solana SBF platform tools | v1.53, architecture v0 |
| LiteSVM                   | 0.16.0                 |

## Network profile and outputs

The checked build uses the default mainnet profile. The `mainnet` and `devnet`
features select supported Raydium and USDC identities, including Tekmerion's
review-bond mint. The Manteon program identity stays fixed. A manual devnet
build must explicitly retain the strict entrypoint:

```sh
cargo-build-sbf --manifest-path programs/manteon/Cargo.toml \
  --sbf-out-dir target/devnet/deploy \
  --no-default-features --features strict-entrypoint,devnet \
  --tools-version v1.53 --arch v0
```

Use the corresponding Tekmerion profile when building it. Applications must
select matching network identities. A local build does not establish a public
network deployment or execute public-network transfers.

The explicit devnet output directory keeps the mainnet binaries separate.

Generated files are ignored under `target/`. Deployable program outputs are
`target/deploy/manteon.so` and `target/deploy/tekmerion.so`; the generated Manteon
interface is `target/idl/manteon.json` with TypeScript types at
`target/types/manteon.ts`. The checks compare generated and maintained IDL
structurally and reject SBF compiler errors and unknown-syscall diagnostics.

## What the tests establish

Core tests and independent numerical references check H30/H15/H7 coefficients,
withdrawal allowances, fees and payouts. Local LiteSVM tests execute the real
Manteon and pinned Tekmerion SBF programs with controlled clocks and funds,
including custody, review integration, lifecycle transitions, failed-transaction
rollback and duplicate-claim protection.

These tests establish local contract behavior. Controlled IPO evidence and
price-source fixtures do not establish factual IPO truth, live price-source
availability, auditor independence or deployed program identity.
