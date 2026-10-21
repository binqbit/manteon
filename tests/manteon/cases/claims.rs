use anchor_lang::{error::ErrorCode, solana_program::program_pack::Pack};
use anchor_spl::token::spl_token;
use manteon::{
    error::ManteonError,
    state::{PoolState, StakeState},
};
use solana_instruction_error::InstructionError;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;

use super::support::{
    DAY, FORECAST, OPEN, OWNER_BALANCE, TestContext,
    result::ResultFixture,
    settlement::{CaseFixture, claim_ix},
    setup::{
        config::{address, pubkey},
        pool::PoolContext,
    },
};

fn custom(index: u8, code: impl Into<u32>) -> TransactionError {
    TransactionError::InstructionError(index, InstructionError::Custom(code.into()))
}

#[test]
fn profitable_claims_split_fees_and_losses_remain_fee_free() {
    for owner_is_recipient in [false, true] {
        let mut context = TestContext::with_profit_fee_recipient(2000, owner_is_recipient);
        let alice = context.alice.insecure_clone();
        let bob = context.bob.insecure_clone();
        let service = context.service.insecure_clone();
        context.activate();
        context.set_time(OPEN);
        context.send(
            &[
                context.stake_ix(&alice, context.alice_token, FORECAST, 1000),
                context.stake_ix(&bob, context.bob_token, FORECAST + 30, 1000),
            ],
            &[&alice, &bob],
        );
        let mut result = ResultFixture::new(&context);
        context.set_time(FORECAST * DAY);
        result.record_time(&context);
        let case = CaseFixture::start(&mut context, &result);
        case.finalize_yes(&mut context);
        context.send(
            &[
                case.begin_ix(&context, &service),
                case.process_ix(&context, &service, &[FORECAST, FORECAST + 30]),
                case.finalize_ix(&context, &service),
            ],
            &[&service],
        );
        // Gross 1333, profit 333, fee 66 = platform 13 + recipient 53.
        let platform = address(manteon::state::GlobalConfig::vault_address(&pubkey(
            context.mint,
        )));
        let recipient_before = context.token_balance(context.fee_recipient);
        let alice_before = context.token_balance(context.alice_token);
        let claim = claim_ix(&context, &alice, FORECAST, pubkey(context.alice_token));
        let mut redirected = claim.clone();
        redirected.accounts[10].pubkey = context.bob_token;
        context.reject(
            &[redirected],
            &[&alice],
            custom(0, ManteonError::InvalidFeeRecipient),
        );
        context.send(std::slice::from_ref(&claim), &[&alice]);
        assert_eq!(
            context.token_balance(context.alice_token),
            alice_before + 1267 + if owner_is_recipient { 53 } else { 0 }
        );
        assert_eq!(
            context.token_balance(context.fee_recipient),
            recipient_before + 53 + if owner_is_recipient { 1267 } else { 0 }
        );
        assert_eq!(context.token_balance(platform), 13);
        assert_eq!(context.pool_state().payout_remaining, 667);
        context.reject(&[claim], &[&alice], custom(0, ManteonError::InvalidStake));
        // Gross 666 < principal; no fee and no dependency on the fee recipient.
        let mut loss = claim_ix(&context, &bob, FORECAST + 30, pubkey(context.bob_token));
        loss.accounts[10].pubkey = address(manteon::ID);
        loss.accounts[10].is_writable = false;
        let bob_before = context.token_balance(context.bob_token);
        context.send(&[loss], &[&bob]);
        assert_eq!(context.token_balance(context.bob_token), bob_before + 666);
        assert_eq!(context.token_balance(platform), 13);
        assert_eq!(
            context.token_balance(context.fee_recipient),
            recipient_before + 53 + if owner_is_recipient { 1267 } else { 0 }
        );
        assert_eq!(context.pool_state().payout_remaining, 1);
        assert_eq!(
            context.token_balance(address(PoolContext::vault(&context.pool))),
            1
        );
    }
}

