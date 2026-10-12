//! Canonical instruction builders kept separate from scenario setup.

use anchor_lang::{InstructionData, ToAccountMetas};
use anchor_spl::token::spl_token;
use solana_address::Address;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_signer::Signer;

use super::context::{TEKMERION, TestContext};
use super::{
    setup::config::{address, pubkey},
    setup::pool::PoolContext,
};

impl TestContext {
    pub fn activate_ix(&self) -> Instruction {
        Instruction {
            program_id: address(manteon::ID),
            accounts: manteon::accounts::ActivatePool {
                service_authority: pubkey(self.service.pubkey()),
                config: pubkey(self.config_address),
                pool: pubkey(self.pool.pubkey()),
                event: PoolContext::event(&self.pool),
                token_mint: self.token_mint,
                policy: self.policy,
                client_authority: self.client_authority,
                tekmerion_program: TEKMERION,
            }
            .to_account_metas(None),
            data: manteon::instruction::ActivatePool {}.data(),
        }
    }

    pub fn stake_ix(&self, owner: &Keypair, source: Address, day: i64, amount: u64) -> Instruction {
        Instruction {
            program_id: address(manteon::ID),
            accounts: manteon::accounts::Stake {
                owner: pubkey(owner.pubkey()),
                payer: pubkey(self.payer.pubkey()),
                config: pubkey(self.config_address),
                pool: pubkey(self.pool.pubkey()),
                position: self.position(day),
                stake: self.stake_key(owner, day),
                collateral_mint: pubkey(self.mint),
                source: pubkey(source),
                vault_authority: PoolContext::vault_authority(&self.pool),
                vault: PoolContext::vault(&self.pool),
                token_program: spl_token::ID,
                system_program: anchor_lang::system_program::ID,
            }
            .to_account_metas(None),
            data: manteon::instruction::Stake {
                forecast_day: day,
                amount,
            }
            .data(),
        }
    }

    pub fn unstake_ix(
        &self,
        owner: &Keypair,
        destination: Address,
        day: i64,
        amount: u64,
    ) -> Instruction {
        Instruction {
            program_id: address(manteon::ID),
            accounts: manteon::accounts::Unstake {
                owner: pubkey(owner.pubkey()),
                config: pubkey(self.config_address),
                pool: pubkey(self.pool.pubkey()),
                position: self.position(day),
                stake: self.stake_key(owner, day),
                collateral_mint: pubkey(self.mint),
                destination: pubkey(destination),
                vault_authority: PoolContext::vault_authority(&self.pool),
                vault: PoolContext::vault(&self.pool),
                token_program: spl_token::ID,
            }
            .to_account_metas(None),
            data: manteon::instruction::Unstake { amount }.data(),
        }
    }

    pub fn exit_ix(&self, owner: &Keypair, destination: Address, day: i64) -> Instruction {
        Instruction {
            program_id: address(manteon::ID),
            accounts: manteon::accounts::Exit {
                owner: pubkey(owner.pubkey()),
                config: pubkey(self.config_address),
                pool: pubkey(self.pool.pubkey()),
                position: self.position(day),
                stake: self.stake_key(owner, day),
                collateral_mint: pubkey(self.mint),
                destination: pubkey(destination),
                vault_authority: PoolContext::vault_authority(&self.pool),
                vault: PoolContext::vault(&self.pool),
                token_program: spl_token::ID,
            }
            .to_account_metas(None),
            data: manteon::instruction::Exit {}.data(),
        }
    }
}
