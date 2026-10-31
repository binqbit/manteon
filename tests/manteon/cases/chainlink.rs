//! Real four-program execution: Manteon -> Tekmerion, Forwarder -> adapter -> Tekmerion.
use crate::support::{
    AUDIT_MINT, DAY, FORECAST, LOCK, OPEN, OWNER_BALANCE, TestContext,
    result::ResultFixture,
    settlement::{CaseFixture, claim_ix},
    setup::{
        config::{address, pubkey},
        programs::load_program,
        token,
    },
    tekmerion::{ChallengeFixture, case_state},
};
use anchor_lang::{AccountDeserialize, InstructionData, prelude::Pubkey};
use manteon::{ProposalMode, error::ManteonError};
use solana_instruction::{AccountMeta, Instruction};
use solana_instruction_error::InstructionError;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;
use tekmerion_chainlink_sdk::{
    self as root,
    confidential::{
        self as sdk, ConfidentialConfig, ConfidentialProfile, ConfidentialRequest,
        payload::{AuditReport, ReportBody, ReportPrefix},
    },
};
use tekmerion_sdk::{
    instructions,
    program::{AuditTrigger, AuditorConfig, CaseMode, CasePhase, CaseResult, Policy},
};
#[path = "../../../vendor/tekmerion/tests/support/forwarder.rs"]
mod forwarder;
use forwarder::*;
struct Transport {
    config: ConfidentialConfig,
    oracles: Pubkey,
}
impl Transport {
    fn enroll(c: &mut TestContext) -> Self {
        let transport = Self::setup(c);
        let admin = c.authority.insecure_clone();
        c.send(
            &[sdk::register_profile(
                pubkey(c.payer.pubkey()),
                pubkey(admin.pubkey()),
                c.policy,
                transport.config.clone(),
            )],
            &[&admin],
        );
        transport
    }
    fn setup(c: &mut TestContext) -> Self {
        load_program(
            &mut c.svm,
            root::ID,
            "CHAINLINK_ADAPTER_SO",
            "target/deploy/chainlink_auditor.so",
        );
        load_program(
            &mut c.svm,
            FORWARDER,
            "CHAINLINK_FORWARDER_SO",
            "target/deploy/keystone_forwarder.so",
        );
        let profile = ConfidentialProfile::address(&c.policy).0;
        let account = c.svm.get_account(&address(c.policy)).unwrap();
        let mut policy = Policy::try_deserialize(&mut account.data.as_slice()).unwrap();
        policy.rules.auditors = vec![AuditorConfig {
            authority: profile,
            fee_recipient: pubkey(c.auditors[0].pubkey()),
        }];
        policy.rules.threshold = 1;
        let admin = c.authority.insecure_clone();
        c.svm.airdrop(&admin.pubkey(), 20_000_000).unwrap();
        c.send(
            &[instructions::update_policy(
                pubkey(admin.pubkey()),
                policy.identifier,
                policy.rules,
            )],
            &[&admin],
        );
        let state = Keypair::new_from_array([0xf1; 32]);
        c.send(
            &[Instruction {
                program_id: FORWARDER,
                accounts: vec![
                    AccountMeta::new(pubkey(state.pubkey()), true),
                    AccountMeta::new(pubkey(admin.pubkey()), true),
                    AccountMeta::new_readonly(anchor_lang::system_program::ID, false),
                ],
                data: discriminator("initialize"),
            }],
            &[&state, &admin],
        );
        let config_id = ((u64::from(DON) << 32) | u64::from(CONFIG_VERSION)).to_be_bytes();
        let oracles = Pubkey::find_program_address(
            &[b"config", state.pubkey().as_ref(), &config_id],
            &FORWARDER,
        )
        .0;
        let mut data = discriminator("init_oracles_config");
        data.extend(DON.to_le_bytes());
        data.extend(CONFIG_VERSION.to_le_bytes());
        data.push(1);
        let mut addresses: Vec<_> = oracle_keys().iter().map(oracle_address).collect();
        addresses.sort();
        data.extend(serialize(&addresses));
        c.send(
            &[Instruction {
                program_id: FORWARDER,
                accounts: vec![
                    AccountMeta::new_readonly(pubkey(state.pubkey()), false),
                    AccountMeta::new(oracles, false),
                    AccountMeta::new(pubkey(admin.pubkey()), true),
                    AccountMeta::new_readonly(anchor_lang::system_program::ID, false),
                ],
                data,
            }],
            &[&admin],
        );
        let config = ConfidentialConfig {
            forwarder_program: FORWARDER,
            forwarder_state: pubkey(state.pubkey()),
            workflow_id: [0x11; 32],
            agent_config_hash: agent_config_hash(),
            max_attempts: 3,
        };
        Self { config, oracles }
    }
    fn deliver(&self, c: &mut TestContext, case: &CaseFixture, answer: u8) {
        c.send(
            &[
                sdk::compute_budget(),
                sdk::prepare_request(pubkey(c.payer.pubkey()), case.identity),
            ],
            &[],
        );
        let profile = ConfidentialProfile::address(&c.policy).0;
        let key = ConfidentialRequest::address(&profile, &case.key).0;
        let a = c.svm.get_account(&address(key)).unwrap();
        let request = ConfidentialRequest::try_deserialize(&mut a.data.as_slice()).unwrap();
        // Each synthetic CRE execution is unique across Cases, as the native
        // Forwarder replay key is scoped by execution/report ID rather than Case.
        let execution = sha(&request.digest);
        let prefix = ReportPrefix {
            version: 3,
            stage: 0,
            request_digest: request.digest,
            attempt_index: 0,
            execution_id: execution,
        };
        for body in [
            ReportBody::InputAccepted,
            ReportBody::Verdict {
                report_hash: [0x66; 32],
                answer,
                reason: if answer == 2 { 5 } else { 0 },
            },
        ] {
            let report = AuditReport {
                prefix: ReportPrefix {
                    stage: if matches!(body, ReportBody::InputAccepted) {
                        0
                    } else {
                        1
                    },
                    ..prefix.clone()
                },
                body,
            };
            let receiver = sdk::receiver_accounts(case.identity, &self.config);
            let mut raw = vec![1];
            raw.extend(execution);
            raw.extend(
                (c.svm
                    .get_sysvar::<anchor_lang::prelude::Clock>()
                    .unix_timestamp as u32)
                    .to_be_bytes(),
            );
            raw.extend(DON.to_be_bytes());
            raw.extend(CONFIG_VERSION.to_be_bytes());
            raw.extend(self.config.workflow_id);
            raw.extend([0x33; 10]);
            raw.extend([0x22; 20]);
            raw.extend([0, if report.prefix.stage == 0 { 2 } else { 3 }]);
            raw.extend(sha(&receiver
                .iter()
                .flat_map(|m| m.pubkey.to_bytes())
                .collect::<Vec<_>>()));
            raw.extend(serialize(&report.encode().unwrap()));
            let ix = signed_report_with_accounts(
                &self.config,
                receiver,
                pubkey(c.payer.pubkey()),
                self.oracles,
                raw,
                &[0, 1],
            );
            c.send(&[sdk::compute_budget(), ix], &[]);
        }
    }
}

