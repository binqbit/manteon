use anchor_lang::{AccountDeserialize, AccountSerialize, InstructionData, error::ErrorCode};
use manteon::{
    error::ManteonError,
    state::{Config, Pool, PoolState},
};
use solana_instruction::Instruction;
use solana_instruction_error::InstructionError;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;
use tekmerion_sdk::program::Policy;

use crate::support::{
    OPEN, TestContext,
    setup::{config::address, pool::PoolContext},
};

fn rejected(error: impl Into<u32>) -> TransactionError {
    TransactionError::InstructionError(0, InstructionError::Custom(error.into()))
}

#[test]
fn atomic_creation_and_activation_roll_back_accounts_rent_and_policy_on_failure() {
    let mut context = TestContext::new();
    let pool = Keypair::new_from_array([0xe9; 32]);
    let service = context.service.insecure_clone();
    let create = context.create_pool_ix(&pool);
    let mut activate = context.activate_ix();
    activate.accounts[2].pubkey = pool.pubkey();
    activate.accounts[3].pubkey = address(PoolContext::event(&pool));
    let fail = Instruction {
        program_id: address(manteon::ID),
        accounts: vec![],
        data: manteon::instruction::Claim {}.data(),
    };
    context.reject(
        &[create.clone(), activate.clone(), fail],
        &[&service, &pool],
        TransactionError::InstructionError(
            2,
            InstructionError::Custom(u32::from(ErrorCode::AccountNotEnoughKeys)),
        ),
    );
    for key in [
        pool.pubkey(),
        address(PoolContext::event(&pool)),
        address(PoolContext::vault(&pool)),
    ] {
        assert!(context.svm.get_account(&key).is_none());
    }
    assert_eq!(context.policy_uses(), 0);

    context.send(&[create, activate], &[&service, &pool]);
    context.set_time(OPEN + 1);
    let account = context.svm.get_account(&pool.pubkey()).unwrap();
    let state = Pool::try_deserialize(&mut account.data.as_slice()).unwrap();
    assert_eq!(state.state, PoolState::Active);
    assert_eq!(state.opens_at, OPEN);
    assert_eq!(context.policy_uses(), 1);
    let config = context.svm.get_account(&context.config_address).unwrap();
    assert_eq!(
        Config::try_deserialize(&mut config.data.as_slice())
            .unwrap()
            .active_pools,
        2
    );
}

#[test]
fn activation_closes_at_open_and_cannot_leave_a_policy_use_after_transaction_failure() {
    for timestamp in [OPEN, OPEN + 1] {
        let mut context = TestContext::new();
        let service = context.service.insecure_clone();
        context.set_time(timestamp);

        context.reject(
            &[context.activate_ix()],
            &[&service],
            rejected(ManteonError::InvalidTime),
        );

        assert_eq!(context.pool_state().state, PoolState::Draft);
        assert_eq!(context.policy_uses(), 0);
    }

    let mut context = TestContext::new();
    let service = context.service.insecure_clone();
    context.set_time(OPEN - 1);
    let failing_instruction = Instruction {
        program_id: address(manteon::ID),
        accounts: vec![],
        data: manteon::instruction::Claim {}.data(),
    };

    context.reject(
        &[context.activate_ix(), failing_instruction],
        &[&service],
        TransactionError::InstructionError(
            1,
            InstructionError::Custom(u32::from(ErrorCode::AccountNotEnoughKeys)),
        ),
    );

    assert_eq!(context.pool_state().state, PoolState::Draft);
    assert_eq!(context.policy_uses(), 0);

    context.activate();

    assert_eq!(context.pool_state().state, PoolState::Active);
    assert_eq!(context.policy_uses(), 1);
}

#[test]
fn activation_authenticates_event_policy_client_and_executable() {
    let mut context = TestContext::new();
    let service = context.service.insecure_clone();
    let sibling = context.create_pool(0xe0);

    for (name, index, replacement, code) in [
        (
            "another valid Event cannot authorize this Pool",
            3,
            address(PoolContext::event(&sibling)),
            u32::from(ErrorCode::ConstraintRaw),
        ),
        (
            "another account cannot replace the configured Policy",
            4,
            context.config_address,
            u32::from(ErrorCode::ConstraintAddress),
        ),
        (
            "a service signer cannot replace the client PDA",
            5,
            service.pubkey(),
            u32::from(ErrorCode::ConstraintSeeds),
        ),
        (
            "an executable cannot replace Tekmerion",
            6,
            address(manteon::ID),
            u32::from(ErrorCode::InvalidProgramId),
        ),
    ] {
        println!("Activation rejection: {name}");
        let mut instruction = context.activate_ix();
        instruction.accounts[index].pubkey = replacement;
        context.reject(&[instruction], &[&service], rejected(code));
    }

    let mut unsigned = context.activate_ix();
    unsigned.accounts[0].is_signer = false;
    context.reject(&[unsigned], &[], rejected(ErrorCode::AccountNotSigner));

    context.activate();

    assert_eq!(context.pool_state().state, PoolState::Active);
    assert_eq!(context.policy_uses(), 1);
}

#[test]
fn activation_rejects_malformed_policy_state_before_acquiring_usage() {
    let mut context = TestContext::new();
    let service = context.service.insecure_clone();
    let key = address(context.policy);
    let authentic = context.svm.get_account(&key).unwrap();
    let decode = || Policy::try_deserialize(&mut authentic.data.as_slice()).unwrap();

    let mut wrong_client = decode();
    wrong_client.client_authority = crate::support::setup::config::pubkey(service.pubkey());
    let mut wrong_identity = decode();
    wrong_identity.identifier += 1;
    let mut invalid_rules = decode();
    invalid_rules.rules.threshold = 0;

    let mut variants = Vec::new();
    for (name, policy) in [
        ("client binding", wrong_client),
        ("canonical identity", wrong_identity),
        ("invalid committee", invalid_rules),
    ] {
        let mut account = authentic.clone();
        policy.try_serialize(&mut &mut account.data[..]).unwrap();
        variants.push((name, account));
    }

    let mut wrong_owner = authentic.clone();
    wrong_owner.owner = address(manteon::ID);
    variants.push(("owner", wrong_owner));

    let mut wrong_discriminator = authentic.clone();
    wrong_discriminator.data[0] ^= 1;
    variants.push(("discriminator", wrong_discriminator));

    let mut truncated = authentic.clone();
    truncated.data.pop();
    variants.push(("truncated layout", truncated));

    let mut oversized = authentic.clone();
    oversized.data.push(0);
    variants.push(("extra trailing state", oversized));

    for (name, account) in variants {
        // Deliberate corruption fixtures exercise the external decoder. The valid
        // baseline was created by Tekmerion, and is restored after each rejection.
        println!("Malformed Policy: {name}");
        context.svm.set_account(key, account).unwrap();
        context.reject(
            &[context.activate_ix()],
            &[&service],
            rejected(ManteonError::InvalidAuditAccount),
        );

        assert_eq!(context.pool_state().state, PoolState::Draft);

        context.svm.set_account(key, authentic.clone()).unwrap();
    }

    context.activate();

    assert_eq!(context.policy_uses(), 1);
}
