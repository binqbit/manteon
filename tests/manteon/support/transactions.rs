//! Explicit signing, transaction admission and full-account rollback checks.

use std::collections::BTreeSet;

use litesvm::{LiteSVM, types::TransactionResult};
use solana_address::Address;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;
use solana_transaction_error::TransactionError;

// Retain LiteSVM's full transaction metadata for assertion diagnostics.
#[allow(clippy::result_large_err)]
pub fn send(
    svm: &mut LiteSVM,
    payer: &Keypair,
    instructions: &[Instruction],
    signers: &[&Keypair],
) -> TransactionResult {
    svm.expire_blockhash();
    let mut transaction_signers = vec![payer];
    for signer in signers {
        if !transaction_signers
            .iter()
            .any(|existing| existing.pubkey() == signer.pubkey())
        {
            transaction_signers.push(signer);
        }
    }
    let transaction = Transaction::new(
        transaction_signers.as_slice(),
        Message::new(instructions, Some(&payer.pubkey())),
        svm.latest_blockhash(),
    );
    assert!(
        wincode::serialize(&transaction).unwrap().len() <= 1232,
        "transaction exceeds the Solana packet limit"
    );
    svm.send_transaction(transaction)
}

pub fn assert_rejected(
    svm: &mut LiteSVM,
    payer: &Keypair,
    instructions: &[Instruction],
    signers: &[&Keypair],
    extra_accounts: impl IntoIterator<Item = Address>,
    expected: TransactionError,
) {
    let payer_before = svm.get_account(&payer.pubkey()).unwrap();
    let mut observed = extra_accounts.into_iter().collect::<BTreeSet<_>>();
    observed.extend(
        instructions
            .iter()
            .flat_map(|instruction| instruction.accounts.iter().map(|meta| meta.pubkey)),
    );
    observed.remove(&payer.pubkey());
    let before = observed
        .iter()
        .map(|key| (*key, svm.get_account(key)))
        .collect::<Vec<_>>();
    let failure = send(svm, payer, instructions, signers).unwrap_err();
    assert_eq!(failure.err, expected, "unexpected rejection: {failure:#?}");
    for (key, account) in before {
        assert_eq!(svm.get_account(&key), account, "rollback for {key}");
    }
    let mut payer_after = svm.get_account(&payer.pubkey()).unwrap();
    assert_eq!(
        payer_before.lamports - payer_after.lamports,
        failure.meta.fee
    );
    payer_after.lamports = payer_before.lamports;
    assert_eq!(payer_before, payer_after, "only transaction fee may change");
}