#[test]
fn chainlink_policy_without_registered_profile_cannot_activate() {
    let mut c = TestContext::new();
    Transport::setup(&mut c);
    let service = c.service.insecure_clone();
    c.reject(
        &[c.activate_ix()],
        &[&service],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ManteonError::InvalidAuditAccount.into()),
        ),
    );
    assert_eq!(
        c.pool_state().state,
        manteon_client::state::PoolState::Draft
    );
}

#[test]
fn chainlink_mixed_committee_cannot_bypass_profile_validation() {
    let mut c = TestContext::new();
    Transport::enroll(&mut c);
    let account = c.svm.get_account(&address(c.policy)).unwrap();
    let mut policy = Policy::try_deserialize(&mut account.data.as_slice()).unwrap();
    policy.rules.auditors.push(AuditorConfig {
        authority: pubkey(c.auditors[1].pubkey()),
        fee_recipient: pubkey(c.auditors[1].pubkey()),
    });
    policy.rules.threshold = 2;
    let admin = c.authority.insecure_clone();
    c.send(
        &[instructions::update_policy(
            pubkey(admin.pubkey()),
            policy.identifier,
            policy.rules,
        )],
        &[&admin],
    );
    let service = c.service.insecure_clone();
    c.reject(
        &[c.activate_ix()],
        &[&service],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ManteonError::InvalidAuditAccount.into()),
        ),
    );
    assert_eq!(
        c.pool_state().state,
        manteon_client::state::PoolState::Draft
    );
}

