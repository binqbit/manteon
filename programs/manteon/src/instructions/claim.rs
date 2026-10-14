use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use manteon_core::{
    accounting::principal::PrincipalBook,
    math::{
        fees::{quote_profit_fees, ProfitFees},
        payout::quote_gross_payout,
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
    let pool = &mut ctx.accounts.pool;
    let stake = &mut ctx.accounts.stake;
    require!(
        stake.state == StakeState::Active && stake.principal > 0,
        ManteonError::InvalidStake
    );
    let occurred_at = ctx.accounts.event.result.map(|result| result.occurred_at());
    require!(
        occurred_at.is_some()
            || (pool.state == PoolState::Refunding
                && matches!(
                    ctx.accounts.event.definition,
                    crate::state::EventDefinition::TokenPriceThresholdCrossing { .. }
                )),
        ManteonError::InvalidEventResult
    );
    let payment = PreparedClaim::prepare(pool, &ctx.accounts.position, stake, occurred_at)?;

    token::validate_owner_account(
        &ctx.accounts.destination,
        &ctx.accounts.owner.key(),
        &pool.collateral_mint,
    )?;
    token::validate_vault(
        &ctx.accounts.vault,
        &pool.collateral_mint,
        &ctx.accounts.vault_authority.key(),
        payment.liability,
    )?;
    if payment.fees.recipient.get() > 0 {
        let recipient = ctx
            .accounts
            .fee_recipient
            .as_ref()
            .ok_or(ManteonError::InvalidFeeRecipient)?;
        require_keys_eq!(
            recipient.owner,
            pool.fee_recipient,
            ManteonError::InvalidFeeRecipient
        );
        require_keys_neq!(
            recipient.key(),
            ctx.accounts.vault.key(),
            ManteonError::InvalidFeeRecipient
        );
        require_keys_neq!(
            recipient.key(),
            ctx.accounts.platform_fee_vault.key(),
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
        &ctx.accounts.platform_fee_vault,
        &pool.collateral_mint,
        &GlobalConfig::address(),
        0,
    )?;
    let pool_key = pool.key();
    let seeds: &[&[u8]] = &[
        Pool::VAULT_AUTHORITY_SEED,
        pool_key.as_ref(),
        &[ctx.bumps.vault_authority],
    ];
    token::transfer_exact(
        &ctx.accounts.collateral_mint,
        &mut ctx.accounts.vault,
        &mut ctx.accounts.destination,
        ctx.accounts.vault_authority.to_account_info(),
        &[seeds],
        payment.fees.net.get(),
    )?;
    if payment.fees.recipient.get() > 0 {
        // The owner may also be the configured recipient; reload aliased balances.
        let recipient = ctx
            .accounts
            .fee_recipient
            .as_mut()
            .ok_or(ManteonError::InvalidFeeRecipient)?;
        recipient.reload()?;
        token::transfer_exact(
            &ctx.accounts.collateral_mint,
            &mut ctx.accounts.vault,
            recipient,
            ctx.accounts.vault_authority.to_account_info(),
            &[seeds],
            payment.fees.recipient.get(),
        )?;
    }
    if payment.fees.platform.get() > 0 {
        token::transfer_exact(
            &ctx.accounts.collateral_mint,
            &mut ctx.accounts.vault,
            &mut ctx.accounts.platform_fee_vault,
            ctx.accounts.vault_authority.to_account_info(),
            &[seeds],
            payment.fees.platform.get(),
        )?;
    }

    payment.record(pool, &mut ctx.accounts.position, stake)?;
    pool.release_if_complete(&mut ctx.accounts.config)?;
    Ok(())
}

/// Validate all accounting before CPI; record only the prepared result after payment.
struct PreparedClaim {
    amount: u64,
    fees: ProfitFees,
    liability: u64,
    refund: Option<PrincipalBook>,
}

impl PreparedClaim {
    fn prepare(
        pool: &Pool,
        position: &Position,
        stake: &Stake,
        occurred_at: Option<i64>,
    ) -> Result<Self> {
        let (amount, liability) = match pool.state {
            PoolState::Claimable => {
                let occurred_at = occurred_at.ok_or(ManteonError::InvalidEventResult)?;
                require!(
                    position.computed && stake.principal <= position.principal,
                    ManteonError::InvalidPosition
                );
                let quote = quote_gross_payout(
                    pool.algorithm.into(),
                    Amount::new(pool.principal),
                    Amount::new(stake.principal),
                    day_error(position.forecast_day, utc_day(occurred_at)),
                    pool.total_weight,
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
