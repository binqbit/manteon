//! Irreversible settlement: freeze, process each Position, then enable claims.
pub(crate) mod begin_settlement;
pub(crate) mod finalize_settlement;
pub(crate) mod process_positions;
