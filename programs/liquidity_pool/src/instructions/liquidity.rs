use anchor_lang::prelude::*;

use anchor_spl::{
    token,
    token::{Mint, MintTo, Token, TokenAccount, Transfer, Burn},
};

use crate::state::PoolState;
use crate::error::ErrorCode;

pub fn add_liquidity(
    ctx: Context<LiquidityOperation>, 
    // amount of token x
    amount_liq0: u64,
    // amount of tokenY
    amount_liq1: u64, 
) -> Result<()> {

    // balances available in user token accounts
    let user_balance0 = ctx.accounts.user0.amount; 
    let user_balance1 = ctx.accounts.user1.amount; 

    // ensure enough balance 
    require!(amount_liq0 <= user_balance0, ErrorCode::NotEnoughBalance);
    require!(amount_liq1 <= user_balance1, ErrorCode::NotEnoughBalance);

    // balances available in pool token accounts
    let vault_balance0 = ctx.accounts.vault0.amount;
    let vault_balance1 = ctx.accounts.vault1.amount;

    // pool pool_state of the pool
    let pool_state = &mut ctx.accounts.pool_state; 
    
    // deposit amounts of token X, token Y and amount of pool token to mint
    let deposit0 = amount_liq0;
    let deposit1; 
    let amount_to_mint;
    
    if vault_balance0 == 0 && vault_balance1 == 0 {

        // case: initilizing the pool amount

        // bit shift, i.e., (a + b)/2
        amount_to_mint = (amount_liq0 + amount_liq1) >> 1;
        // same as 
        // amount_to_mint = (amount_liq0 + amount_liq1) / 2;
        deposit1 = amount_liq1;

    } else { 

        // case: pool amount has already been initialized

        // require equal amount deposit based on pool exchange rate 
        let exchange10 = vault_balance1.checked_div(vault_balance0).unwrap();
        deposit1 = amount_liq0.checked_mul(exchange10).unwrap();
        
        // check for enough funds and user is ok with it
        require!(deposit1 <= amount_liq1, ErrorCode::NotEnoughBalance);
        
        // mint = relative to the entire pool + total amount minted 
        // u128 so we can do multiply first without overflow 
        // then div and recast back 
        amount_to_mint = (
            (deposit1 as u128)
            .checked_mul(pool_state.total_amount_minted as u128).unwrap()
            .checked_div(vault_balance1 as u128).unwrap()
        ) as u64;

        msg!("pool mint amount: {}", amount_to_mint);
    }

    // saftey checks 
    require!(amount_to_mint > 0, ErrorCode::NoPoolMintOutput);

    // values to debug
    msg!("deposits: {} {}", deposit0, deposit1);


    // deposit user funds into vaults
    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(), 
            Transfer {
                from: ctx.accounts.user0.to_account_info(), 
                to: ctx.accounts.vault0.to_account_info(),
                authority: ctx.accounts.owner.to_account_info(), 
            }
        ), 
        deposit0
    )?;

    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(), 
            Transfer {
               from: ctx.accounts.user1.to_account_info(), 
               to: ctx.accounts.vault1.to_account_info(),
                authority: ctx.accounts.owner.to_account_info(), 
            }
        ), 
        deposit1
    )?;

    // update pool pool_state with new total pool minted
    pool_state.total_amount_minted += amount_to_mint;

    // transfer pool token to user account
    let mint_ctx = CpiContext::new(
        ctx.accounts.token_program.to_account_info(), 
        MintTo {
            to: ctx.accounts.user_pool_ata.to_account_info(),
            mint: ctx.accounts.pool_mint.to_account_info(),
            authority: ctx.accounts.pool_authority.to_account_info(),
        }
    );

    let bump = *ctx.bumps.get("pool_authority").unwrap();
    let pool_key = ctx.accounts.pool_state.key();
    let pda_sign = &[b"authority", pool_key.as_ref(), &[bump]];

    token::mint_to(
        mint_ctx.with_signer(&[pda_sign]), 
        amount_to_mint
    )?;

    Ok(())
}

