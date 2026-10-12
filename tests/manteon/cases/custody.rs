use anchor_lang::{
    AccountDeserialize, error::ErrorCode, prelude::Pubkey, solana_program::program_pack::Pack,
};
use anchor_spl::token::spl_token;
use manteon::{
    error::ManteonError,
    state::{Pool, PoolState},
};
use solana_address::Address;
use solana_instruction_error::InstructionError;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_system_interface::instruction as system_instruction;
use solana_transaction_error::TransactionError;

use crate::support::setup::{
    config::{address, pubkey},
    pool::PoolContext,
};
use crate::support::transactions;

fn token(context: &PoolContext, key: Address) -> spl_token::state::Account {
    let account = context.config.svm.get_account(&key).unwrap();
    assert_eq!(account.owner, address(spl_token::ID));
    spl_token::state::Account::unpack(&account.data).unwrap()
}

#[test]
fn anchor_011_prediction_custody_is_isolated_and_donations_create_no_participation() {
    let mut context = PoolContext::new();
    let first = Keypair::new_from_array([0x91; 32]);
    let second = Keypair::new_from_array([0x92; 32]);
    let second_vault = address(PoolContext::vault(&second));

    // Funding a public PDA before creation must not let another wallet prevent
    // its later authenticated initialization or gain token authority over it.
    let prefunding = system_instruction::transfer(
        &pubkey(context.config.payer.pubkey()),
        &pubkey(second_vault),
        context.config.svm.minimum_balance_for_rent_exemption(0),
    );
    transactions::send(
        &mut context.config.svm,
        &context.config.payer,
        &[prefunding],
        &[],
    )
    .unwrap();
    let prefunded = context.config.svm.get_account(&second_vault).unwrap();
    assert_eq!(prefunded.owner, address(anchor_lang::system_program::ID));
    assert!(prefunded.data.is_empty());

    for pool in [&first, &second] {
        context.create(context.instruction(pool), pool);
        let vault_key = address(PoolContext::vault(pool));
        let account = context.config.svm.get_account(&vault_key).unwrap();
        assert_eq!(account.data.len(), spl_token::state::Account::LEN);
        assert!(!account.executable);
        assert_eq!(
            account.lamports,
            context
                .config
                .svm
                .minimum_balance_for_rent_exemption(account.data.len())
        );
        let vault = token(&context, vault_key);
        assert_eq!(vault.mint, pubkey(context.config.mint));
        assert_eq!(vault.owner, PoolContext::vault_authority(pool));
        assert_eq!(vault.amount, 0);
        assert_eq!(vault.state, spl_token::state::AccountState::Initialized);
        assert!(vault.delegate.is_none());
        assert_eq!(vault.delegated_amount, 0);
        assert!(vault.close_authority.is_none());
        assert!(vault.is_native.is_none());
    }
    assert_ne!(PoolContext::vault(&first), PoolContext::vault(&second));
    assert_ne!(
        PoolContext::vault_authority(&first),
        PoolContext::vault_authority(&second)
    );
    let protected = [&first, &second]
        .into_iter()
        .flat_map(|pool| [pool.pubkey(), address(PoolContext::event(pool))])
        .map(|key| (key, context.config.svm.get_account(&key).unwrap()))
        .collect::<Vec<_>>();
    let source_before = token(&context, context.config.fee_recipient).amount;
    let donation = spl_token::instruction::transfer_checked(
        &spl_token::ID,
        &pubkey(context.config.fee_recipient),
        &pubkey(context.config.mint),
        &PoolContext::vault(&first),
        &pubkey(context.config.payer.pubkey()),
        &[],
        17,
        6,
    )
    .unwrap();
    transactions::send(
        &mut context.config.svm,
        &context.config.payer,
        &[donation],
        &[],
    )
    .unwrap();
    assert_eq!(
        token(&context, address(PoolContext::vault(&first))).amount,
        17
    );
    assert_eq!(token(&context, second_vault).amount, 0);
    assert_eq!(
        token(&context, context.config.fee_recipient).amount,
        source_before - 17
    );
    for (key, before) in &protected {
        assert_eq!(context.config.svm.get_account(key).as_ref(), Some(before));
    }
    let pool_account = context.config.svm.get_account(&first.pubkey()).unwrap();
    let pool = Pool::try_deserialize(&mut pool_account.data.as_slice()).unwrap();
    assert_eq!(pool.state, PoolState::Draft);
    assert_eq!(
        (pool.principal, pool.active_dates, pool.payout_remaining),
        (0, 0, 0)
    );

    let unauthorized_debit = spl_token::instruction::transfer_checked(
        &spl_token::ID,
        &PoolContext::vault(&first),
        &pubkey(context.config.mint),
        &pubkey(context.config.fee_recipient),
        &pubkey(context.service.pubkey()),
        &[],
        1,
        6,
    )
    .unwrap();
    transactions::assert_rejected(
        &mut context.config.svm,
        &context.config.payer,
        &[unauthorized_debit],
        &[&context.service],
        protected
            .into_iter()
            .map(|(key, _)| key)
            .chain([second_vault]),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(spl_token::error::TokenError::OwnerMismatch as u32),
        ),
    );
}

#[test]
fn anchor_012_custody_substitutions_cannot_create_or_retarget_pool_vaults() {
    let mut context = PoolContext::new();
    let existing = Keypair::new_from_array([0x93; 32]);
    let pool = Keypair::new_from_array([0x94; 32]);
    context.create(context.instruction(&existing), &existing);
    let wrong_mint = token(&context, context.config.other_mint_recipient).mint;
    let token_2022 = Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
    for (name, index, replacement, code) in [
        (
            "different initialized legacy mint",
            5,
            wrong_mint,
            u32::from(ManteonError::InvalidCollateralMint),
        ),
        (
            "different token program",
            8,
            token_2022,
            u32::from(ErrorCode::InvalidProgramId),
        ),
        (
            "another Pool's vault",
            7,
            PoolContext::vault(&existing),
            u32::from(ErrorCode::ConstraintSeeds),
        ),
        (
            "another Pool's authority",
            6,
            PoolContext::vault_authority(&existing),
            u32::from(ErrorCode::ConstraintSeeds),
        ),
        (
            "vault aliases Pool",
            7,
            pubkey(pool.pubkey()),
            u32::from(ErrorCode::ConstraintSeeds),
        ),
        (
            "vault aliases collateral mint",
            7,
            pubkey(context.config.mint),
            u32::from(ErrorCode::ConstraintSeeds),
        ),
        (
            "service substitutes custody authority",
            6,
            pubkey(context.service.pubkey()),
            u32::from(ErrorCode::ConstraintSeeds),
        ),
    ] {
        println!("Custody rejection: {name}");
        let mut instruction = context.instruction(&pool);
        instruction.accounts[index].pubkey = address(replacement);
        context.assert_rejected(
            &[instruction],
            &pool,
            None,
            TransactionError::InstructionError(0, InstructionError::Custom(code)),
        );
        for key in [
            pool.pubkey(),
            address(PoolContext::event(&pool)),
            address(PoolContext::vault(&pool)),
        ] {
            assert!(context.config.svm.get_account(&key).is_none());
        }
    }
    context.create(context.instruction(&pool), &pool);
}
