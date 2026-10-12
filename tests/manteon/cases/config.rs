use anchor_lang::{
    AccountDeserialize, Discriminator, InstructionData,
    error::ErrorCode,
    prelude::Pubkey,
    solana_program::{bpf_loader_upgradeable, program_pack::Pack},
};
use anchor_spl::token::spl_token;
use manteon::{
    error::ManteonError,
    state::{Config, GlobalConfig},
};
use solana_instruction::Instruction;
use solana_instruction_error::InstructionError;
use solana_keypair::Keypair;
use solana_signer::Signer;

use crate::support::setup::config::{ConfigContext, address, pubkey};
use crate::support::transactions;

fn replace_account(instruction: &mut Instruction, from: Pubkey, to: Pubkey) {
    instruction
        .accounts
        .iter_mut()
        .find(|meta| meta.pubkey == address(from))
        .expect("generated instruction includes expected account")
        .pubkey = address(to);
}

#[test]
fn permissionless_configs_snapshot_fees_without_moving_tokens() {
    let mut context = ConfigContext::new();
    assert_ne!(context.authority.pubkey(), context.payer.pubkey());
    assert_ne!(
        pubkey(context.authority.pubkey()),
        context.service_authority
    );
    assert_ne!(pubkey(context.payer.pubkey()), context.service_authority);
    context.profit_fee_bps = 2000;
    let tokens_before = context.asset_snapshot();
    let first = Keypair::new_from_array([0x71; 32]);
    let second = Keypair::new_from_array([0x72; 32]);

    for config_keypair in [&first, &second] {
        let payer_before = context.svm.get_account(&context.payer.pubkey()).unwrap();
        let fee_vault = address(GlobalConfig::vault_address(&pubkey(context.mint)));
        let vault_rent = if context.svm.get_account(&fee_vault).is_none() {
            context
                .svm
                .minimum_balance_for_rent_exemption(spl_token::state::Account::LEN)
        } else {
            0
        };
        let instruction = context.instruction(config_keypair);
        let metadata = context.create(instruction, config_keypair);
        let account = context.svm.get_account(&config_keypair.pubkey()).unwrap();
        assert_eq!(account.owner, address(manteon::ID));
        assert!(!account.executable);
        assert_eq!(
            account.data.len(),
            8 + 5 * 32 + 4 + 8,
            "retained configuration schema"
        );
        assert_eq!(&account.data[..8], Config::DISCRIMINATOR);
        assert_eq!(
            account.lamports,
            context
                .svm
                .minimum_balance_for_rent_exemption(account.data.len())
        );
        let config = Config::try_deserialize(&mut account.data.as_slice()).unwrap();
        assert_eq!(config.service_authority, context.service_authority);
        assert_eq!(config.collateral_mint, pubkey(context.mint));
        assert_eq!(config.fee_recipient, pubkey(context.fee_recipient));
        assert_eq!(config.audit_config, context.audit_config);
        assert_eq!(config.profit_fee_bps, 2000);
        assert_eq!(config.platform_share_bps, 2000);
        assert_eq!(context.asset_snapshot(), tokens_before);
        let payer_after = context.svm.get_account(&context.payer.pubkey()).unwrap();
        assert_eq!(
            payer_before.lamports - payer_after.lamports,
            account.lamports + vault_rent + metadata.fee,
            "initialization charges Config rent, first vault rent and transaction fee"
        );
    }

    let mut repeated = context.instruction(&first);
    repeated.data = manteon::instruction::CreateConfig {
        service_authority: Pubkey::new_from_array([0x73; 32]),
        audit_config: Pubkey::new_from_array([0x74; 32]),
        profit_fee_bps: 0,
        expected_platform_share_bps: 2000,
    }
    .data();
    context.assert_rejected(repeated, &first, None, InstructionError::Custom(0));
}