#[test]
fn chainlink_dispute_policy_requires_funding_and_keeps_the_service_direct_audit_window() {
    let mut c = TestContext::with_schedule(OPEN, LOCK, OPEN + 100 * DAY);
    Transport::enroll(&mut c);
    let service = c.service.insecure_clone();
    let alice = c.alice.insecure_clone();
    let source = token::create_account(
        &mut c.svm,
        &c.payer,
        0xef,
        address(AUDIT_MINT),
        service.pubkey(),
        100,
    );
    c.activate();
    c.set_time(OPEN);
    c.send(
        &[c.stake_ix(&alice, c.alice_token, FORECAST, 100)],
        &[&alice],
    );
    c.set_time(FORECAST * DAY);
    let result = ResultFixture::new(&c);
    let (case, proposal) =
        CaseFixture::instruction_for(&c, &result, pubkey(service.pubkey()), pubkey(source));
    let mut direct = proposal.clone();
    direct.data = manteon::instruction::ProposeExternalEvent {
        nonce: 0,
        data: case.result,
        mode: ProposalMode::DirectAudit {
            expected_audit_fee: 7,
        },
    }
    .data();
    c.reject(
        &[direct],
        &[&service],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(ManteonError::ServiceWindow.into()),
        ),
    );
    assert!(c.svm.get_account(&address(case.key)).is_none());
    assert_eq!(c.token_balance(source), 100);
    c.send(&[proposal], &[&service]);
    assert_eq!(case_state(&c, &case).mode, CaseMode::Challenge);
    assert_eq!(case_state(&c, &case).phase(), CasePhase::Funding);
    assert_eq!(case_state(&c, &case).audit.deadline, None);
    let challenge = ChallengeFixture::initialize(&mut c, 0xe0, 100);
    challenge.fund(&mut c, &case);
    assert_eq!(
        case_state(&c, &case).audit.deadline,
        Some(FORECAST * DAY + 300)
    );
    assert!(!c.pool_state().public_proposals);
}

