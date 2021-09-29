#![cfg(feature = "test-bpf")]

pub mod common;

use common::clean_ledger_setup_validator;
use solana_client::rpc_client::RpcClient;
use solana_escrow::{
    instruction::{exchange, init_escrow},
    state::Escrow,
};
use solana_program::{
    program_pack::Pack, // required if we want to use the Pack trait functions on spl_token's Account
    pubkey::Pubkey,
};
use solana_sdk::{
    hash::Hash,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use assert_matches::*;

const INITIAL_MINT_AMOUNT: u64 = 10_000;
#[test]
fn test_escrow() {
    let prog_key = Pubkey::new_unique();
    let (test_validator, payer) = clean_ledger_setup_validator(&prog_key).start();
    let (rpc_client, recent_blockhash, _fee_calculator) = test_validator.rpc_client();

    let maker = Keypair::new();
    let taker = Keypair::new();
    // maker puts up escrow_token0_amount, wants to trade for escrow_token1_amount
    let escrow_token0_amount: u64 = 100;
    let escrow_token1_amount: u64 = 200;

    // 0. create two tokens (mint0/1) and mint some tokens to maker & taker's wallet accounts
    let (mint0, _mint1, _mint_owner, maker_token0, taker_token0, maker_token1, taker_token1) =
        create_tokens_and_mint(&rpc_client, &recent_blockhash, &payer, &maker, &taker);

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

    // 3. finish escrow by calling solana_escrow's Exchange entrypoint
    finish_escrow(
        &rpc_client,
        &recent_blockhash,
        &payer,
        &prog_key,
        &escrow_info_pubkey,
        &taker,
        &taker_token0,
        &taker_token1,
        escrow_token0_amount,
    );

    // maker send token0 and received token1. check if all balances are correct
    let balance = get_token_balance(&rpc_client, &maker_token0);
    assert_eq!(balance, INITIAL_MINT_AMOUNT - escrow_token0_amount);
    let balance = get_token_balance(&rpc_client, &maker_token1);
    assert_eq!(balance, INITIAL_MINT_AMOUNT + escrow_token1_amount);
    let balance = get_token_balance(&rpc_client, &taker_token0);
    assert_eq!(balance, INITIAL_MINT_AMOUNT + escrow_token0_amount);
    let balance = get_token_balance(&rpc_client, &taker_token1);
    assert_eq!(balance, INITIAL_MINT_AMOUNT - escrow_token1_amount);
}

pub fn get_token_balance(rpc_client: &RpcClient, pubkey: &Pubkey) -> u64 {
    let account = rpc_client.get_account(pubkey).unwrap();
    let account_info = spl_token::state::Account::unpack(account.data.as_slice()).unwrap();
    account_info.amount
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
    let state_space: u64 = spl_token::state::Mint::LEN as u64;
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

    // create four spl_token::Account accounts (maker/taker & token0/token1) and mint initial tokens
    let state_space: u64 = spl_token::state::Account::LEN as u64;
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
                        INITIAL_MINT_AMOUNT,         //     amount: u64
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
    let tmp_token0 = Keypair::new();

    let state_space: u64 = spl_token::state::Account::LEN as u64;
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

    // return pubkey only for maker_token0 as it's controlled by spl_token now anyway and keypair cannot sign anything
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
    let escrow_info = Keypair::new();

    let state_space: u64 = solana_escrow::state::Escrow::LEN as u64;
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

    // return the escrow info account which is then communicated to taker
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
    taker_token0_expected_amount: u64,
) {
    // fetch escrow info from blockchain like a real taker would do
    // (they'd also need to know what mint tmp_token0_pubkey corresponds to)
    let escrow_info_account = rpc_client.get_account(escrow_info).unwrap();
    let escrow_info_data = Escrow::unpack(escrow_info_account.data.as_slice()).unwrap();

    let (pda_pubkey, _) = Pubkey::find_program_address(&[b"escrow"], escrow_program_id);

    let mut transaction = Transaction::new_with_payer(
        &[exchange(
            escrow_program_id,
            &taker.pubkey(),
            taker_token1,
            taker_token0,
            &escrow_info_data.tmp_token0_pubkey,
            &escrow_info_data.maker_pubkey,
            &escrow_info_data.maker_token1_pubkey,
            escrow_info,
            &spl_token::id(),
            &pda_pubkey,
            // do NOT use the one from escrow_info_data.tmp_token0_pubkey.data.amount, we want to ensure correctness
            taker_token0_expected_amount,
        )
        .unwrap()],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[payer, taker], *recent_blockhash);
    assert_matches!(rpc_client.send_and_confirm_transaction(&transaction), Ok(_));
}
