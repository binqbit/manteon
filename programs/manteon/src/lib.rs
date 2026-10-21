//! Manteon Market: scheduled participation, authenticated event acceptance,
//! irreversible accuracy-weighted settlement and owner-authorized collateral claims.
#![allow(clippy::diverging_sub_expression)]

use anchor_lang::prelude::*;

mod custody;
pub mod error;
mod instructions;
pub mod integration;
pub mod result;
pub mod state;
mod validation;

#[cfg(all(not(feature = "strict-entrypoint"), not(feature = "no-entrypoint")))]
compile_error!("enable strict-entrypoint for deployment or no-entrypoint for library use");
#[cfg(feature = "strict-entrypoint")]
mod entrypoint;

pub use instructions::{
    claim::Claim,
    create_config::CreateConfig,
    global_config::{AcceptGlobalAdmin, ManageGlobalConfig, WithdrawPlatformFees},
    initialize_global_config::InitializeGlobalConfig,
    participation::{exit::Exit, stake::Stake, unstake::Unstake},
    pool::complete_pool::CompletePool,
    pool::{activate_pool::ActivatePool, create_pool::CreatePool},
    price_threshold::{ActivatePricePool, PriceThresholdPool},
    propose_external_event::{ProposalMode, ProposeExternalEvent},
    settlement::{
        begin_settlement::BeginSettlement, finalize_settlement::FinalizeSettlement,
        process_positions::ProcessPositions,
    },
    update_config::UpdateConfig,
};

pub(crate) use instructions::{
    claim::__client_accounts_claim,
    create_config::__client_accounts_create_config,
    global_config::{
        __client_accounts_accept_global_admin, __client_accounts_manage_global_config,
        __client_accounts_withdraw_platform_fees,
    },
    initialize_global_config::__client_accounts_initialize_global_config,
    participation::{
        exit::__client_accounts_exit, stake::__client_accounts_stake,
        unstake::__client_accounts_unstake,
    },
    pool::complete_pool::__client_accounts_complete_pool,
    pool::{
        activate_pool::__client_accounts_activate_pool, create_pool::__client_accounts_create_pool,
    },
    price_threshold::{
        __client_accounts_activate_price_pool, __client_accounts_price_threshold_pool,
    },
    propose_external_event::__client_accounts_propose_external_event,
    settlement::{
        begin_settlement::__client_accounts_begin_settlement,
        finalize_settlement::__client_accounts_finalize_settlement,
        process_positions::__client_accounts_process_positions,
    },
    update_config::__client_accounts_update_config,
};

use state::EventResult;

declare_id!("MNTxcR8N6rWisHYbt73SPyvBZqQUQiW1NKhGa3N1etM");

/// Typed Anchor entrypoints; handlers own validation, CPI and atomic transitions.
#[allow(unsafe_code)]
#[program]
pub mod manteon {
    use super::*;

    pub fn initialize_global_config(
        ctx: Context<InitializeGlobalConfig>,
        admin: Pubkey,
        fee_authority: Pubkey,
        platform_share_bps: u16,
    ) -> Result<()> {
        instructions::initialize_global_config::handle(
            ctx,
            admin,
            fee_authority,
            platform_share_bps,
        )
    }

    pub fn update_global_config(
        ctx: Context<ManageGlobalConfig>,
        fee_authority: Pubkey,
        platform_share_bps: u16,
    ) -> Result<()> {
        instructions::global_config::update(ctx, fee_authority, platform_share_bps)
    }

    pub fn propose_global_admin(ctx: Context<ManageGlobalConfig>, new_admin: Pubkey) -> Result<()> {
        instructions::global_config::propose_admin(ctx, new_admin)
    }

    pub fn accept_global_admin(ctx: Context<AcceptGlobalAdmin>) -> Result<()> {
        instructions::global_config::accept_admin(ctx)
    }

    pub fn withdraw_platform_fees(ctx: Context<WithdrawPlatformFees>, amount: u64) -> Result<()> {
        instructions::global_config::withdraw(ctx, amount)
    }

