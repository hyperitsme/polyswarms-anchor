use anchor_lang::prelude::*;
use anchor_lang::system_program;

declare_id!("REPLACE_WITH_PROGRAM_ID");

#[program]
pub mod polyswarms {
    use super::*;

    /// Create a new market with YES/NO vaults and a fee vault.
    pub fn initialize_market(
        ctx: Context<InitializeMarket>,
        question: String,
        close_time_unix: i64,
        fee_bps: u16,
        resolver: Pubkey,
    ) -> Result<()> {
        require!(question.len() <= 160, ErrorCode::QuestionTooLong);
        require!(fee_bps <= 1000, ErrorCode::FeeTooHigh); // max 10%
        let now = Clock::get()?.unix_timestamp;
        require!(close_time_unix > now + 60, ErrorCode::CloseTooSoon);

        let mkt = &mut ctx.accounts.market;
        mkt.authority = ctx.accounts.authority.key();
        mkt.resolver = resolver;
        mkt.status = MarketStatus::Open;
        mkt.fee_bps = fee_bps;
        mkt.close_time = close_time_unix;
        mkt.winner = Outcome::Unset;
        mkt.total_yes = 0;
        mkt.total_no = 0;
        mkt.question = question;

        mkt.bump_yes = *ctx.bumps.get("vault_yes").unwrap();
        mkt.bump_no = *ctx.bumps.get("vault_no").unwrap();
        mkt.bump_fee = *ctx.bumps.get("fee_vault").unwrap();

        Ok(())
    }

