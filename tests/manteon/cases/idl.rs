use anchor_lang::{Discriminator, InstructionData, ToAccountMetas, prelude::Pubkey};
use serde_json::{Value, json};

use crate::support::setup::programs;

fn assert_instruction(idl: &Value, name: &str, data: Vec<u8>, roles: &[(&str, bool, bool)]) {
    let instruction = idl["instructions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|instruction| instruction["name"] == name)
        .unwrap();
    assert_eq!(instruction["discriminator"], json!(data[..8]));
    let actual = instruction["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|account| {
            (
                account["name"].as_str().unwrap(),
                account["signer"].as_bool().unwrap_or(false),
                account["writable"].as_bool().unwrap_or(false),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, roles, "account roles for {name}");
}

fn fields<'a>(idl: &'a Value, name: &str) -> &'a [Value] {
    idl["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|ty| ty["name"] == name)
        .unwrap()["type"]["fields"]
        .as_array()
        .unwrap()
}

#[test]
fn anchor_005_generated_idl_matches_compiled_instruction_builders() {
    let path = std::env::var_os("MANTEON_IDL_JSON")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| programs::root().join("target/idl/manteon.json"));
    let idl: Value =
        serde_json::from_slice(&std::fs::read(path).expect("generated Anchor IDL")).unwrap();
    assert_eq!(idl["address"], manteon::ID.to_string());
    assert_eq!(
        idl["instructions"].as_array().unwrap().len(),
        22,
        "only the explicit market lifecycle is exposed"
    );
    assert_instruction(
        &idl,
        "create_config",
        manteon::instruction::CreateConfig {
            fee_recipient: Pubkey::new_from_array([0x60; 32]),
            service_authority: Pubkey::new_from_array([0x52; 32]),
            audit_config: Pubkey::new_from_array([0x53; 32]),
            profit_fee_bps: 0,
            expected_platform_share_bps: 2000,
        }
        .data(),
        &[
            ("authority", true, false),
            ("payer", true, true),
            ("config", true, true),
            ("global_config", false, false),
            ("collateral_mint", false, false),
            ("platform_fee_vault", false, true),
            ("token_program", false, false),
            ("system_program", false, false),
        ],
    );
    assert_instruction(
        &idl,
        "create_pool",
        manteon::instruction::CreatePool {
            event: manteon::state::EventDefinition::CompanyIpo {
                token_mint: Pubkey::new_from_array([0x54; 32]),
            },
            agreement_hash: [0xa1; 32],
            expected_config_hash: [0xa2; 32],
            algorithm: manteon::state::PoolAlgorithm::H30,
            opens_at: 10,
            lock_at: 20,
            horizon_at: 30,
        }
        .data(),
        &[
            ("service_authority", true, false),
            ("payer", true, true),
            ("config", false, true),
            ("pool", true, true),
            ("event", false, true),
            ("collateral_mint", false, false),
            ("vault_authority", false, false),
            ("vault", false, true),
            ("token_program", false, false),
            ("system_program", false, false),
        ],
    );
    assert_instruction(
        &idl,
        "propose_company_ipo",
        manteon::instruction::ProposeCompanyIpo {
            data: manteon::state::EventResult {
                extra: None,
                occurred_at: 1_800_000_000,
            },
        }
        .data(),
        &[
            ("author", true, false),
            ("payer", true, true),
            ("config", false, false),
            ("pool", false, true),
            ("event", false, true),
            ("policy", false, true),
            ("client_authority", false, false),
            ("source", false, true),
            ("case", false, true),
            ("author_receipt", false, true),
            ("case_vault", false, true),
            ("bond_mint", false, false),
            ("tekmerion_program", false, false),
            ("token_program", false, false),
            ("system_program", false, false),
        ],
    );
    assert_instruction(
        &idl,
        "activate_price_pool",
        manteon::instruction::ActivatePricePool {}.data(),
        &[
            ("caller", true, true),
            ("config", false, false),
            ("pool", false, true),
            ("event", false, false),
            ("vault", false, false),
            ("monitor", false, true),
            ("system_program", false, false),
        ],
    );
    for (name, data) in [
        (
            "record_price_observation",
            manteon::instruction::RecordPriceObservation {}.data(),
        ),
        (
            "finalize_price_threshold",
            manteon::instruction::FinalizePriceThreshold {}.data(),
        ),
        (
            "expire_price_observation",
            manteon::instruction::ExpirePriceObservation {}.data(),
        ),
    ] {
        assert_instruction(
            &idl,
            name,
            data,
            &[
                ("caller", true, false),
                ("config", false, true),
                ("pool", false, true),
                ("event", false, true),
                ("vault", false, false),
                ("monitor", false, true),
            ],
        );
    }
    let names = idl["instructions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        names,
        [
            "initialize_global_config",
            "update_global_config",
            "propose_global_admin",
            "accept_global_admin",
            "withdraw_platform_fees",
            "create_config",
            "update_config",
            "complete_pool",
            "create_pool",
            "activate_pool",
            "activate_price_pool",
            "record_price_observation",
            "finalize_price_threshold",
            "expire_price_observation",
            "stake",
            "unstake",
            "exit",
            "propose_company_ipo",
            "begin_settlement",
            "process_positions",
            "finalize_settlement",
            "claim"
        ]
        .into_iter()
        .collect()
    );
    let accounts = idl["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 7);
    for (name, discriminator) in [
        ("GlobalConfig", manteon::state::GlobalConfig::DISCRIMINATOR),
        ("Config", manteon::state::Config::DISCRIMINATOR),
        ("Pool", manteon::state::Pool::DISCRIMINATOR),
        ("Event", manteon::state::Event::DISCRIMINATOR),
        ("PriceMonitor", manteon::state::PriceMonitor::DISCRIMINATOR),
        ("Position", manteon::state::Position::DISCRIMINATOR),
        ("Stake", manteon::state::Stake::DISCRIMINATOR),
    ] {
        let account = accounts
            .iter()
            .find(|account| account["name"] == name)
            .unwrap();
        assert_eq!(account["discriminator"], json!(discriminator));
    }
    let config_fields = fields(&idl, "Config");
    assert_eq!(
        config_fields
            .iter()
            .map(|field| field["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "authority",
            "active_pools",
            "service_authority",
            "collateral_mint",
            "fee_recipient",
            "profit_fee_bps",
            "platform_share_bps",
            "audit_config",
        ]
    );
    assert!(config_fields.iter().all(|field| field["type"]
        == if field["name"] == "active_pools" {
            "u64"
        } else if field["name"].as_str().unwrap().ends_with("_bps") {
            "u16"
        } else {
            "pubkey"
        }));
    let pool_fields = fields(&idl, "Pool")
        .iter()
        .map(|field| field["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    let expected = [
        "config",
        "event_id",
        "collateral_mint",
        "audit_config",
        "fee_recipient",
        "profit_fee_bps",
        "platform_share_bps",
        "config_released",
        "agreement_hash",
        "algorithm",
        "opens_at",
        "lock_at",
        "horizon_at",
        "state",
        "next_case_nonce",
        "current_case",
        "principal",
        "active_dates",
        "processed_dates",
        "total_weight",
        "payout_remaining",
        "claimed_principal",
        "service_eligible_at",
    ];
    assert_eq!(
        pool_fields, expected,
        "Pool retains the specified accounting fields"
    );
    let total_weight = fields(&idl, "Pool")
        .iter()
        .find(|field| field["name"] == "total_weight")
        .unwrap();
    assert_eq!(total_weight["type"], "u128");
    assert_eq!(
        fields(&idl, "Event")
            .iter()
            .map(|field| field["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["pool", "definition", "result"]
    );
}

/// The browser vectors originate from these compiled Anchor builders. This
/// catches client schema/role drift independently of its TypeScript encoder.
#[test]
fn browser_participation_vectors_match_anchor_instruction_bytes_and_account_roles() {
    let path = std::env::var_os("MANTEON_CLIENT_VECTORS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| programs::root().join("tests/fixtures/participation-vectors.json"));
    let vectors: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(vectors["programId"], manteon::ID.to_string());
    let keys = &vectors["addresses"];
    let key = |name: &str| keys[name].as_str().unwrap().parse::<Pubkey>().unwrap();
    let owner = key("owner");
    let config = key("config");
    let pool = key("pool");
    let position = key("position");
    let stake = key("stake");
    let collateral_mint = key("mint");
    let vault_authority = key("vaultAuthority");
    let vault = key("vault");
    let token_program = anchor_spl::token::spl_token::ID;
    let forecast_day = vectors["forecastDay"].as_i64().unwrap();
    let amount = vectors["amount"].as_str().unwrap().parse::<u64>().unwrap();
    assert_eq!(
        position,
        Pubkey::find_program_address(
            &[b"position", pool.as_ref(), &forecast_day.to_le_bytes()],
            &manteon::ID,
        )
        .0
    );
    assert_eq!(
        stake,
        Pubkey::find_program_address(&[b"stake", position.as_ref(), owner.as_ref()], &manteon::ID,)
            .0
    );
    let instructions = [
        (
            "stake",
            manteon::instruction::Stake {
                forecast_day,
                amount,
            }
            .data(),
            manteon::accounts::Stake {
                owner,
                payer: owner,
                config,
                pool,
                position,
                stake,
                collateral_mint,
                source: key("tokenAccount"),
                vault_authority,
                vault,
                token_program,
                system_program: anchor_lang::system_program::ID,
            }
            .to_account_metas(None),
        ),
        (
            "unstake",
            manteon::instruction::Unstake { amount }.data(),
            manteon::accounts::Unstake {
                owner,
                config,
                pool,
                position,
                stake,
                collateral_mint,
                destination: key("tokenAccount"),
                vault_authority,
                vault,
                token_program,
            }
            .to_account_metas(None),
        ),
        (
            "exit",
            manteon::instruction::Exit {}.data(),
            manteon::accounts::Exit {
                owner,
                config,
                pool,
                position,
                stake,
                collateral_mint,
                destination: key("tokenAccount"),
                vault_authority,
                vault,
                token_program,
            }
            .to_account_metas(None),
        ),
    ];
    for ((name, data, metas), expected) in instructions
        .into_iter()
        .zip(vectors["instructions"].as_array().unwrap())
    {
        assert_eq!(expected["name"], name);
        assert_eq!(
            expected["data"],
            data.iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let accounts = metas.into_iter().map(|meta| json!({
            "pubkey": meta.pubkey.to_string(), "signer": meta.is_signer, "writable": meta.is_writable,
        })).collect::<Vec<_>>();
        assert_eq!(expected["accounts"], json!(accounts));
    }
}
