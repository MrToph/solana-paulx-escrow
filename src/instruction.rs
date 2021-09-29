use solana_program::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvar,
};
use std::convert::TryInto;
use std::mem::size_of;

use crate::error::EscrowError::*;

pub enum EscrowInstruction {
    /// Starts the trade by creating and populating an escrow account and transferring ownership of the given temp token account to the PDA
    ///
    ///
    /// Accounts expected:
    ///
    /// 0. `[signer] maker` The account of the person initializing the escrow (maker)
    /// 1. `[writable] tmp_token0` Temporary token account that should be created prior to this instruction and owned by the initializer
    /// 2. `[] maker_token1` The maker's token account for the token they will receive should the trade go through
    /// 3. `[writable] escrow_info_account` The escrow account, it will hold all necessary info about the trade.
    /// 4. `[] rent` The rent sysvar
    /// 5. `[] token_program` The token program
    InitEscrow {
        /// the amount the maker expects to receive of token1
        amount: u64,
    },
    /// Accepts a trade
    ///
    ///
    /// Accounts expected:
    ///
    /// 0. `[signer] taker` The account of the person taking the trade (taker)
    /// 1. `[writable] taker_token1` The taker's token account for the token they send
    /// 2. `[writable] taker_token0` The taker's token account for the token they will receive should the trade go through
    /// 3. `[writable] pda_tmp_token0` The PDA's temporary token account to get tokens from and eventually close
    /// 4. `[writable] maker` The maker's main account to send their rent fees to
    /// 5. `[writable] maker_token1` The maker's token account that will receive tokens
    /// 6. `[writable] escrow_info_account` The escrow account holding the escrow info
    /// 7. `[] token_program` The token program
    /// 8. `[] pda_account` The PDA account
    Exchange {
        /// the amount the taker expects to receive of token0 in exhange for the amount in of token0 in pda_tmp_token0
        amount: u64,
    },
}

impl EscrowInstruction {
    /// Unpacks a byte buffer into a [EscrowInstruction](enum.EscrowInstruction.html).
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        let (tag, rest) = input.split_first().ok_or(InvalidInstruction)?;

        Ok(match tag {
            0 => Self::InitEscrow {
                amount: Self::unpack_init_escrow(rest)?,
            },
            1 => Self::Exchange {
                amount: Self::unpack_exchange(rest)?,
            },
            _ => return Err(InvalidInstruction.into()),
        })
    }

    fn unpack_init_escrow(input: &[u8]) -> Result<u64, ProgramError> {
        let amount = input
            .get(..8)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(InvalidInstructionData)?;
        Ok(amount)
    }

    fn unpack_exchange(input: &[u8]) -> Result<u64, ProgramError> {
        // exchange uses the same payload, just reuse the function re
        return Self::unpack_init_escrow(input);
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(size_of::<Self>());
        match self {
            &Self::InitEscrow { amount } => {
                buf.push(0); // tag for enum
                buf.extend_from_slice(&amount.to_le_bytes());
            }
            &Self::Exchange { amount } => {
                buf.push(1); // tag for enum
                buf.extend_from_slice(&amount.to_le_bytes());
            }
        };
        buf
    }
}

/// Creates a `InitEscrow` instruction.
pub fn init_escrow(
    escrow_program_id: &Pubkey,
    maker: &Pubkey,
    tmp_token0: &Pubkey,
    maker_token1: &Pubkey,
    escrow_info: &Pubkey,
    // rent_var
    token_program_id: &Pubkey,
    amount: u64,
) -> Result<Instruction, ProgramError> {
    let data = EscrowInstruction::InitEscrow { amount }.pack();

    let mut accounts = Vec::with_capacity(6);
    accounts.push(AccountMeta::new_readonly(*maker, true));
    accounts.push(AccountMeta::new(*tmp_token0, false));
    accounts.push(AccountMeta::new_readonly(*maker_token1, false));
    accounts.push(AccountMeta::new(*escrow_info, false));
    accounts.push(AccountMeta::new_readonly(sysvar::rent::id(), false));
    accounts.push(AccountMeta::new_readonly(*token_program_id, false));

    Ok(Instruction {
        program_id: *escrow_program_id,
        accounts,
        data,
    })
}

pub fn exchange(
    escrow_program_id: &Pubkey,
    taker: &Pubkey,
    taker_token1: &Pubkey,
    taker_token0: &Pubkey,
    pda_tmp_token0: &Pubkey,
    maker: &Pubkey,
    maker_token1: &Pubkey,
    escrow_info: &Pubkey,
    token_program_id: &Pubkey,
    pda: &Pubkey,
    amount: u64,
) -> Result<Instruction, ProgramError> {
    let data = EscrowInstruction::Exchange { amount }.pack();

    let mut accounts = Vec::with_capacity(9);
    accounts.push(AccountMeta::new_readonly(*taker, true));
    accounts.push(AccountMeta::new(*taker_token1, false));
    accounts.push(AccountMeta::new(*taker_token0, false));
    accounts.push(AccountMeta::new(*pda_tmp_token0, false));
    accounts.push(AccountMeta::new(*maker, false));
    accounts.push(AccountMeta::new(*maker_token1, false));
    accounts.push(AccountMeta::new(*escrow_info, false));
    accounts.push(AccountMeta::new_readonly(*token_program_id, false));
    accounts.push(AccountMeta::new_readonly(*pda, false));

    Ok(Instruction {
        program_id: *escrow_program_id,
        accounts,
        data,
    })
}
