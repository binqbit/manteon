# Manteon concept

Manteon lets people back a forecast of an event's date with capital. It serves
analysts, researchers and participants who have a timing estimate and want its
accuracy to matter when the event happens.

Each pool fixes the event, collateral, participation schedule and accuracy curve
before deposits begin. Participants choose a UTC date and stake tokens. Once
the event is accepted, closer forecasts give each unit of capital more weight
in the distribution. H30, H15 and H7 halve ideal accuracy weight every 30, 15 or
7 days of error. The chosen curve never changes for an existing pool.

The capital distribution shows funded forecasts. It is not a calibrated
probability, and neither a large stake nor a close forecast guarantees profit.
Payouts include any returned principal; participants can lose money. Positive
profit alone pays the pool's fixed fee, capped at 20%, with the platform share
included in that fee. Transaction costs are separate.

Two event mechanisms share participation and payout accounting:

- **Company IPO:** a proposed actual date is reviewed under committed evidence
  rules through Tekmerion. An underfunded challenge may accept a proposal
  optimistically; a funded challenge requires the appointed auditors' verdict.
- **Token price threshold crossing:** anyone can confirm a fixed Raydium CPMM
  rule from authenticated prices. The recorded time is successful on-chain
  confirmation, which can follow the underlying price move.

A deposit creates a 48-hour withdrawal allowance for new capital, ending no
later than entry close. After the horizon, owners can exit while the pool
remains unresolved and Active. Exited capital is removed from subsequent
distribution. An accepted event before entry close returns principal; expired
price monitoring also enables principal refunds.

Applications make rules and evidence readable and prepare transactions. The
program independently checks signatures, dates, source accounts, review
bindings, balances and entitlement consumption. Saved documents can be
verified against chain commitments, and unavailable archives do not remove
eligible withdrawal, exit or claim rights.

See [how it works](how-it-works.md) for the flow and
[the specification](protocol.md) for exact boundaries and arithmetic.