#[test]
fn anchor_007_config_creation_enforces_authority_assets_and_atomic_rollback() {
    let mut context = ConfigContext::new();
    let config = Keypair::new_from_array([0x75; 32]);
    let base = context.instruction(&config);
    let token_2022_id = Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
    let mut wrong_platform = base.clone();
    replace_account(
        &mut wrong_platform,
        GlobalConfig::address(),
        context.audit_config,
    );
    let mut wrong_mint = base.clone();
    replace_account(
        &mut wrong_mint,
        pubkey(context.fee_recipient),
        pubkey(context.other_mint_recipient),
    );
    let mut frozen = base.clone();
    replace_account(
        &mut frozen,
        pubkey(context.fee_recipient),
        pubkey(context.frozen_recipient),
    );
    let mut wrong_token_program = base.clone();
    replace_account(&mut wrong_token_program, spl_token::ID, token_2022_id);
    let mut uninitialized_mint = base.clone();
    replace_account(
        &mut uninitialized_mint,
        pubkey(context.mint),
        pubkey(context.uninitialized_mint),
    );
    let mut zero_service = base.clone();
    zero_service.data = manteon::instruction::CreateConfig {
        service_authority: Pubkey::default(),
        audit_config: context.audit_config,
        profit_fee_bps: 0,
        expected_platform_share_bps: 2000,
    }
    .data();
    let mut zero_audit = base;
    zero_audit.data = manteon::instruction::CreateConfig {
        service_authority: context.service_authority,
        audit_config: Pubkey::default(),
        profit_fee_bps: 0,
        expected_platform_share_bps: 2000,
    }
    .data();

    for (name, instruction, error) in [
        (
            "unrelated platform policy",
            wrong_platform,
            InstructionError::Custom(u32::from(ErrorCode::AccountNotInitialized)),
        ),
        (
            "recipient for a different mint",
            wrong_mint,
            InstructionError::Custom(u32::from(ManteonError::InvalidFeeRecipient)),
        ),
        (
            "frozen fee recipient",
            frozen,
            InstructionError::Custom(u32::from(ManteonError::InvalidFeeRecipient)),
        ),
        (
            "wrong token program",
            wrong_token_program,
            InstructionError::Custom(u32::from(ErrorCode::InvalidProgramId)),
        ),
        (
            "uninitialized mint",
            uninitialized_mint,
            InstructionError::UninitializedAccount,
        ),
        (
            "zero service authority",
            zero_service,
            InstructionError::Custom(u32::from(ManteonError::InvalidServiceAuthority)),
        ),
        (
            "zero audit reference",
            zero_audit,
            InstructionError::Custom(u32::from(ManteonError::InvalidAuditConfig)),
        ),
    ] {
        println!("Config rejection: {name}");
        context.assert_rejected(instruction, &config, None, error);
        assert!(context.svm.get_account(&config.pubkey()).is_none());
    }
}

#[test]
fn anchor_007_config_rejects_native_sol_collateral_without_committing_rent() {
    let mut context = ConfigContext::new();
    let config = Keypair::new_from_array([0x77; 32]);
    let native_recipient = context.create_native_token_account(0x78, 1_000_000);
    let native_mint = context
        .svm
        .get_account(&address(spl_token::native_mint::ID))
        .unwrap();
    assert_eq!(native_mint.owner, address(spl_token::ID));
    let mint = spl_token::state::Mint::unpack(&native_mint.data).unwrap();
    assert_eq!(mint.decimals, spl_token::native_mint::DECIMALS);

    let recipient = context.svm.get_account(&native_recipient).unwrap();
    assert_eq!(recipient.owner, address(spl_token::ID));
    let token = spl_token::state::Account::unpack(&recipient.data).unwrap();
    assert_eq!(token.mint, spl_token::native_mint::ID);
    assert_eq!(token.owner, pubkey(context.payer.pubkey()));
    assert_eq!(token.state, spl_token::state::AccountState::Initialized);
    assert!(token.is_native.is_some());
    assert_eq!(token.amount, 1_000_000);

    let mut instruction = context.instruction(&config);
    replace_account(
        &mut instruction,
        pubkey(context.mint),
        spl_token::native_mint::ID,
    );
    replace_account(
        &mut instruction,
        pubkey(context.fee_recipient),
        pubkey(native_recipient),
    );
    replace_account(
        &mut instruction,
        GlobalConfig::vault_address(&pubkey(context.mint)),
        GlobalConfig::vault_address(&spl_token::native_mint::ID),
    );
    // The shared rejection assertion checks account bytes and all asset balances;
    // payer may lose only the transaction fee, never Config rent or wrapped SOL.
    context.assert_rejected(
        instruction,
        &config,
        None,
        InstructionError::Custom(u32::from(ManteonError::UnsupportedCollateral)),
    );
    assert!(context.svm.get_account(&config.pubkey()).is_none());
}

