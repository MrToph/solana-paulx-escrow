#![cfg(feature = "test-bpf")]
#![allow(dead_code)]

pub mod common;

use common::clean_ledger_setup_validator;
use solana_client::rpc_client::RpcClient;
use solana_program::{
    instruction::{AccountMeta, Instruction},
    program_option::COption,
    pubkey::Pubkey,
};
use solana_sdk::{
    hash::Hash,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use spl_token::{
    instruction::approve,
    state::{Account as Token, AccountState, Mint},
};
use solana_escrow::instruction::init_escrow;
use {assert_matches::*, solana_validator::test_validator::*};

// look at this: https://github.com/solana-labs/solana-program-library/blob/ddc029e14dc99f54d9cc76b433d300efb6ed9902/token-lending/program/src/processor.rs
#[test]
fn test_escrow() {
    // let PROG_KEY = solana_escrow::id(); // we don't have the signing key for this one yet. problem?
    let prog_key = Pubkey::new_unique();
    let (test_validator, payer) = clean_ledger_setup_validator(&prog_key).start();
    let (rpc_client, recent_blockhash, _fee_calculator) = test_validator.rpc_client();

    let maker = Keypair::new();
    let maker_escrow_token0_amount: u64 = 100;

    // 0. create token ("mint") and mint some tokens to maker's main wallet (maker_token0)
    let (mint, _mint_owner, maker_token0) =
        create_tokens_and_mint(&rpc_client, &recent_blockhash, &payer, &maker);

    // perform this https://github.com/paul-schaaf/escrow-ui/blob/master/src/util/initEscrow.ts
    // 1. create maker's tmp token0 balance account and sends escrow amount to it
    let tmp_token0 = create_tmp_maker_token0(
        &rpc_client,
        &recent_blockhash,
        &payer,
        &mint.pubkey(),
        &maker,
        &maker_token0,
        maker_escrow_token0_amount,
    );

    // 2. create empty account owned by escrow program and call solana_escrow's InitEscrow entrypoint
    start_escrow(
        &rpc_client,
        &recent_blockhash,
        &payer,
        &prog_key,
        &maker,
        &tmp_token0,
        &maker_token0, // TODO: create token1 spl_token::Account for maker
        maker_escrow_token0_amount,
    );
}

fn create_tokens_and_mint(
    rpc_client: &RpcClient,
    recent_blockhash: &Hash,
    payer: &Keypair,
    maker: &Keypair,
) -> (Keypair, Keypair, Pubkey) {
    let mint = Keypair::new();
    let mint_owner = Keypair::new();
    let maker_token0 = Keypair::new(); // main token0 wallet for maker

    // create the token aka spl_token::Mint account
    let state_space: u64 = 82u64; // spl_token::state::Mint::LEN; // see spl_token's state for Mint
    let account_lamports = rpc_client
        .get_minimum_balance_for_rent_exemption(state_space as usize)
        .unwrap();
    let mut transaction = Transaction::new_with_payer(
        &[
            system_instruction::create_account(
                &payer.pubkey(),  // from_pubkey: &Pubkey,
                &mint.pubkey(),   //     to_pubkey: &Pubkey,
                account_lamports, //     lamports: u64,
                state_space,      //     space: u64,
                &spl_token::id(), //     owner: &Pubkey
            ),
            spl_token::instruction::initialize_mint(
                &spl_token::id(),     //     token_program_id: &Pubkey,
                &mint.pubkey(),       //     mint_pubkey: &Pubkey,
                &mint_owner.pubkey(), //     mint_authority_pubkey: &Pubkey,
                None,                 //     freeze_authority_pubkey: Option<&Pubkey>,
                0,                    //     decimals: u8
            )
            .unwrap(),
        ],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, &mint], *recent_blockhash);
    assert_matches!(rpc_client.send_and_confirm_transaction(&transaction), Ok(_));

    // create an spl_token::Account account for maker for mint, and mint some tokens for it
    let state_space: u64 = 165u64; // spl_token::state::Account::LEN; // see spl_token's state for Account
    let account_lamports = rpc_client
        .get_minimum_balance_for_rent_exemption(state_space as usize)
        .unwrap();
    let mut transaction = Transaction::new_with_payer(
        &[
            system_instruction::create_account(
                &payer.pubkey(),        // from_pubkey: &Pubkey,
                &maker_token0.pubkey(), //     to_pubkey: &Pubkey,
                account_lamports,       //     lamports: u64,
                state_space,            //     space: u64,
                &spl_token::id(),       //     owner: &Pubkey
            ),
            spl_token::instruction::initialize_account(
                &spl_token::id(),       //  token_program_id: &Pubkey,
                &maker_token0.pubkey(), //     account_pubkey: &Pubkey,
                &mint.pubkey(),         //     mint_pubkey: &Pubkey,
                &maker.pubkey(),        //     owner_pubkey: &Pubkey
            )
            .unwrap(),
            // mint tokens to maker's main wallet
            spl_token::instruction::mint_to(
                &spl_token::id(),        //  token_program_id: &Pubkey,
                &mint.pubkey(),          //     mint_pubkey: &Pubkey,
                &maker_token0.pubkey(),  //     account_pubkey: &Pubkey,
                &mint_owner.pubkey(),    //     owner_pubkey: &Pubkey,
                &[&mint_owner.pubkey()], //     signer_pubkeys: &[&Pubkey],
                10_000,                  //     amount: u64
            )
            .unwrap(),
        ],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, &maker_token0, &mint_owner], *recent_blockhash);
    assert_matches!(rpc_client.send_and_confirm_transaction(&transaction), Ok(_));

    // return pubkey for maker_token0 as it's controlled by spl_token now anyway and cannot sign anything
    (mint, mint_owner, maker_token0.pubkey())
}

