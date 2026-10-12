# Manteon

Manteon is a Solana protocol for forecasts about when an event will happen.
Participants choose a UTC date and commit capital. After the event is accepted,
the protocol distributes the remaining pool according to capital and forecast
accuracy. An application supplies discovery, evidence, documents and a wallet
interface; the program enforces participation, custody and settlement.

The repository contains the Anchor program, a pure Rust calculation library,
a Rust account reader and local contract tests. Company IPO results use the
pinned [Tekmerion](vendor/tekmerion/README.md) review protocol. Token price
crossings use authenticated on-chain sources and need no audit.

## Read the protocol

| Guide                                      | Contents                                                              |
| ------------------------------------------ | --------------------------------------------------------------------- |
| [Concept](docs/concept.md)                 | Purpose, participation and the meaning of a funded forecast.          |
| [How it works](docs/how-it-works.md)       | Operations, transaction sequences and account state diagrams.         |
| [Protocol specification](docs/protocol.md) | Accounts, addresses, instructions, dates and exact payout arithmetic. |
| [Event rules](docs/events.md)              | IPO review and fixed price-source verification.                       |
| [Configuration](docs/configuration.md)     | Roles, retained settings, profit fees and platform treasury.          |
| [Document commitments](docs/agreements.md) | Agreement integrity, typed results and verification boundaries.       |
| [Build and checks](docs/build.md)          | Pinned tools, standalone commands and outputs.                        |
| [Tests](tests/README.md)                   | Core, independent references and real SBF execution in LiteSVM.       |

## Run checks

Initialize the nested dependency and run from this repository's root:

```sh
git submodule update --init --recursive
bash scripts/check.sh --restore
```

The first run installs pinned tools and restores locked dependencies. Later
`bash scripts/check.sh` runs use offline locked builds. Checks require Bash,
Python 3.11+, rustup and a C build toolchain; they require no wallet, validator,
RPC endpoint or funded account. They build both programs and the Manteon IDL
and execute local tests. They do not deploy contracts.

## Repository layout

| Path                                  | Ownership                                                            |
| ------------------------------------- | -------------------------------------------------------------------- |
| `programs/manteon/`                   | Anchor accounts, validation, token custody and event adapters.       |
| `packages/core/`                      | Checked arithmetic and accounting, without RPC or account ownership. |
| `packages/client-rust/`               | Validated account reads and program type re-exports.                 |
| `idl/manteon.json`                    | Maintained Anchor interface, checked against generated output.       |
| `tests/manteon/`                      | Real Manteon and Tekmerion SBF execution in LiteSVM.                 |
| `tests/reference/`, `tests/fixtures/` | Independent numerical models and shared vectors.                     |
| `vendor/tekmerion/`                   | Pinned review protocol and its SDKs.                                 |
| `scripts/`                            | Standalone build and test checks.                                    |

The program ID is `MNTxcR8N6rWisHYbt73SPyvBZqQUQiW1NKhGa3N1etM`.
Source and interface describe supported behavior; a program ID alone does not
establish deployment on a particular network. Integrators must authenticate
their network, account owners and the program versions they use.