#[test]
fn configs_remain_permissionless_after_upgrade_authority_revocation() {
    let mut context = ConfigContext::new();
    let original = context.authority.insecure_clone();
    let author = Keypair::new_from_array([0xe3; 32]);
    context.svm.airdrop(&author.pubkey(), 1_000_000).unwrap();
    let revoke = bpf_loader_upgradeable::set_upgrade_authority(
        &manteon::ID,
        &pubkey(original.pubkey()),
        None,
    );
    transactions::send(&mut context.svm, &context.payer, &[revoke], &[&original]).unwrap();
    context.authority = author;
    let config = Keypair::new_from_array([0xe4; 32]);
    context.create(context.instruction(&config), &config);
    assert!(context.svm.get_account(&config.pubkey()).is_some());
}

#[test]
fn platform_policy_requires_upgrade_authority_and_cannot_be_reinitialized() {
    let mut context = ConfigContext::with_platform_share(None);
    let hostile = Keypair::new_from_array([0xe5; 32]);
    context.svm.airdrop(&hostile.pubkey(), 1_000_000).unwrap();
    let base = context.initialize_global_config_instruction(2000);
    let mut unauthorized = base.clone();
    replace_account(
        &mut unauthorized,
        pubkey(context.authority.pubkey()),
        pubkey(hostile.pubkey()),
    );
    transactions::assert_rejected(
        &mut context.svm,
        &context.payer,
        &[unauthorized],
        &[&hostile],
        [address(GlobalConfig::address())],
        solana_transaction_error::TransactionError::InstructionError(
            0,
            InstructionError::Custom(u32::from(ManteonError::UnauthorizedPlatform)),
        ),
    );
    let invalid = context.initialize_global_config_instruction(10001);
    transactions::assert_rejected(
        &mut context.svm,
        &context.payer,
        &[invalid],
        &[&context.authority],
        [address(GlobalConfig::address())],
        solana_transaction_error::TransactionError::InstructionError(
            0,
            InstructionError::Custom(u32::from(ManteonError::InvalidFeeRates)),
        ),
    );
    transactions::send(
        &mut context.svm,
        &context.payer,
        &[base],
        &[&context.authority],
    )
    .unwrap();
    let account = context
        .svm
        .get_account(&address(GlobalConfig::address()))
        .unwrap();
    assert_eq!(
        GlobalConfig::try_deserialize(&mut account.data.as_slice())
            .unwrap()
            .platform_share_bps,
        2000
    );
    let repeated = context.initialize_global_config_instruction(9999);
    assert!(
        transactions::send(
            &mut context.svm,
            &context.payer,
            &[repeated],
            &[&context.authority]
        )
        .is_err()
    );
    assert_eq!(
        context
            .svm
            .get_account(&address(GlobalConfig::address()))
            .unwrap(),
        account
    );
}

#[test]
fn config_profit_fee_cap_is_enforced_by_contract() {
    let mut context = ConfigContext::new();
    context.profit_fee_bps = 2001;
    let config = Keypair::new_from_array([0xe6; 32]);
    context.assert_rejected(
        context.instruction(&config),
        &config,
        None,
        InstructionError::Custom(u32::from(ManteonError::InvalidFeeRates)),
    );
    assert!(
        context
            .svm
            .get_account(&address(GlobalConfig::vault_address(&pubkey(context.mint))))
            .is_none()
    );
}
