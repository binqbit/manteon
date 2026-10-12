//! Loader-authenticated, one-time deployment policy initialization.

use anchor_lang::prelude::*;

use crate::{error::ManteonError, state::GlobalConfig};

#[derive(Accounts)]
pub struct InitializeGlobalConfig<'info> {
    pub authority: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(
        init,
        payer = payer,
        seeds = [GlobalConfig::SEED],
        bump,
        space = GlobalConfig::DISCRIMINATOR.len() + GlobalConfig::INIT_SPACE
    )]
    pub global_config: Account<'info, GlobalConfig>,
    #[account(
        constraint = program.programdata_address()? == Some(program_data.key())
            @ ManteonError::InvalidProgramData
    )]
    pub program: Program<'info, crate::program::Manteon>,
    #[account(
        constraint = program_data.upgrade_authority_address == Some(authority.key())
            @ ManteonError::UnauthorizedPlatform
    )]
    pub program_data: Account<'info, ProgramData>,
    pub system_program: Program<'info, System>,
}

pub(crate) fn handle(
    ctx: Context<InitializeGlobalConfig>,
    admin: Pubkey,
    fee_authority: Pubkey,
    platform_share_bps: u16,
) -> Result<()> {
    super::global_config::validate_settings(admin, fee_authority, platform_share_bps)?;
    ctx.accounts.global_config.set_inner(GlobalConfig {
        admin,
        pending_admin: None,
        fee_authority,
        platform_share_bps,
    });
    Ok(())
}
