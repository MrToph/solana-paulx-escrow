use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack},
    pubkey::Pubkey,
    sysvar::{rent::Rent, Sysvar},
};
use spl_token::state::Account as TokenAccount;
use crate::{error::EscrowError, instruction::EscrowInstruction, state::Escrow};

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = EscrowInstruction::unpack(instruction_data)?;

    match instruction {
        EscrowInstruction::InitEscrow { amount } => {
            msg!("Instruction - InitEscrow");
            process_init_escrow(accounts, amount, program_id)
        }
        EscrowInstruction::Exchange { amount } => {
            msg!("Instruction: Exchange");
            process_exchange(accounts, amount, program_id)
        }
    }
}

fn process_init_escrow(
    accounts: &[AccountInfo],
    amount: u64,
    program_id: &Pubkey,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let maker = next_account_info(account_info_iter)?;

    if !maker.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let tmp_token0 = next_account_info(account_info_iter)?;

    let maker_token1 = next_account_info(account_info_iter)?;
    // this could still be a mint_account, but it would fail later at taker when trying to move funds to it?
    if *maker_token1.owner != spl_token::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    // we don't need to check owner==self here because we write to it and it would fail?
    let escrow_info_account = next_account_info(account_info_iter)?;
    // TODO: try this: sysvar::rent::id()
    let rent = &Rent::from_account_info(next_account_info(account_info_iter)?)?;

    if !rent.is_exempt(
        escrow_info_account.lamports(),
        escrow_info_account.data_len(),
    ) {
        return Err(EscrowError::NotRentExempt.into());
    }

    // we overwrite any contents, so it doesn't matter what's written in here
    let mut escrow_info = Escrow::unpack_unchecked(&escrow_info_account.data.borrow())?;
    if escrow_info.is_initialized() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    escrow_info.is_initialized = true;
    escrow_info.maker_pubkey = *maker.key;
    escrow_info.tmp_token0_pubkey = *tmp_token0.key;
    escrow_info.maker_token1_pubkey = *maker_token1.key;
    escrow_info.maker_token1_expected_amount = amount;

    Escrow::pack(escrow_info, &mut escrow_info_account.data.borrow_mut())?;
    // this pda will control all tmp_token0 accounts
    let (pda, _bump_seed) = Pubkey::find_program_address(&[b"escrow"], program_id);

    let token_program = next_account_info(account_info_iter)?;
    let owner_change_ix = spl_token::instruction::set_authority(
        token_program.key,
        tmp_token0.key,
        Some(&pda),
        spl_token::instruction::AuthorityType::AccountOwner,
        maker.key,
        &[&maker.key],
    )?;

    msg!("Calling the token program to transfer token account ownership...");
    invoke(
        &owner_change_ix,
        &[tmp_token0.clone(), maker.clone(), token_program.clone()],
    )?;

    Ok(())
}

fn process_exchange(accounts: &[AccountInfo], taker_token0_expected_amount: u64, program_id: &Pubkey) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let taker = next_account_info(account_info_iter)?;

    if !taker.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // will pay from this
    let taker_token1 = next_account_info(account_info_iter)?;

    // will receive to this
    let taker_token0 = next_account_info(account_info_iter)?;

    /** @audit this could be any account, why no check required that:
       1) account.owner is spl_token
       2) account.data.mint is indeed the desired token for maker

       we check it against the one stored in escrow. this one was created by maker.
       when 
    */
    let pda_tmp_token0 = next_account_info(account_info_iter)?;
    let pda_tmp_token0_data = TokenAccount::unpack(&pda_tmp_token0.data.borrow())?;

    // this prevents front-running by maker changing the amounts
    if taker_token0_expected_amount != pda_tmp_token0_data.amount {
        return Err(EscrowError::ExpectedAmountMismatch.into());
    }

    let maker = next_account_info(account_info_iter)?;
    let maker_token1 = next_account_info(account_info_iter)?;
    let escrow_account = next_account_info(account_info_iter)?;

    let escrow_info = Escrow::unpack(&escrow_account.data.borrow())?;
    // validate the provided account args against the one stored in the escrow info
    if escrow_info.tmp_token0_pubkey != *pda_tmp_token0.key {
        return Err(ProgramError::InvalidAccountData);
    }
    if escrow_info.maker_pubkey != *maker.key {
        return Err(ProgramError::InvalidAccountData);
    }
    if escrow_info.maker_token1_pubkey != *maker_token1.key {
        return Err(ProgramError::InvalidAccountData);
    }

    let token_program = next_account_info(account_info_iter)?;

    let transfer_token1_to_maker = spl_token::instruction::transfer(
        token_program.key,
        taker_token1.key,
        maker_token1.key,
        taker.key,
        &[&taker.key],
        escrow_info.maker_token1_expected_amount,
    )?;
    msg!("Calling the token program to transfer token1s to the maker...");
    invoke(
        &transfer_token1_to_maker,
        &[
            taker_token1.clone(),
            maker_token1.clone(),
            // taker is the data.owner & signer and signer exctension allows us to send on their behalf
            taker.clone(),
            token_program.clone(),
        ],
    )?;

    let (pda, bump_seed) = Pubkey::find_program_address(&[b"escrow"], program_id);
    let pda_account = next_account_info(account_info_iter)?;

    let transfer_token0_to_taker = spl_token::instruction::transfer(
        token_program.key,
        pda_tmp_token0.key,
        taker_token0.key,
        &pda,
        // we can sign as pda as we created pda
        &[&pda],
        pda_tmp_token0_data.amount,
    )?;
    msg!("Calling the token program to transfer token0s to the taker...");
    invoke_signed(
        &transfer_token0_to_taker,
        &[
            pda_tmp_token0.clone(),
            taker_token0.clone(),
            pda_account.clone(),
            token_program.clone(),
        ],
        &[&[&b"escrow"[..], &[bump_seed]]],
    )?;

    let close_pda_tmp_token0 = spl_token::instruction::close_account(
        token_program.key,
        pda_tmp_token0.key,
        // maker paid for the rent, refund them
        maker.key,
        &pda,
        &[&pda],
    )?;
    msg!("Calling the token program to close pda's temp account...");
    invoke_signed(
        &close_pda_tmp_token0,
        &[
            pda_tmp_token0.clone(),
            maker.clone(),
            pda_account.clone(),
            token_program.clone(),
        ],
        &[&[&b"escrow"[..], &[bump_seed]]],
    )?;

    msg!("Closing the escrow account...");
    // we can directly write to maker's account as long as we only increase the SOL balance
    **maker.lamports.borrow_mut() = maker
        .lamports()
        .checked_add(escrow_account.lamports())
        .ok_or(EscrowError::AmountOverflow)?;
    // important to null the data here, as someone could piggy-back another instruction after this one
    // 0 lamports does not immediately drop the account data due to insufficient rent, only after tx
    **escrow_account.lamports.borrow_mut() = 0;
    *escrow_account.data.borrow_mut() = &mut [];

    Ok(())
}
