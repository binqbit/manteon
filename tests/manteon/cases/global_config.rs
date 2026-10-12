//! Governance isolation, exact treasury transfers and PDA-authorized CPI execution.

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas, prelude::Pubkey};
use anchor_spl::token::spl_token;
use manteon::{
    error::ManteonError,
    state::{Config, GlobalConfig},
};
use solana_address::Address;
use solana_instruction::{AccountMeta, Instruction};
use solana_instruction_error::InstructionError;
use solana_keypair::Keypair;
use solana_program_runtime::{
    declare_process_instruction, solana_sbpf::program::BuiltinFunctionDefinition,
};
use solana_signer::Signer;
use solana_transaction_error::TransactionError;

use crate::support::{
    setup::config::{ConfigContext, address, pubkey},
    transactions,
};

fn global(context: &ConfigContext) -> GlobalConfig {
    let account = context
        .svm
        .get_account(&address(GlobalConfig::address()))
        .unwrap();
    GlobalConfig::try_deserialize(&mut account.data.as_slice()).unwrap()
}

fn manage(authority: Address, data: Vec<u8>) -> Instruction {
    Instruction {
        program_id: address(manteon::ID),
        accounts: manteon::accounts::ManageGlobalConfig {
            authority: pubkey(authority),
            global_config: GlobalConfig::address(),
        }
        .to_account_metas(None),
        data,
    }
}

fn withdraw(context: &ConfigContext, authority: Address, amount: u64) -> Instruction {
    Instruction {
        program_id: address(manteon::ID),
        accounts: manteon::accounts::WithdrawPlatformFees {
            authority: pubkey(authority),
            global_config: GlobalConfig::address(),
            collateral_mint: pubkey(context.mint),
            platform_fee_vault: GlobalConfig::vault_address(&pubkey(context.mint)),
            destination: pubkey(context.fee_recipient),
            token_program: spl_token::ID,
        }
        .to_account_metas(None),
        data: manteon::instruction::WithdrawPlatformFees { amount }.data(),
    }
}

fn reject(context: &mut ConfigContext, ix: Instruction, signer: &Keypair, code: impl Into<u32>) {
    transactions::assert_rejected(
        &mut context.svm,
        &context.payer,
        &[ix],
        &[signer],
        [],
        TransactionError::InstructionError(0, InstructionError::Custom(code.into())),
    );
}

fn fund_vault(context: &mut ConfigContext, amount: u64) -> Address {
    let config = Keypair::new_from_array([0xd8; 32]);
    context.create(context.instruction(&config), &config);
    let vault = GlobalConfig::vault_address(&pubkey(context.mint));
    let ix: Instruction = spl_token::instruction::mint_to(
        &spl_token::ID,
        &pubkey(context.mint),
        &vault,
        &pubkey(context.payer.pubkey()),
        &[],
        amount,
    )
    .unwrap();
    transactions::send(&mut context.svm, &context.payer, &[ix], &[]).unwrap();
    address(vault)
}

#[test]
fn global_roles_require_nonzero_identities_at_initialization() {
    let mut context = ConfigContext::with_platform_share(None);
    let signer = context.authority.insecure_clone();
    for (admin, fee_authority) in [
        (Pubkey::default(), pubkey(signer.pubkey())),
        (pubkey(signer.pubkey()), Pubkey::default()),
    ] {
        let mut ix = context.initialize_global_config_instruction(2000);
        ix.data = manteon::instruction::InitializeGlobalConfig {
            admin,
            fee_authority,
            platform_share_bps: 2000,
        }
        .data();
        reject(
            &mut context,
            ix,
            &signer,
            ManteonError::InvalidGlobalAuthority,
        );
        assert!(
            context
                .svm
                .get_account(&address(GlobalConfig::address()))
                .is_none()
        );
    }
}

