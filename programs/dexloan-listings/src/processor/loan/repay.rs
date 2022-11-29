use anchor_lang::{
  prelude::*,
  solana_program::{
      program::{invoke},
  }
};
use anchor_spl::token::{Mint, Token, TokenAccount};
use crate::constants::*;
use crate::error::*;
use crate::state::{Loan, LoanState, TokenManager};
use crate::utils::*;

#[derive(Accounts)]
pub struct RepayLoan<'info> {
    #[account(
        constraint = signer.key() == SIGNER_PUBKEY
    )]
    pub signer: Signer<'info>,
    #[account(mut)]
    pub borrower: Signer<'info>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = borrower
    )]
    pub deposit_token_account: Box<Account<'info, TokenAccount>>,
    /// CHECK: contrained on loan_account
    #[account(mut)]
    pub lender: AccountInfo<'info>,
    #[account(
        mut,
        seeds = [
            Loan::PREFIX,
            mint.key().as_ref(),
            borrower.key().as_ref(),
        ],
        bump,
        has_one = borrower,
        has_one = mint,
        constraint = loan.lender.unwrap() == lender.key(), 
        constraint = loan.state == LoanState::Active,
        close = borrower
    )]
    pub loan: Box<Account<'info, Loan>>,
    #[account(
        mut,
        seeds = [
            TokenManager::PREFIX,
            mint.key().as_ref(),
            borrower.key().as_ref()
        ],
        bump,
    )]   
    pub token_manager: Box<Account<'info, TokenManager>>,
    pub mint: Box<Account<'info, Mint>>,
    /// CHECK: validated in cpi
    pub edition: UncheckedAccount<'info>,
    /// CHECK: validated in cpi
    pub metadata_program: UncheckedAccount<'info>, 
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub clock: Sysvar<'info, Clock>,
}

pub fn handle_repay_loan(ctx: Context<RepayLoan>) -> Result<()> {
    let loan = &mut ctx.accounts.loan;
    let token_manager = &mut ctx.accounts.token_manager;

    token_manager.accounts.loan = false;

    let duration = ctx.accounts.clock.unix_timestamp.checked_sub(
        loan.start_date.unwrap()
    ).ok_or(DexloanError::NumericalOverflow)?;

    let expiry = loan.start_date.unwrap().checked_add(loan.duration).ok_or(DexloanError::NumericalOverflow)?;
    let is_overdue = ctx.accounts.clock.unix_timestamp > expiry;
    
    let amount_due = calculate_loan_repayment(
        loan.amount.unwrap(),
        loan.basis_points,
        duration,
        is_overdue
    )?;

    let creator_fee = calculate_loan_repayment(
        loan.amount.unwrap(),
        u32::from(loan.creator_basis_points),
        duration,
        false
    )?;

    let total_amount_due = amount_due.checked_add(creator_fee).ok_or(DexloanError::NumericalOverflow)?;

    // Transfer payment
    invoke(
        &anchor_lang::solana_program::system_instruction::transfer(
            &loan.borrower,
            &loan.lender.unwrap(),
            amount_due,
        ),
        &[
            ctx.accounts.borrower.to_account_info(),
            ctx.accounts.lender.to_account_info(),
        ]
    )?;

    if token_manager.accounts.rental == false {
        thaw_and_revoke_token_account(
            token_manager,
            ctx.accounts.token_program.to_account_info(),
            ctx.accounts.deposit_token_account.to_account_info(),
            ctx.accounts.borrower.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.edition.to_account_info()
        )?;
    }

    Ok(())
}