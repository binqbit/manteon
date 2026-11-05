use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use manteon_core::{
    accounting::principal::PrincipalBook,
    math::{
        fees::{quote_profit_fees, ProfitFees},
        premium::{self, Weight},
        utc_day::{day_error, utc_day},
    },
    types::Amount,
};

use crate::{
    custody::token,
    error::ManteonError,
    state::{Config, Event, GlobalConfig, Pool, PoolState, Position, Stake, StakeState},
};

#[derive(Accounts)]
pub struct Claim<'info> {
    // Signers.
    pub owner: Signer<'info>,

    // Protocol accounts.
    #[account(
        mut,
        constraint = pool.event_id == event.key()
    )]
    pub pool: Box<Account<'info, Pool>>,
    #[account(
        seeds = [Event::SEED, pool.key().as_ref()],
        bump,
        has_one = pool
    )]
    pub event: Box<Account<'info, Event>>,
    #[account(
        mut,
        seeds = [Position::SEED, pool.key().as_ref(), &position.forecast_day.to_le_bytes()],
        bump,
        has_one = pool
    )]
    pub position: Box<Account<'info, Position>>,
    #[account(
        mut,
        seeds = [Stake::SEED, position.key().as_ref(), owner.key().as_ref()],
        bump,
        has_one = owner,
        has_one = position
    )]
    pub stake: Box<Account<'info, Stake>>,

    #[account(mut, address = pool.config)]
    pub config: Box<Account<'info, Config>>,

    // Token custody.
    #[account(address = pool.collateral_mint)]
    pub collateral_mint: Box<Account<'info, Mint>>,
    #[account(mut)]
    pub destination: Box<Account<'info, TokenAccount>>,
    /// CHECK: canonical transfer signer, no data is read.
    #[account(
        seeds = [Pool::VAULT_AUTHORITY_SEED, pool.key().as_ref()],
        bump
    )]
    pub vault_authority: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [Pool::VAULT_SEED, pool.key().as_ref()],
        bump
    )]
    pub vault: Box<Account<'info, TokenAccount>>,
    #[account(mut, dup)]
    pub fee_recipient: Option<Account<'info, TokenAccount>>,
    #[account(
        mut,
        seeds = [GlobalConfig::VAULT_SEED, collateral_mint.key().as_ref()],
        bump
    )]
    pub platform_fee_vault: Box<Account<'info, TokenAccount>>,

    // Programs.
    pub token_program: Program<'info, Token>,
}

pub(crate) fn handle(ctx: Context<Claim>) -> Result<()> {
    validate_entitlement(&ctx.accounts.pool, &ctx.accounts.stake, &ctx.accounts.event)?;
    let occurred_at = ctx.accounts.event.result.map(|result| result.occurred_at());
    let payment = PreparedClaim::prepare(
        &ctx.accounts.pool,
        &ctx.accounts.position,
        &ctx.accounts.stake,
        occurred_at,
    )?;
    execute_claim(
        payment,
        &mut ctx.accounts.pool,
        &mut ctx.accounts.position,
        &mut ctx.accounts.stake,
        &mut ctx.accounts.config,
        &ctx.accounts.owner,
        &ctx.accounts.collateral_mint,
        &mut ctx.accounts.destination,
        &ctx.accounts.vault_authority,
        &mut ctx.accounts.vault,
        ctx.accounts.fee_recipient.as_mut(),
        &mut ctx.accounts.platform_fee_vault,
        ctx.bumps.vault_authority,
    )
}