fn create_tmp_maker_token0(
    rpc_client: &RpcClient,
    recent_blockhash: &Hash,
    payer: &Keypair,
    mint_pubkey: &Pubkey,
    maker: &Keypair,
    maker_token0_pubkey: &Pubkey,
    maker_escrow_token0_amount: u64,
) -> Pubkey {
    let tmp_token0 = Keypair::new(); // main token0 wallet for maker

    let state_space: u64 = 165u64; // spl_token::state::Account::LEN; // see spl_token's state for Account
    let account_lamports = rpc_client
        .get_minimum_balance_for_rent_exemption(state_space as usize)
        .unwrap();

    let mut transaction = Transaction::new_with_payer(
        &[
            system_instruction::create_account(
                &payer.pubkey(),      // from_pubkey: &Pubkey,
                &tmp_token0.pubkey(), //     to_pubkey: &Pubkey,
                account_lamports,     //     lamports: u64,
                state_space,          //     space: u64,
                &spl_token::id(),     //     owner: &Pubkey
            ),
            spl_token::instruction::initialize_account(
                &spl_token::id(),     //  token_program_id: &Pubkey,
                &tmp_token0.pubkey(), //     account_pubkey: &Pubkey,
                &mint_pubkey,         //     mint_pubkey: &Pubkey,
                &maker.pubkey(),      //     owner_pubkey: &Pubkey
            )
            .unwrap(),
            spl_token::instruction::transfer(
                &spl_token::id(),     //     token_program_id: &Pubkey,
                &maker_token0_pubkey, //     source_pubkey: &Pubkey,
                &tmp_token0.pubkey(), //     destination_pubkey: &Pubkey,
                // account that has maker_token0.data.owner or delegate
                &maker.pubkey(),            //     authority_pubkey: &Pubkey,
                &[],                        //     signer_pubkeys: &[&Pubkey],
                maker_escrow_token0_amount, //     amount: u64
            )
            .unwrap(),
        ],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, &tmp_token0, maker], *recent_blockhash);
    assert_matches!(rpc_client.send_and_confirm_transaction(&transaction), Ok(_));

    // return pubkey for maker_token0 as it's controlled by spl_token now anyway and cannot sign anything
    tmp_token0.pubkey()
}

fn start_escrow(
    rpc_client: &RpcClient,
    recent_blockhash: &Hash,
    payer: &Keypair,
    escrow_program_id: &Pubkey,
    maker: &Keypair,
    tmp_token0: &Pubkey,
    maker_token1: &Pubkey,
    maker_escrow_token0_amount: u64,
) {
    let escrow_info = Keypair::new(); // main token0 wallet for maker

    let state_space: u64 = 105u64; // solana_escrow::state::Escrow::LEN;
    let account_lamports = rpc_client
        .get_minimum_balance_for_rent_exemption(state_space as usize)
        .unwrap();

    let mut transaction = Transaction::new_with_payer(
        &[
            system_instruction::create_account(
                &payer.pubkey(),       // from_pubkey: &Pubkey,
                &escrow_info.pubkey(), //     to_pubkey: &Pubkey,
                account_lamports,      //     lamports: u64,
                state_space,           //     space: u64,
                escrow_program_id,      //     owner: &Pubkey
            ),
            init_escrow(
                escrow_program_id,
                &maker.pubkey(),
                tmp_token0,
                maker_token1,
                &escrow_info.pubkey(),
                &spl_token::id(),
                maker_escrow_token0_amount,
            ).unwrap(),
        ],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, &escrow_info, maker], *recent_blockhash);
    assert_matches!(rpc_client.send_and_confirm_transaction(&transaction), Ok(_));
}