    /// Place a bet to YES or NO. `side_index` must be 1 (YES) or 2 (NO).
    pub fn place_bet(
        ctx: Context<PlaceBet>,
        side: Outcome,
        side_index: u8,
        lamports: u64,
    ) -> Result<()> {
        let mkt = &mut ctx.accounts.market;
        require!(mkt.status == MarketStatus::Open, ErrorCode::MarketClosed);
        let now = Clock::get()?.unix_timestamp;
        require!(now < mkt.close_time, ErrorCode::MarketClosed);

        require!(matches!(side, Outcome::Yes | Outcome::No), ErrorCode::InvalidSide);
        let expected_idx = if side == Outcome::Yes { 1u8 } else { 2u8 };
        require!(side_index == expected_idx, ErrorCode::InvalidSeed);
        require!(lamports >= 50_000, ErrorCode::MinStake); // >= 0.00005 SOL

        let dest = match side {
            Outcome::Yes => ctx.accounts.vault_yes.to_account_info(),
            Outcome::No => ctx.accounts.vault_no.to_account_info(),
            Outcome::Unset => return err!(ErrorCode::InvalidSide),
        };

        // Transfer user SOL into the selected vault.
        let cpi = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.user.to_account_info(),
                to: dest,
            },
        );
        system_program::transfer(cpi, lamports)?;

        if side == Outcome::Yes {
            mkt.total_yes = mkt.total_yes.checked_add(lamports).ok_or(ErrorCode::MathOverflow)?;
        } else {
            mkt.total_no = mkt.total_no.checked_add(lamports).ok_or(ErrorCode::MathOverflow)?;
        }

        let pos = &mut ctx.accounts.position;
        if pos.amount == 0 {
            pos.market = mkt.key();
            pos.owner = ctx.accounts.user.key();
            pos.side = side;
            pos.claimed = false;
        }
        pos.amount = pos.amount.checked_add(lamports).ok_or(ErrorCode::MathOverflow)?;

        emit!(Placed {
            owner: pos.owner,
            market: mkt.key(),
            side,
            amount: lamports,
        });

        Ok(())
    }

    /// Anyone can close a market after close_time (prevents late bets).
    pub fn close_market(ctx: Context<CloseMarket>) -> Result<()> {
        let mkt = &mut ctx.accounts.market;
        require!(mkt.status == MarketStatus::Open, ErrorCode::InvalidStatus);
        let now = Clock::get()?.unix_timestamp;
        require!(now >= mkt.close_time, ErrorCode::TooEarlyToClose);
        mkt.status = MarketStatus::Closed;
        Ok(())
    }

    /// Resolver (multisig/admin) sets the outcome.
    pub fn resolve_market(ctx: Context<ResolveMarket>, winner: Outcome) -> Result<()> {
        let mkt = &mut ctx.accounts.market;
        require!(mkt.status == MarketStatus::Closed, ErrorCode::InvalidStatus);
        require!(ctx.accounts.resolver.key() == mkt.resolver, ErrorCode::NotResolver);
        require!(matches!(winner, Outcome::Yes | Outcome::No | Outcome::Unset), ErrorCode::InvalidSide);

        mkt.winner = winner;
        mkt.status = MarketStatus::Resolved;
        emit!(Resolved { market: mkt.key(), winner });
        Ok(())
    }

    /// Winner claims proportional payout. If UNSET, acts as refund.
    pub fn claim(ctx: Context<Claim>) -> Result<()> {
        let mkt = &mut ctx.accounts.market;
        require!(mkt.status == MarketStatus::Resolved, ErrorCode::InvalidStatus);
        require!(!ctx.accounts.position.claimed, ErrorCode::AlreadyClaimed);
        require!(ctx.accounts.position.owner == ctx.accounts.owner.key(), ErrorCode::NotOwner);
        require!(ctx.accounts.position.market == mkt.key(), ErrorCode::InvalidMarket);

        let winner = mkt.winner;

        // INVALID/UNSET → refund staked amount from either vault.
        if winner == Outcome::Unset {
            let refund = ctx.accounts.position.amount;
            transfer_from_any_vault(refund, &ctx.accounts.vault_yes, &ctx.accounts.vault_no, &ctx.accounts.owner, &ctx.accounts.market)?;
            ctx.accounts.position.claimed = true;
            return Ok(());
        }

        require!(ctx.accounts.position.side == winner, ErrorCode::NotWinningSide);

        let total_yes = mkt.total_yes as u128;
        let total_no = mkt.total_no as u128;
        let pot = total_yes + total_no;
        require!(pot > 0, ErrorCode::EmptyPot);

        let fee_bps = mkt.fee_bps as u128;
        let user_amt = ctx.accounts.position.amount as u128;
        let winner_total = if winner == Outcome::Yes { total_yes } else { total_no };
        require!(winner_total > 0, ErrorCode::EmptyPot);

        let fee_total = pot.saturating_mul(fee_bps) / 10_000u128;
        let distributable = pot.saturating_sub(fee_total);

        let payout_u128 = (distributable.saturating_mul(user_amt)) / winner_total;
        let payout = u64::try_from(payout_u128).map_err(|_| ErrorCode::MathOverflow)?;

        let user_fee_u128 = (fee_total.saturating_mul(user_amt)) / winner_total;
        let user_fee = u64::try_from(user_fee_u128).map_err(|_| ErrorCode::MathOverflow)?;

        match winner {
            Outcome::Yes => {
                transfer_exact_from_pda(payout, &ctx.accounts.vault_yes, &ctx.accounts.owner, &ctx.accounts.market)?;
                if user_fee > 0 {
                    transfer_exact_from_pda(user_fee, &ctx.accounts.vault_yes, &ctx.accounts.fee_vault, &ctx.accounts.market)?;
                }
            }
            Outcome::No => {
                transfer_exact_from_pda(payout, &ctx.accounts.vault_no, &ctx.accounts.owner, &ctx.accounts.market)?;
                if user_fee > 0 {
                    transfer_exact_from_pda(user_fee, &ctx.accounts.vault_no, &ctx.accounts.fee_vault, &ctx.accounts.market)?;
                }
            }
            Outcome::Unset => {}
        }

        ctx.accounts.position.claimed = true;
        emit!(Claimed {
            owner: ctx.accounts.owner.key(),
            market: mkt.key(),
            amount: payout,
        });
        Ok(())
    }

    /// Admin withdraws accumulated fees from fee_vault to authority.
    pub fn admin_withdraw_fee(ctx: Context<AdminWithdrawFee>, lamports: u64) -> Result<()> {
        require!(ctx.accounts.authority.key() == ctx.accounts.market.authority, ErrorCode::NotAuthority);
        transfer_exact_from_pda(lamports, &ctx.accounts.fee_vault, &ctx.accounts.authority, &ctx.accounts.market)
    }
}

/* ---------------- helpers ---------------- */

fn transfer_from_any_vault(
    amount: u64,
    vault_yes: &AccountInfo,
    vault_no: &AccountInfo,
    to: &AccountInfo,
    _market: &Account<Market>,
) -> Result<()> {
    if **vault_yes.lamports.borrow() >= amount {
        transfer_exact_from_pda(amount, vault_yes, to, _market)?;
    } else {
        transfer_exact_from_pda(amount, vault_no, to, _market)?;
    }
    Ok(())
}

fn transfer_exact_from_pda(amount: u64, from: &AccountInfo, to: &AccountInfo, _market: &Account<Market>) -> Result<()> {
    **from.try_borrow_mut_lamports()? = from.lamports().checked_sub(amount).ok_or(ErrorCode::MathOverflow)?;
    **to.try_borrow_mut_lamports()? = to.lamports().checked_add(amount).ok_or(ErrorCode::MathOverflow)?;
    Ok(())
}

/* ---------------- accounts & types ---------------- */