#[test]
fn chainlink_service_can_pay_first_audit_without_opening_public_proposals() {
    let mut c = TestContext::with_schedule(OPEN, LOCK, OPEN + 100 * DAY);
    let account = c.svm.get_account(&address(c.policy)).unwrap();
    let mut policy = Policy::try_deserialize(&mut account.data.as_slice()).unwrap();
    policy.rules.audit_trigger = AuditTrigger::OnCreation;
    let admin = c.authority.insecure_clone();
    c.send(
        &[instructions::update_policy(
            pubkey(admin.pubkey()),
            policy.identifier,
            policy.rules,
        )],
        &[&admin],
    );
    let transport = Transport::enroll(&mut c);
    let service = c.service.insecure_clone();
    let alice = c.alice.insecure_clone();
    let source = token::create_account(
        &mut c.svm,
        &c.payer,
        0xef,
        address(AUDIT_MINT),
        service.pubkey(),
        7,
    );
    c.activate();
    c.set_time(OPEN);
    c.send(
        &[c.stake_ix(&alice, c.alice_token, FORECAST, 100)],
        &[&alice],
    );
    c.set_time(FORECAST * DAY);
    let result = ResultFixture::new(&c);
    let (outsider_case, mut outsider) = CaseFixture::instruction(&c, &result);
    let (case, mut proposal) =
        CaseFixture::instruction_for(&c, &result, pubkey(service.pubkey()), pubkey(source));
    c.reject(
        std::slice::from_ref(&proposal),
        &[&service],
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(
                tekmerion_sdk::program::errors::TekmerionError::AuditRequired.into(),
            ),
        ),
    );
    assert!(c.svm.get_account(&address(case.key)).is_none());
    assert_eq!(c.token_balance(source), 7);
    assert!(c.pool_state().service_eligible_at.is_none());
    proposal.data = manteon::instruction::ProposeExternalEvent {
        nonce: 0,
        data: case.result,
        mode: ProposalMode::DirectAudit {
            expected_audit_fee: 7,
        },
    }
    .data();
    c.send(&[proposal], &[&service]);
    let created = case_state(&c, &case);
    assert_eq!(created.mode, CaseMode::DirectAudit);
    assert_eq!(created.phase(), CasePhase::Funded);
    assert_eq!(created.author_principal, 7);
    assert_eq!(created.challenge_target, 0);
    assert_eq!(created.audit.deadline, Some(FORECAST * DAY + 300));
    assert_eq!(c.token_balance(source), 0);
    assert_eq!(c.token_balance(address(case.vault)), 7);
    assert!(!c.pool_state().public_proposals);
    assert_eq!(c.pool_state().service_eligible_at, Some(FORECAST * DAY));
    assert!(c.event_state().result.is_none());

    // Immediate auditing does not bypass the service priority window for other authors.
    for mode in [
        ProposalMode::Bonded,
        ProposalMode::DirectAudit {
            expected_audit_fee: 7,
        },
    ] {
        outsider.data = manteon::instruction::ProposeExternalEvent {
            nonce: 0,
            data: outsider_case.result,
            mode,
        }
        .data();
        c.reject(
            std::slice::from_ref(&outsider),
            &[&alice],
            TransactionError::InstructionError(
                0,
                InstructionError::Custom(ManteonError::ServiceWindow.into()),
            ),
        );
    }
    assert!(c.svm.get_account(&address(outsider_case.key)).is_none());

    transport.deliver(&mut c, &case, 0);
    c.send(
        &[
            sdk::compute_budget(),
            sdk::apply_verdict(case.identity, pubkey(c.auditor_tokens[0])),
        ],
        &[],
    );
    assert_eq!(case_state(&c, &case).result(), Some(CaseResult::Yes));
    c.send(&[case.begin_ix(&c, &alice)], &[&alice]);
    assert_eq!(c.pool_state().accepted_case, Some(case.key));
    assert_eq!(c.event_state().result, Some(case.result));
    assert_eq!(c.pool_state().principal, 100);
    assert!(!c.pool_state().public_proposals);
}

#[test]
fn chainlink_policy_edit_after_enrollment_cannot_activate_a_market() {
    let mut c = TestContext::new();
    Transport::enroll(&mut c);
    let a = c.svm.get_account(&address(c.policy)).unwrap();
    let mut policy = Policy::try_deserialize(&mut a.data.as_slice()).unwrap();
    policy.rules.audit_timeout += 1;
    let admin = c.authority.insecure_clone();
    c.send(
        &[instructions::update_policy(
            pubkey(admin.pubkey()),
            policy.identifier,
            policy.rules,
        )],
        &[&admin],
    );
    let ix = c.activate_ix();
    let service = c.service.insecure_clone();
    assert!(crate::support::transactions::send(&mut c.svm, &c.payer, &[ix], &[&service]).is_err());
    assert_eq!(
        c.pool_state().state,
        manteon_client::state::PoolState::Draft
    );
}

