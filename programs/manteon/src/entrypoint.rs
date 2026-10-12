//! Complete Manteon payload validation before Anchor's generated dispatch.
//!
//! Anchor accepts a decoded prefix. Reuse its generated argument types to
//! reject unread suffixes without duplicating layouts or account validation.
//! This is the runtime entrypoint for both direct instructions and CPI.

use anchor_lang::{prelude::*, solana_program::entrypoint::ProgramResult};

use crate::instruction::*;

// Solana's standard entrypoint macro provides the required unsafe runtime glue.
#[allow(unsafe_code)]
mod runtime {
    use super::process_instruction;
    anchor_lang::solana_program::entrypoint!(process_instruction);
}

fn process_instruction<'info>(
    program_id: &'info Pubkey,
    accounts: &'info [AccountInfo<'info>],
    data: &'info [u8],
) -> ProgramResult {
    let validation = validate(data);
    validation.map_err(|error| {
        error.log();
        anchor_lang::solana_program::program_error::ProgramError::from(error)
    })?;
    crate::entry(program_id, accounts, data)
}

fn decode_complete<T: AnchorDeserialize>(payload: &[u8]) -> Result<()> {
    T::try_from_slice(payload)
        .map(|_| ())
        .map_err(|_| error!(anchor_lang::error::ErrorCode::InstructionDidNotDeserialize))
}

fn validate(data: &[u8]) -> Result<()> {
    macro_rules! instruction {
        ($ty:ty) => {
            if let Some(payload) = data.strip_prefix(<$ty>::DISCRIMINATOR) {
                return decode_complete::<$ty>(payload);
            }
        };
    }
    instruction!(CreateConfig);
    instruction!(UpdateConfig);
    instruction!(CompletePool);
    instruction!(InitializeGlobalConfig);
    instruction!(UpdateGlobalConfig);
    instruction!(ProposeGlobalAdmin);
    instruction!(AcceptGlobalAdmin);
    instruction!(WithdrawPlatformFees);
    instruction!(CreatePool);
    instruction!(ActivatePool);
    instruction!(ActivatePricePool);
    instruction!(RecordPriceObservation);
    instruction!(FinalizePriceThreshold);
    instruction!(ExpirePriceObservation);
    instruction!(Stake);
    instruction!(Unstake);
    instruction!(Exit);
    instruction!(ProposeCompanyIpo);
    instruction!(BeginSettlement);
    instruction!(ProcessPositions);
    instruction!(FinalizeSettlement);
    instruction!(Claim);
    Err(error!(
        anchor_lang::error::ErrorCode::InstructionFallbackNotFound
    ))
}
