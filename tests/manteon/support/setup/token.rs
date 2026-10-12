//! Create funded token fixtures through real System and SPL instructions.

use anchor_lang::solana_program::program_pack::Pack;
use anchor_spl::token::spl_token;
use litesvm::LiteSVM;
use solana_address::Address;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_system_interface::instruction as system_instruction;

use crate::support::transactions;

use super::config::pubkey;

/// The setup payer is the fixture mint's authority; ownership is explicit.
pub fn create_account(
    svm: &mut LiteSVM,
    payer: &Keypair,
    seed: u8,
    mint: Address,
    owner: Address,
    amount: u64,
) -> Address {
    let token = Keypair::new_from_array([seed; 32]);
    let instructions = [
        system_instruction::create_account(
            &pubkey(payer.pubkey()),
            &pubkey(token.pubkey()),
            svm.minimum_balance_for_rent_exemption(spl_token::state::Account::LEN),
            spl_token::state::Account::LEN as u64,
            &spl_token::ID,
        ),
        spl_token::instruction::initialize_account3(
            &spl_token::ID,
            &pubkey(token.pubkey()),
            &pubkey(mint),
            &pubkey(owner),
        )
        .unwrap(),
        spl_token::instruction::mint_to(
            &spl_token::ID,
            &pubkey(mint),
            &pubkey(token.pubkey()),
            &pubkey(payer.pubkey()),
            &[],
            amount,
        )
        .unwrap(),
    ];
    transactions::send(svm, payer, &instructions, &[&token])
        .expect("create funded token account through System and SPL instructions");
    token.pubkey()
}
