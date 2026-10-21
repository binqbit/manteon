use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use manteon_core::validation::time::Schedule;

use crate::{
    error::ManteonError,
    state::{Config, Event, EventDefinition, Pool, PoolAlgorithm, PoolState},
};

#[derive(Accounts)]
pub struct CreatePool<'info> {
    // Signers.
    pub service_authority: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,

    // Market accounts.
    #[account(
        mut,
        has_one = service_authority @ ManteonError::UnauthorizedServiceAuthority,
        has_one = collateral_mint @ ManteonError::InvalidCollateralMint
    )]
    pub config: Account<'info, Config>,
    #[account(
        init,
        payer = payer,
        space = Pool::DISCRIMINATOR.len() + Pool::INIT_SPACE
    )]
    pub pool: Account<'info, Pool>,
    #[account(
        init,
        payer = payer,
        space = Event::DISCRIMINATOR.len() + Event::INIT_SPACE,
        seeds = [Event::SEED, pool.key().as_ref()],
        bump
    )]
    pub event: Account<'info, Event>,

    // Prediction custody.
    /// Initialized legacy SPL collateral selected by Config.
    pub collateral_mint: Account<'info, Mint>,
    /// CHECK: Only the canonical PDA address is used as token authority; no data is read.
    #[account(
        seeds = [Pool::VAULT_AUTHORITY_SEED, pool.key().as_ref()],
        bump
    )]
    pub vault_authority: UncheckedAccount<'info>,
    #[account(
        init,
        payer = payer,
        seeds = [Pool::VAULT_SEED, pool.key().as_ref()],
        bump,
        token::mint = collateral_mint,
        token::authority = vault_authority,
        token::token_program = token_program
    )]
    pub vault: Account<'info, TokenAccount>,

    // Programs.
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

// Keep the handler aligned with the flat Anchor instruction ABI.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle(
    ctx: Context<CreatePool>,
    event: EventDefinition,
    algorithm: PoolAlgorithm,
    agreement_hash: [u8; 32],
    expected_config_hash: [u8; 32],
    opens_at: i64,
    lock_at: i64,
    horizon_at: i64,
) -> Result<()> {
    require!(
        ctx.accounts.config.terms_hash() == expected_config_hash,
        ManteonError::ConfigTermsChanged
    );
    Schedule {
        opens_at,
        lock_at,
        horizon_at,
    }
    .validate_creation(Clock::get()?.unix_timestamp)
    .map_err(|_| error!(ManteonError::InvalidPoolSchedule))?;
    require!(
        manteon_core::math::utc_day::utc_day(horizon_at)
            - manteon_core::math::utc_day::utc_day(lock_at)
            <= algorithm.max_error_days(),
        ManteonError::UnsupportedDate
    );
    require!(agreement_hash != [0; 32], ManteonError::InvalidAgreement);
    if let EventDefinition::TokenPriceThresholdCrossing { params } = event {
        params.validate()?;
    }

    // External identity and occurrence facts are evaluated against the agreement.
    // Price activation validates its tracked Mint and fixed on-chain sources.
    ctx.accounts.pool.set_inner(Pool {
        config: ctx.accounts.config.key(),
        event_id: ctx.accounts.event.key(),
        collateral_mint: ctx.accounts.config.collateral_mint,
        audit_config: ctx.accounts.config.audit_config,
        fee_recipient: ctx.accounts.config.fee_recipient,
        profit_fee_bps: ctx.accounts.config.profit_fee_bps,
        platform_share_bps: ctx.accounts.config.platform_share_bps,
        config_released: false,
        agreement_hash,
        algorithm,
        opens_at,
        lock_at,
        horizon_at,
        state: PoolState::Draft,
        accepted_case: None,
        principal: 0,
        active_dates: 0,
        processed_dates: 0,
        total_weight: 0,
        payout_remaining: 0,
        claimed_principal: 0,
        service_eligible_at: None,
        public_proposals: false,
    });

    ctx.accounts.event.set_inner(Event {
        pool: ctx.accounts.pool.key(),
        definition: event,
        result: None,
    });
    ctx.accounts.config.retain()?;
    Ok(())
}
