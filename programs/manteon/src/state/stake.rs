use anchor_lang::prelude::*;
use manteon_core::{accounting::allowance::AllowanceSnapshot, types::Amount};

use crate::error::ManteonError;

/// One owner's principal and personal withdrawal allowance for a Position.
#[account]
#[derive(InitSpace)]
pub struct Stake {
    // Beneficiary and forecast Position.
    pub owner: Pubkey,
    pub position: Pubkey,

    // Participating principal and personal withdrawal allowance.
    /// Participating principal, including funds outside the personal allowance.
    pub principal: u64,
    /// Spendable only while the personal window and Pool state permit withdrawal.
    pub withdrawable_principal: u64,
    pub withdraw_until: i64,

    // Entitlement consumption.
    pub state: StakeState,
}

impl Stake {
    /// Followed by the Position key and owner key; no per-deposit nonce.
    pub const SEED: &'static [u8] = b"stake";

    /// Accept a fresh zeroed account or this owner's active Stake at the Position.
    pub(crate) fn validate_deposit(&self, owner: Pubkey, position: Pubkey) -> Result<()> {
        if self.position == Pubkey::default() {
            require!(
                self.owner == Pubkey::default()
                    && self.principal == 0
                    && self.withdrawable_principal == 0
                    && self.withdraw_until == 0
                    && self.state == StakeState::Active,
                ManteonError::InvalidStake
            );
        } else {
            require!(
                self.owner == owner
                    && self.position == position
                    && self.state == StakeState::Active,
                ManteonError::InvalidStake
            );
        }
        Ok(())
    }

    /// Read checked allowance amounts; the instruction still authorizes withdrawal.
    pub(crate) fn allowance(&self) -> Result<AllowanceSnapshot> {
        Ok(AllowanceSnapshot::new(
            Amount::new(self.principal),
            Amount::new(self.withdrawable_principal),
            self.withdraw_until,
        )
        .map_err(ManteonError::from)?)
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, InitSpace, Clone, Copy, Debug, Eq, PartialEq)]
pub enum StakeState {
    Active,
    Paid,
    Refunded,
}