pub(crate) fn validate_entitlement(pool: &Pool, stake: &Stake, event: &Event) -> Result<()> {
    require!(
        stake.state == StakeState::Active && stake.principal > 0,
        ManteonError::InvalidStake
    );
    require!(
        event.result.is_some()
            || (pool.state == PoolState::Refunding
                && matches!(
                    event.definition,
                    crate::state::EventDefinition::TokenPriceThresholdCrossing { .. }
                )),
        ManteonError::InvalidEventResult
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_claim<'info>(
    payment: PreparedClaim,
    pool: &mut Account<'info, Pool>,
    position: &mut Account<'info, Position>,
    stake: &mut Account<'info, Stake>,
    config: &mut Account<'info, Config>,
    owner: &Signer<'info>,
    collateral_mint: &Account<'info, Mint>,
    destination: &mut Account<'info, TokenAccount>,
    vault_authority: &UncheckedAccount<'info>,
    vault: &mut Account<'info, TokenAccount>,
    fee_recipient: Option<&mut Account<'info, TokenAccount>>,
    platform_fee_vault: &mut Account<'info, TokenAccount>,
    vault_bump: u8,
) -> Result<()> {
    token::validate_owner_account(destination, &owner.key(), &pool.collateral_mint)?;
    token::validate_vault(
        vault,
        &pool.collateral_mint,
        &vault_authority.key(),
        payment.liability,
    )?;
    if payment.fees.recipient.get() > 0 {
        let recipient = fee_recipient
            .as_ref()
            .ok_or(ManteonError::InvalidFeeRecipient)?;
        require_keys_eq!(
            recipient.owner,
            pool.fee_recipient,
            ManteonError::InvalidFeeRecipient
        );
        require_keys_neq!(
            recipient.key(),
            vault.key(),
            ManteonError::InvalidFeeRecipient
        );
        require_keys_neq!(
            recipient.key(),
            platform_fee_vault.key(),
            ManteonError::InvalidFeeRecipient
        );
        require!(
            recipient.mint == pool.collateral_mint
                && recipient.state
                    == anchor_spl::token::spl_token::state::AccountState::Initialized,
            ManteonError::InvalidFeeRecipient
        );
    }
    token::validate_vault(
        platform_fee_vault,
        &pool.collateral_mint,
        &GlobalConfig::address(),
        0,
    )?;
    let pool_key = pool.key();
    let seeds: &[&[u8]] = &[Pool::VAULT_AUTHORITY_SEED, pool_key.as_ref(), &[vault_bump]];
    token::transfer_exact(
        collateral_mint,
        vault,
        destination,
        vault_authority.to_account_info(),
        &[seeds],
        payment.fees.net.get(),
    )?;
    if payment.fees.recipient.get() > 0 {
        // The owner may also be the configured recipient; reload aliased balances.
        let recipient = fee_recipient.ok_or(ManteonError::InvalidFeeRecipient)?;
        recipient.reload()?;
        token::transfer_exact(
            collateral_mint,
            vault,
            recipient,
            vault_authority.to_account_info(),
            &[seeds],
            payment.fees.recipient.get(),
        )?;
    }
    if payment.fees.platform.get() > 0 {
        token::transfer_exact(
            collateral_mint,
            vault,
            platform_fee_vault,
            vault_authority.to_account_info(),
            &[seeds],
            payment.fees.platform.get(),
        )?;
    }

    payment.record(pool, position, stake)?;
    pool.release_if_complete(config)?;
    Ok(())
}

/// Validate all accounting before CPI; record only the prepared result after payment.
pub(crate) struct PreparedClaim {
    amount: u64,
    fees: ProfitFees,
    liability: u64,
    refund: Option<PrincipalBook>,
}

impl PreparedClaim {
    pub(crate) fn prepare(
        pool: &Pool,
        position: &Position,
        stake: &Stake,
        occurred_at: Option<i64>,
    ) -> Result<Self> {
        stake.validate_premium(pool)?;
        require!(
            stake.principal <= position.principal && stake.premium_units <= position.premium_units,
            ManteonError::InvalidAccounting
        );
        premium::validate_units(
            Amount::new(stake.principal),
            stake.premium_units,
            pool.early_premium_bps,
        )
        .map_err(ManteonError::from)?;
        premium::validate_units(
            Amount::new(position.principal),
            position.premium_units,
            pool.early_premium_bps,
        )
        .map_err(ManteonError::from)?;
        let (amount, liability) = match pool.state {
            PoolState::Claimable => {
                let occurred_at = occurred_at.ok_or(ManteonError::InvalidEventResult)?;
                require!(
                    position.computed && stake.principal <= position.principal,
                    ManteonError::InvalidPosition
                );
                let quote = premium::quote_gross(
                    pool.algorithm.into(),
                    Amount::new(pool.principal),
                    Amount::new(stake.principal),
                    stake.premium_units,
                    pool.early_premium_bps,
                    day_error(position.forecast_day, utc_day(occurred_at)),
                    Weight::from_le_bytes(pool.total_weight),
                )
                .map_err(ManteonError::from)?;
                (quote.get(), pool.payout_remaining)
            }
            PoolState::Refunding => (stake.principal, pool.principal),
            _ => return err!(ManteonError::InvalidPoolState),
        };
        require!(amount <= liability, ManteonError::InvalidAccounting);
        let fees = quote_profit_fees(
            Amount::new(stake.principal),
            Amount::new(amount),
            pool.profit_fee_bps,
            pool.platform_share_bps,
        )
        .map_err(ManteonError::from)?;

        let refund = if pool.state == PoolState::Refunding {
            Some(
                pool.principal_book(position)?
                    .quote_remove(Amount::new(amount))
                    .map_err(ManteonError::from)?,
            )
        } else {
            None
        };
        Ok(Self {
            amount,
            fees,
            liability,
            refund,
        })
    }

    fn record(self, pool: &mut Pool, position: &mut Position, stake: &mut Stake) -> Result<()> {
        if let Some(book) = self.refund {
            pool.principal = book.pool_principal().get();
            pool.active_dates = book.active_dates();
            position.principal = book.position_principal().get();
            position.premium_units = position
                .premium_units
                .checked_sub(stake.premium_units)
                .ok_or(ManteonError::InvalidAccounting)?;
            stake.state = StakeState::Refunded;
        } else {
            pool.claimed_principal = pool
                .claimed_principal
                .checked_add(stake.principal)
                .filter(|claimed| *claimed <= pool.principal)
                .ok_or(ManteonError::InvalidAccounting)?;
            pool.payout_remaining = self.liability - self.amount;
            stake.state = StakeState::Paid;
            // Pool/Position/Stake principal and total_weight remain immutable.
        }
        // Refund claims retain historical Stake principal; the terminal state consumes it.
        stake.withdrawable_principal = 0;
        Ok(())
    }
}
