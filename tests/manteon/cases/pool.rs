use anchor_lang::{AccountDeserialize, InstructionData, error::ErrorCode, prelude::Pubkey};
use manteon::{
    error::ManteonError,
    state::{Event, EventDefinition, Pool, PoolState},
};
use solana_instruction_error::InstructionError;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;

use crate::support::setup::{
    config::{address, pubkey},
    pool::{NOW, PoolContext},
};

fn rejected(error: impl Into<u32>) -> TransactionError {
    TransactionError::InstructionError(0, InstructionError::Custom(error.into()))
}

#[test]
fn anchor_008_service_creates_independent_pools_with_canonical_empty_events() {
    let mut context = PoolContext::new();
    assert_ne!(context.service.pubkey(), context.config.payer.pubkey());
    assert_ne!(context.service.pubkey(), context.config.authority.pubkey());
    let assets_before = context.config.asset_snapshot();
    let config_before = context
        .config
        .svm
        .get_account(&context.config_address)
        .unwrap();
    let first = Keypair::new_from_array([0x84; 32]);
    let second = Keypair::new_from_array([0x85; 32]);
    assert_ne!(PoolContext::event(&first), PoolContext::event(&second));

    for keypair in [&first, &second] {
        let payer_before = context
            .config
            .svm
            .get_account(&context.config.payer.pubkey())
            .unwrap();
        let metadata = context.create(context.instruction(keypair), keypair);
        let pool_account = context.config.svm.get_account(&keypair.pubkey()).unwrap();
        let event_key = PoolContext::event(keypair);
        let event_account = context.config.svm.get_account(&address(event_key)).unwrap();
        let vault_account = context
            .config
            .svm
            .get_account(&address(PoolContext::vault(keypair)))
            .unwrap();
        assert_eq!(pool_account.data.len(), 337);
        assert_eq!(event_account.data.len(), 261);
        for account in [&pool_account, &event_account] {
            assert_eq!(account.owner, address(manteon::ID));
            assert!(!account.executable);
            assert_eq!(
                account.lamports,
                context
                    .config
                    .svm
                    .minimum_balance_for_rent_exemption(account.data.len())
            );
        }
        let pool = Pool::try_deserialize(&mut pool_account.data.as_slice()).unwrap();
        assert_eq!(pool.config, pubkey(context.config_address));
        assert_eq!(pool.event_id, event_key);
        assert_eq!(pool.collateral_mint, pubkey(context.config.mint));
        assert_eq!(pool.audit_config, context.config.audit_config);
        assert_eq!(
            (pool.opens_at, pool.lock_at, pool.horizon_at),
            (NOW + 10, NOW + 20, NOW + 30)
        );
        assert_eq!(pool.state, PoolState::Draft);
        assert_eq!(pool.current_case, None);
        assert_eq!(pool.next_case_nonce, 0);
        assert_eq!(pool.principal, 0);
        assert_eq!(pool.active_dates, 0);
        assert_eq!(pool.processed_dates, 0);
        assert_eq!(pool.total_weight, 0);
        assert_eq!(pool.payout_remaining, 0);
        assert_eq!(pool.service_eligible_at, None);
        let event = Event::try_deserialize(&mut event_account.data.as_slice()).unwrap();
        assert_eq!(event.pool, pubkey(keypair.pubkey()));
        assert_eq!(event.result, None);
        assert_eq!(
            event.definition,
            EventDefinition::CompanyIpo {
                token_mint: context.token_mint,
            }
        );
        let current_config = context
            .config
            .svm
            .get_account(&context.config_address)
            .unwrap();
        let current_config =
            manteon::state::Config::try_deserialize(&mut current_config.data.as_slice()).unwrap();
        assert_eq!(
            current_config.active_pools,
            if keypair.pubkey() == first.pubkey() {
                1
            } else {
                2
            }
        );
        let original =
            manteon::state::Config::try_deserialize(&mut config_before.data.as_slice()).unwrap();
        assert_eq!(current_config.terms_hash(), original.terms_hash());
        assert_eq!(context.config.asset_snapshot(), assets_before);
        let payer_after = context
            .config
            .svm
            .get_account(&context.config.payer.pubkey())
            .unwrap();
        assert_eq!(
            payer_before.lamports - payer_after.lamports,
            pool_account.lamports + event_account.lamports + vault_account.lamports + metadata.fee
        );
    }

    let mut replay = context.instruction(&first);
    replay.data = manteon::instruction::CreatePool {
        event: manteon::state::EventDefinition::CompanyIpo {
            token_mint: Pubkey::new_from_array([0x86; 32]),
        },
        agreement_hash: [0xa1; 32],
        expected_config_hash: context.config_terms_hash(),
        algorithm: manteon::state::PoolAlgorithm::H30,
        opens_at: NOW + 100,
        lock_at: NOW + 200,
        horizon_at: NOW + 300,
    }
    .data();
    context.assert_rejected(&[replay], &first, None, rejected(0u32));
}

