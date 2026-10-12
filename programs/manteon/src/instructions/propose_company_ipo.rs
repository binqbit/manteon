use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use tekmerion_sdk::{
    cpi,
    program::{program::Tekmerion, CasePhase, CaseResult, CompletionMethod, Policy},
};

use crate::{
    error::ManteonError,
    integration::tekmerion,
    state::{Config, Event, EventResult, Pool, PoolState},
    validation::admission,
};

#[derive(Accounts)]
pub struct ProposeCompanyIpo<'info> {
    // Signers.
    pub author: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,

    // Market and immutable agreement.
    pub config: Account<'info, Config>,
    #[account(
        mut,
        has_one = config,
        constraint = pool.event_id == event.key()
    )]
    pub pool: Account<'info, Pool>,
    #[account(
        mut,
        seeds = [Event::SEED, pool.key().as_ref()],
        bump,
        has_one = pool
    )]
    pub event: Account<'info, Event>,
    // Audit policy and CPI authority.
    /// CHECK: exact Policy owner, layout, PDA and signer checked by adapter.
    #[account(
        mut,
        address = pool.audit_config
    )]
    pub policy: UncheckedAccount<'info>,
    /// CHECK: canonical Manteon CPI signer, without transfer authority.
    #[account(
        seeds = [tekmerion::CLIENT_SEED, policy.key().as_ref()],
        bump
    )]
    pub client_authority: UncheckedAccount<'info>,

    // Author funding.
    #[account(
        mut,
        token::mint = bond_mint,
        token::authority = author
    )]
    pub source: Account<'info, TokenAccount>,

    // Audit accounts initialized by Tekmerion.
    /// CHECK: Tekmerion creates the canonical Case atomically through CPI.
    #[account(mut)]
    pub case: UncheckedAccount<'info>,
    /// CHECK: Tekmerion initializes the canonical Case/author receipt through CPI.
    #[account(mut)]
    pub author_receipt: UncheckedAccount<'info>,

    // Audit custody.
    /// CHECK: Tekmerion creates and authenticates its own bond vault.
    #[account(mut)]
    pub case_vault: UncheckedAccount<'info>,
    #[account(address = tekmerion_sdk::USDC_MINT)]
    pub bond_mint: Account<'info, Mint>,

    // Programs.
    pub tekmerion_program: Program<'info, Tekmerion>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub(crate) fn handle<'info>(
    ctx: Context<'info, ProposeCompanyIpo<'info>>,
    data: EventResult,
) -> Result<()> {
    let pool = &ctx.accounts.pool;
    require!(
        !pool.config_released && pool.state == PoolState::Active,
        ManteonError::InvalidPoolState
    );
    let now = Clock::get()?.unix_timestamp;
    require!(
        !matches!(
            ctx.accounts.event.definition,
            crate::state::EventDefinition::TokenPriceThresholdCrossing { .. }
        ),
        ManteonError::InvalidPriceRule
    );
    data.validate(&ctx.accounts.event.definition, now)?;
    if data.occurred_at() >= pool.lock_at {
        admission::validate_day(
            pool,
            manteon_core::math::utc_day::utc_day(data.occurred_at()),
        )?;
    }
    let service_eligible_at = admission::candidate_access(
        pool,
        ctx.accounts.author.key(),
        ctx.accounts.config.service_authority,
        now,
    )?;

    // Authenticate the Policy and completed predecessor before funding a new attempt.
    let policy = tekmerion::policy(&ctx.accounts.policy)?;
    require!(policy.active_uses > 0, ManteonError::InvalidAuditAccount);
    validate_previous_case(pool, &ctx.accounts.event, ctx.remaining_accounts, now)?;

    let nonce = pool.next_case_nonce;
    let next_nonce = nonce.checked_add(1).ok_or(ManteonError::Arithmetic)?;
    let result_hash = data.document_hash(
        &ctx.accounts.event.definition,
        pool.key(),
        &pool.agreement_hash,
    )?;

    ctx.accounts.create_case(
        &policy,
        nonce,
        result_hash,
        ctx.bumps.client_authority,
        ctx.remaining_accounts,
    )?;

    // Publish the new candidate only after the CPI and its postconditions succeed.
    let pool = &mut ctx.accounts.pool;
    pool.current_case = Some(ctx.accounts.case.key());
    pool.next_case_nonce = next_nonce;
    pool.service_eligible_at = Some(service_eligible_at);
    ctx.accounts.event.result = Some(data);
    Ok(())
}