#[test]
fn every_claim_order_preserves_entitlements_frozen_basis_and_rounding_residue() {
    let orders = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for order in orders {
        let mut context = TestContext::new();
        let alice = context.alice.insecure_clone();
        let bob = context.bob.insecure_clone();
        let service = context.service.insecure_clone();
        context.activate();
        context.set_time(OPEN);
        for (owner, token, day, amount) in [
            (&alice, context.alice_token, FORECAST, 101),
            (&bob, context.bob_token, FORECAST, 7),
            (&alice, context.alice_token, FORECAST + 30, 100),
        ] {
            context.send(&[context.stake_ix(owner, token, day, amount)], &[owner]);
        }
        let mut result = ResultFixture::new(&context);
        context.set_time(FORECAST * DAY);
        result.record_time(&context);
        let case = CaseFixture::start(&mut context, &result);
        case.finalize_yes(&mut context);
        context.send(&[case.begin_ix(&context, &service)], &[&service]);
        context.send(
            &[
                case.process_ix(&context, &service, &[FORECAST + 30, FORECAST]),
                case.finalize_ix(&context, &service),
            ],
            &[&service],
        );
        let fee_before = context.token_balance(context.fee_recipient);
        let vault = PoolContext::vault(&context.pool);
        // Surplus received after finalization cannot increase any entitlement.
        let donation = spl_token::instruction::transfer(
            &spl_token::ID,
            &pubkey(context.bob_token),
            &vault,
            &pubkey(bob.pubkey()),
            &[],
            17,
        )
        .unwrap();
        context.send(&[donation], &[&bob]);
        context.set_time(FORECAST * DAY + 365 * DAY);
        // Independent exact oracle: P=208, Z=108+100/2=158.
        // floor(P*S*A/Z) gives 132, 9, 65 and leaves two token atoms.
        let entitlements = [
            (&alice, context.alice_token, FORECAST, 101, 132),
            (&bob, context.bob_token, FORECAST, 7, 9),
            (&alice, context.alice_token, FORECAST + 30, 100, 65),
        ];
        let mut paid = 0;
        for index in order {
            let (owner, token, day, principal, amount) = entitlements[index];
            let before = context.token_balance(token);
            let claim = claim_ix(&context, owner, day, pubkey(token));
            context.send(std::slice::from_ref(&claim), &[owner]);
            paid += amount;
            assert_eq!(context.token_balance(token), before + amount);
            let pool = context.pool_state();
            assert_eq!(
                (
                    pool.principal,
                    pool.total_weight,
                    pool.active_dates,
                    pool.processed_dates
                ),
                (208, 158_u128 << 64, 2, 2)
            );
            assert_eq!(pool.payout_remaining, 208 - paid);
            assert_eq!(
                context.token_balance(address(vault)),
                pool.payout_remaining + 17
            );
            assert_eq!(context.position_state(FORECAST).principal, 108);
            assert_eq!(context.position_state(FORECAST + 30).principal, 100);
            let stake = context.stake_state(owner, day);
            assert_eq!(
                (stake.state, stake.principal, stake.withdrawable_principal),
                (StakeState::Paid, principal, 0)
            );
            context.reject(&[claim], &[owner], custom(0, ManteonError::InvalidStake));
        }
        assert_eq!(
            context.token_balance(context.alice_token),
            OWNER_BALANCE - 201 + 197
        );
        assert_eq!(
            context.token_balance(context.bob_token),
            OWNER_BALANCE - 7 - 17 + 9
        );
        assert_eq!(context.token_balance(context.fee_recipient), fee_before);
        assert_eq!(context.pool_state().payout_remaining, 2);
    }
}

