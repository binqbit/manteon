# Manteon Rust account reader

`manteon-client` validates supplied account snapshots and re-exports the
program's account, instruction, state and result types. It shares the Anchor
workspace and lockfile; it performs no RPC request, signing or transaction
submission.

Use `read_pool(pool, config, event, vault)` with `AccountInfo` snapshots acquired
from the intended network and a consistent read checkpoint. It checks owner,
complete layout, canonical addresses, reciprocal membership, lifecycle
accounting and vault backing. `PoolView` returns decoded Config/Pool/Event,
observed vault balance and unallocated balance. Donations are separate from
participating principal and confer no withdrawal right.

`read_price_monitor(pool_key, pool, event, monitor_account)` validates the
optional canonical monitor against a validated price Pool/Event pair from the
same bank. `monitor_account` is `Option<&AccountInfo>`: absence is valid only
for a Draft; activated price pools require their monitor account. The crate
also re-exports the program's price reader for consumers that acquire
authenticated source snapshots.

These checks do not establish RPC honesty, finality, document integrity or
Tekmerion Policy readiness. The acquisition layer owns network/commitment
selection; the contract still validates every transaction when executed.

Add a path dependency on this package and select the matching `mainnet` or
`devnet` feature. Consumers must compile with the program's compatible Anchor
and Solana types. See [Cargo.toml](Cargo.toml), [public API](src/lib.rs) and the
[protocol specification](../../docs/protocol.md). The standalone
`bash scripts/check-anchor.sh` at repository root checks this workspace and its
VM integration.
