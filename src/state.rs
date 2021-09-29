use solana_program::{
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack, Sealed},
    pubkey::Pubkey,
};

use arrayref::{array_mut_ref, array_ref, array_refs, mut_array_refs};

pub struct Escrow {
    pub is_initialized: bool,
    pub maker_pubkey: Pubkey,
    pub tmp_token0_pubkey: Pubkey,
    /// maker owned account that will receive the tokens from the taker
    pub maker_token1_pubkey: Pubkey,
    pub maker_token1_expected_amount: u64,
}

// Sealed = Solana's Sized
impl Sealed for Escrow {}

impl IsInitialized for Escrow {
    fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}

impl Pack for Escrow {
    const LEN: usize = 105;
    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let src = array_ref![src, 0, Escrow::LEN];
        let (
            is_initialized,
            maker_pubkey,
            tmp_token0_pubkey,
            maker_token1_pubkey,
            maker_token1_expected_amount,
        ) = array_refs![src, 1, 32, 32, 32, 8];
        let is_initialized = match is_initialized {
            [0] => false,
            [1] => true,
            _ => return Err(ProgramError::InvalidAccountData),
        };

        Ok(Escrow {
            is_initialized,
            maker_pubkey: Pubkey::new_from_array(*maker_pubkey),
            tmp_token0_pubkey: Pubkey::new_from_array(*tmp_token0_pubkey),
            maker_token1_pubkey: Pubkey::new_from_array(*maker_token1_pubkey),
            maker_token1_expected_amount: u64::from_le_bytes(*maker_token1_expected_amount),
        })
    }

    fn pack_into_slice(&self, dst: &mut [u8]) {
        let dst = array_mut_ref![dst, 0, Escrow::LEN];
        let (
            is_initialized_dst,
            maker_pubkey_dst,
            tmp_token0_pubkey_dst,
            maker_token1_pubkey_dst,
            expected_amount_dst,
        ) = mut_array_refs![dst, 1, 32, 32, 32, 8];

        is_initialized_dst[0] = self.is_initialized as u8;
        maker_pubkey_dst.copy_from_slice(self.maker_pubkey.as_ref());
        tmp_token0_pubkey_dst.copy_from_slice(self.tmp_token0_pubkey.as_ref());
        maker_token1_pubkey_dst.copy_from_slice(self.maker_token1_pubkey.as_ref());
        *expected_amount_dst = self.maker_token1_expected_amount.to_le_bytes();
    }
}