#[test]
fn global_updates_preserve_market_fee_snapshots_and_require_accepted_admin_transfer() {
    let mut context = ConfigContext::new();
    let admin = context.authority.insecure_clone();
    let outsider = context.payer.insecure_clone();
    let next = Keypair::new_from_array([0xd9; 32]);
    context.svm.airdrop(&next.pubkey(), 1_000_000).unwrap();
    let first = Keypair::new_from_array([0xda; 32]);
    context.create(context.instruction(&first), &first);
    assert_eq!(global(&context).admin, pubkey(admin.pubkey()));
    let update = |authority, recipient, share| {
        manage(
            authority,
            manteon::instruction::UpdateGlobalConfig {
                fee_authority: recipient,
                platform_share_bps: share,
            }
            .data(),
        )
    };
    reject(
        &mut context,
        update(outsider.pubkey(), pubkey(next.pubkey()), 3000),
        &outsider,
        ManteonError::UnauthorizedGlobalAdmin,
    );
    reject(
        &mut context,
        update(admin.pubkey(), Pubkey::default(), 3000),
        &admin,
        ManteonError::InvalidGlobalAuthority,
    );
    reject(
        &mut context,
        update(admin.pubkey(), pubkey(next.pubkey()), 10001),
        &admin,
        ManteonError::InvalidFeeRates,
    );
    transactions::send(
        &mut context.svm,
        &context.payer,
        &[update(admin.pubkey(), pubkey(next.pubkey()), 3000)],
        &[&admin],
    )
    .unwrap();
    let stored = context.svm.get_account(&first.pubkey()).unwrap();
    assert_eq!(
        Config::try_deserialize(&mut stored.data.as_slice())
            .unwrap()
            .platform_share_bps,
        2000
    );
    let second = Keypair::new_from_array([0xdb; 32]);
    context.create(context.instruction(&second), &second);
    let stored = context.svm.get_account(&second.pubkey()).unwrap();
    assert_eq!(
        Config::try_deserialize(&mut stored.data.as_slice())
            .unwrap()
            .platform_share_bps,
        3000
    );
    let propose = manage(
        admin.pubkey(),
        manteon::instruction::ProposeGlobalAdmin {
            new_admin: pubkey(next.pubkey()),
        }
        .data(),
    );
    transactions::send(&mut context.svm, &context.payer, &[propose], &[&admin]).unwrap();
    assert_eq!(global(&context).admin, pubkey(admin.pubkey()));
    reject(
        &mut context,
        update(next.pubkey(), pubkey(next.pubkey()), 4000),
        &next,
        ManteonError::UnauthorizedGlobalAdmin,
    );
    reject(
        &mut context,
        manage(
            outsider.pubkey(),
            manteon::instruction::AcceptGlobalAdmin {}.data(),
        ),
        &outsider,
        ManteonError::UnauthorizedGlobalAdmin,
    );
    let accept = manage(
        next.pubkey(),
        manteon::instruction::AcceptGlobalAdmin {}.data(),
    );
    transactions::send(&mut context.svm, &context.payer, &[accept], &[&next]).unwrap();
    assert_eq!(global(&context).admin, pubkey(next.pubkey()));
    assert_eq!(global(&context).pending_admin, None);
    reject(
        &mut context,
        update(admin.pubkey(), pubkey(admin.pubkey()), 2000),
        &admin,
        ManteonError::UnauthorizedGlobalAdmin,
    );
    // A cancelled nomination never replaces the administrator.
    let nominate = manage(
        next.pubkey(),
        manteon::instruction::ProposeGlobalAdmin {
            new_admin: pubkey(admin.pubkey()),
        }
        .data(),
    );
    let cancel = manage(
        next.pubkey(),
        manteon::instruction::ProposeGlobalAdmin {
            new_admin: pubkey(next.pubkey()),
        }
        .data(),
    );
    transactions::send(
        &mut context.svm,
        &context.payer,
        &[nominate, cancel],
        &[&next],
    )
    .unwrap();
    assert_eq!(global(&context).pending_admin, None);
}

#[test]
fn platform_withdrawals_use_only_the_fee_authority_and_canonical_collection_balance() {
    let mut context = ConfigContext::new();
    let admin = context.authority.insecure_clone();
    let treasury = context.payer.insecure_clone();
    let vault = fund_vault(&mut context, 1000);
    let configure = manage(
        admin.pubkey(),
        manteon::instruction::UpdateGlobalConfig {
            fee_authority: pubkey(treasury.pubkey()),
            platform_share_bps: 2000,
        }
        .data(),
    );
    transactions::send(&mut context.svm, &context.payer, &[configure], &[&admin]).unwrap();
    let ix = withdraw(&context, admin.pubkey(), 1);
    reject(
        &mut context,
        ix,
        &admin,
        ManteonError::UnauthorizedFeeAuthority,
    );
    for (amount, error) in [
        (0, ManteonError::InvalidAmount),
        (1001, ManteonError::InsufficientBacking),
    ] {
        let ix = withdraw(&context, treasury.pubkey(), amount);
        reject(&mut context, ix, &treasury, error);
    }
    for destination in [context.other_mint_recipient, context.frozen_recipient] {
        let mut ix = withdraw(&context, treasury.pubkey(), 1);
        ix.accounts[4].pubkey = destination;
        reject(
            &mut context,
            ix,
            &treasury,
            ManteonError::InvalidTokenAccount,
        );
    }
    let mut wrong_vault = withdraw(&context, treasury.pubkey(), 1);
    wrong_vault.accounts[3].pubkey = context.frozen_recipient;
    reject(
        &mut context,
        wrong_vault,
        &treasury,
        anchor_lang::error::ErrorCode::ConstraintSeeds,
    );
    let mut alias = withdraw(&context, treasury.pubkey(), 1);
    alias.accounts[4].pubkey = vault;
    reject(
        &mut context,
        alias,
        &treasury,
        anchor_lang::error::ErrorCode::ConstraintDuplicateMutableAccount,
    );
    let before = context.svm.get_account(&context.fee_recipient).unwrap();
    for amount in [400, 600] {
        let ix = withdraw(&context, treasury.pubkey(), amount);
        transactions::send(&mut context.svm, &context.payer, &[ix], &[&treasury]).unwrap();
    }
    use anchor_lang::solana_program::program_pack::Pack;
    let after = context.svm.get_account(&context.fee_recipient).unwrap();
    assert_eq!(
        spl_token::state::Account::unpack(&after.data)
            .unwrap()
            .amount,
        spl_token::state::Account::unpack(&before.data)
            .unwrap()
            .amount
            + 1000
    );
    assert_eq!(
        spl_token::state::Account::unpack(&context.svm.get_account(&vault).unwrap().data)
            .unwrap()
            .amount,
        0
    );
}

