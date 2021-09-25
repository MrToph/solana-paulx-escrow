#![cfg(feature = "test-bpf")]

use {
    assert_matches::*,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
    },
    solana_sdk::{signature::Signer, transaction::Transaction},
    solana_validator::test_validator::*,
};

use solana_escrow::{
  processor::process_instruction
};

// look at this: https://github.com/solana-labs/solana-program-library/blob/ddc029e14dc99f54d9cc76b433d300efb6ed9902/token-lending/program/src/processor.rs
// and this https://github.com/hashblock/solana-cli-program-template/blob/main/program/tests/lib.rs
#[test]
fn test_validator_transaction() {
    let program_id = Pubkey::new_unique();

    let (test_validator, payer) = TestValidatorGenesis::default()
        .add_program("solana_escrow", program_id) // this must match the package name in Cargo.toml and be snake_case!
        .start();
    let (rpc_client, recent_blockhash, _fee_calculator) = test_validator.rpc_client();

    let mut transaction = Transaction::new_with_payer(
        &[Instruction {
            program_id,
            accounts: vec![AccountMeta::new(payer.pubkey(), false)],
            data: vec![0, 1, 2, 3, 4, 5, 6, 7, 8],
        }],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer], recent_blockhash);

    assert_matches!(rpc_client.send_and_confirm_transaction(&transaction), Ok(_));
}