#[cfg(feature = "local-fixtures")]
#[test]
fn confidential_chainlink_claim_failure_retry_verdict_and_market_settlement() {
    let mut c = TestContext::new();
    let transport = Transport::enroll(&mut c);
    let payer = pubkey(c.payer.pubkey());
    let config = &transport.config;
    c.activate();
    let alice = c.alice.insecure_clone();
    c.set_time(OPEN);
    c.send(
        &[c.stake_ix(&alice, c.alice_token, FORECAST, 100)],
        &[&alice],
    );
    c.set_time(FORECAST * DAY);
    let mut result = ResultFixture::new(&c);
    result.record_time(&c);
    let case = CaseFixture::start(&mut c, &result);
    let challenge = ChallengeFixture::initialize(&mut c, 0xe0, 200);
    challenge.fund(&mut c, &case);
    c.send(
        &[
            sdk::compute_budget(),
            sdk::prepare_request(payer, case.identity),
        ],
        &[],
    );
    let profile = ConfidentialProfile::address(&c.policy).0;
    let key = ConfidentialRequest::address(&profile, &case.key).0;
    let a = c.svm.get_account(&address(key)).unwrap();
    let q = ConfidentialRequest::try_deserialize(&mut a.data.as_slice()).unwrap();
    let original_deadline = case_state(&c, &case).audit.deadline;
    for index in 0..2u16 {
        let execution_id = [index as u8 + 1; 32];
        let prefix = ReportPrefix {
            version: 3,
            stage: 0,
            request_digest: q.digest,
            attempt_index: index,
            execution_id,
        };
        for body in [
            ReportBody::InputAccepted,
            if index == 0 {
                ReportBody::AttemptFailed {
                    code: 2,
                    report_hash: [0x77; 32],
                }
            } else {
                ReportBody::Verdict {
                    report_hash: [0x66; 32],
                    answer: 0,
                    reason: 0,
                }
            },
        ] {
            let report = AuditReport {
                prefix: ReportPrefix {
                    stage: match body {
                        ReportBody::InputAccepted => 0,
                        ReportBody::Verdict { .. } => 1,
                        _ => 2,
                    },
                    ..prefix.clone()
                },
                body,
            };
            let report_id = [0, if report.prefix.stage == 0 { 2 } else { 3 }];
            let receiver = sdk::receiver_accounts(case.identity, config);
            let mut raw = vec![1];
            raw.extend(execution_id);
            raw.extend(
                (c.svm
                    .get_sysvar::<anchor_lang::prelude::Clock>()
                    .unix_timestamp as u32)
                    .to_be_bytes(),
            );
            raw.extend(DON.to_be_bytes());
            raw.extend(CONFIG_VERSION.to_be_bytes());
            raw.extend(config.workflow_id);
            raw.extend([0x33; 10]);
            raw.extend([0x22; 20]);
            raw.extend(report_id);
            raw.extend(sha(&receiver
                .iter()
                .flat_map(|m| m.pubkey.to_bytes())
                .collect::<Vec<_>>()));
            raw.extend(serialize(&report.encode().unwrap()));
            let ix = signed_report_with_accounts(
                config,
                receiver,
                payer,
                transport.oracles,
                raw,
                &[0, 1],
            );
            c.send(&[sdk::compute_budget(), ix], &[]);
        }
    }
    c.send(
        &[
            sdk::compute_budget(),
            sdk::apply_verdict(case.identity, pubkey(c.auditor_tokens[0])),
        ],
        &[],
    );
    assert_eq!(case_state(&c, &case).result(), Some(CaseResult::Yes));
    assert_eq!(case_state(&c, &case).audit.deadline, original_deadline);
    let service = c.service.insecure_clone();
    c.send(&[case.begin_ix(&c, &service)], &[&service]);
    assert_eq!(
        c.pool_state().state,
        manteon_client::state::PoolState::Settling
    );
}
#[test]
fn chainlink_yes_no_unknown_keep_market_custody_and_only_yes_can_settle() {
    for (mode, answer) in [
        ProposalMode::Bonded,
        ProposalMode::DirectAudit {
            expected_audit_fee: 7,
        },
    ]
    .into_iter()
    .flat_map(|mode| (0..=2).map(move |answer| (mode, answer)))
    {
        let mut c = TestContext::new();
        let transport = Transport::enroll(&mut c);
        c.activate();
        let alice = c.alice.insecure_clone();
        let service = c.service.insecure_clone();
        c.set_time(OPEN);
        c.send(
            &[c.stake_ix(&alice, c.alice_token, FORECAST, 100)],
            &[&alice],
        );
        let mut result = ResultFixture::new(&c);
        c.set_time(FORECAST * DAY);
        result.record_time(&c);
        let case = match mode {
            ProposalMode::Bonded => {
                let case = CaseFixture::start(&mut c, &result);
                let challenge = ChallengeFixture::initialize(&mut c, 0xe0, 100);
                challenge.fund(&mut c, &case);
                case
            }
            ProposalMode::DirectAudit { expected_audit_fee } => {
                let (case, mut proposal) = CaseFixture::instruction(&c, &result);
                proposal.data = manteon::instruction::ProposeExternalEvent {
                    nonce: case.identity.nonce,
                    data: case.result,
                    mode,
                }
                .data();
                c.send(&[proposal], &[&alice]);
                assert_eq!(
                    case_state(&c, &case).mode,
                    tekmerion_sdk::program::CaseMode::DirectAudit
                );
                assert_eq!(c.token_balance(address(case.vault)), expected_audit_fee);
                assert_eq!(
                    c.token_balance(c.audit_source),
                    OWNER_BALANCE - expected_audit_fee
                );
                case
            }
        };
        transport.deliver(&mut c, &case, answer);
        assert_eq!(
            case_state(&c, &case).result(),
            None,
            "delivery alone does not settle"
        );
        let balance = c.token_balance(c.auditor_tokens[0]);
        let apply = sdk::apply_verdict(case.identity, pubkey(c.auditor_tokens[0]));
        c.send(std::slice::from_ref(&apply), &[]);
        c.send(&[sdk::compute_budget(), apply], &[]);
        assert_eq!(
            c.token_balance(c.auditor_tokens[0]),
            balance + 7,
            "one fee across retries"
        );
        assert_eq!(c.pool_state().principal, 100);
        assert_eq!(
            case_state(&c, &case).result(),
            Some([CaseResult::Yes, CaseResult::No, CaseResult::Unknown][answer as usize])
        );
        if answer == 0 {
            c.send(
                &[
                    case.begin_ix(&c, &service),
                    case.process_ix(&c, &service, &[FORECAST]),
                    case.finalize_ix(&c, &service),
                ],
                &[&service],
            );
            c.send(
                &[claim_ix(&c, &alice, FORECAST, pubkey(c.alice_token))],
                &[&alice],
            );
            assert_eq!(c.token_balance(c.alice_token), OWNER_BALANCE);
        } else {
            let before = c.pool_state();
            let begin = case.begin_ix(&c, &service);
            let failure =
                crate::support::transactions::send(&mut c.svm, &c.payer, &[begin], &[&service]);
            assert!(failure.is_err());
            assert_eq!(c.pool_state().state, before.state);
            assert_eq!(c.pool_state().principal, 100);
        }
    }
}
#[test]
fn chainlink_ready_result_cannot_override_core_timeout() {
    let mut c = TestContext::new();
    let transport = Transport::enroll(&mut c);
    c.activate();
    let mut result = ResultFixture::new(&c);
    c.set_time(FORECAST * DAY);
    result.record_time(&c);
    let case = CaseFixture::start(&mut c, &result);
    let challenge = ChallengeFixture::initialize(&mut c, 0xe0, 100);
    challenge.fund(&mut c, &case);
    transport.deliver(&mut c, &case, 0);
    c.set_time(case_state(&c, &case).audit.deadline.unwrap());
    c.send(&[instructions::finalize_timeout(case.identity)], &[]);
    let apply = sdk::apply_verdict(case.identity, pubkey(c.auditor_tokens[0]));
    assert!(
        crate::support::transactions::send(
            &mut c.svm,
            &c.payer,
            &[sdk::compute_budget(), apply],
            &[]
        )
        .is_err()
    );
    assert_eq!(case_state(&c, &case).result(), None);
    assert_eq!(
        case_state(&c, &case).completion_method(),
        Some(tekmerion_sdk::program::CompletionMethod::TechnicalFailure)
    );
    assert_eq!(case_state(&c, &case).fee_entitlement, 0);
}

