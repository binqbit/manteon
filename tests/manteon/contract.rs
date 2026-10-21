//! Manteon contract scenarios executed against local SBF programs.

mod support;

#[path = "cases/activation.rs"]
mod activation;
#[path = "cases/audit_guards.rs"]
mod audit_guards;
#[path = "cases/audited.rs"]
mod audited;
#[path = "cases/batches.rs"]
mod batches;
#[path = "cases/claims.rs"]
mod claims;
#[path = "cases/config.rs"]
mod config;
#[path = "cases/config_usage.rs"]
mod config_usage;
#[path = "cases/custody.rs"]
mod custody;
#[path = "cases/direct_audit.rs"]
mod direct_audit;
#[path = "cases/encoding.rs"]
mod encoding;
#[path = "cases/fallback.rs"]
mod fallback;
#[path = "cases/global_config.rs"]
mod global_config;
#[path = "cases/idl.rs"]
mod idl;
#[path = "cases/multiple_cases.rs"]
mod multiple_cases;
#[path = "cases/participation.rs"]
mod participation;
#[path = "cases/participation_guards.rs"]
mod participation_guards;
#[path = "cases/pool.rs"]
mod pool;
#[path = "cases/price_threshold.rs"]
mod price_threshold;
#[path = "cases/reads.rs"]
mod reads;
#[path = "cases/results.rs"]
mod results;
#[path = "cases/retries.rs"]
mod retries;
#[path = "cases/settlement.rs"]
mod settlement;
#[path = "cases/setup.rs"]
mod setup;
