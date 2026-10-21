use anchor_lang::{InstructionData, error::ErrorCode, prelude::Pubkey};
use manteon::state::EventResult;
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
    setup::{
        config::{ConfigContext, address},
        pool::PoolContext,
        programs::program_svm,
    },
    transactions,
};

const RELAY_ID: Address = Address::new_from_array([0xb3; 32]);

// A test-only native caller exercises real Manteon SBF through the VM's CPI
// boundary. It forwards existing privileges and introduces no PDA signer.
declare_process_instruction!(Relay, 1, |context| {
    let instruction = context
        .transaction_context
        .get_current_instruction_context()?;
    let accounts = (0..instruction.get_number_of_instruction_accounts() - 1)
        .map(|index| {
            Ok(AccountMeta {
                pubkey: *instruction.get_key_of_instruction_account(index)?,
                is_signer: instruction.is_instruction_account_signer(index)?,
                is_writable: instruction.is_instruction_account_writable(index)?,
            })
        })
        .collect::<Result<Vec<_>, InstructionError>>()?;
    let inner = Instruction {
        program_id: address(manteon::ID),
        accounts,
        data: instruction.get_instruction_data().to_vec(),
    };

    context.native_invoke_signed(inner, &[])
});

fn finalization() -> Vec<u8> {
    manteon::instruction::ProposeExternalEvent {
        nonce: 0,
        data: EventResult {
            extra: None,
            occurred_at: 1_800_000_000,
        },
    }
    .data()
}

fn assert_malformed(data: Vec<u8>, expected: ErrorCode) {
    let mut svm = program_svm();
    let payer = Keypair::new_from_array([0x41; 32]);
    let unrelated = Keypair::new_from_array([0x42; 32]).pubkey();
    svm.airdrop(&payer.pubkey(), 10_000_000).unwrap();
    svm.airdrop(&unrelated, svm.minimum_balance_for_rent_exemption(0))
        .unwrap();

    transactions::assert_rejected(
        &mut svm,
        &payer,
        &[Instruction {
            program_id: address(manteon::ID),
            accounts: vec![AccountMeta::new(unrelated, false)],
            data,
        }],
        &[],
        [],
        TransactionError::InstructionError(0, InstructionError::Custom(u32::from(expected))),
    );
}

#[test]
fn anchor_004_malformed_candidate_cannot_reach_the_handler() {
    let mut truncated = finalization();
    truncated.truncate(8);
    assert_malformed(truncated, ErrorCode::InstructionDidNotDeserialize);

    let mut wrong_tag = finalization();
    // Author nonce and shared timestamp precede the optional additional facts.
    wrong_tag[24] = 0xff;
    assert_malformed(wrong_tag, ErrorCode::InstructionDidNotDeserialize);
}

#[test]
fn anchor_016_creation_requires_complete_canonical_instruction_data() {
    let error = InstructionError::Custom(u32::from(ErrorCode::InstructionDidNotDeserialize));
    let mut config = ConfigContext::new();
    let config_key = Keypair::new_from_array([0xb1; 32]);
    let canonical = config.instruction(&config_key);
    let mut appended = canonical.clone();
    appended.data.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    let mut truncated = canonical.clone();
    truncated.data.pop();

    for invalid in [appended, truncated] {
        config.assert_rejected(invalid, &config_key, None, error.clone());
    }

    config.create(canonical, &config_key);

    let mut pool = PoolContext::new();
    let pool_key = Keypair::new_from_array([0xb2; 32]);
    let canonical = pool.instruction(&pool_key);
    let mut appended = canonical.clone();
    appended.data.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    let mut truncated = canonical.clone();
    truncated.data.pop();

    for invalid in [appended, truncated] {
        pool.assert_rejected(
            &[invalid],
            &pool_key,
            None,
            TransactionError::InstructionError(0, error.clone()),
        );
    }

    pool.create(canonical, &pool_key);
}

