use anchor_lang::prelude::Clock;
use manteon::state::PoolState;
use solana_signer::Signer;

use crate::support::{
    FORECAST, OPEN, OWNER_BALANCE, TestContext,
    setup::{
        config::address,
        pool::{NOW, PoolContext},
    },
};

#[test]
fn mha_003_contexts_isolate_market_state_custody_and_clock() {
    let mut first = TestContext::new();
    let mut second = TestContext::new();

    assert_eq!(first.pool.pubkey(), second.pool.pubkey());
    assert_eq!(first.policy, second.policy);
    assert_eq!(first.alice_token, second.alice_token);

    let initial_clock = second.svm.get_sysvar::<Clock>();
    assert_eq!(initial_clock.unix_timestamp, NOW);

    let first_owner = first.alice.insecure_clone();
    first.activate();
    first.set_time(OPEN);
    first.send(
        &[first.stake_ix(&first_owner, first.alice_token, FORECAST, 257)],
        &[&first_owner],
    );

    assert_eq!(first.pool_state().principal, 257);
    assert_eq!(first.policy_uses(), 1);

    let untouched = second.pool_state();

    assert_eq!(untouched.state, PoolState::Draft);
    assert_eq!(untouched.principal, 0);
    assert_eq!(untouched.active_dates, 0);
    assert_eq!(second.policy_uses(), 0);
    assert_eq!(second.svm.get_sysvar::<Clock>(), initial_clock);
    assert_eq!(second.token_balance(second.alice_token), OWNER_BALANCE);
    assert_eq!(second.token_balance(second.audit_source), OWNER_BALANCE);

    let vault = address(PoolContext::vault(&second.pool));
    assert_eq!(second.token_balance(vault), 0);

    for absent in [
        second.position(FORECAST),
        second.stake_key(&second.alice, FORECAST),
    ] {
        assert!(second.svm.get_account(&address(absent)).is_none());
    }

    let second_owner = second.alice.insecure_clone();
    second.activate();
    second.set_time(OPEN + 1);
    second.send(
        &[second.stake_ix(&second_owner, second.alice_token, FORECAST, 375)],
        &[&second_owner],
    );

    assert_eq!(second.pool_state().principal, 375);
    assert_eq!(second.policy_uses(), 1);
    assert_eq!(second.token_balance(vault), 375);
    assert_eq!(
        second.token_balance(second.alice_token),
        OWNER_BALANCE - 375
    );

    assert_eq!(first.pool_state().principal, 257);
    assert_eq!(first.token_balance(vault), 257);
    assert_eq!(first.token_balance(first.alice_token), OWNER_BALANCE - 257);
    assert_eq!(first.svm.get_sysvar::<Clock>().unix_timestamp, OPEN);
}
