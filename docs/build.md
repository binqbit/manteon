# Build and checks

Run from the Manteon repository root, with its nested dependency initialized:

```sh
git submodule update --init --recursive
bash scripts/check.sh --restore
```

Use Bash, Python 3.11+, rustup and a C build toolchain, including `pkg-config`
and OpenSSL development headers for host tooling. Initial restoration downloads
pinned compilers/tools and locked dependencies. Later runs reuse those inputs
and build offline. No wallet, validator, RPC, account funding or application
checkout is required.

| Command                                  | Work                                                                                       |
| ---------------------------------------- | ------------------------------------------------------------------------------------------ |
| `bash scripts/check.sh --restore`        | Restore all pinned inputs, then run core and Anchor checks.                                |
| `bash scripts/check.sh`                  | Run both suites with offline locked inputs.                                                |
| `bash scripts/check-core.sh`             | Core formatting, Clippy, Rust tests/example and independent Python references.             |
| `bash scripts/check-anchor.sh --restore` | Restore contract tools/dependencies, then build/check both SBF programs, IDL and VM tests. |
| `bash scripts/check-anchor.sh`           | Run the contract suite offline.                                                            |

The compiler graphs remain separate: the pure core is the root Cargo workspace;
the Anchor workspace includes the Rust account reader; LiteSVM has its own
workspace and lockfile. This prevents runtime dependencies from entering the
pure core.

| Input                     | Pin                    |
| ------------------------- | ---------------------- |
| Core Rust                 | 1.85.1                 |
| Anchor host Rust          | 1.89.0                 |
| LiteSVM test Rust         | 1.93.0                 |
| CLI bootstrap Rust        | 1.95.0                 |
| Anchor CLI                | 1.2.1                  |
| cargo-build-sbf           | 4.0.0                  |
| Solana SBF platform tools | v1.53, architecture v0 |
| LiteSVM                   | 0.16.0                 |

The checked build uses the default mainnet audit profile. `mainnet` and `devnet`
features select Tekmerion's public USDC identity; the Manteon program ID stays
fixed. A manual devnet build must explicitly retain the strict entrypoint:

```sh
cargo-build-sbf --manifest-path programs/manteon/Cargo.toml --no-default-features --features strict-entrypoint,devnet --tools-version v1.53 --arch v0
```

Use the corresponding Tekmerion profile when deploying it. Applications and
consumers must use matching network identities. The standalone checks build
locally and do not establish a deployment or execute public-network transfers.

Outputs are ignored under `target/`: SBF programs at
`target/deploy/manteon.so` and `target/deploy/tekmerion.so`, generated IDL at
`target/idl/manteon.json`, generated TypeScript types at
`target/types/manteon.ts`, and isolated host/build-tool directories. Generated
IDL is compared structurally with [idl/manteon.json](../idl/manteon.json).
The checked build rejects SBF compiler errors and unknown syscall diagnostics.

These outputs are development/check artifacts, without release provenance or
deployment keys. See [test coverage](../tests/README.md) and
[the actual check script](../scripts/check-anchor.sh).
