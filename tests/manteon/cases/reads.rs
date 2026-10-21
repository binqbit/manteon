use anchor_lang::{
    AccountDeserialize, AccountSerialize,
    prelude::{AccountInfo, Pubkey},
    solana_program::{program_option::COption, program_pack::Pack},
};
use anchor_spl::token::spl_token;
use manteon::state::{
    CrossingDirection, Event, EventDefinition, EventResult, EventResultExtra, Pool, PoolState,
    PriceMonitor,
};
use manteon_client::{PoolView, ReadError, read_pool, read_price_monitor};
use solana_account::Account;
use solana_keypair::Keypair;
use solana_signer::Signer;

use crate::support::setup::{
    config::{address, pubkey},
    pool::PoolContext,
};
use crate::support::{TestContext, price::Sources, transactions};

type Snapshot = [(Pubkey, Account); 4];

fn snapshot(context: &PoolContext, pool: &Keypair) -> Snapshot {
    [
        pubkey(pool.pubkey()),
        pubkey(context.config_address),
        PoolContext::event(pool),
        PoolContext::vault(pool),
    ]
    .map(|key| (key, context.config.svm.get_account(&address(key)).unwrap()))
}

fn market_snapshot(context: &TestContext) -> Snapshot {
    [
        pubkey(context.pool.pubkey()),
        pubkey(context.config_address),
        PoolContext::event(&context.pool),
        PoolContext::vault(&context.pool),
    ]
    .map(|key| (key, context.svm.get_account(&address(key)).unwrap()))
}

pub(super) fn read_context(context: &TestContext) -> Result<PoolView, ReadError> {
    read(&mut market_snapshot(context))
}

fn read_monitor_account(
    pool_key: &Pubkey,
    pool: &Pool,
    event: &Event,
    (key, account): &mut (Pubkey, Account),
) -> Result<Option<PriceMonitor>, ReadError> {
    let owner = pubkey(account.owner);
    let info = AccountInfo::new(
        key,
        false,
        false,
        &mut account.lamports,
        &mut account.data,
        &owner,
        account.executable,
    );
    read_price_monitor(pool_key, pool, event, Some(&info))
}

pub(super) fn read_context_monitor(
    context: &TestContext,
) -> Result<Option<PriceMonitor>, ReadError> {
    let view = read_context(context)?;
    let key = context.price_monitor();
    match context.svm.get_account(&address(key)) {
        Some(account) => read_monitor_account(
            &pubkey(context.pool.pubkey()),
            &view.pool,
            &view.event,
            &mut (key, account),
        ),
        None => read_price_monitor(
            &pubkey(context.pool.pubkey()),
            &view.pool,
            &view.event,
            None,
        ),
    }
}

fn read(snapshot: &mut Snapshot) -> Result<PoolView, ReadError> {
    let owners = snapshot
        .each_ref()
        .map(|(_, account)| pubkey(account.owner));
    let accounts = snapshot
        .iter_mut()
        .zip(&owners)
        .map(|((key, account), owner)| {
            AccountInfo::new(
                key,
                false,
                false,
                &mut account.lamports,
                &mut account.data,
                owner,
                account.executable,
            )
        })
        .collect::<Vec<_>>();
    read_pool(&accounts[0], &accounts[1], &accounts[2], &accounts[3])
}

fn write(account: &mut Account, value: &impl AccountSerialize) {
    account.data.fill(0);
    value
        .try_serialize(&mut account.data.as_mut_slice())
        .unwrap();
}