#[test]
fn anchor_009_pool_schedule_uses_execution_clock_and_committed_ordering() {
    let mut context = PoolContext::new();
    let pool = Keypair::new_from_array([0x87; 32]);
    for (opens_at, lock_at, horizon_at) in [
        (NOW - 1, NOW + 20, NOW + 30),
        (NOW, NOW + 20, NOW + 30),
        (NOW + 10, NOW + 10, NOW + 30),
        (NOW + 20, NOW + 10, NOW + 30),
        (NOW + 10, NOW + 20, NOW + 19),
    ] {
        let mut instruction = context.instruction(&pool);
        instruction.data = manteon::instruction::CreatePool {
            event: manteon::state::EventDefinition::CompanyIpo {
                token_mint: context.token_mint,
            },
            agreement_hash: [0xa1; 32],
            expected_config_hash: context.config_terms_hash(),
            algorithm: manteon::state::PoolAlgorithm::H30,
            opens_at,
            lock_at,
            horizon_at,
        }
        .data();
        context.assert_rejected(
            &[instruction],
            &pool,
            None,
            rejected(ManteonError::InvalidPoolSchedule),
        );
    }
    // No arithmetic is needed to order timestamps, and no maximum horizon has
    // been selected. The equal LOCK/horizon boundary remains valid at i64::MAX.
    let mut instruction = context.instruction(&pool);
    instruction.data = manteon::instruction::CreatePool {
        event: manteon::state::EventDefinition::CompanyIpo {
            token_mint: context.token_mint,
        },
        agreement_hash: [0xa1; 32],
        expected_config_hash: context.config_terms_hash(),
        algorithm: manteon::state::PoolAlgorithm::H30,
        opens_at: i64::MAX - 1,
        lock_at: i64::MAX,
        horizon_at: i64::MAX,
    }
    .data();
    context.create(instruction, &pool);
    let account = context.config.svm.get_account(&pool.pubkey()).unwrap();
    let stored = Pool::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(
        (stored.opens_at, stored.lock_at, stored.horizon_at),
        (i64::MAX - 1, i64::MAX, i64::MAX)
    );
}

