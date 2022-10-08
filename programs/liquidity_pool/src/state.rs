use anchor_lang::prelude::*;

#[account]
#[derive(Default)]
pub struct PoolState {
    pub total_amount_minted: u64, 
    pub fee_numerator: u64, 
    pub fee_denominator: u64,
}
