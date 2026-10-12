//! Deterministic domain tests; no Solana runtime or token CPI is simulated.

mod support;

#[path = "unit/allowance.rs"]
mod allowance;
#[path = "unit/amounts.rs"]
mod amounts;
#[path = "unit/principal.rs"]
mod principal;
#[path = "unit/time.rs"]
mod time;
