use anchor_lang::{AccountDeserialize, InstructionData};
use manteon::{
    error::ManteonError,
    state::{Event, EventResult, PoolState},
};
use solana_instruction_error::InstructionError;
use solana_signer::Signer;
use solana_transaction_error::TransactionError;

use crate::support::{
    DAY, FORECAST, OPEN, TestContext,
    result::ResultFixture,
    settlement::{CaseFixture, claim_ix},
    setup::{
        config::{address, pubkey},
        pool::PoolContext,
    },
    tekmerion::case_state,
};

fn invalid(error: ManteonError) -> TransactionError {
    TransactionError::InstructionError(0, InstructionError::Custom(error.into()))
}

#[test]
fn external_result_is_typed_past_bounded_and_never_funds_invalid_facts() {
    let mut context = TestContext::new();
    context.activate();
    let alice = context.alice.insecure_clone();
    context.set_time(FORECAST * DAY);
    let result = ResultFixture::new(&context);
    let (_, valid) = CaseFixture::instruction(&context, &result);

    for data in [
        EventResult {
            extra: Some(manteon::state::EventResultExtra::PriceVerification {
                lower_usd_e9: 2_000_000_000,
                upper_usd_e9: 2_000_000_001,
            }),
            occurred_at: FORECAST * DAY,
        },
        EventResult {
            extra: None,
            occurred_at: -1,
        },
        EventResult {
            extra: None,
            occurred_at: FORECAST * DAY + 1,
        },
        EventResult {
            extra: None,
            occurred_at: manteon::result::MAX_TIMESTAMP + 1,
        },
    ] {
        let mut instruction = valid.clone();
        instruction.data = manteon::instruction::ProposeExternalEvent {
            nonce: 0,
            data,
            mode: manteon::ProposalMode::Bonded,
        }
        .data();
        context.reject(
            &[instruction],
            &[&alice],
            invalid(ManteonError::InvalidEventResult),
        );
    }

    assert_eq!(context.pool_state().service_eligible_at, None);
}

#[test]
fn external_event_settles_the_audited_occurrence_time_and_binds_both_documents() {
    {
        let mut context = TestContext::new();
        context.activate();
        let alice = context.alice.insecure_clone();
        let service = context.service.insecure_clone();
        context.set_time(OPEN);
        context.send(
            &[context.stake_ix(&alice, context.alice_token, FORECAST, 100)],
            &[&alice],
        );

        context.set_time((FORECAST + 3) * DAY);
        let result = ResultFixture::new(&context);
        let (mut case, mut instruction) = CaseFixture::instruction(&context, &result);
        let data = EventResult {
            extra: None,
            occurred_at: FORECAST * DAY,
        };
        instruction.data = manteon::instruction::ProposeExternalEvent {
            nonce: 0,
            data,
            mode: manteon::ProposalMode::Bonded,
        }
        .data();
        case.result = data;

        let metadata = context.send(&[instruction], &[&alice]);
        println!(
            "External event canonical result and Case CPI: {} CU",
            metadata.compute_units_consumed
        );

        assert!(
            metadata.compute_units_consumed < 200_000,
            "canonical formatting and CPI remain within one default instruction budget: {}",
            metadata.compute_units_consumed
        );
        assert_eq!(
            context.pool_state().service_eligible_at,
            Some((FORECAST + 3) * DAY)
        );

        let event_key = address(PoolContext::event(&context.pool));
        let original = context.svm.get_account(&event_key).unwrap();
        let event = Event::try_deserialize(&mut original.data.as_slice()).unwrap();
        let stored = case_state(&context, &case);

        assert_eq!(stored.subject, pubkey(context.pool.pubkey()));
        assert_eq!(stored.agreement_hash, context.pool_state().agreement_hash);
        assert_eq!(
            stored.result_hash,
            data.document_hash(&event.definition, stored.subject, &stored.agreement_hash)
                .unwrap()
        );

        case.finalize_yes(&mut context);

        // The accepting instruction must supply exactly the candidate committed in this Case.
        assert!(event.result.is_none());
        case.result.occurred_at += 1;
        context.reject(
            &[case.begin_ix(&context, &service)],
            &[&service],
            invalid(ManteonError::InvalidAuditAccount),
        );
        case.result = data;
        assert_eq!(context.svm.get_account(&event_key).unwrap(), original);

        context.send(
            &[
                case.begin_ix(&context, &service),
                case.process_ix(&context, &service, &[FORECAST]),
                case.finalize_ix(&context, &service),
            ],
            &[&service],
        );

        assert_eq!(context.pool_state().state, PoolState::Claimable);
        assert_eq!(context.pool_state().total_weight, 100_u128 << 64);

        context.send(
            &[claim_ix(
                &context,
                &alice,
                FORECAST,
                pubkey(context.alice_token),
            )],
            &[&alice],
        );

        assert_eq!(context.pool_state().payout_remaining, 0);
    }
}