#[test]
fn chainlink_retry_has_new_request_and_prior_claims_survive() {
    for answer in [1, 2] {
        let mut c = TestContext::new();
        let transport = Transport::enroll(&mut c);
        c.activate();
        c.set_time(FORECAST * DAY);
        let mut result = ResultFixture::new(&c);
        result.record_time(&c);
        let old = CaseFixture::start(&mut c, &result);
        let challenge = ChallengeFixture::initialize(&mut c, 0xe0, 200);
        challenge.fund(&mut c, &old);
        transport.deliver(&mut c, &old, answer);
        c.send(
            &[
                sdk::compute_budget(),
                sdk::apply_verdict(old.identity, pubkey(c.auditor_tokens[0])),
            ],
            &[],
        );
        let (new, mut proposal) = CaseFixture::instruction(&c, &result);
        proposal
            .accounts
            .push(AccountMeta::new_readonly(old.key, false));
        let alice = c.alice.insecure_clone();
        c.send(&[proposal], &[&alice]);
        assert_eq!(new.identity.nonce, old.identity.nonce + 1);
        challenge.fund(&mut c, &new);
        transport.deliver(&mut c, &new, 0);
        let profile = ConfidentialProfile::address(&c.policy).0;
        let old_request = ConfidentialRequest::address(&profile, &old.key).0;
        let new_request = ConfidentialRequest::address(&profile, &new.key).0;
        assert_ne!(old_request, new_request);
        let read = |key: Pubkey| {
            let a = c.svm.get_account(&address(key)).unwrap();
            ConfidentialRequest::try_deserialize(&mut a.data.as_slice()).unwrap()
        };
        assert_ne!(read(old_request).digest, read(new_request).digest);
        let fee_before = c.token_balance(c.auditor_tokens[0]);
        c.send(
            &[
                sdk::compute_budget(),
                sdk::apply_verdict(old.identity, pubkey(c.auditor_tokens[0])),
            ],
            &[],
        );
        assert_eq!(case_state(&c, &new).result(), None);
        assert_eq!(c.token_balance(c.auditor_tokens[0]), fee_before);
        let bob = c.bob.insecure_clone();
        c.send(
            &[instructions::claim(
                old.identity,
                pubkey(alice.pubkey()),
                instructions::ClaimDestination {
                    owner: pubkey(bob.pubkey()),
                    destination: pubkey(challenge.source),
                },
            )],
            &[&bob],
        );
        assert_eq!(
            c.token_balance(challenge.source),
            if answer == 1 { 193 } else { 100 }
        );
        c.send(
            &[
                sdk::compute_budget(),
                sdk::apply_verdict(new.identity, pubkey(c.auditor_tokens[0])),
            ],
            &[],
        );
        assert_eq!(case_state(&c, &new).result(), Some(CaseResult::Yes));
        assert_eq!(c.pool_state().accepted_case, None);
        assert_eq!(c.event_state().result, None);
        c.send(&[new.begin_ix(&c, &alice)], &[&alice]);
        assert_eq!(c.pool_state().accepted_case, Some(new.key));
        assert_eq!(c.event_state().result, Some(new.result));
        assert_eq!(c.token_balance(c.auditor_tokens[0]), fee_before + 7);
    }
}

fn agent_config_hash() -> [u8; 32] {
    let config: serde_json::Value = serde_json::from_str(include_str!(
        "../../../vendor/tekmerion/tests/fixtures/confidential/agent-config.json"
    ))
    .unwrap();
    sha(serde_json::to_string(&config).unwrap().as_bytes())
}
