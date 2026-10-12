//! Load the pinned, compiled programs into an isolated local bank.

use std::{fs, path::PathBuf};

use anchor_lang::prelude::Pubkey;
use litesvm::LiteSVM;
use solana_address::Address;

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn program_svm() -> LiteSVM {
    let mut svm = LiteSVM::new();
    load_program(
        &mut svm,
        manteon::ID,
        "MANTEON_PROGRAM_SO",
        "target/deploy/manteon.so",
    );
    svm
}

pub fn load_tekmerion(svm: &mut LiteSVM) {
    load_program(
        svm,
        tekmerion_sdk::ID,
        "TEKMERION_PROGRAM_SO",
        "target/deploy/tekmerion.so",
    );
}

fn load_program(svm: &mut LiteSVM, id: Pubkey, variable: &str, default: &str) {
    let artifact = std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join(default));
    let bytes = fs::read(&artifact).unwrap_or_else(|error| {
        panic!(
            "Build the contract first with scripts/check-anchor.sh: {}: {error}",
            artifact.display()
        )
    });
    let program_id = Address::new_from_array(id.to_bytes());
    svm.add_program(program_id, &bytes)
        .expect("load real SBF artifact");
    assert!(svm.get_account(&program_id).unwrap().executable);
}
