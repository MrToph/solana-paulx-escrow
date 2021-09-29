#![cfg(feature = "test-bpf")]
#![allow(dead_code)]

pub mod common;

use common::clean_ledger_setup_validator;
use solana_client::rpc_client::RpcClient;
use solana_escrow::instruction::{exchange, init_escrow};
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
use {assert_matches::*, solana_validator::test_validator::*};

// look at this: https://github.com/solana-labs/solana-program-library/blob/ddc029e14dc99f54d9cc76b433d300efb6ed9902/token-lending/program/src/processor.rs
#[test]
fn test_escrow() {
    // let PROG_KEY = solana_escrow::id(); // we don't have the signing key for this one yet. problem?
    let prog_key = Pubkey::new_unique();
    let (test_validator, payer) = clean_ledger_setup_validator(&prog_key).start();
    let (rpc_client, recent_blockhash, _fee_calculator) = test_validator.rpc_client();

    let maker = Keypair::new();
    let taker = Keypair::new();
    // maker puts up this amount, wants to trade for escrow_token1_amount
    let escrow_token0_amount: u64 = 100;
    let escrow_token1_amount: u64 = 200;

    // 0. create token ("mint") and mint some tokens to maker's main wallet (maker_token0)
    let (mint0, mint1, _mint_owner, maker_token0, taker_token0, maker_token1, taker_token1) =
        create_tokens_and_mint(&rpc_client, &recent_blockhash, &payer, &maker, &taker);

    // perform this https://github.com/paul-schaaf/escrow-ui/blob/master/src/util/initEscrow.ts
    // 1. create maker's tmp token0 balance account and sends escrow amount to it
    let tmp_token0 = create_tmp_maker_token0(
        &rpc_client,
        &recent_blockhash,
        &payer,
        &mint0,
        &maker,
        &maker_token0,
        escrow_token0_amount,
    );

    // 2. create empty account owned by escrow program and call solana_escrow's InitEscrow entrypoint
    let escrow_info_pubkey = start_escrow(
        &rpc_client,
        &recent_blockhash,
        &payer,
        &prog_key,
        &maker,
        &tmp_token0,
        &maker_token1,
        escrow_token1_amount,
    );

    // 3. create token0&token1 accounts for taker. fund token1 account appropriately
    // then finish escrow by calling solana_escrow's Exchange entrypoint
    finish_escrow(
        &rpc_client,
        &recent_blockhash,
        &payer,
        &prog_key,
        &escrow_info_pubkey,
        &taker,
        &taker_token0,
        &taker_token1,
        &maker,
        &maker_token1,
        &tmp_token0,
        escrow_token0_amount,
    );

    
}