fn validate_previous_case(
    pool: &Account<Pool>,
    event: &Account<Event>,
    remaining: &[AccountInfo],
    now: i64,
) -> Result<()> {
    match pool.current_case {
        None => require!(
            remaining.is_empty()
                && pool.next_case_nonce == 0
                && pool.service_eligible_at.is_none()
                && event.result.is_none(),
            ManteonError::InvalidAuditAccount
        ),
        Some(_) => {
            require!(remaining.len() == 1, ManteonError::InvalidAuditAccount);
            let previous = tekmerion::current_case(&remaining[0], pool.key(), pool, event)?;
            require!(
                previous.phase() == CasePhase::Finalized
                    && previous.finalized_at.is_some_and(|at| at >= 0 && at <= now)
                    && matches!(
                        (previous.result(), previous.completion_method()),
                        (
                            Some(CaseResult::No | CaseResult::Unknown),
                            Some(CompletionMethod::Audited)
                        ) | (None, Some(CompletionMethod::TechnicalFailure))
                    ),
                ManteonError::InvalidAuditAccount
            );
        }
    }
    Ok(())
}

impl<'info> ProposeCompanyIpo<'info> {
    /// Fund one Case through the SDK and authenticate the resulting audit state.
    fn create_case(
        &self,
        policy: &Policy,
        nonce: u64,
        result_hash: [u8; 32],
        client_bump: u8,
        previous: &[AccountInfo<'info>],
    ) -> Result<()> {
        let policy_key = self.policy.key();
        let bump = [client_bump];
        let seeds = [tekmerion::CLIENT_SEED, policy_key.as_ref(), bump.as_ref()];
        let signer_seeds = [&seeds[..]];
        let context = CpiContext::new_with_signer(
            self.tekmerion_program.key(),
            cpi::accounts::CreateCase {
                payer: self.payer.to_account_info(),
                policy: self.policy.to_account_info(),
                client_authority: self.client_authority.to_account_info(),
                subject: self.pool.to_account_info(),
                author: self.author.to_account_info(),
                source: self.source.to_account_info(),
                case: self.case.to_account_info(),
                author_receipt: self.author_receipt.to_account_info(),
                vault: self.case_vault.to_account_info(),
                mint: self.bond_mint.to_account_info(),
                token_program: self.token_program.to_account_info(),
                system_program: self.system_program.to_account_info(),
            },
            &signer_seeds,
        )
        .with_remaining_accounts(previous.to_vec());
        cpi::create_case(
            context,
            nonce,
            policy.rules.author_bond,
            self.pool.agreement_hash,
            result_hash,
        )?;

        let created = tekmerion::case(&self.case)?;
        require!(
            created.policy == policy_key
                && created.subject == self.pool.key()
                && created.author == self.author.key()
                && created.nonce == nonce
                && created.previous_case == self.pool.current_case
                && created.agreement_hash == self.pool.agreement_hash
                && created.result_hash == result_hash
                && created.author_principal == policy.rules.author_bond
                && created.phase() == CasePhase::Funding
                && created.result().is_none()
                && created.completion_method().is_none()
                && created.finalized_at.is_none(),
            ManteonError::InvalidAuditAccount
        );
        let retained = tekmerion::policy(&self.policy)?;
        require!(
            Some(retained.active_uses) == policy.active_uses.checked_add(1),
            ManteonError::InvalidAuditAccount
        );
        Ok(())
    }
}
