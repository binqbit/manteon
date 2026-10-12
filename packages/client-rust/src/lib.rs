//! Client-side validation of Manteon account snapshots.
//!
//! Reuses the program's Anchor types and instruction builders. Snapshot checks
//! do not establish RPC provenance, network finality or auditor readiness.

pub mod error;
mod price_monitor;
mod read;

pub use error::ReadError;
pub use manteon::integration::price;
pub use manteon::{accounts, instruction, result, state, ID};
pub use price_monitor::read_price_monitor;
pub use read::{read_pool, PoolView};
