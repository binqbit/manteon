//! Configuration retention spans drafts, settlement, zero payouts and returns.
use crate::support::{
    DAY, FORECAST, HORIZON, OPEN, TestContext,
    result::ResultFixture,
    settlement::{CaseFixture, claim_ix},
    setup::{
        config::{address, pubkey},
        pool::PoolContext,
    },
};
use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anchor_spl::token::spl_token;
use manteon::{error::ManteonError, state::Config};
use solana_instruction::Instruction;
use solana_instruction_error::InstructionError;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;

fn settings(context: &TestContext) -> Config {
    Config::try_deserialize(
        &mut context
            .svm
            .get_account(&context.config_address)
            .unwrap()
            .data
            .as_slice(),
    )
    .unwrap()
}
fn complete(
    config: anchor_lang::prelude::Pubkey,
    pool: anchor_lang::prelude::Pubkey,
    caller: anchor_lang::prelude::Pubkey,
) -> Instruction {
    Instruction {
        program_id: address(manteon::ID),
        accounts: manteon::accounts::CompletePool {
            caller,
            config,
            pool,
        }
        .to_account_metas(None),
        data: manteon::instruction::CompletePool {}.data(),
    }
}
fn update(context: &TestContext, signer: &Keypair) -> Instruction {
    Instruction {
        program_id: address(manteon::ID),
        accounts: manteon::accounts::UpdateConfig {
            authority: pubkey(signer.pubkey()),
            payer: pubkey(context.payer.pubkey()),
            config: pubkey(context.config_address),
            global_config: manteon::state::GlobalConfig::address(),
            collateral_mint: pubkey(context.mint),
            platform_fee_vault: manteon::state::GlobalConfig::vault_address(&pubkey(context.mint)),
            token_program: spl_token::ID,
            system_program: anchor_lang::system_program::ID,
        }
        .to_account_metas(None),
        data: manteon::instruction::UpdateConfig {
            service_authority: pubkey(context.service.pubkey()),
            audit_config: context.policy,
            fee_recipient: settings(context).fee_recipient,
            profit_fee_bps: 2000,
            expected_platform_share_bps: 2000,
        }
        .data(),
    }
}
fn rejected(error: ManteonError) -> TransactionError {
    TransactionError::InstructionError(0, InstructionError::Custom(error.into()))
}

#[test]
fn multiple_drafts_lock_settings_until_each_releases_once() {
    let mut context = TestContext::new();
    let second = context.create_pool(0xe1);
    let admin = context.authority.insecure_clone();
    let alice = context.alice.insecure_clone();
    assert_eq!(settings(&context).active_pools, 2);
    context.reject(
        &[update(&context, &alice)],
        &[&alice],
        rejected(ManteonError::UnauthorizedConfigAuthority),
    );
    context.reject(
        &[update(&context, &admin)],
        &[&admin],
        rejected(ManteonError::ConfigInUse),
    );
    let first = complete(
        pubkey(context.config_address),
        pubkey(context.pool.pubkey()),
        pubkey(admin.pubkey()),
    );
    context.send(std::slice::from_ref(&first), &[&admin]);
    assert_eq!(settings(&context).active_pools, 1);
    context.reject(
        &[first],
        &[&admin],
        rejected(ManteonError::InvalidPoolState),
    );
    context.reject(
        &[update(&context, &admin)],
        &[&admin],
        rejected(ManteonError::ConfigInUse),
    );
    let finish = complete(
        pubkey(context.config_address),
        pubkey(second.pubkey()),
        pubkey(admin.pubkey()),
    );
    context.send(&[finish], &[&admin]);
    assert_eq!(settings(&context).active_pools, 0);
    context.send(&[update(&context, &admin)], &[&admin]);
    assert_eq!(settings(&context).profit_fee_bps, 2000);
    // Completed Pool fees remain unchanged after the reusable configuration changes.
    assert_eq!(context.pool_state().profit_fee_bps, 0);
    assert!(super::reads::read_context(&context).is_ok());
}