const PROXY: Address = Address::new_from_array([0xdc; 32]);
// Test-only CPI signer. Production multisig quorum checks belong to its program.
declare_process_instruction!(GovernanceProxy, 1, |context| {
    let ix = context
        .transaction_context
        .get_current_instruction_context()?;
    let (authority, bump) = Address::find_program_address(&[b"governance"], &PROXY);
    let accounts = (0..ix.get_number_of_instruction_accounts() - 1)
        .map(|index| {
            let pubkey = *ix.get_key_of_instruction_account(index)?;
            Ok(AccountMeta {
                pubkey,
                is_signer: pubkey == authority,
                is_writable: ix.is_instruction_account_writable(index)?,
            })
        })
        .collect::<Result<Vec<_>, InstructionError>>()?;
    let inner = Instruction {
        program_id: address(manteon::ID),
        accounts,
        data: ix.get_instruction_data().to_vec(),
    };
    context.native_invoke_signed(inner, &[&[b"governance", &[bump]]])
});

#[test]
fn signing_pdas_can_accept_admin_update_settings_and_withdraw_through_cpi() {
    let mut context = ConfigContext::new();
    context.svm.add_builtin(PROXY, GovernanceProxy::register);
    let (authority, _) = Address::find_program_address(&[b"governance"], &PROXY);
    assert!(!pubkey(authority).is_on_curve());
    context.svm.airdrop(&authority, 1_000_000).unwrap();
    let admin = context.authority.insecure_clone();
    fund_vault(&mut context, 100);
    let nominate = manage(
        admin.pubkey(),
        manteon::instruction::ProposeGlobalAdmin {
            new_admin: pubkey(authority),
        }
        .data(),
    );
    transactions::send(&mut context.svm, &context.payer, &[nominate], &[&admin]).unwrap();
    let relay = |mut ix: Instruction| {
        ix.program_id = PROXY;
        ix.accounts[0].is_signer = false;
        ix.accounts
            .push(AccountMeta::new_readonly(address(manteon::ID), false));
        ix
    };
    let accept = relay(manage(
        authority,
        manteon::instruction::AcceptGlobalAdmin {}.data(),
    ));
    let update = relay(manage(
        authority,
        manteon::instruction::UpdateGlobalConfig {
            fee_authority: pubkey(authority),
            platform_share_bps: 3000,
        }
        .data(),
    ));
    transactions::send(&mut context.svm, &context.payer, &[accept, update], &[]).unwrap();
    assert_eq!(global(&context).admin, pubkey(authority));
    assert_eq!(global(&context).fee_authority, pubkey(authority));
    let ix = relay(withdraw(&context, authority, 100));
    let receipt = transactions::send(&mut context.svm, &context.payer, &[ix], &[]).unwrap();
    assert!(
        receipt
            .logs
            .contains(&format!("Program {} invoke [2]", manteon::ID))
    );
}

#[test]
fn config_creation_rejects_changed_global_fee_terms_instead_of_rewriting_the_review() {
    let mut context = ConfigContext::new();
    let config = Keypair::new_from_array([0xdd; 32]);
    let reviewed = context.instruction(&config);
    let admin = context.authority.insecure_clone();
    let update = manage(
        admin.pubkey(),
        manteon::instruction::UpdateGlobalConfig {
            fee_authority: pubkey(admin.pubkey()),
            platform_share_bps: 3000,
        }
        .data(),
    );
    transactions::send(&mut context.svm, &context.payer, &[update], &[&admin]).unwrap();
    transactions::assert_rejected(
        &mut context.svm,
        &context.payer,
        &[reviewed],
        &[&admin, &config],
        [],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ManteonError::FeeTermsChanged.into()),
        ),
    );
    assert!(context.svm.get_account(&config.pubkey()).is_none());
    assert!(
        context
            .svm
            .get_account(&address(GlobalConfig::vault_address(&pubkey(context.mint))))
            .is_none()
    );
    context.create(context.instruction(&config), &config);
    let stored = context.svm.get_account(&config.pubkey()).unwrap();
    assert_eq!(
        Config::try_deserialize(&mut stored.data.as_slice())
            .unwrap()
            .platform_share_bps,
        3000
    );
}
