# Manteon Rust account reader

`manteon-client` helps Rust applications interpret Manteon account snapshots
without trusting deserialization alone. It validates the relationships among a
pool, its configuration, event and collateral vault, then returns their recorded
facts and observed backing balance.

The reader checks program ownership, complete account layouts, canonical
identities, reciprocal membership, lifecycle accounting and vault backing.
Extra vault tokens are reported separately from recorded liabilities. Donations
confer no stake or withdrawal right.

Price monitoring has an additional read for the retained observation baseline.
Acquire it at the same checkpoint as the pool and event. A Draft can lack a
monitor; activated price pools retain one, with a baseline while Active and
none after confirmation or expiry. General pool and claim reads do not require
that monitoring account.

## Integration and limits

The application acquires all snapshots from its intended network at a consistent
checkpoint and chosen commitment. Local validation does not establish RPC
honesty, network finality, document integrity or an accepted Tekmerion review.
The pool's review-policy reference alone does not establish policy readiness.
Every submitted transaction is validated again by the contract when executed.

The package performs no RPC request, signing or transaction submission. It also
exposes the protocol's instruction, state and canonical-result types and its
price-source reader, so consumers can use compatible representations.

Add it as a path dependency and select the network profile matching the program.
The default is `mainnet`; a `devnet` consumer must disable the default feature
and select `devnet`. Use compatible Anchor and Solana types. See
[the protocol specification](../../docs/protocol.md) and
[event rules](../../docs/events.md) for the meaning of the returned facts.

The package shares the Anchor workspace. From the Manteon repository root,
`bash scripts/check-anchor.sh` checks it and its local VM integration. See
[build requirements](../../docs/build.md) for first-time restoration.
