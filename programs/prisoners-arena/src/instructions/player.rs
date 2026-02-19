//! Player instructions

use anchor_lang::prelude::*;
use anchor_lang::system_program;
use crate::state::{Config, Tournament, Entry, Strategy, TournamentState, CLAIM_EXPIRY_SECONDS, BYTES_PER_PLAYER};
use crate::error::ArenaError;
use match_logic::MAX_BYTECODE_LEN;

/// Enter the current tournament with a commitment hash
#[derive(Accounts)]
pub struct EnterTournament<'info> {
    #[account(
        seeds = [b"config"],
        bump = config.bump
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [b"tournament", tournament.id.to_le_bytes().as_ref()],
        bump = tournament.bump,
        realloc = Tournament::BASE_SPACE + ((tournament.players.len() + 1) * BYTES_PER_PLAYER),
        realloc::payer = player,
        realloc::zero = false
    )]
    pub tournament: Account<'info, Tournament>,

    #[account(
        init,
        payer = player,
        space = Entry::LEN,
        seeds = [b"entry", tournament.key().as_ref(), player.key().as_ref()],
        bump
    )]
    pub entry: Account<'info, Entry>,

    #[account(mut)]
    pub player: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn enter_tournament(
    ctx: Context<EnterTournament>,
    commitment: [u8; 32],
) -> Result<()> {
    let config = &ctx.accounts.config;
    let tournament = &mut ctx.accounts.tournament;
    let entry = &mut ctx.accounts.entry;
    let player = &ctx.accounts.player;

    // Validate state — players can join anytime while in Registration
    require!(
        tournament.state == TournamentState::Registration,
        ArenaError::RegistrationClosed
    );

    let clock = Clock::get()?;

    // Check max participants
    require!(
        tournament.players.len() < config.max_participants as usize,
        ArenaError::TournamentFull
    );

    // Use snapshotted stake from tournament
    let stake = tournament.stake;

    // Transfer stake from player to tournament
    system_program::transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: player.to_account_info(),
                to: tournament.to_account_info(),
            },
        ),
        stake,
    )?;

    // Initialize entry with commitment (strategy hidden until reveal)
    entry.tournament = tournament.key();
    entry.player = player.key();
    entry.index = tournament.players.len() as u32;
    entry.commitment = commitment;
    entry.strategy = Strategy::default();           // zeroed until reveal
    entry.revealed = false;
    entry.score = 0;
    entry.matches_played = 0;
    entry.paid_out = false;
    entry.created_at = clock.unix_timestamp;
    entry.bump = ctx.bumps.entry;
    entry.bytecode_len = 0;
    entry.bytecode = [0u8; 64];

    // Add player to tournament's players vec (strategy sentinel until reveal)
    tournament.players.push(player.key());
    tournament.scores.push(0);
    tournament.strategies.push(u8::MAX);                  // sentinel: unrevealed
    tournament.participant_count += 1;
    tournament.entries_remaining += 1;
    tournament.pool += stake;

    msg!(
        "Player {} entered tournament {} at index {} with commitment",
        player.key(),
        tournament.id,
        entry.index,
    );

    Ok(())
}

/// Reveal strategy during Reveal phase
#[derive(Accounts)]
pub struct RevealStrategy<'info> {
    #[account(
        mut,
        has_one = tournament,
        has_one = player,
    )]
    pub entry: Account<'info, Entry>,

    #[account(mut)]
    pub tournament: Account<'info, Tournament>,

    #[account(mut)]
    pub player: Signer<'info>,
}

pub fn reveal_strategy(
    ctx: Context<RevealStrategy>,
    strategy: Strategy,
    salt: [u8; 16],
    bytecode: Option<Vec<u8>>,
) -> Result<()> {
    let tournament = &mut ctx.accounts.tournament;
    let entry = &mut ctx.accounts.entry;

    let clock = Clock::get()?;

    // State check
    require!(
        tournament.state == TournamentState::Reveal,
        ArenaError::InvalidState
    );

    // Deadline check
    require!(
        clock.unix_timestamp <= tournament.reveal_ends,
        ArenaError::RevealPeriodEnded
    );

    // Not already revealed
    require!(!entry.revealed, ArenaError::AlreadyRevealed);

    if strategy == Strategy::Custom {
        // Custom strategy: bytecode is required and must be valid
        let code = bytecode.as_ref().ok_or(ArenaError::InvalidBytecode)?;
        require!(!code.is_empty() && code.len() <= MAX_BYTECODE_LEN, ArenaError::InvalidBytecode);
        match_logic::validate_bytecode(code).map_err(|_| ArenaError::InvalidBytecode)?;

        // Two-level commitment: SHA256(9u8 || SHA256(bytecode) || salt[16])
        let bytecode_hash = solana_sha256_hasher::hash(code);
        let mut preimage = Vec::with_capacity(49);
        preimage.push(Strategy::Custom as u8);
        preimage.extend_from_slice(&bytecode_hash.to_bytes());
        preimage.extend_from_slice(&salt);

        let hash = solana_sha256_hasher::hash(&preimage);
        require!(
            hash.to_bytes() == entry.commitment,
            ArenaError::CommitmentMismatch
        );

        // Store bytecode in entry
        entry.bytecode_len = code.len() as u8;
        entry.bytecode[..code.len()].copy_from_slice(code);
    } else {
        // Builtin strategy: SHA256(strategy_u8 || salt[16]) — 17-byte preimage
        let mut preimage = Vec::with_capacity(17);
        preimage.push(strategy as u8);
        preimage.extend_from_slice(&salt);

        let hash = solana_sha256_hasher::hash(&preimage);
        require!(
            hash.to_bytes() == entry.commitment,
            ArenaError::CommitmentMismatch
        );
    }

    // Store revealed strategy
    entry.strategy = strategy;
    entry.revealed = true;

    // Update tournament vecs
    let idx = entry.index as usize;
    tournament.strategies[idx] = strategy as u8;

    // Track progress
    tournament.reveals_completed += 1;

    msg!(
        "Player {} revealed strategy {:?} in tournament {}",
        entry.player,
        strategy,
        tournament.id,
    );

    Ok(())
}

