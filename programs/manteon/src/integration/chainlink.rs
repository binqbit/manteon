//! Enroll a PDA auditor before enabling deposits, not after funds are committed.
use crate::error::ManteonError;
use anchor_lang::prelude::*;
use chainlink_auditor::state::ConfidentialProfile;
use tekmerion_sdk::program::Policy;

/// Identify the canonical adapter auditor in an authenticated Policy.
/// An Active Pool has already validated this Profile and retained its fixed rules.
pub(crate) fn uses_chainlink(policy_key: Pubkey, policy: &Policy) -> bool {
    let key = ConfidentialProfile::address(&policy_key).0;
    policy.rules.auditors.iter().any(|a| a.authority == key)
}

pub fn validate(policy_key: Pubkey, policy: &Policy, remaining: &[AccountInfo]) -> Result<()> {
    if !uses_chainlink(policy_key, policy) {
        return Ok(());
    }
    let key = ConfidentialProfile::address(&policy_key).0;
    let check = || -> Result<()> {
        let account = remaining
            .iter()
            .find(|a| a.key == &key)
            .ok_or(ManteonError::InvalidAuditAccount)?;
        require!(
            account.owner == &chainlink_auditor::ID && !account.is_writable,
            ManteonError::InvalidAuditAccount
        );
        let bytes = account.try_borrow_data()?;
        let profile = ConfidentialProfile::try_deserialize(&mut bytes.as_ref())?;
        chainlink_auditor::confidential::binding::validate_profile(
            key, &profile, policy_key, policy,
        )?;
        Ok(())
    };
    check().map_err(|_| error!(ManteonError::InvalidAuditAccount))
}
