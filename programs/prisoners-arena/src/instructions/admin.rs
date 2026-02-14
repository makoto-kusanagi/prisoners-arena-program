//! Admin instructions

use anchor_lang::prelude::*;
use crate::state::{Config, Tournament, TournamentState};
use crate::error::ArenaError;

/// Initialize global config and Tournament #0
#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    #[account(
        init,
        payer = admin,
        space = Config::LEN,
        seeds = [b"config"],
        bump
    )]
    pub config: Account<'info, Config>,

    /// Tournament #0 created on init with base size (grows via realloc as players join)
    #[account(
        init,
        payer = admin,
        space = Tournament::BASE_SPACE,
        seeds = [b"tournament", 0u32.to_le_bytes().as_ref()],
        bump
    )]
    pub tournament: Account<'info, Tournament>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn initialize_config(
    ctx: Context<InitializeConfig>,
    operator: Pubkey,
    stake: u64,
    min_participants: u16,
    max_participants: u16,
    registration_duration: i64,
    matches_per_player: u16,
    reveal_duration: i64,
) -> Result<()> {
    // Validate min_participants is even and >= 2
    require!(
        min_participants >= 2 && min_participants % 2 == 0,
        ArenaError::InvalidMinParticipants
    );
    
    let config = &mut ctx.accounts.config;
    
    config.admin = ctx.accounts.admin.key();
    config.operator = operator;
    config.house_fee_bps = 0; // Start with 0% fee
    config.stake = stake;
    config.min_participants = min_participants;
    config.max_participants = max_participants;
    config.registration_duration = registration_duration;
    config.matches_per_player = matches_per_player;
    config.accumulated_fees = 0;
    config.current_tournament_id = 0;
    config.reveal_duration = reveal_duration;
    config.bump = ctx.bumps.config;

    // Initialize Tournament #0
    let tournament = &mut ctx.accounts.tournament;
    let clock = Clock::get()?;
    
    tournament.id = 0;
    tournament.state = TournamentState::Registration;
    tournament.stake = stake;
    tournament.house_fee_bps = 0;
    tournament.matches_per_player = matches_per_player;
    tournament.registration_duration = registration_duration;
    tournament.reveal_duration = reveal_duration;
    tournament.pool = 0;
    tournament.participant_count = 0;
    tournament.registration_ends = clock.unix_timestamp + registration_duration;
    tournament.matches_completed = 0;
    tournament.matches_total = 0;
    tournament.randomness_seed = [0u8; 32];
    tournament.min_winning_score = 0;
    tournament.winner_count = 0;
    tournament.winner_pool = 0;
    tournament.claims_processed = 0;
    tournament.payout_started_at = 0;
    tournament.entries_remaining = 0;
    tournament.round_tier = 0;
    tournament.reveal_ends = 0;
    tournament.reveals_completed = 0;
    tournament.forfeits = 0;
    tournament.players = Vec::new();
    tournament.scores = Vec::new();
    tournament.strategies = Vec::new();
    tournament.strategy_params = Vec::new();
    tournament.bump = ctx.bumps.tournament;

    msg!("Config initialized by {}, operator = {}", config.admin, config.operator);
    msg!("Tournament #0 created, registration ends at {}", tournament.registration_ends);
    
    Ok(())
}

/// Update config parameters
#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump = config.bump,
        has_one = admin @ ArenaError::Unauthorized
    )]
    pub config: Account<'info, Config>,

    pub admin: Signer<'info>,
}

pub fn update_config(
    ctx: Context<UpdateConfig>,
    operator: Option<Pubkey>,
    house_fee_bps: Option<u16>,
    stake: Option<u64>,
    min_participants: Option<u16>,
    max_participants: Option<u16>,
    registration_duration: Option<i64>,
    matches_per_player: Option<u16>,
    reveal_duration: Option<i64>,
) -> Result<()> {
    let config = &mut ctx.accounts.config;

    if let Some(op) = operator {
        config.operator = op;
    }

    if let Some(fee) = house_fee_bps {
        require!(fee <= 10000, ArenaError::Overflow);
        config.house_fee_bps = fee;
    }

    if let Some(s) = stake {
        require!(s > 0, ArenaError::Overflow);
        config.stake = s;
    }

    if let Some(participants) = min_participants {
        require!(
            participants >= 2 && participants % 2 == 0,
            ArenaError::InvalidMinParticipants
        );
        config.min_participants = participants;
    }

    if let Some(max) = max_participants {
        require!(max >= config.min_participants, ArenaError::Overflow);
        config.max_participants = max;
    }

    if let Some(duration) = registration_duration {
        require!(duration > 0, ArenaError::Overflow);
        config.registration_duration = duration;
    }

    if let Some(k) = matches_per_player {
        require!(k > 0, ArenaError::Overflow);
        config.matches_per_player = k;
    }

    if let Some(duration) = reveal_duration {
        require!(duration > 0, ArenaError::Overflow);
        config.reveal_duration = duration;
    }

    msg!("Config updated");
    Ok(())
}

/// Withdraw accumulated fees
#[derive(Accounts)]
pub struct WithdrawFees<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump = config.bump,
        has_one = admin @ ArenaError::Unauthorized
    )]
    pub config: Account<'info, Config>,

    #[account(mut)]
    pub admin: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn withdraw_fees(ctx: Context<WithdrawFees>) -> Result<()> {
    let config = &mut ctx.accounts.config;
    
    require!(config.accumulated_fees > 0, ArenaError::NoFeesToWithdraw);

    let amount = config.accumulated_fees;
    config.accumulated_fees = 0;

    // Transfer from config account to admin
    **config.to_account_info().try_borrow_mut_lamports()? -= amount;
    **ctx.accounts.admin.try_borrow_mut_lamports()? += amount;

    msg!("Withdrew {} lamports in fees", amount);
    Ok(())
}
