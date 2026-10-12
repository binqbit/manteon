//! Official CPMM wire fixtures. Pool lifecycle, stakes and payouts execute real SBF instructions.

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas, prelude::Pubkey};
use manteon::{
    integration::price::{RAYDIUM_PROGRAM, SOL_USD_FEED_ID, USDC, WRAPPED_SOL},
    state::{CrossingDirection, PriceMonitor, PriceProtocol, PriceThreshold, UsdQuoteSource},
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_signer::Signer;

use super::{
    TestContext,
    setup::{
        config::{address, pubkey},
        pool::{NOW, PoolContext},
    },
};

pub struct Sources {
    pub keys: [Pubkey; 4],
    conversion: Conversion,
}

enum Conversion {
    Usdc,
    Pyth(Pubkey),
    Reference([Pubkey; 4]),
}

fn key(byte: u8) -> Pubkey {
    Pubkey::new_from_array([byte; 32])
}
fn put(data: &mut [u8], at: usize, bytes: &[u8]) {
    data[at..at + bytes.len()].copy_from_slice(bytes);
}
fn account(context: &mut TestContext, key: Pubkey, owner: Pubkey, data: Vec<u8>) {
    context
        .svm
        .set_account(
            address(key),
            Account {
                lamports: context.svm.minimum_balance_for_rent_exemption(data.len()),
                data,
                owner: address(owner),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
}
fn discriminator(name: &str, size: usize) -> Vec<u8> {
    let mut data = vec![0; size];
    let tag = match name {
        "PoolState" => [247, 237, 227, 245, 215, 195, 222, 70],
        "ObservationState" => [122, 174, 197, 53, 129, 9, 165, 132],
        _ => panic!("unknown source account"),
    };
    data[..8].copy_from_slice(&tag);
    data
}

impl Sources {
    pub fn rule(direction: CrossingDirection) -> PriceThreshold {
        PriceThreshold {
            protocol: PriceProtocol::RaydiumCpmm,
            pool_id: key(0x91),
            token_mint: key(0x92),
            direction,
            target_price_usd_e9: 1_000_000_000,
            usd_quote_source: UsdQuoteSource::Usdc,
            twap_window_seconds: 3600,
            max_observation_gap_seconds: 14_400,
            minimum_quote_liquidity_usd_e9: 100_000_000_000_000,
        }
    }

    /// Sources are deliberately synthetic account bytes; only Manteon executes SBF.
    pub fn install(context: &mut TestContext, numerator: u64, denominator: u64) -> Self {
        let manteon::state::EventDefinition::TokenPriceThresholdCrossing { params } =
            context.event_state().definition
        else {
            panic!("price fixture requires a price event");
        };
        let sol_quote = !matches!(params.usd_quote_source, UsdQuoteSource::Usdc);
        let keys = install_pool(
            context,
            params.pool_id,
            [key(0x93), key(0x94)],
            [
                context.token_mint,
                if sol_quote { WRAPPED_SOL } else { USDC },
            ],
            [6, if sol_quote { 9 } else { 6 }],
            [
                200_000_000_000 * denominator / numerator,
                if sol_quote {
                    2_000_000_000_000
                } else {
                    200_000_000_000
                },
            ],
        );
        let conversion = match params.usd_quote_source {
            UsdQuoteSource::Usdc => Conversion::Usdc,
            UsdQuoteSource::PythSolUsd {
                price_update_account,
                receiver_program,
                ..
            } => {
                let mut data = vec![0; 134];
                data[..8].copy_from_slice(&[34, 241, 35, 99, 157, 126, 244, 205]);
                data[40] = 1;
                put(&mut data, 41, &SOL_USD_FEED_ID);
                put(&mut data, 73, &100_000_000_i64.to_le_bytes());
                // $100/SOL, with $0.10 confidence (0.1%), fixes conservative bounds.
                put(&mut data, 81, &100_000_u64.to_le_bytes());
                put(&mut data, 89, &(-6_i32).to_le_bytes());
                put(&mut data, 93, &NOW.to_le_bytes());
                put(&mut data, 101, &(NOW - 1).to_le_bytes());
                account(context, price_update_account, receiver_program, data);
                Conversion::Pyth(price_update_account)
            }
            UsdQuoteSource::RaydiumSolUsdc { pool_id } => Conversion::Reference(install_pool(
                context,
                pool_id,
                [key(0xa3), key(0xa4)],
                [WRAPPED_SOL, USDC],
                [9, 6],
                [2_000_000_000_000, 200_000_000_000],
            )),
        };
        Self { keys, conversion }
    }

    pub fn update(
        &self,
        context: &mut TestContext,
        timestamp: i64,
        cumulative_x32: u128,
        numerator: u64,
        denominator: u64,
    ) {
        update_observation(context, self.keys[1], timestamp, cumulative_x32);
        let mut base = context.svm.get_account(&address(self.keys[2])).unwrap();
        put(
            &mut base.data,
            64,
            &(200_000_000_000_u64 * denominator / numerator).to_le_bytes(),
        );
        context
            .svm
            .set_account(address(self.keys[2]), base)
            .unwrap();
        match self.conversion {
            Conversion::Usdc => {}
            Conversion::Pyth(key) => {
                let mut oracle = context.svm.get_account(&address(key)).unwrap();
                put(&mut oracle.data, 93, &timestamp.to_le_bytes());
                put(&mut oracle.data, 101, &(timestamp - 1).to_le_bytes());
                context.svm.set_account(address(key), oracle).unwrap();
            }
            Conversion::Reference(keys) => {
                let elapsed = (timestamp - NOW) as u128;
                update_observation(context, keys[1], timestamp, ((1_u128 << 32) / 10) * elapsed);
            }
        }
        context.set_time(timestamp);
    }

    pub fn ix(&self, context: &TestContext, caller: &Keypair, operation: &str) -> Instruction {
        let mut accounts = if operation == "activate" {
            manteon::accounts::ActivatePricePool {
                caller: pubkey(caller.pubkey()),
                config: pubkey(context.config_address),
                pool: pubkey(context.pool.pubkey()),
                event: PoolContext::event(&context.pool),
                vault: PoolContext::vault(&context.pool),
                monitor: context.price_monitor(),
                system_program: anchor_lang::system_program::ID,
            }
            .to_account_metas(None)
        } else {
            manteon::accounts::PriceThresholdPool {
                caller: pubkey(caller.pubkey()),
                config: pubkey(context.config_address),
                pool: pubkey(context.pool.pubkey()),
                event: PoolContext::event(&context.pool),
                vault: PoolContext::vault(&context.pool),
                monitor: context.price_monitor(),
            }
            .to_account_metas(None)
        };
        if operation != "expire" {
            accounts.extend(
                self.keys
                    .iter()
                    .map(|key| AccountMeta::new_readonly(address(*key), false)),
            );
            match self.conversion {
                Conversion::Usdc => {}
                Conversion::Pyth(key) => {
                    accounts.push(AccountMeta::new_readonly(address(key), false))
                }
                Conversion::Reference(keys) => accounts.extend(
                    keys.iter()
                        .map(|key| AccountMeta::new_readonly(address(*key), false)),
                ),
            }
        }
        let data = match operation {
            "activate" => manteon::instruction::ActivatePricePool {}.data(),
            "observe" => manteon::instruction::RecordPriceObservation {}.data(),
            "finalize" => manteon::instruction::FinalizePriceThreshold {}.data(),
            "expire" => manteon::instruction::ExpirePriceObservation {}.data(),
            _ => panic!("unknown price operation"),
        };
        Instruction {
            program_id: address(manteon::ID),
            accounts,
            data,
        }
    }
}

/// Official CPMM wire layout, shared by the primary and SOL/USDC source fixture.
fn install_pool(
    context: &mut TestContext,
    pool: Pubkey,
    vaults: [Pubkey; 2],
    mints: [Pubkey; 2],
    decimals: [u8; 2],
    reserves: [u64; 2],
) -> [Pubkey; 4] {
    let observation =
        Pubkey::find_program_address(&[b"observation", pool.as_ref()], &RAYDIUM_PROGRAM).0;
    let keys = [pool, observation, vaults[0], vaults[1]];
    let (authority, bump) =
        Pubkey::find_program_address(&[b"vault_and_lp_mint_auth_seed"], &RAYDIUM_PROGRAM);
    let mut data = discriminator("PoolState", 637);
    for (offset, key) in [
        (72, vaults[0]),
        (104, vaults[1]),
        (168, mints[0]),
        (200, mints[1]),
        (232, anchor_spl::token_2022::ID),
        (264, anchor_spl::token::ID),
        (296, observation),
    ] {
        put(&mut data, offset, key.as_ref());
    }
    data[328] = bump;
    data[331] = decimals[0];
    data[332] = decimals[1];
    account(context, pool, RAYDIUM_PROGRAM, data);
    let mut history = discriminator("ObservationState", 4075);
    history[8] = 1;
    put(&mut history, 11, pool.as_ref());
    put(&mut history, 43, &(NOW as u64).to_le_bytes());
    put(&mut history, 4043, &(NOW as u64).to_le_bytes());
    account(context, observation, RAYDIUM_PROGRAM, history);
    for (index, owner) in [anchor_spl::token_2022::ID, anchor_spl::token::ID]
        .into_iter()
        .enumerate()
    {
        let mut data = vec![0; 165];
        put(&mut data, 0, mints[index].as_ref());
        put(&mut data, 32, authority.as_ref());
        put(&mut data, 64, &reserves[index].to_le_bytes());
        data[108] = 1;
        account(context, vaults[index], owner, data);
    }
    keys
}

fn update_observation(context: &mut TestContext, key: Pubkey, timestamp: i64, cumulative: u128) {
    let mut observation = context.svm.get_account(&address(key)).unwrap();
    put(
        &mut observation.data,
        4043,
        &(timestamp as u64).to_le_bytes(),
    );
    put(&mut observation.data, 51, &cumulative.to_le_bytes());
    context.svm.set_account(address(key), observation).unwrap();
}

pub fn settlement(context: &TestContext, caller: &Keypair, days: &[i64]) -> [Instruction; 2] {
    let mut accounts = manteon::accounts::ProcessPositions {
        caller: pubkey(caller.pubkey()),
        config: pubkey(context.config_address),
        pool: pubkey(context.pool.pubkey()),
        event: PoolContext::event(&context.pool),
        case: None,
    }
    .to_account_metas(None);
    accounts.extend(
        days.iter()
            .map(|day| AccountMeta::new(address(context.position(*day)), false)),
    );
    [
        Instruction {
            program_id: address(manteon::ID),
            accounts,
            data: manteon::instruction::ProcessPositions {}.data(),
        },
        Instruction {
            program_id: address(manteon::ID),
            accounts: manteon::accounts::FinalizeSettlement {
                caller: pubkey(caller.pubkey()),
                config: pubkey(context.config_address),
                pool: pubkey(context.pool.pubkey()),
                event: PoolContext::event(&context.pool),
                case: None,
            }
            .to_account_metas(None),
            data: manteon::instruction::FinalizeSettlement {}.data(),
        },
    ]
}

impl TestContext {
    pub fn price_monitor(&self) -> Pubkey {
        Pubkey::find_program_address(
            &[PriceMonitor::SEED, self.pool.pubkey().as_ref()],
            &manteon::ID,
        )
        .0
    }

    pub fn price_monitor_state(&self) -> PriceMonitor {
        let account = self
            .svm
            .get_account(&address(self.price_monitor()))
            .unwrap();
        PriceMonitor::try_deserialize(&mut account.data.as_slice()).unwrap()
    }
}
