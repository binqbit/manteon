# Manteon

Manteon is a Solana protocol for forecasts about **when an event will happen**.
Participants choose a UTC date and commit capital to a pool. Once the event is
accepted, the protocol distributes the remaining capital according to each
forecast's accuracy and stake size.

Pools support company IPO dates and token price threshold crossings. IPO facts
use the separate [Tekmerion review protocol](vendor/tekmerion/docs/concept.md).
Price crossings use fixed, authenticated on-chain sources. Both paths share
participation, custody, fees and payout accounting.

Manteon is independent of any application. An application presents pools,
evidence and documents and prepares wallet transactions. The protocol enforces
the agreed schedule, signatures, transfers and settlement rules.

## Read the protocol

| Guide                                      | What it explains                                                    |
| ------------------------------------------ | ------------------------------------------------------------------- |
| [Concept](docs/concept.md)                 | What a funded date forecast means and how accuracy affects capital. |
| [How it works](docs/how-it-works.md)       | Participation, event acceptance, settlement and lifecycle diagrams. |
| [Protocol specification](docs/protocol.md) | Exact time boundaries, withdrawal rules and payout arithmetic.      |
| [Event rules](docs/events.md)              | IPO evidence review, price confirmation and source safeguards.      |
| [Configuration](docs/configuration.md)     | Service and governance roles, fixed fees and reusable settings.     |
| [Document commitments](docs/agreements.md) | How documents bind rules and results, and how to verify them.       |
| [Build and checks](docs/build.md)          | Standalone build and test commands.                                 |

## Run checks

From this repository's root:

```sh
git submodule update --init --recursive
bash scripts/check.sh --restore
```

The first run restores pinned tools and locked dependencies. Later runs use
`bash scripts/check.sh` with those inputs offline. The checks build both
protocols and execute local tests; they require no wallet, validator, RPC
endpoint or funded account. See [build requirements](docs/build.md).
