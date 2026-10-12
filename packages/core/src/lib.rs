//! Manteon's runtime-independent contract calculations.
//!
//! This crate provides value-only calculations, not a deployed Solana program,
//! account ABI, token custodian or source of event authority. The Anchor runtime
//! authenticates accounts, operation phase, signers and Clock,
//! and commits accounting changes together with verified token transfers.

#![no_std]

pub mod accounting;
pub mod constants;
pub mod error;
pub mod math;
pub mod types;
pub mod validation;

pub use error::DomainError;