#[test]
fn payout_and_refund_require_the_owner_and_bound_accounts() {
    for refund in [false, true] {
        let mut context = TestContext::new();
        let alice = context.alice.insecure_clone();
        let bob = context.bob.insecure_clone();
        let service = context.service.insecure_clone();
        context.activate();
        context.set_time(OPEN);
        for (owner, token, day) in [
            (&alice, context.alice_token, FORECAST),
            (&bob, context.bob_token, FORECAST),
            (&alice, context.alice_token, FORECAST + 30),
        ] {
            context.send(&[context.stake_ix(owner, token, day, 100)], &[owner]);
        }
        let mut result = ResultFixture::new(&context);
        if !refund {
            context.set_time(FORECAST * DAY);
        }
        result.record_time(&context);
        let case = CaseFixture::start(&mut context, &result);
        case.finalize_yes(&mut context);
        let claim = claim_ix(&context, &alice, FORECAST, pubkey(context.alice_token));
        context.reject(
            std::slice::from_ref(&claim),
            &[&alice],
            custom(0, ManteonError::InvalidEventResult),
        );
        context.send(&[case.begin_ix(&context, &service)], &[&service]);
        if !refund {
            // A frozen but incomplete book is not claimable.
            context.reject(
                std::slice::from_ref(&claim),
                &[&alice],
                custom(0, ManteonError::InvalidPoolState),
            );
            context.send(
                &[
                    case.process_ix(&context, &service, &[FORECAST, FORECAST + 30]),
                    case.finalize_ix(&context, &service),
                ],
                &[&service],
            );
        }
        let mut unsigned = claim.clone();
        unsigned.accounts[0].is_signer = false;
        context.reject(&[unsigned], &[], custom(0, ErrorCode::AccountNotSigner));
        let mut wrong_owner = claim.clone();
        wrong_owner.accounts[0].pubkey = bob.pubkey();
        context.reject(
            &[wrong_owner],
            &[&bob],
            custom(0, ErrorCode::ConstraintSeeds),
        );
        for (name, index, key, code) in [
            (
                "another owner's Stake",
                4,
                context.stake_key(&bob, FORECAST),
                u32::from(ErrorCode::ConstraintSeeds),
            ),
            (
                "another date's Position",
                3,
                context.position(FORECAST + 30),
                u32::from(ErrorCode::ConstraintSeeds),
            ),
            (
                "missing Position",
                3,
                context.position(FORECAST + 1),
                u32::from(ErrorCode::AccountNotInitialized),
            ),
            (
                "another owner's destination",
                7,
                pubkey(context.bob_token),
                u32::from(ManteonError::InvalidTokenAccount),
            ),
            (
                "wrong destination mint",
                7,
                pubkey(context.audit_source),
                u32::from(ManteonError::InvalidTokenAccount),
            ),
            (
                "source aliases destination",
                9,
                pubkey(context.alice_token),
                u32::from(ErrorCode::ConstraintDuplicateMutableAccount),
            ),
            (
                "caller replaces vault authority",
                8,
                pubkey(alice.pubkey()),
                u32::from(ErrorCode::ConstraintSeeds),
            ),
        ] {
            println!("Reject claim account: {name}, refund={refund}");
            let mut altered = claim.clone();
            altered.accounts[index].pubkey = address(key);
            context.reject(&[altered], &[&alice], custom(0, code));
        }
        // Frozen SPL destinations must not consume either kind of entitlement.
        let freeze = spl_token::instruction::freeze_account(
            &spl_token::ID,
            &pubkey(context.alice_token),
            &pubkey(context.mint),
            &pubkey(context.payer.pubkey()),
            &[],
        )
        .unwrap();
        context.send(&[freeze], &[]);
        context.reject(
            std::slice::from_ref(&claim),
            &[&alice],
            custom(0, ManteonError::InvalidTokenAccount),
        );
        let thaw = spl_token::instruction::thaw_account(
            &spl_token::ID,
            &pubkey(context.alice_token),
            &pubkey(context.mint),
            &pubkey(context.payer.pubkey()),
            &[],
        )
        .unwrap();
        context.send(&[thaw], &[]);
        // Duplicate calls in a single transaction roll back the first transfer.
        context.reject(
            &[claim.clone(), claim.clone()],
            &[&alice],
            custom(1, ManteonError::InvalidStake),
        );
        assert_eq!(
            context.stake_state(&alice, FORECAST).state,
            StakeState::Active
        );
        context.send(&[claim], &[&alice]);
        assert_eq!(
            context.stake_state(&alice, FORECAST).state,
            if refund {
                StakeState::Refunded
            } else {
                StakeState::Paid
            }
        );
    }
}

