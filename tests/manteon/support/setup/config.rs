use anchor_lang::{
    AccountDeserialize, InstructionData, ToAccountMetas,
    prelude::Pubkey,
    solana_program::{
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        program_option::COption,
        program_pack::Pack,
    },
};
use anchor_spl::token::spl_token;
use litesvm::{LiteSVM, types::TransactionMetadata};
use solana_account::Account;
use solana_address::Address;
use solana_instruction::Instruction;
use solana_instruction_error::InstructionError;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_system_interface::instruction as system_instruction;
use solana_transaction_error::TransactionError;

use crate::support::transactions;

pub fn address(key: Pubkey) -> Address {
    Address::new_from_array(key.to_bytes())
}

pub fn pubkey(key: Address) -> Pubkey {
    Pubkey::new_from_array(key.to_bytes())
}

/// Deployment metadata and the native mint's genesis state are external fixtures.
/// Other assets and every Config are created through signed runtime transactions.
pub struct ConfigContext {
    pub svm: LiteSVM,
    pub authority: Keypair,
    pub payer: Keypair,
    pub program_data: Address,
    pub mint: Address,
    pub fee_recipient: Address,
    pub other_mint_recipient: Address,
    pub frozen_recipient: Address,
    pub uninitialized_mint: Address,
    pub service_authority: Pubkey,
    pub audit_config: Pubkey,
    pub profit_fee_bps: u16,
    assets: Vec<Address>,
}

impl Default for ConfigContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigContext {
    pub fn new() -> Self {
        Self::with_platform_share(Some(2_000))
    }

    pub fn with_platform_share(platform_share_bps: Option<u16>) -> Self {
        let mut svm = super::programs::program_svm();
        let authority = Keypair::new_from_array([0x61; 32]);
        let payer = Keypair::new_from_array([0x62; 32]);
        svm.airdrop(&payer.pubkey(), 100_000_000).unwrap();
        svm.airdrop(&authority.pubkey(), 1_000_000).unwrap();

        // LiteSVM installs immutable deployment metadata. Supply the external
        // deployment authority without fabricating any Manteon account state.
        let program_data = address(bpf_loader_upgradeable::get_program_data_address(
            &manteon::ID,
        ));
        let mut deployment = svm.get_account(&program_data).unwrap();
        let prior: UpgradeableLoaderState = bincode::deserialize(&deployment.data).unwrap();
        let UpgradeableLoaderState::ProgramData { slot, .. } = prior else {
            panic!("expected loader ProgramData")
        };
        let metadata = bincode::serialize(&UpgradeableLoaderState::ProgramData {
            slot,
            upgrade_authority_address: Some(pubkey(authority.pubkey())),
        })
        .unwrap();
        let metadata_len = UpgradeableLoaderState::size_of_programdata_metadata();
        assert_eq!(metadata.len(), metadata_len);
        deployment.data[..metadata_len].copy_from_slice(&metadata);
        svm.set_account(program_data, deployment).unwrap();

        let mut context = Self {
            svm,
            authority,
            payer,
            program_data,
            mint: Address::default(),
            fee_recipient: Address::default(),
            other_mint_recipient: Address::default(),
            frozen_recipient: Address::default(),
            uninitialized_mint: Address::default(),
            service_authority: Pubkey::new_from_array([0x63; 32]),
            audit_config: Pubkey::new_from_array([0x64; 32]),
            profit_fee_bps: 0,
            assets: Vec::new(),
        };
        context.mint = context.create_mint(0x65, true);
        let other_mint = context.create_mint(0x66, true);
        context.uninitialized_mint = context.create_mint(0x67, false);
        context.fee_recipient = context.create_token_account(0x68, context.mint, false);
        context.other_mint_recipient = context.create_token_account(0x69, other_mint, false);
        context.frozen_recipient = context.create_token_account(0x6a, context.mint, true);
        if let Some(share) = platform_share_bps {
            let instruction = context.initialize_global_config_instruction(share);
            transactions::send(
                &mut context.svm,
                &context.payer,
                &[instruction],
                &[&context.authority],
            )
            .unwrap();
        }
        context
    }

    fn setup(&mut self, instructions: &[Instruction], new_account: &Keypair) {
        transactions::send(&mut self.svm, &self.payer, instructions, &[new_account])
            .expect("create fixture through real System and SPL instructions");
        self.assets.push(new_account.pubkey());
    }

    fn create_mint(&mut self, seed: u8, initialize: bool) -> Address {
        let mint = Keypair::new_from_array([seed; 32]);
        let mut instructions = vec![system_instruction::create_account(
            &pubkey(self.payer.pubkey()),
            &pubkey(mint.pubkey()),
            self.svm
                .minimum_balance_for_rent_exemption(spl_token::state::Mint::LEN),
            spl_token::state::Mint::LEN as u64,
            &spl_token::ID,
        )];
        if initialize {
            instructions.push(
                spl_token::instruction::initialize_mint2(
                    &spl_token::ID,
                    &pubkey(mint.pubkey()),
                    &pubkey(self.payer.pubkey()),
                    Some(&pubkey(self.payer.pubkey())),
                    6,
                )
                .unwrap(),
            );
        }
        self.setup(&instructions, &mint);
        mint.pubkey()
    }