#[test]
fn anchor_010_pool_creation_rejects_substitution_and_rolls_back_composed_failures() {
    let mut context = PoolContext::new();
    let pool = Keypair::new_from_array([0x88; 32]);
    let another_pool = Keypair::new_from_array([0x89; 32]);
    context.create(context.instruction(&another_pool), &another_pool);

    let mut wrong_service = context.instruction(&pool);
    wrong_service.accounts[0].pubkey = context.config.authority.pubkey();
    let platform = context.config.authority.insecure_clone();
    context.assert_rejected(
        &[wrong_service],
        &pool,
        Some(&platform),
        rejected(ManteonError::UnauthorizedServiceAuthority),
    );

    let mut wrong_config = context.instruction(&pool);
    wrong_config.accounts[2].pubkey = context.config.authority.pubkey();
    let mut wrong_event = context.instruction(&pool);
    wrong_event.accounts[4].pubkey = address(PoolContext::event(&another_pool));
    let mut zero_mint = context.instruction(&pool);
    zero_mint.data = manteon::instruction::CreatePool {
        event: manteon::state::EventDefinition::CompanyIpo {
            token_mint: Pubkey::default(),
        },
        agreement_hash: [0xa1; 32],
        expected_config_hash: context.config_terms_hash(),
        algorithm: manteon::state::PoolAlgorithm::H30,
        opens_at: NOW + 10,
        lock_at: NOW + 20,
        horizon_at: NOW + 30,
    }
    .data();
    for (instruction, expected) in [
        (
            wrong_config,
            rejected(ErrorCode::AccountOwnedByWrongProgram),
        ),
        (wrong_event, rejected(ErrorCode::ConstraintSeeds)),
        (zero_mint, rejected(ManteonError::InvalidEventMint)),
    ] {
        context.assert_rejected(&[instruction], &pool, None, expected);
    }
    let missing_claim_accounts = solana_instruction::Instruction {
        program_id: address(manteon::ID),
        accounts: vec![],
        data: manteon::instruction::Claim {}.data(),
    };
    context.assert_rejected(
        &[context.instruction(&pool), missing_claim_accounts],
        &pool,
        None,
        TransactionError::InstructionError(
            1,
            InstructionError::Custom(u32::from(ErrorCode::AccountNotEnoughKeys)),
        ),
    );
    assert!(context.config.svm.get_account(&pool.pubkey()).is_none());
    assert!(
        context
            .config
            .svm
            .get_account(&address(PoolContext::vault(&pool)))
            .is_none()
    );
    assert!(
        context
            .config
            .svm
            .get_account(&address(PoolContext::event(&pool)))
            .is_none()
    );
    context.create(context.instruction(&pool), &pool);
}

#[test]
fn pool_rejects_empty_agreement_before_creating_market_accounts() {
    let mut context = PoolContext::new();
    let pool = Keypair::new_from_array([0x98; 32]);
    let mut instruction = context.instruction(&pool);
    instruction.data = manteon::instruction::CreatePool {
        event: EventDefinition::CompanyIpo {
            token_mint: context.token_mint,
        },
        agreement_hash: [0; 32],
        expected_config_hash: context.config_terms_hash(),
        algorithm: manteon::state::PoolAlgorithm::H30,
        opens_at: NOW + 10,
        lock_at: NOW + 20,
        horizon_at: NOW + 30,
    }
    .data();
    context.assert_rejected(
        &[instruction],
        &pool,
        None,
        rejected(ManteonError::InvalidAgreement),
    );
    assert!(context.config.svm.get_account(&pool.pubkey()).is_none());
}

#[test]
fn pool_creation_commits_every_algorithm_and_enforces_its_horizon_domain() {
    use manteon::state::PoolAlgorithm;
    for algorithm in [PoolAlgorithm::H30, PoolAlgorithm::H15, PoolAlgorithm::H7] {
        let mut context = PoolContext::new();
        let pool = Keypair::new_from_array([0x91; 32]);
        let data = |horizon_at| {
            manteon::instruction::CreatePool {
                event: manteon::state::EventDefinition::CompanyIpo {
                    token_mint: context.token_mint,
                },
                algorithm,
                agreement_hash: [0xa1; 32],
                expected_config_hash: context.config_terms_hash(),
                opens_at: NOW + 10,
                lock_at: NOW + 20,
                horizon_at,
            }
            .data()
        };
        let last = NOW + 20 + algorithm.max_error_days() * 86400;
        let valid_data = data(last);
        let invalid_data = data(last + 86400);
        let mut invalid = context.instruction(&pool);
        invalid.data = invalid_data;
        context.assert_rejected(
            &[invalid],
            &pool,
            None,
            rejected(ManteonError::UnsupportedDate),
        );
        assert!(context.config.svm.get_account(&pool.pubkey()).is_none());
        let mut valid = context.instruction(&pool);
        valid.data = valid_data;
        context.create(valid, &pool);
        let account = context.config.svm.get_account(&pool.pubkey()).unwrap();
        assert_eq!(
            Pool::try_deserialize(&mut account.data.as_slice())
                .unwrap()
                .algorithm,
            algorithm
        );
    }
}