#[test]
fn cancellation_and_refund_compose_atomically_and_cannot_pay_twice() {
    let mut context = TestContext::with_profit_fee_recipient(2000, false);
    let alice = context.alice.insecure_clone();
    let service = context.service.insecure_clone();
    context.activate();
    context.set_time(OPEN);
    context.send(
        &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
        &[&alice],
    );
    let mut result = ResultFixture::new(&context);
    result.record_time(&context);
    let case = CaseFixture::start(&mut context, &result);
    case.finalize_yes(&mut context);
    let begin = case.begin_ix(&context, &service);
    let claim = claim_ix(&context, &alice, FORECAST, pubkey(context.alice_token));
    let wrong_destination = claim_ix(&context, &alice, FORECAST, pubkey(context.bob_token));
    context.reject(
        &[begin.clone(), wrong_destination],
        &[&service, &alice],
        custom(1, ManteonError::InvalidTokenAccount),
    );
    assert_eq!(context.pool_state().state, PoolState::Active);
    context.reject(
        &[begin.clone(), claim.clone(), claim.clone()],
        &[&service, &alice],
        custom(2, ManteonError::InvalidStake),
    );
    assert_eq!(context.pool_state().state, PoolState::Active);
    assert_eq!(
        context.token_balance(context.alice_token),
        OWNER_BALANCE - 100
    );
    context.send(&[begin, claim.clone()], &[&service, &alice]);
    assert!(context.pool_state().config_released);
    assert_eq!(context.pool_state().state, PoolState::Refunding);
    assert_eq!(
        (
            context.pool_state().principal,
            context.pool_state().active_dates
        ),
        (0, 0)
    );
    assert_eq!(context.token_balance(context.alice_token), OWNER_BALANCE);
    context.reject(&[claim], &[&alice], custom(0, ManteonError::InvalidStake));
}

#[test]
fn deficient_custody_blocks_freeze_and_claim_even_when_one_owner_could_be_paid() {
    for refund in [false, true] {
        let mut context = TestContext::new();
        let alice = context.alice.insecure_clone();
        let bob = context.bob.insecure_clone();
        let service = context.service.insecure_clone();
        context.activate();
        context.set_time(OPEN);
        context.send(
            &[
                context.stake_ix(&alice, context.alice_token, FORECAST, 100),
                context.stake_ix(&bob, context.bob_token, FORECAST + 30, 100),
            ],
            &[&alice, &bob],
        );
        let mut result = ResultFixture::new(&context);
        if !refund {
            context.set_time(FORECAST * DAY);
        }
        result.record_time(&context);
        let case = CaseFixture::start(&mut context, &result);
        case.finalize_yes(&mut context);
        let vault = address(PoolContext::vault(&context.pool));
        let authentic = context.svm.get_account(&vault).unwrap();
        // Deliberate invalid-custody fixture: no public instruction can withdraw
        // these vault funds. Positive lifecycle state was created entirely onchain.
        let mut deficient = authentic.clone();
        let mut token = spl_token::state::Account::unpack(&deficient.data).unwrap();
        token.amount = 199;
        spl_token::state::Account::pack(token, &mut deficient.data).unwrap();
        context.svm.set_account(vault, deficient.clone()).unwrap();
        let begin = case.begin_ix(&context, &service);
        context.reject(
            std::slice::from_ref(&begin),
            &[&service],
            custom(0, ManteonError::InsufficientBacking),
        );
        assert_eq!(context.pool_state().state, PoolState::Active);
        context.svm.set_account(vault, authentic.clone()).unwrap();
        context.send(&[begin], &[&service]);
        if !refund {
            context.send(
                &[
                    case.process_ix(&context, &service, &[FORECAST, FORECAST + 30]),
                    case.finalize_ix(&context, &service),
                ],
                &[&service],
            );
        }
        context.svm.set_account(vault, deficient).unwrap();
        let claim = claim_ix(&context, &alice, FORECAST, pubkey(context.alice_token));
        // 199 covers Alice's own 100 refund / 133 payout, but not all 200 of
        // recorded liabilities. It cannot be distributed on a first-come basis.
        context.reject(
            std::slice::from_ref(&claim),
            &[&alice],
            custom(0, ManteonError::InsufficientBacking),
        );
        assert_eq!(
            context.stake_state(&alice, FORECAST).state,
            StakeState::Active
        );
        context.svm.set_account(vault, authentic).unwrap();
        context.send(&[claim], &[&alice]);
        let amount = if refund { 100 } else { 133 };
        assert_eq!(
            context.token_balance(context.alice_token),
            OWNER_BALANCE - 100 + amount
        );
        assert_eq!(context.token_balance(vault), 200 - amount);
    }
}