#[test]
fn anchor_017_financial_operations_require_canonical_encoding() {
    // Every dispatch branch must reject an otherwise valid payload with a suffix.
    // Account validation must not run first or accidentally accept decoded prefixes.
    for mut data in [
        manteon::instruction::InitializeGlobalConfig {
            admin: Pubkey::new_from_array([0x51; 32]),
            fee_authority: Pubkey::new_from_array([0x52; 32]),
            platform_share_bps: 2000,
        }
        .data(),
        manteon::instruction::UpdateGlobalConfig {
            fee_authority: Pubkey::new_from_array([0x52; 32]),
            platform_share_bps: 2000,
        }
        .data(),
        manteon::instruction::ProposeGlobalAdmin {
            new_admin: Pubkey::new_from_array([0x51; 32]),
        }
        .data(),
        manteon::instruction::AcceptGlobalAdmin {}.data(),
        manteon::instruction::WithdrawPlatformFees { amount: 1 }.data(),
        manteon::instruction::CreateConfig {
            fee_recipient: Pubkey::new_from_array([0x60; 32]),
            service_authority: Pubkey::new_from_array([0x51; 32]),
            audit_config: Pubkey::new_from_array([0x52; 32]),
            profit_fee_bps: 0,
            expected_platform_share_bps: 2000,
        }
        .data(),
        manteon::instruction::UpdateConfig {
            fee_recipient: Pubkey::new_from_array([0x60; 32]),
            service_authority: Pubkey::new_from_array([0x51; 32]),
            audit_config: Pubkey::new_from_array([0x52; 32]),
            profit_fee_bps: 1000,
            expected_platform_share_bps: 2000,
        }
        .data(),
        manteon::instruction::CompletePool {}.data(),
        manteon::instruction::CreatePool {
            event: manteon::state::EventDefinition::ExternalEvent,
            agreement_hash: [0xa1; 32],
            expected_config_hash: [0xa2; 32],
            algorithm: manteon::state::PoolAlgorithm::H30,
            opens_at: 1,
            lock_at: 2,
            horizon_at: 3,
        }
        .data(),
        manteon::instruction::ActivatePool {}.data(),
        manteon::instruction::ActivatePricePool {}.data(),
        manteon::instruction::RecordPriceObservation {}.data(),
        manteon::instruction::FinalizePriceThreshold {}.data(),
        manteon::instruction::ExpirePriceObservation {}.data(),
        manteon::instruction::Stake {
            forecast_day: 1,
            amount: 1,
        }
        .data(),
        manteon::instruction::Unstake { amount: 1 }.data(),
        manteon::instruction::Exit {}.data(),
        finalization(),
        manteon::instruction::BeginSettlement {
            data: manteon::state::EventResult {
                extra: None,
                occurred_at: 0,
            },
        }
        .data(),
        manteon::instruction::ProcessPositions {}.data(),
        manteon::instruction::FinalizeSettlement {}.data(),
        manteon::instruction::Claim {}.data(),
    ] {
        let mut truncated = data.clone();
        truncated.pop();
        let error = if truncated.len() < 8 {
            ErrorCode::InstructionFallbackNotFound
        } else {
            ErrorCode::InstructionDidNotDeserialize
        };
        assert_malformed(truncated, error);

        data.push(0);
        assert_malformed(data, ErrorCode::InstructionDidNotDeserialize);
    }

    assert_malformed(vec![0xff; 8], ErrorCode::InstructionFallbackNotFound);
}

#[test]
fn anchor_018_cpi_cannot_bypass_complete_payload_validation() {
    let mut context = ConfigContext::new();
    context.svm.add_builtin(RELAY_ID, Relay::register);
    let config = Keypair::new_from_array([0xb4; 32]);
    let mut canonical = context.instruction(&config);
    canonical.program_id = RELAY_ID;
    canonical
        .accounts
        .push(AccountMeta::new_readonly(address(manteon::ID), false));
    let mut appended = canonical.clone();
    appended.data.push(0);
    context.assert_rejected(
        appended,
        &config,
        None,
        InstructionError::Custom(u32::from(ErrorCode::InstructionDidNotDeserialize)),
    );
    let result = context.create(canonical, &config);

    assert!(
        result
            .logs
            .contains(&format!("Program {} invoke [2]", manteon::ID))
    );
}