#[derive(Accounts)]
pub struct InitializeMarket<'info> {
    #[account(init, payer = authority, space = Market::SPACE)]
    pub market: Account<'info, Market>,

    #[account(init, payer = authority, space = 8, seeds=[b"vault_yes", market.key().as_ref()], bump)]
    pub vault_yes: SystemAccount<'info>,
    #[account(init, payer = authority, space = 8, seeds=[b"vault_no", market.key().as_ref()], bump)]
    pub vault_no: SystemAccount<'info>,
    #[account(init, payer = authority, space = 8, seeds=[b"fee_vault", market.key().as_ref()], bump)]
    pub fee_vault: SystemAccount<'info>,

    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct PlaceBet<'info> {
    #[account(mut)]
    pub market: Account<'info, Market>,

    #[account(mut, seeds=[b"vault_yes", market.key().as_ref()], bump=market.bump_yes)]
    pub vault_yes: SystemAccount<'info>,
    #[account(mut, seeds=[b"vault_no", market.key().as_ref()], bump=market.bump_no)]
    pub vault_no: SystemAccount<'info>,

    #[account(
        init_if_needed,
        payer = user,
        space = Position::SPACE,
        seeds=[b"position", market.key().as_ref(), user.key().as_ref(), &[side_index]],
        bump
    )]
    pub position: Account<'info, Position>,

    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseMarket<'info> {
    #[account(mut)]
    pub market: Account<'info, Market>,
}

#[derive(Accounts)]
pub struct ResolveMarket<'info> {
    #[account(mut)]
    pub market: Account<'info, Market>,
    pub resolver: Signer<'info>,
}

#[derive(Accounts)]
pub struct Claim<'info> {
    #[account(mut)]
    pub market: Account<'info, Market>,

    #[account(mut, seeds=[b"vault_yes", market.key().as_ref()], bump=market.bump_yes)]
    pub vault_yes: SystemAccount<'info>,
    #[account(mut, seeds=[b"vault_no", market.key().as_ref()], bump=market.bump_no)]
    pub vault_no: SystemAccount<'info>,
    #[account(mut, seeds=[b"fee_vault", market.key().as_ref()], bump=market.bump_fee)]
    pub fee_vault: SystemAccount<'info>,

    #[account(mut, constraint = position.owner == owner.key(), constraint = position.market == market.key())]
    pub position: Account<'info, Position>,

    #[account(mut)]
    pub owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct AdminWithdrawFee<'info> {
    #[account(mut)]
    pub market: Account<'info, Market>,
    #[account(mut, seeds=[b"fee_vault", market.key().as_ref()], bump=market.bump_fee)]
    pub fee_vault: SystemAccount<'info>,
    #[account(mut)]
    pub authority: Signer<'info>,
}

#[account]
pub struct Market {
    pub authority: Pubkey,
    pub resolver: Pubkey,
    pub status: MarketStatus,
    pub fee_bps: u16,
    pub close_time: i64,
    pub winner: Outcome,
    pub total_yes: u64,
    pub total_no: u64,
    pub question: String,
    pub bump_yes: u8,
    pub bump_no: u8,
    pub bump_fee: u8,
}
impl Market {
    pub const SPACE: usize = 8
        + 32 + 32
        + 1 + 2 + 8 + 1
        + 8 + 8
        + (4 + 160)
        + 1 + 1 + 1;
}

#[account]
pub struct Position {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub side: Outcome,
    pub amount: u64,
    pub claimed: bool,
}
impl Position { pub const SPACE: usize = 8 + 32 + 32 + 1 + 8 + 1; }

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum MarketStatus { Open, Closed, Resolved }

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum Outcome { Unset, Yes, No }

#[event]
pub struct Placed { pub owner: Pubkey, pub market: Pubkey, pub side: Outcome, pub amount: u64 }
#[event]
pub struct Resolved { pub market: Pubkey, pub winner: Outcome }
#[event]
pub struct Claimed { pub owner: Pubkey, pub market: Pubkey, pub amount: u64 }

#[error_code]
pub enum ErrorCode {
    #[msg("Question too long")] QuestionTooLong,
    #[msg("Fee too high")] FeeTooHigh,
    #[msg("Close time too soon")] CloseTooSoon,
    #[msg("Market is closed")] MarketClosed,
    #[msg("Too early to close")] TooEarlyToClose,
    #[msg("Invalid status")] InvalidStatus,
    #[msg("Not resolver")] NotResolver,
    #[msg("Invalid side")] InvalidSide,
    #[msg("Invalid position seed")] InvalidSeed,
    #[msg("Not winning side")] NotWinningSide,
    #[msg("Already claimed")] AlreadyClaimed,
    #[msg("Not owner")] NotOwner,
    #[msg("Invalid market")] InvalidMarket,
    #[msg("Empty pot")] EmptyPot,
    #[msg("Math overflow")] MathOverflow,
    #[msg("Min stake not met")] MinStake,
}