/// Claim refund during Registration or Reveal (allowed anytime before Running)
#[derive(Accounts)]
pub struct ClaimRefund<'info> {
    #[account(
        mut,
        seeds = [b"tournament", tournament.id.to_le_bytes().as_ref()],
        bump = tournament.bump
    )]
    pub tournament: Account<'info, Tournament>,

    #[account(
        mut,
        seeds = [b"entry", tournament.key().as_ref(), player.key().as_ref()],
        bump = entry.bump,
        has_one = player,
        has_one = tournament,
        close = player
    )]
    pub entry: Account<'info, Entry>,

    #[account(mut)]
    pub player: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn claim_refund(ctx: Context<ClaimRefund>) -> Result<()> {
    let tournament = &mut ctx.accounts.tournament;
    let entry = &ctx.accounts.entry;
    let player = &ctx.accounts.player;

    // Refund allowed during Registration or Reveal phase
    require!(
        tournament.state == TournamentState::Registration
            || tournament.state == TournamentState::Reveal,
        ArenaError::InvalidState
    );

    // Use snapshotted stake from tournament
    let refund_amount = tournament.stake;

    // Transfer stake back to player from tournament
    **tournament.to_account_info().try_borrow_mut_lamports()? -= refund_amount;
    **player.try_borrow_mut_lamports()? += refund_amount;

    // Mark player slot as refunded (set to default pubkey)
    tournament.players[entry.index as usize] = Pubkey::default();
    tournament.strategies[entry.index as usize] = u8::MAX; // 255 = refunded/invalid
    tournament.participant_count -= 1;
    tournament.entries_remaining -= 1;
    tournament.pool -= refund_amount;

    // If player had already revealed, decrement reveals_completed
    if entry.revealed {
        tournament.reveals_completed -= 1;
    }

    msg!(
        "Refunded {} lamports to player {} from tournament {}",
        refund_amount,
        player.key(),
        tournament.id
    );

    Ok(())
}

/// Claim payout if winner
#[derive(Accounts)]
pub struct ClaimPayout<'info> {
    #[account(
        mut,
        seeds = [b"tournament", tournament.id.to_le_bytes().as_ref()],
        bump = tournament.bump
    )]
    pub tournament: Account<'info, Tournament>,

    #[account(
        mut,
        seeds = [b"entry", tournament.key().as_ref(), player.key().as_ref()],
        bump = entry.bump,
        has_one = player,
        has_one = tournament,
        close = player
    )]
    pub entry: Account<'info, Entry>,

    #[account(mut)]
    pub player: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn claim_payout(ctx: Context<ClaimPayout>) -> Result<()> {
    let tournament = &mut ctx.accounts.tournament;
    let entry = &mut ctx.accounts.entry;
    let player = &ctx.accounts.player;
    let clock = Clock::get()?;

    // Must be in Payout state
    require!(
        tournament.state == TournamentState::Payout,
        ArenaError::InvalidState
    );

    // Must not have already claimed
    require!(!entry.paid_out, ArenaError::AlreadyPaid);

    // Check 30-day claim expiry
    require!(
        clock.unix_timestamp < tournament.payout_started_at + CLAIM_EXPIRY_SECONDS,
        ArenaError::ClaimExpired
    );

    // Must be a winner (score >= min_winning_score)
    require!(
        entry.score >= tournament.min_winning_score,
        ArenaError::NotWinner
    );

    // Calculate equal share (all winners split equally)
    let payout = tournament.winner_pool
        .checked_div(tournament.winner_count as u64)
        .ok_or(ArenaError::Overflow)?;

    // Transfer payout to player
    **tournament.to_account_info().try_borrow_mut_lamports()? -= payout;
    **player.try_borrow_mut_lamports()? += payout;

    // Mark as paid
    entry.paid_out = true;
    tournament.claims_processed += 1;
    tournament.entries_remaining -= 1;

    msg!(
        "Paid {} lamports to player {} from tournament {}",
        payout,
        player.key(),
        tournament.id
    );

    Ok(())
}