fn create_tokens_and_mint(
    rpc_client: &RpcClient,
    recent_blockhash: &Hash,
    payer: &Keypair,
    maker: &Keypair,
    taker: &Keypair,
) -> (Pubkey, Pubkey, Keypair, Pubkey, Pubkey, Pubkey, Pubkey) {
    let mint0 = Keypair::new();
    let mint1 = Keypair::new();
    // use the same mint owner for both tokens
    let mint_owner = Keypair::new();
    let maker_token0 = Keypair::new(); // main token0 wallet for maker
    let maker_token1 = Keypair::new(); // main token1 wallet for maker
    let taker_token0 = Keypair::new(); // main token0 wallet for taker
    let taker_token1 = Keypair::new(); // main token1 wallet for taker

    // create the token0 aka spl_token::Mint account
    let state_space: u64 = 82u64; // spl_token::state::Mint::LEN; // see spl_token's state for Mint
    let account_lamports = rpc_client
        .get_minimum_balance_for_rent_exemption(state_space as usize)
        .unwrap();
    let mut transaction = Transaction::new_with_payer(
        &[
            system_instruction::create_account(
                &payer.pubkey(),  // from_pubkey: &Pubkey,
                &mint0.pubkey(),  //     to_pubkey: &Pubkey,
                account_lamports, //     lamports: u64,
                state_space,      //     space: u64,
                &spl_token::id(), //     owner: &Pubkey
            ),
            spl_token::instruction::initialize_mint(
                &spl_token::id(),     //     token_program_id: &Pubkey,
                &mint0.pubkey(),      //     mint_pubkey: &Pubkey,
                &mint_owner.pubkey(), //     mint_authority_pubkey: &Pubkey,
                None,                 //     freeze_authority_pubkey: Option<&Pubkey>,
                0,                    //     decimals: u8
            )
            .unwrap(),
        ],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, &mint0], *recent_blockhash);
    assert_matches!(rpc_client.send_and_confirm_transaction(&transaction), Ok(_));

    // create the token1 aka spl_token::Mint account
    let mut transaction = Transaction::new_with_payer(
        &[
            system_instruction::create_account(
                &payer.pubkey(),  // from_pubkey: &Pubkey,
                &mint1.pubkey(),  //     to_pubkey: &Pubkey,
                account_lamports, //     lamports: u64,
                state_space,      //     space: u64,
                &spl_token::id(), //     owner: &Pubkey
            ),
            spl_token::instruction::initialize_mint(
                &spl_token::id(),     //     token_program_id: &Pubkey,
                &mint1.pubkey(),      //     mint_pubkey: &Pubkey,
                &mint_owner.pubkey(), //     mint_authority_pubkey: &Pubkey,
                None,                 //     freeze_authority_pubkey: Option<&Pubkey>,
                0,                    //     decimals: u8
            )
            .unwrap(),
        ],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, &mint1], *recent_blockhash);
    assert_matches!(rpc_client.send_and_confirm_transaction(&transaction), Ok(_));

    // create an spl_token::Account account for maker for mint0, and mint some tokens for it
    let state_space: u64 = 165u64; // spl_token::state::Account::LEN; // see spl_token's state for Account
    let account_lamports = rpc_client
        .get_minimum_balance_for_rent_exemption(state_space as usize)
        .unwrap();

    let mints = [&mint0, &mint1];
    let owners = [&maker, &taker];
    let spl_token_accounts = [&maker_token0, &taker_token0, &maker_token1, &taker_token1];
    for i in 0..mints.len() {
        for j in 0..owners.len() {
            let mint = mints[i];
            let owner = owners[j];
            let spl_token_account = spl_token_accounts[i * owners.len() + j];
            println!(
                "loop iter: {} {} {}",
                mint.pubkey(),
                owner.pubkey(),
                spl_token_account.pubkey()
            );
            let mut transaction = Transaction::new_with_payer(
                &[
                    system_instruction::create_account(
                        &payer.pubkey(),             // from_pubkey: &Pubkey,
                        &spl_token_account.pubkey(), //     to_pubkey: &Pubkey,
                        account_lamports,            //     lamports: u64,
                        state_space,                 //     space: u64,
                        &spl_token::id(),            //     owner: &Pubkey
                    ),
                    spl_token::instruction::initialize_account(
                        &spl_token::id(),            //  token_program_id: &Pubkey,
                        &spl_token_account.pubkey(), //     account_pubkey: &Pubkey,
                        &mint.pubkey(),              //     mint_pubkey: &Pubkey,
                        &owner.pubkey(),             //     owner_pubkey: &Pubkey
                    )
                    .unwrap(),
                    // mint tokens to maker's main wallet
                    spl_token::instruction::mint_to(
                        &spl_token::id(),            //  token_program_id: &Pubkey,
                        &mint.pubkey(),              //     mint_pubkey: &Pubkey,
                        &spl_token_account.pubkey(), //     account_pubkey: &Pubkey,
                        &mint_owner.pubkey(),        //     owner_pubkey: &Pubkey,
                        &[&mint_owner.pubkey()],     //     signer_pubkeys: &[&Pubkey],
                        10_000,                      //     amount: u64
                    )
                    .unwrap(),
                ],
                Some(&payer.pubkey()),
            );
            transaction.sign(&[payer, &spl_token_account, &mint_owner], *recent_blockhash);
            assert_matches!(rpc_client.send_and_confirm_transaction(&transaction), Ok(_));
        }
    }

    // return pubkey only for spl_token::Account accounts as they are controlled by spl_token now anyway
    // and cannot sign anything themselves
    (
        mint0.pubkey(),
        mint1.pubkey(),
        mint_owner,
        maker_token0.pubkey(),
        taker_token0.pubkey(),
        maker_token1.pubkey(),
        taker_token1.pubkey(),
    )
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
    maker_token1_desired_amount: u64,
) -> Pubkey {
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
                escrow_program_id,     //     owner: &Pubkey
            ),
            init_escrow(
                escrow_program_id,
                &maker.pubkey(),
                tmp_token0,
                maker_token1,
                &escrow_info.pubkey(),
                &spl_token::id(),
                maker_token1_desired_amount,
            )
            .unwrap(),
        ],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, &escrow_info, maker], *recent_blockhash);
    assert_matches!(rpc_client.send_and_confirm_transaction(&transaction), Ok(_));

    // return the escrow info account which is then communincated to taker
    escrow_info.pubkey()
}

fn finish_escrow(
    rpc_client: &RpcClient,
    recent_blockhash: &Hash,
    payer: &Keypair,
    escrow_program_id: &Pubkey,
    escrow_info: &Pubkey,
    taker: &Keypair,
    taker_token0: &Pubkey,
    taker_token1: &Pubkey,
    maker: &Keypair,
    maker_token1: &Pubkey,
    tmp_token0: &Pubkey,
    taker_token0_expected_amount: u64,
) {
    // TODO: can we fetch escrow_info data and reconstruct most of the pubkeys etc.?
    let (pda_pubkey, _) = Pubkey::find_program_address(&[b"escrow"], escrow_program_id); // how do we construct this?

    let state_space: u64 = 105u64; // solana_escrow::state::Escrow::LEN;
    let account_lamports = rpc_client
        .get_minimum_balance_for_rent_exemption(state_space as usize)
        .unwrap();

    let mut transaction = Transaction::new_with_payer(
        &[
            exchange(
                escrow_program_id,
                &taker.pubkey(),
                taker_token1,
                taker_token0,
                tmp_token0,
                &maker.pubkey(),
                maker_token1,
                escrow_info,
                &spl_token::id(),
                &pda_pubkey, // How do I create this one?
                taker_token0_expected_amount,
            )
            .unwrap(),
        ],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, taker], *recent_blockhash);
    assert_matches!(rpc_client.send_and_confirm_transaction(&transaction), Ok(_));
}