#[test]
fn anchor_013_reader_exposes_created_pool_and_unallocated_donations() {
    let mut context = PoolContext::new();
    let pool = Keypair::new_from_array([0xa1; 32]);
    context.create(context.instruction(&pool), &pool);
    let mut accounts = snapshot(&context, &pool);
    let before = accounts.clone();
    let view = read(&mut accounts).unwrap();
    assert_eq!(accounts, before, "reading cannot modify snapshots");
    assert_eq!(view.pool.config, pubkey(context.config_address));
    assert_eq!(
        view.config.service_authority,
        pubkey(context.service.pubkey())
    );
    assert_eq!(view.pool.state, PoolState::Draft);
    assert_eq!(view.event.pool, pubkey(pool.pubkey()));
    assert_eq!(view.event.definition, EventDefinition::ExternalEvent);
    assert_eq!((view.vault_balance, view.unallocated_balance), (0, 0));

    let donation = spl_token::instruction::transfer_checked(
        &spl_token::ID,
        &pubkey(context.config.fee_recipient),
        &pubkey(context.config.mint),
        &PoolContext::vault(&pool),
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
    let mut after = snapshot(&context, &pool);
    assert_eq!(&after[..3], &before[..3]);
    let donated = read(&mut after).unwrap();
    assert_eq!(
        (donated.vault_balance, donated.unallocated_balance),
        (17, 17)
    );
    assert_eq!(donated.pool.principal, 0);
    assert_eq!(donated.pool.active_dates, 0);
    assert_eq!(donated.pool.payout_remaining, 0);
}

#[test]
fn anchor_014_reader_rejects_foreign_accounts_and_invalid_encodings() {
    let mut context = PoolContext::new();
    let first = Keypair::new_from_array([0xa2; 32]);
    let second = Keypair::new_from_array([0xa3; 32]);
    for pool in [&first, &second] {
        context.create(context.instruction(pool), pool);
    }
    let config = Keypair::new_from_array([0xa4; 32]);
    context
        .config
        .create(context.config.instruction(&config), &config);
    let base = snapshot(&context, &first);
    let other = snapshot(&context, &second);
    for (name, index, replacement, error) in [
        (
            "independent valid Config",
            1,
            (
                pubkey(config.pubkey()),
                context.config.svm.get_account(&config.pubkey()).unwrap(),
            ),
            ReadError::InvalidRelationship,
        ),
        (
            "other Pool Event",
            2,
            other[2].clone(),
            ReadError::InvalidRelationship,
        ),
        (
            "other Pool vault",
            3,
            other[3].clone(),
            ReadError::InvalidAddress,
        ),
    ] {
        let mut changed = base.clone();
        changed[index] = replacement;
        assert_eq!(read(&mut changed).err(), Some(error), "{name}");
    }

    let mut noncanonical = base.clone();
    noncanonical[2].0 = pubkey(second.pubkey());
    let mut decoded = Pool::try_deserialize(&mut noncanonical[0].1.data.as_slice()).unwrap();
    decoded.event_id = noncanonical[2].0;
    write(&mut noncanonical[0].1, &decoded);
    assert_eq!(
        read(&mut noncanonical).err(),
        Some(ReadError::InvalidAddress)
    );

    let mut aliased = base.clone();
    aliased[2].0 = aliased[0].0;
    assert_eq!(
        read(&mut aliased).err(),
        Some(ReadError::InvalidRelationship)
    );

    for index in [0, 3] {
        let mut wrong_owner = base.clone();
        wrong_owner[index].1.owner = address(anchor_lang::system_program::ID);
        assert_eq!(read(&mut wrong_owner).err(), Some(ReadError::WrongOwner));
    }
    let mut executable = base.clone();
    executable[1].1.executable = true;
    let mut truncated = base.clone();
    truncated[2].1.data.pop();
    let mut extended = base.clone();
    extended[0].1.data.push(0);
    let mut discriminator = base;
    discriminator[0].1.data[0] ^= 0xff;
    for mut changed in [executable, truncated, extended, discriminator] {
        assert_eq!(read(&mut changed).err(), Some(ReadError::InvalidData));
    }
}

#[test]
fn anchor_015_reader_rejects_decodable_but_inconsistent_snapshot_state() {
    let mut context = PoolContext::new();
    let pool = Keypair::new_from_array([0xa5; 32]);
    context.create(context.instruction(&pool), &pool);
    let base = snapshot(&context, &pool);
    // Invalid decoder fixtures stay local: no forged state is installed in the VM.
    let mut relationship = base.clone();
    let mut event = Event::try_deserialize(&mut relationship[2].1.data.as_slice()).unwrap();
    event.pool = pubkey(context.config_address);
    write(&mut relationship[2].1, &event);
    assert_eq!(
        read(&mut relationship).err(),
        Some(ReadError::InvalidRelationship)
    );

    let mut audit = base.clone();
    let mut decoded = Pool::try_deserialize(&mut audit[0].1.data.as_slice()).unwrap();
    decoded.audit_config = Pubkey::new_from_array([0xa6; 32]);
    write(&mut audit[0].1, &decoded);
    assert_eq!(read(&mut audit).err(), Some(ReadError::InvalidRelationship));

    let mut accounting = base.clone();
    let mut decoded = Pool::try_deserialize(&mut accounting[0].1.data.as_slice()).unwrap();
    decoded.principal = 1;
    decoded.active_dates = 1;
    write(&mut accounting[0].1, &decoded);
    assert_eq!(
        read(&mut accounting).err(),
        Some(ReadError::InvalidAccounting)
    );

    let mut schedule = base.clone();
    let mut decoded = Pool::try_deserialize(&mut schedule[0].1.data.as_slice()).unwrap();
    decoded.opens_at = decoded.lock_at;
    write(&mut schedule[0].1, &decoded);
    assert_eq!(
        read(&mut schedule).err(),
        Some(ReadError::InvalidAccounting)
    );

    for state in [
        PoolState::Active,
        PoolState::Refunding,
        PoolState::Settling,
        PoolState::Claimable,
    ] {
        let mut impossible = base.clone();
        let mut decoded = Pool::try_deserialize(&mut impossible[0].1.data.as_slice()).unwrap();
        decoded.state = state;
        decoded.accepted_case = Some(Pubkey::new_from_array([0xa8; 32]));
        write(&mut impossible[0].1, &decoded);
        assert_eq!(
            read(&mut impossible).err(),
            Some(ReadError::InvalidAccounting)
        );
    }
    let mut underreported = base.clone();
    let mut decoded = Pool::try_deserialize(&mut underreported[0].1.data.as_slice()).unwrap();
    decoded.state = PoolState::Settling;
    decoded.principal = 1;
    decoded.active_dates = 1;
    decoded.accepted_case = Some(Pubkey::new_from_array([0xa7; 32]));
    decoded.service_eligible_at = Some(decoded.lock_at);
    let mut event = Event::try_deserialize(&mut underreported[2].1.data.as_slice()).unwrap();
    event.result = Some(manteon::state::EventResult {
        extra: None,
        occurred_at: decoded.lock_at,
    });
    write(&mut underreported[0].1, &decoded);
    write(&mut underreported[2].1, &event);
    assert_eq!(
        read(&mut underreported).err(),
        Some(ReadError::InvalidAccounting)
    );

    let original = spl_token::state::Account::unpack(&base[3].1.data).unwrap();
    let mut authority = original;
    authority.owner = pubkey(context.service.pubkey());
    let mut delegated = original;
    delegated.delegate = COption::Some(pubkey(context.service.pubkey()));
    delegated.delegated_amount = 1;
    let mut frozen = original;
    frozen.state = spl_token::state::AccountState::Frozen;
    let mut mint = original;
    mint.mint = Pubkey::new_from_array([0xa7; 32]);
    for (name, token) in [
        ("wrong transfer authority", authority),
        ("delegate", delegated),
        ("frozen custody", frozen),
        ("wrong mint", mint),
    ] {
        let mut changed = base.clone();
        spl_token::state::Account::pack(token, &mut changed[3].1.data).unwrap();
        assert_eq!(
            read(&mut changed).err(),
            Some(ReadError::InvalidVault),
            "{name}"
        );
    }
}

#[test]
fn price_monitor_reader_authenticates_its_account_and_lifecycle_separately() {
    let mut context = TestContext::with_price_threshold(Sources::rule(CrossingDirection::Up));
    let pool_key = pubkey(context.pool.pubkey());
    let monitor_key = context.price_monitor();
    assert!(read_context_monitor(&context).unwrap().is_none());
    let mut uninitialized = (monitor_key, Account::default());
    uninitialized.1.lamports = 17;
    assert!(
        read_monitor_account(
            &pool_key,
            &context.pool_state(),
            &context.event_state(),
            &mut uninitialized
        )
        .unwrap()
        .is_none()
    );

    let service = context.service.insecure_clone();
    let sources = Sources::install(&mut context, 1, 2);
    context.send(&[sources.ix(&context, &service, "activate")], &[&service]);
    let monitor = read_context_monitor(&context).unwrap().unwrap();
    assert!(monitor.baseline.is_some());
    let pool = context.pool_state();
    let event = context.event_state();
    let base = (
        monitor_key,
        context.svm.get_account(&address(monitor_key)).unwrap(),
    );
    assert_eq!(
        read_price_monitor(&pool_key, &pool, &event, None).err(),
        Some(ReadError::InvalidData)
    );

    for (mutation, error) in [
        (0, ReadError::InvalidAddress),
        (1, ReadError::WrongOwner),
        (2, ReadError::InvalidData),
        (3, ReadError::InvalidRelationship),
        (4, ReadError::InvalidAccounting),
    ] {
        let mut changed = base.clone();
        let mut decoded = context.price_monitor_state();
        match mutation {
            0 => changed.0 = Pubkey::new_unique(),
            1 => changed.1.owner = address(anchor_lang::system_program::ID),
            2 => {
                changed.1.data.pop();
            }
            3 => {
                decoded.pool = Pubkey::new_unique();
                write(&mut changed.1, &decoded);
            }
            _ => {
                decoded.baseline = None;
                write(&mut changed.1, &decoded);
            }
        }
        assert_eq!(
            read_monitor_account(&pool_key, &pool, &event, &mut changed).err(),
            Some(error)
        );
    }
    for state in [PoolState::Draft, PoolState::Refunding] {
        let mut changed_pool = context.pool_state();
        changed_pool.state = state;
        assert_eq!(
            read_monitor_account(&pool_key, &changed_pool, &event, &mut base.clone()).err(),
            Some(ReadError::InvalidAccounting)
        );
    }
    let mut external = context.event_state();
    external.definition = EventDefinition::ExternalEvent;
    assert_eq!(
        read_monitor_account(&pool_key, &pool, &external, &mut base.clone()).err(),
        Some(ReadError::InvalidRelationship)
    );
}

#[test]
fn market_reader_validates_price_results_without_monitoring_accounts() {
    let mut context = TestContext::with_price_threshold(Sources::rule(CrossingDirection::Up));
    let service = context.service.insecure_clone();
    let sources = Sources::install(&mut context, 1, 2);
    context.send(&[sources.ix(&context, &service, "activate")], &[&service]);
    let base = market_snapshot(&context);
    let lock = context.pool_state().lock_at;
    let result = EventResult {
        occurred_at: lock,
        extra: Some(EventResultExtra::PriceVerification {
            lower_usd_e9: 2_000_000_000,
            upper_usd_e9: 2_000_000_001,
        }),
    };
    for (state, result) in [
        (PoolState::Active, Some(result)),
        (PoolState::Refunding, Some(result)),
        (
            PoolState::Settling,
            Some(EventResult {
                occurred_at: lock - 1,
                ..result
            }),
        ),
    ] {
        let mut changed = base.clone();
        let mut pool = context.pool_state();
        pool.state = state;
        let mut event = context.event_state();
        event.result = result;
        write(&mut changed[0].1, &pool);
        write(&mut changed[2].1, &event);
        assert_eq!(read(&mut changed).err(), Some(ReadError::InvalidAccounting));
    }
    let mut changed = base;
    let mut pool = context.pool_state();
    pool.service_eligible_at = Some(lock);
    write(&mut changed[0].1, &pool);
    assert_eq!(read(&mut changed).err(), Some(ReadError::InvalidAccounting));
}