    fn create_token_account(&mut self, seed: u8, mint: Address, frozen: bool) -> Address {
        let token = super::token::create_account(
            &mut self.svm,
            &self.payer,
            seed,
            mint,
            self.payer.pubkey(),
            1_000_000,
        );
        self.assets.push(token);
        if frozen {
            let freeze = spl_token::instruction::freeze_account(
                &spl_token::ID,
                &pubkey(token),
                &pubkey(mint),
                &pubkey(self.payer.pubkey()),
                &[],
            )
            .unwrap();
            transactions::send(&mut self.svm, &self.payer, &[freeze], &[])
                .expect("freeze fixture token account");
        }
        token
    }

    pub fn create_native_token_account(&mut self, seed: u8, amount: u64) -> Address {
        let mint = address(spl_token::native_mint::ID);
        // Legacy SPL's native mint is a genesis account, not a mint created by
        // InitializeMint. LiteSVM loads the token program but omits this account.
        if self.svm.get_account(&mint).is_none() {
            let mut data = vec![0; spl_token::state::Mint::LEN];
            spl_token::state::Mint::pack(
                spl_token::state::Mint {
                    mint_authority: COption::None,
                    supply: 0,
                    decimals: spl_token::native_mint::DECIMALS,
                    is_initialized: true,
                    freeze_authority: COption::None,
                },
                &mut data,
            )
            .unwrap();
            self.svm
                .set_account(
                    mint,
                    Account {
                        lamports: self.svm.minimum_balance_for_rent_exemption(data.len()),
                        data,
                        owner: address(spl_token::ID),
                        executable: false,
                        rent_epoch: 0,
                    },
                )
                .unwrap();
        }
        self.assets.push(mint);

        let token = Keypair::new_from_array([seed; 32]);
        let rent = self
            .svm
            .minimum_balance_for_rent_exemption(spl_token::state::Account::LEN);
        let instructions = [
            system_instruction::create_account(
                &pubkey(self.payer.pubkey()),
                &pubkey(token.pubkey()),
                rent.checked_add(amount).unwrap(),
                spl_token::state::Account::LEN as u64,
                &spl_token::ID,
            ),
            spl_token::instruction::initialize_account3(
                &spl_token::ID,
                &pubkey(token.pubkey()),
                &spl_token::native_mint::ID,
                &pubkey(self.payer.pubkey()),
            )
            .unwrap(),
        ];
        self.setup(&instructions, &token);
        token.pubkey()
    }

    pub fn initialize_global_config_instruction(&self, platform_share_bps: u16) -> Instruction {
        Instruction {
            program_id: address(manteon::ID),
            accounts: manteon::accounts::InitializeGlobalConfig {
                authority: pubkey(self.authority.pubkey()),
                payer: pubkey(self.payer.pubkey()),
                global_config: manteon::state::GlobalConfig::address(),
                program: manteon::ID,
                program_data: pubkey(self.program_data),
                system_program: anchor_lang::system_program::ID,
            }
            .to_account_metas(None),
            data: manteon::instruction::InitializeGlobalConfig {
                admin: pubkey(self.authority.pubkey()),
                fee_authority: pubkey(self.authority.pubkey()),
                platform_share_bps,
            }
            .data(),
        }
    }

    pub fn instruction(&self, config: &Keypair) -> Instruction {
        Instruction {
            program_id: address(manteon::ID),
            accounts: manteon::accounts::CreateConfig {
                authority: pubkey(self.authority.pubkey()),
                payer: pubkey(self.payer.pubkey()),
                config: pubkey(config.pubkey()),
                global_config: manteon::state::GlobalConfig::address(),
                platform_fee_vault: manteon::state::GlobalConfig::vault_address(&pubkey(self.mint)),
                collateral_mint: pubkey(self.mint),
                fee_recipient: pubkey(self.fee_recipient),
                token_program: spl_token::ID,
                system_program: anchor_lang::system_program::ID,
            }
            .to_account_metas(None),
            data: manteon::instruction::CreateConfig {
                service_authority: self.service_authority,
                audit_config: self.audit_config,
                profit_fee_bps: self.profit_fee_bps,
                expected_platform_share_bps: self
                    .svm
                    .get_account(&address(manteon::state::GlobalConfig::address()))
                    .and_then(|account| {
                        manteon::state::GlobalConfig::try_deserialize(&mut account.data.as_slice())
                            .ok()
                    })
                    .map_or(2000, |settings| settings.platform_share_bps),
            }
            .data(),
        }
    }

    pub fn create(&mut self, instruction: Instruction, config: &Keypair) -> TransactionMetadata {
        transactions::send(
            &mut self.svm,
            &self.payer,
            &[instruction],
            &[&self.authority, config],
        )
        .unwrap()
    }

    pub fn asset_snapshot(&self) -> Vec<(Address, Option<Account>)> {
        self.assets
            .iter()
            .map(|key| (*key, self.svm.get_account(key)))
            .collect()
    }

    pub fn assert_rejected(
        &mut self,
        instruction: Instruction,
        config: &Keypair,
        authority_override: Option<&Keypair>,
        expected: InstructionError,
    ) {
        transactions::assert_rejected(
            &mut self.svm,
            &self.payer,
            &[instruction],
            &[authority_override.unwrap_or(&self.authority), config],
            self.assets.iter().copied(),
            TransactionError::InstructionError(0, expected),
        );
    }
}
