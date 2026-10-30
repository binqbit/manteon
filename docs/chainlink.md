# Chainlink review for external-event results

Chainlink is Manteon's preferred integration for automated external-event appeals. Through
Tekmerion, a market can appoint an AI-assisted auditor running on Chainlink
instead of a human committee. The question stays the same: **does the proposed
event result satisfy this market's original agreement?**

The integration is implemented for local testing. Public enrollment remains
closed until a live confidential deployment is qualified. This guide describes
the implemented review and acceptance rules; it does not announce an active
public service.

## How it fits a market

The Chainlink reviewer obtains the original proposal, verifies its documents
and evaluates it under the research rules agreed in advance. Its authenticated
answer becomes a Tekmerion decision; trading and payout math stay the same.
Confirmed temporary technical failures can permit another execution within the
agreed limit and original deadline. A negative verdict is a completed audit,
not a technical failure. See [how the audit is verified](../vendor/tekmerion/docs/confidential-auditor.md)
for the safeguards and conditions for public use.

The auditor and review terms are selected before a market opens and are locked
when it is activated. Participants can read the agreement and see which review
process applies before committing capital.

1. The service proposes the external event's occurrence time. The market's
   fixed policy determines how audit starts: an immediate policy requires the
   audit fee for every new Case, including the service's first proposal; a
   dispute policy starts with an author bond and waits for challenge funding.
2. The automated auditor reviews a funded proposal against the fixed agreement
   and evidence sources. Under a dispute policy, eligible authors can also
   voluntarily pay for a new direct audit after public submission opens.
3. Tekmerion records the authenticated decision. An accepted Yes must then be
   applied to Manteon before settlement or refunds can proceed.
4. A No, Unknown or technical timeout does not accept or replace an event
   result. While no result has been accepted, eligible authors can submit a
   fresh proposal. Earlier review claims remain available.

Only a dispute policy permits optimistic acceptance when a challenge remains
underfunded at its deadline. Such a result has **not been checked by AI**.
Immediate policies require an audited result and cannot use that path. The
interface selects the review method from the Policy, independently of auditor
identity. Other authors always follow the public-submission conditions.
Direct-audit Cases require an audited verdict; their fee is refunded only on
technical timeout. Authors can have independent concurrent Cases once public
submission opens. The first finalized Yes applied to the market fixes its result;
remaining Cases complete their own reviews and claims without overriding it.
The full [external-event rules](events.md#external-event) explain acceptance and timing.

## The agreement is the audit's reference

The agreement identifies the exact event and entities, what counts as occurrence,
the permitted source links, the UTC timestamp rule and any additional conditions.
For an IPO, for example, it must identify the company and instrument and distinguish
a completed offering from an announcement or planned date. Insufficient or conflicting evidence calls for Unknown.

Follow Tekmerion's [guide to writing audit rules](../vendor/tekmerion/docs/chainlink.md#write-rules-an-auditor-can-apply).
The [document commitment](agreements.md) prevents the agreed text from being
silently replaced. The audit commits a final Markdown report with its assessment
and source links; it does not archive every consulted page or the full dialogue.
These controls make the decision inspectable; they do not
guarantee that every source or AI interpretation is correct.

## What participants retain

Review bonds and contributions stay separate from forecast capital. The auditor
does not choose payout formulas, move a participant's position or change the
market's schedule. Existing withdrawal, horizon-exit and claim rights continue
to follow the protocol rules. Token price events keep their own on-chain
verification and do not use this external-event auditor.

Existing markets cannot silently switch auditors or evaluation rules.
See [Tekmerion's Chainlink guide](../vendor/tekmerion/docs/chainlink.md)
for the review safeguards and privacy boundary; Manteon uses that mechanism
without a second audit system.
