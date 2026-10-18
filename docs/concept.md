# Manteon concept

Manteon lets people put capital behind a forecast of an event's date. A pool
brings those forecasts together and rewards a combination of **capital committed
and closeness to the accepted event date**.

An analyst might forecast when a company will complete an IPO. A token holder
might forecast when a fixed market price will cross a target. In each case,
participants answer a timing question by choosing a UTC calendar date.

## What participants agree to

Before participation begins, each pool fixes its event rule, collateral token,
opening time, entry close, horizon, accuracy curve and fee terms. It also
commits to a rules document. Those terms cannot be edited for that pool.

The opening and entry close define when capital can enter. The horizon defines
when owners can recover capital from a pool that remains unresolved and Active.
For price events, it is also the last possible confirmation time. External event review
can continue beyond the horizon.

Owners may back several dates. Capital on the same date is grouped for
settlement, but each owner's stake remains a separate entitlement.

## How accuracy affects a payout

Each unit of capital receives more weight when its chosen date is closer to the
accepted UTC event date. The error is the absolute number of calendar days, so
early and late forecasts with the same error receive the same coefficient.

The pool selects one of three curves:

| Curve | Additional error that halves ideal accuracy weight |
| ----- | -------------------------------------------------: |
| H30   |                                            30 days |
| H15   |                                            15 days |
| H7    |                                             7 days |

H7 puts more emphasis on precise dates; H30 reduces weight more gradually. The
curve belongs to the pool and stays fixed. Every additional half-life halves
ideal weight again: one half-life gives one-half weight, two give one-quarter.

Suppose an H15 pool contains two equal stakes of 100 tokens. One forecast is
exact and the other misses by 15 days. Their weights are in a two-to-one ratio,
so the 200-token pool allocates approximately 133 and 67 tokens before fees and
token-unit rounding. The second owner loses capital even though the forecast
was only fifteen days away.

A larger stake can outweigh a more accurate smaller stake. Payouts include any
returned principal, and a close forecast does not guarantee a profit. The pool
shows funded forecasts; interpreting them as calibrated probabilities requires
an additional model.

## How the event becomes accepted

**External event.** A proposer reports when the event defined by the Agreement
actually occurred and deposits a separate review bond. IPO is one possible Agreement template. Tekmerion gives participants
time to fund a challenge. An underfunded challenge can accept the report
optimistically; a fully funded challenge requires agreement among the appointed
auditors. External evidence and the committed agreement define the question.

**Token price threshold crossing.** Any wallet can submit the fixed price
sources. Manteon requires both an interval average and the current spot price
to satisfy the target, with the full uncertainty bounds on the required side.
The event time is the successful on-chain confirmation time, which can follow
the underlying market move. This path uses no review bond or auditors.

## Keeping or recovering capital

New capital receives a 48-hour withdrawal allowance, usable only before entry
closes and while the pool remains Active. A top-up during an existing allowance
does not extend its deadline. After it expires, a new deposit gives an allowance
for that new amount alone.

At the horizon, owners can exit with their remaining principal while the pool
is Active, including during pending external event review. Exited capital leaves the
future distribution. Once an accepted result freezes the pool for settlement,
withdrawals and exits stop.

An accepted occurrence before entry close returns principal instead of running
a competition. Expired price monitoring also enables principal refunds. These
rights require explicit owner transactions; reaching a date does not move money.

The fixed profit fee applies only to positive profit and is capped at 20%. The
platform receives an agreed share of that fee. Losses, withdrawals, exits and
principal refunds incur no protocol profit fee; transaction costs are separate.

Applications present readable rules, evidence and wallet actions. The protocol
checks each transaction against its recorded terms. Document availability and
the service's responsiveness do not remove eligible owner withdrawal, exit or
claim rights.

See [how it works](how-it-works.md) for the lifecycle and
[the specification](protocol.md) for exact boundaries and arithmetic.