#[test]
fn competitive_completion_waits_for_zero_payout_stakes_despite_rounding_dust() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();
    let bob = context.bob.insecure_clone();
    let service = context.service.insecure_clone();
    context.activate();
    context.set_time(OPEN);
    for (owner, token, day, amount) in [
        (&alice, context.alice_token, FORECAST, 100),
        (&bob, context.bob_token, FORECAST + 30, 1),
        (&alice, context.alice_token, FORECAST + 31, 1),
    ] {
        context.send(&[context.stake_ix(owner, token, day, amount)], &[owner]);
    }
    let mut result = ResultFixture::new(&context);
    context.set_time(FORECAST * DAY);
    result.record_time(&context);
    let case = CaseFixture::start(&mut context, &result);
    case.finalize_yes(&mut context);
    context.send(
        &[
            case.begin_ix(&context, &service),
            case.process_ix(
                &context,
                &service,
                &[FORECAST, FORECAST + 30, FORECAST + 31],
            ),
            case.finalize_ix(&context, &service),
        ],
        &[&service],
    );
    assert_eq!(settings(&context).active_pools, 1);
    context.reject(
        &[complete(
            pubkey(context.config_address),
            pubkey(context.pool.pubkey()),
            pubkey(alice.pubkey()),
        )],
        &[&alice],
        rejected(ManteonError::PoolNotComplete),
    );
    for (index, (owner, token, day)) in [
        (&alice, context.alice_token, FORECAST),
        (&bob, context.bob_token, FORECAST + 30),
        (&alice, context.alice_token, FORECAST + 31),
    ]
    .into_iter()
    .enumerate()
    {
        let before = context.token_balance(token);
        context.send(&[claim_ix(&context, owner, day, pubkey(token))], &[owner]);
        if index > 0 {
            assert_eq!(context.token_balance(token), before);
        }
        assert_eq!(
            settings(&context).active_pools,
            if index == 2 { 0 } else { 1 }
        );
    }
    let pool = context.pool_state();
    assert!(pool.config_released);
    assert_eq!(pool.claimed_principal, pool.principal);
    assert_eq!(pool.payout_remaining, 1);
    assert_eq!(
        super::reads::read_context(&context)
            .unwrap()
            .unallocated_balance,
        1
    );
    assert!(super::reads::read_context(&context).is_ok());
}

#[test]
fn full_horizon_exit_and_permissionless_empty_completion_release_settings() {
    let mut context = TestContext::new();
    let alice = context.alice.insecure_clone();
    context.activate();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );
    context.reject(
        &[complete(
            pubkey(context.config_address),
            pubkey(context.pool.pubkey()),
            pubkey(alice.pubkey()),
        )],
        &[&alice],
        rejected(ManteonError::PoolNotComplete),
    );
    context.set_time(HORIZON);
    context.send(
        &[context.exit_ix(&alice, context.alice_token, FORECAST)],
        &[&alice],
    );
    assert_eq!(settings(&context).active_pools, 0);
    assert!(context.pool_state().config_released);
    let mut empty = TestContext::new();
    empty.activate();
    empty.set_time(HORIZON);
    let stranger = empty.alice.insecure_clone();
    empty.send(
        &[complete(
            pubkey(empty.config_address),
            pubkey(empty.pool.pubkey()),
            pubkey(stranger.pubkey()),
        )],
        &[&stranger],
    );
    assert_eq!(settings(&empty).active_pools, 0);
}

#[test]
fn stale_reviewed_config_terms_reject_pool_creation_atomically() {
    let mut context = PoolContext::new();
    let pool = Keypair::new_from_array([0xe2; 32]);
    let reviewed = context.instruction(&pool);
    let account = context
        .config
        .svm
        .get_account(&context.config_address)
        .unwrap();
    let mut config = Config::try_deserialize(&mut account.data.as_slice()).unwrap();
    config.profit_fee_bps = 1000;
    use anchor_lang::AccountSerialize;
    let mut bytes = Vec::new();
    config.try_serialize(&mut bytes).unwrap();
    let mut changed = account;
    changed.data = bytes;
    context
        .config
        .svm
        .set_account(context.config_address, changed)
        .unwrap();
    context.assert_rejected(
        &[reviewed],
        &pool,
        None,
        rejected(ManteonError::ConfigTermsChanged),
    );
    assert!(context.config.svm.get_account(&pool.pubkey()).is_none());
    context.create(context.instruction(&pool), &pool);
    let account = context
        .config
        .svm
        .get_account(&context.config_address)
        .unwrap();
    assert_eq!(
        Config::try_deserialize(&mut account.data.as_slice())
            .unwrap()
            .active_pools,
        1
    );
}

#[test]
fn empty_competitive_settlement_releases_without_waiting_for_claims() {
    let mut context = TestContext::new();
    let service = context.service.insecure_clone();
    context.activate();
    let mut result = ResultFixture::new(&context);
    context.set_time(FORECAST * DAY);
    result.record_time(&context);
    let case = CaseFixture::start(&mut context, &result);
    case.finalize_yes(&mut context);
    context.send(
        &[
            case.begin_ix(&context, &service),
            case.finalize_ix(&context, &service),
        ],
        &[&service],
    );
    assert_eq!(settings(&context).active_pools, 0);
    assert!(context.pool_state().config_released);
    context.reject(
        &[case.finalize_ix(&context, &service)],
        &[&service],
        rejected(ManteonError::InvalidPoolState),
    );
}
