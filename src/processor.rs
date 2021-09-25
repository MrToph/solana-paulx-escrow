use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::invoke,
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack},
    pubkey::Pubkey,
    sysvar::{rent::Rent, Sysvar},
};

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
