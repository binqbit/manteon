//! Provision the external USDC mint address; create assets and contract states
//! through the real SPL, System, and program instructions.

use anchor_lang::{
    InstructionData, ToAccountMetas,
    prelude::{Clock, Pubkey},
    solana_program::program_pack::Pack,
};
use anchor_spl::{token::spl_token, token_2022::spl_token_2022};
use solana_account::Account;
use solana_address::Address;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_system_interface::instruction as system_instruction;
use tekmerion_sdk::{
    instructions,
    program::{AuditorConfig, Policy, PolicyRules},
};

use super::{
    config::{ConfigContext, address, pubkey},
    pool::{NOW, PoolContext},
    programs, token,
};
use crate::support::{
    context::{AUDIT_MINT, HORIZON, LOCK, OPEN, OWNER_BALANCE, TestContext},
    transactions,
};

impl TestContext {
    /// Create another independent Pool under the same Config and schedule.
    /// Activation and selecting it as the context's current Pool stay explicit.
    pub fn config_terms_hash(&self) -> [u8; 32] {
        let account = self.svm.get_account(&self.config_address).unwrap();
        <manteon::state::Config as anchor_lang::AccountDeserialize>::try_deserialize(
            &mut account.data.as_slice(),
        )
        .unwrap()
        .terms_hash()
    }

    pub fn create_pool(&mut self, seed: u8) -> Keypair {
        let pool = Keypair::new_from_array([seed; 32]);
        let instruction = self.create_pool_ix(&pool);
        transactions::send(
            &mut self.svm,
            &self.payer,
            &[instruction],
            &[&self.service, &pool],
        )
        .unwrap();
        pool
    }

    pub fn create_pool_ix(&self, pool: &Keypair) -> Instruction {
        let schedule = self.pool_state();
        Instruction {
            program_id: address(manteon::ID),
            accounts: manteon::accounts::CreatePool {
                service_authority: pubkey(self.service.pubkey()),
                payer: pubkey(self.payer.pubkey()),
                config: pubkey(self.config_address),
                pool: pubkey(pool.pubkey()),
                event: PoolContext::event(pool),
                collateral_mint: pubkey(self.mint),
                vault_authority: PoolContext::vault_authority(pool),
                vault: PoolContext::vault(pool),
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
                algorithm: schedule.algorithm,
                opens_at: schedule.opens_at,
                lock_at: schedule.lock_at,
                horizon_at: schedule.horizon_at,
            }
            .data(),
        }
    }

    pub fn new() -> Self {
        Self::with_schedule(OPEN, LOCK, HORIZON)
    }

    pub fn with_schedule(opens_at: i64, lock_at: i64, horizon_at: i64) -> Self {
        Self::with_event_schedule(
            opens_at,
            lock_at,
            horizon_at,
            None,
            manteon::state::PoolAlgorithm::H30,
            (0, false),
        )
    }

    pub fn with_price_threshold(params: manteon::state::PriceThreshold) -> Self {
        Self::with_price_threshold_and_algorithm(params, manteon::state::PoolAlgorithm::H30)
    }

    pub fn with_price_threshold_and_algorithm(
        params: manteon::state::PriceThreshold,
        algorithm: manteon::state::PoolAlgorithm,
    ) -> Self {
        Self::with_event_schedule(
            OPEN,
            NOW + 7200,
            NOW + 86_400,
            Some(params),
            algorithm,
            (0, false),
        )
    }

    pub fn with_algorithm(algorithm: manteon::state::PoolAlgorithm) -> Self {
        Self::with_event_schedule(OPEN, LOCK, HORIZON, None, algorithm, (0, false))
    }

    pub fn with_profit_fee_recipient(profit_fee_bps: u16, owner_is_recipient: bool) -> Self {
        Self::with_event_schedule(
            OPEN,
            LOCK,
            HORIZON,
            None,
            manteon::state::PoolAlgorithm::H30,
            (profit_fee_bps, owner_is_recipient),
        )
    }

