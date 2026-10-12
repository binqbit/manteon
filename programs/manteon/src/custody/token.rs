use anchor_lang::prelude::*;
use anchor_spl::token::{self, spl_token, Mint, TokenAccount, TransferChecked};

use crate::error::ManteonError;

/// Validate custody state after the instruction authenticates its owner and PDA.
pub(crate) fn validate_vault(
    vault: &TokenAccount,
    mint: &Pubkey,
    authority: &Pubkey,
    liability: u64,
) -> Result<()> {
    require!(
        vault.mint == *mint
            && vault.owner == *authority
            && vault.state == spl_token::state::AccountState::Initialized
            && vault.delegate.is_none()
            && vault.delegated_amount == 0
            && vault.close_authority.is_none()
            && vault.is_native.is_none()
            && vault.mint != spl_token::native_mint::ID,
        ManteonError::InvalidVault
    );
    require!(vault.amount >= liability, ManteonError::InsufficientBacking);
    Ok(())
}

/// Require a usable collateral account controlled by the recorded owner.
pub(crate) fn validate_owner_account(
    account: &TokenAccount,
    owner: &Pubkey,
    mint: &Pubkey,
) -> Result<()> {
    require!(
        account.owner == *owner
            && account.mint == *mint
            && account.state == spl_token::state::AccountState::Initialized
            && account.is_native.is_none(),
        ManteonError::InvalidTokenAccount
    );
    Ok(())
}

/// Transfer exact base units and confirm both balance effects before bookkeeping.
pub(crate) fn transfer_exact<'info>(
    mint: &Account<'info, Mint>,
    source: &mut Account<'info, TokenAccount>,
    destination: &mut Account<'info, TokenAccount>,
    authority: AccountInfo<'info>,
    signer_seeds: &[&[&[u8]]],
    amount: u64,
) -> Result<()> {
    require_keys_neq!(
        source.key(),
        destination.key(),
        ManteonError::InvalidTokenAccount
    );
    let source_before = source.amount;
    let destination_before = destination.amount;
    token::transfer_checked(
        CpiContext::new_with_signer(
            spl_token::ID,
            TransferChecked {
                from: source.to_account_info(),
                mint: mint.to_account_info(),
                to: destination.to_account_info(),
                authority,
            },
            signer_seeds,
        ),
        amount,
        mint.decimals,
    )?;
    source.reload()?;
    destination.reload()?;
    require!(
        source_before.checked_sub(amount) == Some(source.amount)
            && destination_before.checked_add(amount) == Some(destination.amount),
        ManteonError::UnexpectedTokenBalance
    );
    Ok(())
}
