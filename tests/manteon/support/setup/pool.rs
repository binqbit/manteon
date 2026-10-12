use anchor_lang::{
    InstructionData, ToAccountMetas,
    prelude::{Clock, Pubkey},
};
use anchor_spl::token::spl_token;
use litesvm::types::TransactionMetadata;
use manteon::state::{Event, Pool};
use solana_address::Address;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;

use crate::support::transactions;

use super::config::{ConfigContext, address, pubkey};

pub const NOW: i64 = 1_800_000_000;

/// An instruction-created Config with its own service signer and controlled Clock.
pub struct PoolContext {
    pub config: ConfigContext,
    pub config_address: Address,
    pub service: Keypair,
    pub token_mint: Pubkey,
}

impl Default for PoolContext {
    fn default() -> Self {
        Self::new()
    }
}

impl PoolContext {
    pub fn config_terms_hash(&self) -> [u8; 32] {
        let account = self.config.svm.get_account(&self.config_address).unwrap();
        <manteon::state::Config as anchor_lang::AccountDeserialize>::try_deserialize(
            &mut account.data.as_slice(),
        )
        .unwrap()
        .terms_hash()
    }

    pub fn new() -> Self {
        let mut config = ConfigContext::new();
        let service = Keypair::new_from_array([0x81; 32]);
        config.svm.airdrop(&service.pubkey(), 1_000_000).unwrap();
        config.service_authority = pubkey(service.pubkey());
        let config_keypair = Keypair::new_from_array([0x82; 32]);
        config.create(config.instruction(&config_keypair), &config_keypair);
        let mut clock = config.svm.get_sysvar::<Clock>();
        clock.unix_timestamp = NOW;
        config.svm.set_sysvar(&clock);
        Self {
            config,
            config_address: config_keypair.pubkey(),
            service,
            // The intended private identity is a reference, not collateral or
            // authenticated origin evidence. Its account is deliberately absent.
            token_mint: Pubkey::new_from_array([0x83; 32]),
        }
    }

    pub fn event(pool: &Keypair) -> Pubkey {
        Pubkey::find_program_address(&[Event::SEED, pool.pubkey().as_ref()], &manteon::ID).0
    }

    pub fn vault(pool: &Keypair) -> Pubkey {
        Pubkey::find_program_address(&[Pool::VAULT_SEED, pool.pubkey().as_ref()], &manteon::ID).0
    }

    pub fn vault_authority(pool: &Keypair) -> Pubkey {
        Pubkey::find_program_address(
            &[Pool::VAULT_AUTHORITY_SEED, pool.pubkey().as_ref()],
            &manteon::ID,
        )
        .0
    }

    pub fn instruction(&self, pool: &Keypair) -> Instruction {
        Instruction {
            program_id: address(manteon::ID),
            accounts: manteon::accounts::CreatePool {
                service_authority: pubkey(self.service.pubkey()),
                payer: pubkey(self.config.payer.pubkey()),
                config: pubkey(self.config_address),
                pool: pubkey(pool.pubkey()),
                event: Self::event(pool),
                collateral_mint: pubkey(self.config.mint),
                vault_authority: Self::vault_authority(pool),
                vault: Self::vault(pool),
                token_program: spl_token::ID,
                system_program: anchor_lang::system_program::ID,
            }
            .to_account_metas(None),
            data: manteon::instruction::CreatePool {
                event: manteon::state::EventDefinition::CompanyIpo {
                    token_mint: self.token_mint,
                },
                agreement_hash: [0xa1; 32],
                expected_config_hash: self.config_terms_hash(),
                algorithm: manteon::state::PoolAlgorithm::H30,
                opens_at: NOW + 10,
                lock_at: NOW + 20,
                horizon_at: NOW + 30,
            }
            .data(),
        }
    }

    pub fn create(&mut self, instruction: Instruction, pool: &Keypair) -> TransactionMetadata {
        transactions::send(
            &mut self.config.svm,
            &self.config.payer,
            &[instruction],
            &[&self.service, pool],
        )
        .unwrap()
    }

    pub fn assert_rejected(
        &mut self,
        instructions: &[Instruction],
        pool: &Keypair,
        service_override: Option<&Keypair>,
        expected: TransactionError,
    ) {
        let assets = self.config.asset_snapshot().into_iter().map(|(key, _)| key);
        transactions::assert_rejected(
            &mut self.config.svm,
            &self.config.payer,
            instructions,
            &[service_override.unwrap_or(&self.service), pool],
            assets,
            expected,
        );
    }
}