    pub fn create_config(
        ctx: Context<CreateConfig>,
        service_authority: Pubkey,
        audit_config: Pubkey,
        fee_recipient: Pubkey,
        profit_fee_bps: u16,
        expected_platform_share_bps: u16,
    ) -> Result<()> {
        instructions::create_config::handle(
            ctx,
            service_authority,
            audit_config,
            fee_recipient,
            profit_fee_bps,
            expected_platform_share_bps,
        )
    }

    pub fn update_config(
        ctx: Context<UpdateConfig>,
        service_authority: Pubkey,
        audit_config: Pubkey,
        fee_recipient: Pubkey,
        profit_fee_bps: u16,
        expected_platform_share_bps: u16,
    ) -> Result<()> {
        instructions::update_config::handle(
            ctx,
            service_authority,
            audit_config,
            fee_recipient,
            profit_fee_bps,
            expected_platform_share_bps,
        )
    }

    pub fn complete_pool(ctx: Context<CompletePool>) -> Result<()> {
        instructions::pool::complete_pool::handle(ctx)
    }

    // Flat fields are the reviewed Anchor instruction ABI.
    #[allow(clippy::too_many_arguments)]
    pub fn create_pool(
        ctx: Context<CreatePool>,
        event: state::EventDefinition,
        algorithm: state::PoolAlgorithm,
        agreement_hash: [u8; 32],
        expected_config_hash: [u8; 32],
        opens_at: i64,
        lock_at: i64,
        horizon_at: i64,
    ) -> Result<()> {
        instructions::pool::create_pool::handle(
            ctx,
            event,
            algorithm,
            agreement_hash,
            expected_config_hash,
            opens_at,
            lock_at,
            horizon_at,
        )
    }

    pub fn activate_pool(ctx: Context<ActivatePool>) -> Result<()> {
        instructions::pool::activate_pool::handle(ctx)
    }

    pub fn activate_price_pool(ctx: Context<ActivatePricePool>) -> Result<()> {
        instructions::price_threshold::activate(ctx)
    }

    pub fn record_price_observation(ctx: Context<PriceThresholdPool>) -> Result<()> {
        instructions::price_threshold::observe(ctx)
    }

    pub fn finalize_price_threshold(ctx: Context<PriceThresholdPool>) -> Result<()> {
        instructions::price_threshold::finalize(ctx)
    }

    pub fn expire_price_observation(ctx: Context<PriceThresholdPool>) -> Result<()> {
        instructions::price_threshold::expire(ctx)
    }

    pub fn stake(ctx: Context<Stake>, forecast_day: i64, amount: u64) -> Result<()> {
        instructions::participation::stake::handle(ctx, forecast_day, amount)
    }

    pub fn unstake(ctx: Context<Unstake>, amount: u64) -> Result<()> {
        instructions::participation::unstake::handle(ctx, amount)
    }

    pub fn exit(ctx: Context<Exit>) -> Result<()> {
        instructions::participation::exit::handle(ctx)
    }

    pub fn propose_external_event<'info>(
        ctx: Context<'info, ProposeExternalEvent<'info>>,
        nonce: u64,
        data: EventResult,
        mode: ProposalMode,
    ) -> Result<()> {
        instructions::propose_external_event::handle(ctx, nonce, data, mode)
    }

    /// Consume authenticated Yes and freeze atomically; pre-LOCK events authorize refunds.
    pub fn begin_settlement(ctx: Context<BeginSettlement>, data: EventResult) -> Result<()> {
        instructions::settlement::begin_settlement::handle(ctx, data)
    }

    /// Remaining accounts are one to eight distinct writable canonical Positions.
    pub fn process_positions<'info>(ctx: Context<'info, ProcessPositions<'info>>) -> Result<()> {
        instructions::settlement::process_positions::handle(ctx)
    }

    pub fn finalize_settlement(ctx: Context<FinalizeSettlement>) -> Result<()> {
        instructions::settlement::finalize_settlement::handle(ctx)
    }

    pub fn claim(ctx: Context<Claim>) -> Result<()> {
        instructions::claim::handle(ctx)
    }
}