pub fn remove_liquidity(
    ctx: Context<LiquidityOperation>, 
    burn_amount: u64,
) -> Result<()> {

    // user pool mint balance
    let user_pool_mint_balance = ctx.accounts.user_pool_ata.amount; 

    // safety check
    require!(burn_amount <= user_pool_mint_balance, ErrorCode::NotEnoughBalance);


    let pool_state = &mut ctx.accounts.pool_state;
    let pool_key = pool_state.key();

    // safety check
    require!(pool_state.total_amount_minted >= burn_amount, ErrorCode::BurnTooMuch);
    
    // pool vault token balances
    let vault0_amount = ctx.accounts.vault0.amount as u128;
    let vault1_amount = ctx.accounts.vault1.amount as u128;
    let u128_burn_amount = burn_amount as u128;

    // compute how much to give back 
    // amount0 -> token X and amount1 -> token Y
    let [amount0, amount1] = [
        u128_burn_amount
            .checked_mul(vault0_amount).unwrap()
            .checked_div(pool_state.total_amount_minted as u128).unwrap() as u64,
        u128_burn_amount
            .checked_mul(vault1_amount).unwrap()
            .checked_div(pool_state.total_amount_minted as u128).unwrap() as u64
    ];

    // variables required to get user account
    let bump = *ctx.bumps.get("pool_authority").unwrap();
    let pda_sign = &[b"authority", pool_key.as_ref(), &[bump]];

    // burn pool tokens 
     token::burn(CpiContext::new(
        ctx.accounts.token_program.to_account_info(), 
        Burn { 
            mint: ctx.accounts.pool_mint.to_account_info(), 
            from: ctx.accounts.user_pool_ata.to_account_info(), 
            authority:  ctx.accounts.owner.to_account_info(),
        }
    ).with_signer(&[pda_sign]), burn_amount)?;

    // update pool state with new total pool mint balance
    pool_state.total_amount_minted -= burn_amount; 

    // deposit user funds into vaults: token X
    token::transfer(
        CpiContext::new(
        ctx.accounts.token_program.to_account_info(), 
        Transfer {
                    from: ctx.accounts.vault0.to_account_info(), 
                    to: ctx.accounts.user0.to_account_info(),
                    authority: ctx.accounts.pool_authority.to_account_info(), 
                }
        ).with_signer(&[pda_sign]),
        amount0
    )?;

    // deposit user funds into vaults: token Y
    token::transfer(
        CpiContext::new(
        ctx.accounts.token_program.to_account_info(), 
            Transfer {
                from: ctx.accounts.vault1.to_account_info(), 
                to: ctx.accounts.user1.to_account_info(),
                authority: ctx.accounts.pool_authority.to_account_info(), 
            }
        ).with_signer(&[pda_sign]),
         amount1
    )?;

    Ok(())
}

#[derive(Accounts)]
pub struct LiquidityOperation<'info> {

    pub owner: Signer<'info>,

    // pool state
    #[account(mut)]
    pub pool_state: Box<Account<'info, PoolState>>,
    
    /// CHECK: safe
    #[account(seeds=[b"authority", pool_state.key().as_ref()], bump)]
    pub pool_authority: AccountInfo<'info>,

    // pool token accounts

    // pool account for token X
    #[account(
        mut, 
        constraint = vault0.mint == user0.mint,
        seeds=[b"vault0", pool_state.key().as_ref()],
        bump
    )]
    pub vault0: Box<Account<'info, TokenAccount>>, 

    // pool account for token Y
    #[account(
        mut, 
        constraint = vault1.mint == user1.mint,
        seeds=[b"vault1", pool_state.key().as_ref()], 
        bump
    )]
    pub vault1: Box<Account<'info, TokenAccount>>,

    // pool token mint
    #[account(
        mut, 
        constraint = user_pool_ata.mint == pool_mint.key(),
        seeds=[b"pool_mint", pool_state.key().as_ref()], 
        bump
    )]
    pub pool_mint: Box<Account<'info, Mint>>,  
    
    // user token accounts 

    // user account for token X
    #[account(
        mut, 
        has_one = owner
    )]
    pub user0: Box<Account<'info, TokenAccount>>, 

    // user account for token Y
    #[account(
        mut, 
        has_one = owner
    )]
    pub user1: Box<Account<'info, TokenAccount>>, 

    // user account for pool token
    #[account(
        mut, 
        has_one = owner
    )]
    pub user_pool_ata: Box<Account<'info, TokenAccount>>, 
   
    // other 
    pub token_program: Program<'info, Token>,
}