#[test]
fn recipient_closure_or_authority_change_cannot_block_a_winning_claim() {
    for close_recipient in [false, true] {
        let mut context = TestContext::with_profit_fee_recipient(2000, false);
        let alice = context.alice.insecure_clone();
        let bob = context.bob.insecure_clone();
        let service = context.service.insecure_clone();
        context.activate();
        context.set_time(OPEN);
        context.send(
            &[
                context.stake_ix(&alice, context.alice_token, FORECAST, 1000),
                context.stake_ix(&bob, context.bob_token, FORECAST + 30, 1000),
            ],
            &[&alice, &bob],
        );
        let mut result = ResultFixture::new(&context);
        context.set_time(FORECAST * DAY);
        result.record_time(&context);
        let case = CaseFixture::start(&mut context, &result);
        case.finalize_yes(&mut context);
        context.send(
            &[
                case.begin_ix(&context, &service),
                case.process_ix(&context, &service, &[FORECAST, FORECAST + 30]),
                case.finalize_ix(&context, &service),
            ],
            &[&service],
        );
        let recipient_authority = context.pool_state().fee_recipient;
        assert_eq!(recipient_authority, pubkey(context.payer.pubkey()));
        if close_recipient {
            let drain = spl_token::instruction::transfer(
                &spl_token::ID,
                &pubkey(context.fee_recipient),
                &pubkey(context.bob_token),
                &recipient_authority,
                &[],
                context.token_balance(context.fee_recipient),
            )
            .unwrap();
            let close = spl_token::instruction::close_account(
                &spl_token::ID,
                &pubkey(context.fee_recipient),
                &recipient_authority,
                &recipient_authority,
                &[],
            )
            .unwrap();
            context.send(&[drain, close], &[]);
        } else {
            let change = spl_token::instruction::set_authority(
                &spl_token::ID,
                &pubkey(context.fee_recipient),
                Some(&pubkey(bob.pubkey())),
                spl_token::instruction::AuthorityType::AccountOwner,
                &recipient_authority,
                &[],
            )
            .unwrap();
            context.send(&[change], &[]);
            let stale = claim_ix(&context, &alice, FORECAST, pubkey(context.alice_token));
            context.reject(
                &[stale],
                &[&alice],
                custom(0, ManteonError::InvalidFeeRecipient),
            );
        }
        // The participant can create a replacement and claim atomically, without
        // a recipient signature. Here its recorded authority is the transaction payer.
        context.svm.airdrop(&alice.pubkey(), 10_000_000).unwrap();
        let replacement = solana_keypair::Keypair::new_from_array([0xed; 32]);
        let create = anchor_lang::solana_program::system_instruction::create_account(
            &pubkey(alice.pubkey()),
            &pubkey(replacement.pubkey()),
            context
                .svm
                .minimum_balance_for_rent_exemption(spl_token::state::Account::LEN),
            spl_token::state::Account::LEN as u64,
            &spl_token::ID,
        );
        let initialize = spl_token::instruction::initialize_account3(
            &spl_token::ID,
            &pubkey(replacement.pubkey()),
            &pubkey(context.mint),
            &recipient_authority,
        )
        .unwrap();
        let mut claim = claim_ix(&context, &alice, FORECAST, pubkey(context.alice_token));
        claim.accounts[10].pubkey = replacement.pubkey();
        let before = context.token_balance(context.alice_token);
        crate::support::transactions::send(
            &mut context.svm,
            &alice,
            &[create, initialize, claim],
            &[&replacement],
        )
        .unwrap();
        assert_eq!(context.token_balance(context.alice_token) - before, 1267);
        assert_eq!(context.token_balance(replacement.pubkey()), 53);
        assert_eq!(context.pool_state().fee_recipient, recipient_authority);
        assert_eq!(
            context.stake_state(&alice, FORECAST).state,
            StakeState::Paid
        );
        let mut loss = claim_ix(&context, &bob, FORECAST + 30, pubkey(context.bob_token));
        loss.accounts[10].pubkey = address(manteon::ID);
        loss.accounts[10].is_writable = false;
        context.send(&[loss], &[&bob]);
        assert!(context.pool_state().config_released);
    }
}
