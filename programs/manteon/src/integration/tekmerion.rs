//! Authenticated boundary to the pinned Tekmerion SDK and audit asset.
//!
//! No auditor business logic is reproduced here. Owner, complete account size,
//! discriminator, canonical address and immutable client bindings are checked
//! before interpreting the SDK's canonical Policy and Case accounts.
use anchor_lang::prelude::*;
use tekmerion_sdk::program::{Case, Policy};

use crate::{
    error::ManteonError,
    state::{Event, EventResult, Pool},
};

pub(crate) const CLIENT_SEED: &[u8] = b"tekmerion";

fn read<T: AccountDeserialize>(account: &AccountInfo, size: usize) -> Result<T> {
    require!(
        !account.executable && account.owner == &tekmerion_sdk::ID,
        ManteonError::InvalidAuditAccount
    );
    let data = account.try_borrow_data()?;
    require!(data.len() == size, ManteonError::InvalidAuditAccount);
    T::try_deserialize(&mut data.as_ref()).map_err(|_| error!(ManteonError::InvalidAuditAccount))
}

pub(crate) fn policy(account: &AccountInfo) -> Result<Policy> {
    let value: Policy = read(account, Policy::SPACE)?;
    let (key, bump) = Policy::address(&value.admin, value.identifier);
    require_keys_eq!(key, account.key(), ManteonError::InvalidAuditAccount);
    require!(
        value.bump == bump && value.rules.has_valid_parameters(),
        ManteonError::InvalidAuditAccount
    );
    let (client, _) =
        Pubkey::find_program_address(&[CLIENT_SEED, account.key.as_ref()], &crate::ID);
    require_keys_eq!(
        client,
        value.client_authority,
        ManteonError::InvalidAuditAccount
    );
    Ok(value)
}

pub(crate) fn case(account: &AccountInfo) -> Result<Case> {
    let value: Case = read(account, Case::SPACE)?;
    let (key, bump) = Case::address(&value.policy, &value.subject, &value.author, value.nonce);
    let (_, vault_bump) = Case::vault_address(&key);
    let previous_case = value
        .nonce
        .checked_sub(1)
        .map(|nonce| Case::address(&value.policy, &value.subject, &value.author, nonce).0);
    require_keys_eq!(key, account.key(), ManteonError::InvalidAuditAccount);
    require!(
        value.bump == bump
            && value.vault_bump == vault_bump
            && value.previous_case == previous_case,
        ManteonError::InvalidAuditAccount
    );
    Ok(value)
}

/// Authenticate any author's candidate against this market's immutable terms.
pub(crate) fn candidate_case(
    account: &AccountInfo,
    pool_key: Pubkey,
    pool: &Pool,
    event: &Event,
    candidate: &EventResult,
) -> Result<Case> {
    let case = case(account)?;
    require!(
        case.policy == pool.audit_config
            && case.subject == pool_key
            && case.agreement_hash == pool.agreement_hash
            && candidate.document_hash(&event.definition, pool_key, &pool.agreement_hash)?
                == case.result_hash,
        ManteonError::InvalidAuditAccount
    );
    Ok(case)
}
