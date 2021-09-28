// https://github.com/hashblock/solana-cli-program-template/blob/main/tests/common/mod.rs
use {
    solana_program::pubkey::Pubkey,
    solana_sdk::{
        commitment_config::CommitmentConfig,
        signature::{read_keypair_file, Keypair},
        signer::Signer,
    },
    solana_validator::test_validator::TestValidatorGenesis,
    std::{path::PathBuf, str::FromStr},
};
// use solana_escrow::{id};

const LEDGER_PATH: &str = "./.ledger";
// this must match the package name in Cargo.toml and be snake_case!
const PROG_NAME: &str = "solana_escrow";

/// Setup the test validator with predefined properties
pub fn setup_validator(program_key: &Pubkey) -> TestValidatorGenesis {
    // std::env::set_var("BPF_OUT_DIR", PROG_PATH);
    let mut test_validator = TestValidatorGenesis::default();
    test_validator.ledger_path(LEDGER_PATH);
    test_validator.add_program(PROG_NAME, *program_key);
    test_validator
}

/// Ensures an empty ledger before setting up the validator
pub fn clean_ledger_setup_validator(program_key: &Pubkey) -> TestValidatorGenesis {
    if PathBuf::from_str(LEDGER_PATH).unwrap().exists() {
        std::fs::remove_dir_all(LEDGER_PATH).unwrap();
    }
    setup_validator(program_key)
}
