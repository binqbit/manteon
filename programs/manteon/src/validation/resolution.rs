use anchor_lang::prelude::*;
use tekmerion_sdk::program::{CasePhase, CaseResult, CompletionMethod};

use crate::{
    error::ManteonError,
    integration::tekmerion,
    state::{Config, Event, EventDefinition, EventResult, Pool, PoolState},
    validation::admission,
};

/// Every settlement phase authenticates either the accepted external Case or a verified price result.
pub(crate) fn authorize(
    caller: Pubkey,
    config: &Config,
    pool: &Account<'_, Pool>,
    event: &Account<'_, Event>,
    case: Option<&AccountInfo<'_>>,
    now: i64,
) -> Result<()> {
    if let EventDefinition::TokenPriceThresholdCrossing { .. } = event.definition {
        require!(case.is_none(), ManteonError::InvalidAuditAccount);
        let result = event.result.ok_or(ManteonError::InvalidEventResult)?;
        let occurred_at = result.occurred_at;
        result.validate(&event.definition, now)?;
        require!(
            pool.accepted_case.is_none()
                && pool.service_eligible_at.is_none()
                && !pool.public_proposals
                && occurred_at <= pool.horizon_at
                && ((occurred_at < pool.lock_at) == (pool.state == PoolState::Refunding))
                && matches!(
                    pool.state,
                    PoolState::Settling | PoolState::Claimable | PoolState::Refunding
                ),
            ManteonError::InvalidPriceObservation
        );
        return Ok(());
    }
    let account = case.ok_or(ManteonError::InvalidAuditAccount)?;
    require!(
        pool.accepted_case == Some(account.key()),
        ManteonError::InvalidAuditAccount
    );
    let candidate = event.result.ok_or(ManteonError::InvalidEventResult)?;
    let eligible_at = accepted(pool.key(), pool, event, account, &candidate, now)?;
    admission::service_access(caller, config.service_authority, eligible_at, now)
}

/// Authenticate final Yes and the original immutable timestamp together.
/// The returned service anchor is derived from facts and cannot be reset by a caller.
pub(crate) fn accepted(
    pool_key: Pubkey,
    pool: &Pool,
    event: &Event,
    case: &AccountInfo,
    candidate: &EventResult,
    now: i64,
) -> Result<i64> {
    let case = tekmerion::candidate_case(case, pool_key, pool, event, candidate)?;
    require!(
        case.phase() == CasePhase::Finalized
            && case.result() == Some(CaseResult::Yes)
            && matches!(
                case.completion_method(),
                Some(CompletionMethod::Optimistic | CompletionMethod::Audited)
            ),
        ManteonError::InvalidAuditAccount
    );
    candidate.validate(&event.definition, now)?;
    let finalized = case.finalized_at.ok_or(ManteonError::InvalidAuditAccount)?;
    require!(
        finalized >= 0 && finalized <= now,
        ManteonError::InvalidEventResult
    );
    Ok(finalized)
}