    pub(crate) fn with_event_schedule(
        opens_at: i64,
        lock_at: i64,
        horizon_at: i64,
        price: Option<manteon::state::PriceThreshold>,
        algorithm: manteon::state::PoolAlgorithm,
        fees: (u16, bool),
    ) -> Self {
        let mut config = ConfigContext::new();
        config.profit_fee_bps = fees.0;
        programs::load_tekmerion(&mut config.svm);
        let mut clock = config.svm.get_sysvar::<Clock>();
        clock.unix_timestamp = NOW;
        config.svm.set_sysvar(&clock);
        let service = Keypair::new_from_array([0xb0; 32]);
        let alice = Keypair::new_from_array([0xb1; 32]);
        let bob = Keypair::new_from_array([0xb2; 32]);
        for signer in [&service, &alice, &bob] {
            config.svm.airdrop(&signer.pubkey(), 1_000_000).unwrap();
        }
        if price.is_some() {
            // Price activation pays the rent for its dedicated monitoring PDA.
            config.svm.airdrop(&service.pubkey(), 2_000_000).unwrap();
        }
        let auditors = [0xc8, 0xc9, 0xca].map(|seed| Keypair::new_from_array([seed; 32]));
        for auditor in &auditors {
            config.svm.airdrop(&auditor.pubkey(), 1_000_000).unwrap();
        }
        let policy = Policy::address(&pubkey(config.authority.pubkey()), 0).0;
        let client_authority =
            Pubkey::find_program_address(&[b"tekmerion", policy.as_ref()], &manteon::ID).0;
        let create_policy = instructions::create_policy(instructions::CreatePolicy {
            payer: pubkey(config.payer.pubkey()),
            admin: pubkey(config.authority.pubkey()),
            identifier: 0,
            client_authority,
            rules: PolicyRules {
                auditors: auditors
                    .iter()
                    .map(|auditor| AuditorConfig {
                        authority: pubkey(auditor.pubkey()),
                        fee_recipient: pubkey(auditor.pubkey()),
                    })
                    .collect(),
                threshold: 2,
                author_bond: 100,
                challenge_target: 100,
                audit_fee: 7,
                funding_duration: 60,
                audit_timeout: 300,
            },
        });
        transactions::send(
            &mut config.svm,
            &config.payer,
            &[create_policy],
            &[&config.authority],
        )
        .unwrap();

        config.service_authority = pubkey(service.pubkey());
        config.audit_config = policy;
        let config_key = Keypair::new_from_array([0xb3; 32]);
        let collateral = config.mint;
        let alice_token = create_token(&mut config, 0xb4, alice.pubkey(), collateral);
        let bob_token = create_token(&mut config, 0xb5, bob.pubkey(), collateral);
        if fees.1 {
            config.fee_recipient = alice_token;
            config.fee_authority = pubkey(alice.pubkey());
        }
        config.create(config.instruction(&config_key), &config_key);
        // The USDC address is external: there is no test keypair for its mint.
        // Only this VM fixture supplies a mint authority for initial balances.
        config
            .svm
            .set_account(
                address(AUDIT_MINT),
                Account {
                    lamports: config
                        .svm
                        .minimum_balance_for_rent_exemption(spl_token::state::Mint::LEN),
                    data: vec![0; spl_token::state::Mint::LEN],
                    owner: address(spl_token::ID),
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let initialize = spl_token::instruction::initialize_mint2(
            &spl_token::ID,
            &AUDIT_MINT,
            &pubkey(config.payer.pubkey()),
            None,
            6,
        )
        .unwrap();
        transactions::send(&mut config.svm, &config.payer, &[initialize], &[]).unwrap();
        let audit_source = create_token(&mut config, 0xb6, alice.pubkey(), address(AUDIT_MINT));
        let auditor_tokens = std::array::from_fn(|index| {
            create_token(
                &mut config,
                0xd0 + index as u8,
                auditors[index].pubkey(),
                address(AUDIT_MINT),
            )
        });
        // Private PreStocks identity uses Token-2022 independently of collateral.
        let private_mint = Keypair::new_from_array([0xb8; 32]);
        let token_mint = pubkey(private_mint.pubkey());
        let create_private_mint = system_instruction::create_account(
            &pubkey(config.payer.pubkey()),
            &token_mint,
            config
                .svm
                .minimum_balance_for_rent_exemption(spl_token_2022::state::Mint::LEN),
            spl_token_2022::state::Mint::LEN as u64,
            &spl_token_2022::ID,
        );
        let initialize_private_mint = spl_token_2022::instruction::initialize_mint2(
            &spl_token_2022::ID,
            &token_mint,
            &pubkey(config.payer.pubkey()),
            None,
            6,
        )
        .unwrap();
        transactions::send(
            &mut config.svm,
            &config.payer,
            &[create_private_mint, initialize_private_mint],
            &[&private_mint],
        )
        .unwrap();
        let mut market = PoolContext {
            config,
            config_address: config_key.pubkey(),
            service,
            token_mint,
        };
        let pool = Keypair::new_from_array([0xb7; 32]);
        let mut create_pool = market.instruction(&pool);
        create_pool.data = manteon::instruction::CreatePool {
            event: match price {
                Some(mut params) => {
                    params.token_mint = token_mint;
                    manteon::state::EventDefinition::TokenPriceThresholdCrossing { params }
                }
                None => manteon::state::EventDefinition::CompanyIpo { token_mint },
            },
            agreement_hash: [0xa1; 32],
            expected_config_hash: market.config_terms_hash(),
            algorithm,
            opens_at,
            lock_at,
            horizon_at,
        }
        .data();
        market.create(create_pool, &pool);
        Self {
            svm: market.config.svm,
            payer: market.config.payer,
            authority: market.config.authority,
            mint: market.config.mint,
            fee_recipient: market.config.fee_recipient,
            config_address: market.config_address,
            service: market.service,
            token_mint,
            auditors,
            auditor_tokens,
            pool,
            policy,
            client_authority,
            alice,
            bob,
            alice_token,
            bob_token,
            audit_source,
        }
    }
}

fn create_token(config: &mut ConfigContext, seed: u8, owner: Address, mint: Address) -> Address {
    token::create_account(
        &mut config.svm,
        &config.payer,
        seed,
        mint,
        owner,
        OWNER_BALANCE,
    )
}
