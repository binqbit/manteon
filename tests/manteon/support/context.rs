//! Real lifecycle context, account addresses, and runtime assertions.

use anchor_lang::prelude::{Clock, Pubkey};
use litesvm::{LiteSVM, types::TransactionMetadata};
use solana_address::Address;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;

use super::{
    setup::{
        config::address,
        pool::{NOW, PoolContext},
    },
    transactions,
};

pub use tekmerion_sdk::{ID as TEKMERION, USDC_MINT as AUDIT_MINT};
pub const DAY: i64 = 86_400;
pub const OPEN: i64 = NOW + 10;
pub const LOCK: i64 = OPEN + 6 * DAY;
pub const HORIZON: i64 = LOCK + 2 * DAY;
pub const FORECAST: i64 = (LOCK + DAY - 1) / DAY;
pub const OWNER_BALANCE: u64 = 1_000_000;

/// Genuine Policy/Config/Pool creation. Activation remains explicit in each case.
pub struct TestContext {
    pub svm: LiteSVM,
    pub payer: Keypair,
    pub authority: Keypair,
    pub service: Keypair,
    pub config_address: Address,
    pub mint: Address,
    pub fee_recipient: Address,
    pub token_mint: Pubkey,
    pub auditors: [Keypair; 3],
    pub auditor_tokens: [Address; 3],
    pub pool: Keypair,
    pub policy: Pubkey,
    pub client_authority: Pubkey,
    pub alice: Keypair,
    pub bob: Keypair,
    pub alice_token: Address,
    pub bob_token: Address,
    pub audit_source: Address,
}

impl Default for TestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TestContext {
    pub fn activate(&mut self) {
        let ix = self.activate_ix();
        transactions::send(&mut self.svm, &self.payer, &[ix], &[&self.service]).unwrap();
    }

    pub fn set_time(&mut self, timestamp: i64) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp = timestamp;
        clock.slot += 1;
        self.svm.set_sysvar(&clock);
    }

    pub fn send(
        &mut self,
        instructions: &[Instruction],
        signers: &[&Keypair],
    ) -> TransactionMetadata {
        transactions::send(&mut self.svm, &self.payer, instructions, signers).unwrap()
    }

    pub fn reject(
        &mut self,
        instructions: &[Instruction],
        signers: &[&Keypair],
        error: TransactionError,
    ) {
        transactions::assert_rejected(
            &mut self.svm,
            &self.payer,
            instructions,
            signers,
            [
                self.pool.pubkey(),
                address(PoolContext::event(&self.pool)),
                address(PoolContext::vault(&self.pool)),
                self.config_address,
                address(self.policy),
                self.alice_token,
                self.bob_token,
                self.audit_source,
            ],
            error,
        );
    }
}
