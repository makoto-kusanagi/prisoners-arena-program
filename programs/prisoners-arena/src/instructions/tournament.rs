//! Tournament lifecycle instructions

use anchor_lang::prelude::*;
use crate::state::{Config, Tournament, Entry, TournamentState, CLAIM_EXPIRY_SECONDS, TOURNAMENT_CLOSURE_SECONDS, MATCHES_PER_TX};
use crate::error::ArenaError;

/// Close registration and transition to Reveal phase
#[derive(Accounts)]
pub struct CloseRegistration<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump = config.bump,
        has_one = operator @ ArenaError::Unauthorized
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [b"tournament", tournament.id.to_le_bytes().as_ref()],
        bump = tournament.bump
    )]
    pub tournament: Account<'info, Tournament>,

    pub operator: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn close_registration(ctx: Context<CloseRegistration>) -> Result<()> {
    let config = &ctx.accounts.config;
    let tournament = &mut ctx.accounts.tournament;
    let clock = Clock::get()?;

    require!(
        tournament.state == TournamentState::Registration,
        ArenaError::InvalidState
    );

    require!(
        clock.unix_timestamp >= tournament.registration_ends,
        ArenaError::RegistrationOpen
    );

    // Check if minimum participants reached
    require!(
        tournament.participant_count >= config.min_participants as u32,
        ArenaError::MinParticipantsNotReached
    );

    // Transition to Reveal phase (NOT Running — that happens after close_reveal)
    tournament.state = TournamentState::Reveal;
    tournament.reveal_ends = clock.unix_timestamp + tournament.reveal_duration;

    msg!(
        "Tournament {} registration closed with {} participants, reveal phase until {}",
        tournament.id,
        tournament.participant_count,
        tournament.reveal_ends
    );

    Ok(())
}

/// Close the reveal phase and transition to Running
/// Called by operator after reveal deadline + all forfeits processed
#[derive(Accounts)]
pub struct CloseReveal<'info> {
    #[account(
        seeds = [b"config"],
        bump = config.bump,
        has_one = operator @ ArenaError::Unauthorized
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [b"tournament", tournament.id.to_le_bytes().as_ref()],
        bump = tournament.bump
    )]
    pub tournament: Account<'info, Tournament>,

    /// CHECK: SlotHashes sysvar for randomness
    #[account(address = anchor_lang::solana_program::sysvar::slot_hashes::ID)]
    pub slot_hashes: AccountInfo<'info>,

    /// Optional: Entry to refund if odd active participant count
    pub refund_entry: Option<Account<'info, Entry>>,

    /// Player to receive refund if odd count
    /// CHECK: Validated via refund_entry.player
    #[account(mut)]
    pub refund_player: Option<AccountInfo<'info>>,

    pub operator: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn close_reveal(ctx: Context<CloseReveal>) -> Result<()> {
    let tournament = &mut ctx.accounts.tournament;
    let clock = Clock::get()?;

    require!(
        tournament.state == TournamentState::Reveal,
        ArenaError::InvalidState
    );

    require!(
        clock.unix_timestamp > tournament.reveal_ends,
        ArenaError::RevealPeriodNotEnded
    );

    // Verify all non-forfeited players have revealed
    let active_count = tournament.participant_count - tournament.forfeits;
    require!(
        tournament.reveals_completed == active_count,
        ArenaError::UnprocessedForfeits
    );

    // Handle zero active players (all forfeited/refunded)
    if active_count == 0 {
        // Transition to Running with 0 matches so finalize_tournament can
        // create the next tournament and sweep forfeited stakes to fees.
        tournament.state = TournamentState::Running;
        tournament.matches_total = 0;
        tournament.matches_completed = 0;
        msg!("Tournament {} has no active players after reveal, advancing to Running for finalization", tournament.id);
        return Ok(());
    }

    // If odd participant count, refund the last active registrant
    if active_count % 2 == 1 {
        // Find the last valid (non-refunded/non-forfeited) player
        let last_index = tournament.players.iter()
            .rposition(|pk| *pk != Pubkey::default())
            .ok_or(ArenaError::InvalidState)?;
        
        let last_player = tournament.players[last_index];
        
        // Verify refund_entry and refund_player are provided and match
        let refund_entry = ctx.accounts.refund_entry.as_ref()
            .ok_or(ArenaError::InvalidEntryAccount)?;
        let refund_player = ctx.accounts.refund_player.as_ref()
            .ok_or(ArenaError::InvalidEntryAccount)?;
        
        require!(
            refund_entry.player == last_player && refund_player.key() == last_player,
            ArenaError::InvalidEntryAccount
        );
        require!(
            refund_entry.index == last_index as u32,
            ArenaError::InvalidEntryAccount
        );

        // Process refund
        let refund_amount = tournament.stake;
        **tournament.to_account_info().try_borrow_mut_lamports()? -= refund_amount;
        **refund_player.try_borrow_mut_lamports()? += refund_amount;

        tournament.players[last_index] = Pubkey::default();
        tournament.strategies[last_index] = u8::MAX;
        tournament.strategy_params[last_index] = crate::state::StrategyParams::default();
        tournament.participant_count -= 1;
        tournament.reveals_completed -= 1;
        tournament.pool -= refund_amount;

        msg!("Refunded last player {} to ensure even participant count", last_player);
    }

    // After odd-player refund, re-check if zero active players remain
    let active_after_refund = tournament.participant_count - tournament.forfeits;
    if active_after_refund == 0 {
        tournament.state = TournamentState::Running;
        tournament.matches_total = 0;
        tournament.matches_completed = 0;
        msg!("Tournament {} has no active players after odd-player refund, advancing to Running for finalization", tournament.id);
        return Ok(());
    }

    // Generate randomness seed from slot hash (moved from close_registration)
    let slot_hashes_data = ctx.accounts.slot_hashes.try_borrow_data()?;
    let mut seed = [0u8; 32];
    require!(slot_hashes_data.len() >= 48, ArenaError::SlotHashUnavailable);
    seed.copy_from_slice(&slot_hashes_data[16..48]);
    
    // Mix in tournament-specific data
    let tournament_bytes = tournament.id.to_le_bytes();
    for (i, b) in tournament_bytes.iter().enumerate() {
        seed[i] ^= b;
    }
    tournament.randomness_seed = seed;

    // Apply adaptive K based on active participant count
    let active = tournament.participant_count - tournament.forfeits;
    let effective_k = match_logic::effective_k(active, tournament.matches_per_player);
    tournament.matches_per_player = effective_k;
    tournament.round_tier = if active <= 1000 { 0 } else { 1 };

    // Calculate total matches
    tournament.matches_total = match_logic::calculate_match_count(
        active,
        tournament.matches_per_player,
        &tournament.randomness_seed,
    );

    tournament.state = TournamentState::Running;

    msg!(
        "Tournament {} reveal closed, {} active participants, {} matches",
        tournament.id,
        active,
        tournament.matches_total
    );

    Ok(())
}

/// Forfeit an unrevealed entry after reveal deadline
/// Called by operator for each unrevealed entry. Stake stays in pool.
#[derive(Accounts)]
pub struct ForfeitUnrevealed<'info> {
    #[account(
        seeds = [b"config"],
        bump = config.bump,
        has_one = operator @ ArenaError::Unauthorized
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        has_one = tournament,
        close = operator,  // rent → operator
    )]
    pub entry: Account<'info, Entry>,

    #[account(mut)]
    pub tournament: Account<'info, Tournament>,

    #[account(mut)]
    pub operator: Signer<'info>,
}

pub fn forfeit_unrevealed(ctx: Context<ForfeitUnrevealed>) -> Result<()> {
    let tournament = &mut ctx.accounts.tournament;
    let entry = &ctx.accounts.entry;
    let clock = Clock::get()?;

    require!(
        tournament.state == TournamentState::Reveal,
        ArenaError::InvalidState
    );

    require!(
        clock.unix_timestamp > tournament.reveal_ends,
        ArenaError::RevealPeriodNotEnded
    );

    require!(!entry.revealed, ArenaError::AlreadyRevealed);

    // Mark player slot as forfeited
    let idx = entry.index as usize;
    tournament.strategies[idx] = u8::MAX;  // sentinel stays
    tournament.players[idx] = Pubkey::default();

    // Track forfeiture (stake stays in pool — benefits remaining players)
    tournament.forfeits += 1;
    tournament.entries_remaining -= 1;

    msg!(
        "Forfeited unrevealed entry {} (player {}) in tournament {}",
        idx,
        entry.player,
        tournament.id,
    );

    Ok(())
}

/// Run a batch of matches (up to 5 per transaction)
#[derive(Accounts)]
pub struct RunMatches<'info> {
    #[account(
        seeds = [b"config"],
        bump = config.bump,
        has_one = operator @ ArenaError::Unauthorized
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [b"tournament", tournament.id.to_le_bytes().as_ref()],
        bump = tournament.bump
    )]
    pub tournament: Account<'info, Tournament>,

    pub operator: Signer<'info>,
    // Entry accounts passed via remaining_accounts
}

pub fn run_matches<'info>(
    ctx: Context<'_, '_, '_, 'info, RunMatches<'info>>,
) -> Result<()> {
    let tournament = &mut ctx.accounts.tournament;

    require!(
        tournament.state == TournamentState::Running,
        ArenaError::InvalidState
    );

    // Safety check: verify all active strategies are revealed (belt-and-suspenders)
    for i in 0..tournament.players.len() {
        if tournament.players[i] != Pubkey::default() {
            require!(
                tournament.strategies[i] != u8::MAX,
                ArenaError::UnrevealedStrategy
            );
        }
    }

    // Process up to MATCHES_PER_TX matches
    let matches_to_run = MATCHES_PER_TX.min(tournament.matches_total - tournament.matches_completed);
    
    if matches_to_run == 0 {
        msg!("No matches remaining");
        return Ok(());
    }

    // Snapshot starting index before the loop mutates matches_completed
    let start_index = tournament.matches_completed;

    // Build a map of player index -> remaining_account index for entry accounts
    let remaining_accounts = &ctx.remaining_accounts;
    
    for batch_idx in 0..matches_to_run {
        let match_index = start_index + batch_idx;
        
        // Get pairing for this match (use active count = participant_count - forfeits)
        let active_count = tournament.participant_count - tournament.forfeits;
        let pairing = match_logic::get_pairing_for_match(
            active_count,
            tournament.matches_per_player,
            &tournament.randomness_seed,
            match_index,
        ).ok_or(ArenaError::InvalidMatch)?;

        let (idx_a, idx_b) = pairing;

        // Skip if either player is refunded/forfeited (default pubkey)
        let player_a = tournament.players.get(idx_a as usize)
            .ok_or(ArenaError::InvalidMatch)?;
        let player_b = tournament.players.get(idx_b as usize)
            .ok_or(ArenaError::InvalidMatch)?;

        if *player_a == Pubkey::default() || *player_b == Pubkey::default() {
            // Skip this match (player refunded/forfeited)
            tournament.matches_completed += 1;
            continue;
        }

        // Find entry accounts in remaining_accounts
        let entry_a = find_entry_account(remaining_accounts, &tournament.key(), player_a)?;
        let entry_b = find_entry_account(remaining_accounts, &tournament.key(), player_b)?;

        // Deserialize entries
        let entry_a_data = entry_a.try_borrow_data()?;
        let entry_b_data = entry_b.try_borrow_data()?;
        
        let mut entry_a_account = deserialize_entry(&entry_a_data)?;
        let mut entry_b_account = deserialize_entry(&entry_b_data)?;
        drop(entry_a_data);
        drop(entry_b_data);

        // Verify indices match
        require!(entry_a_account.index == idx_a, ArenaError::InvalidEntryAccount);
        require!(entry_b_account.index == idx_b, ArenaError::InvalidEntryAccount);

        // Run the match using match-logic crate
        let strategy_a = crate::state::to_match_strategy(entry_a_account.strategy, &entry_a_account.strategy_params);
        let strategy_b = crate::state::to_match_strategy(entry_b_account.strategy, &entry_b_account.strategy_params);

        let result = match_logic::run_match(
            &strategy_a,
            &strategy_b,
            &tournament.randomness_seed,
            match_index,
            tournament.participant_count,
        );

        // Update scores (both entry and tournament)
        entry_a_account.score += result.total_score_a;
        entry_b_account.score += result.total_score_b;
        entry_a_account.matches_played += 1;
        entry_b_account.matches_played += 1;

        // Write back
        let mut entry_a_data = entry_a.try_borrow_mut_data()?;
        let mut entry_b_data = entry_b.try_borrow_mut_data()?;
        serialize_entry(&entry_a_account, &mut entry_a_data)?;
        serialize_entry(&entry_b_account, &mut entry_b_data)?;
        drop(entry_a_data);
        drop(entry_b_data);

        tournament.scores[idx_a as usize] += result.total_score_a;
        tournament.scores[idx_b as usize] += result.total_score_b;

        tournament.matches_completed += 1;

        msg!(
            "Match {}: {} vs {} -> {} : {}",
            match_index,
            idx_a,
            idx_b,
            result.total_score_a,
            result.total_score_b
        );
    }

    msg!(
        "Tournament {}: {}/{} matches completed",
        tournament.id,
        tournament.matches_completed,
        tournament.matches_total
    );

    Ok(())
}

/// Helper to find an entry account in remaining_accounts
fn find_entry_account<'info>(
    remaining_accounts: &[AccountInfo<'info>],
    tournament_key: &Pubkey,
    player_key: &Pubkey,
) -> Result<AccountInfo<'info>> {
    // Derive expected PDA
    let (expected_pda, _bump) = Pubkey::find_program_address(
        &[b"entry", tournament_key.as_ref(), player_key.as_ref()],
        &crate::ID,
    );

    for account in remaining_accounts {
        if account.key() == expected_pda {
            return Ok(account.clone());
        }
    }

    Err(ArenaError::InvalidEntryAccount.into())
}

/// Workaround for deserializing Entry from account data
fn deserialize_entry(data: &[u8]) -> Result<Entry> {
    if data.len() < 8 {
        return Err(ArenaError::InvalidEntryAccount.into());
    }
    Entry::try_deserialize(&mut &data[..])
        .map_err(|_| ArenaError::InvalidEntryAccount.into())
}

fn serialize_entry(entry: &Entry, data: &mut [u8]) -> Result<()> {
    let mut writer = &mut data[..];
    entry.try_serialize(&mut writer)
        .map_err(|_| ArenaError::InvalidEntryAccount.into())
}

/// Finalize tournament and determine winners, create next tournament
#[derive(Accounts)]
pub struct FinalizeTournament<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump = config.bump,
        has_one = operator @ ArenaError::Unauthorized
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [b"tournament", tournament.id.to_le_bytes().as_ref()],
        bump = tournament.bump
    )]
    pub tournament: Account<'info, Tournament>,

    /// Next tournament to create (starts with base size, grows via realloc as players join)
    #[account(
        init,
        payer = operator,
        space = Tournament::BASE_SPACE,
        seeds = [b"tournament", (config.current_tournament_id + 1).to_le_bytes().as_ref()],
        bump
    )]
    pub next_tournament: Account<'info, Tournament>,

    #[account(mut)]
    pub operator: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn finalize_tournament(ctx: Context<FinalizeTournament>) -> Result<()> {
    let config = &mut ctx.accounts.config;
    let tournament = &mut ctx.accounts.tournament;
    let next_tournament = &mut ctx.accounts.next_tournament;
    let clock = Clock::get()?;

    require!(
        tournament.state == TournamentState::Running,
        ArenaError::InvalidState
    );

    require!(
        tournament.matches_completed >= tournament.matches_total,
        ArenaError::MatchesIncomplete
    );

    // Active participants = participant_count - forfeits
    let active = tournament.participant_count - tournament.forfeits;

    if active == 0 {
        // No active players — all forfeited. Sweep remaining pool to fees.
        tournament.winner_count = 0;
        tournament.winner_pool = 0;
        tournament.min_winning_score = 0;

        let rent = Rent::get()?;
        let tournament_info = tournament.to_account_info();
        let min_balance = rent.minimum_balance(tournament_info.data_len());
        let transferable = tournament_info.lamports().saturating_sub(min_balance);

        if transferable > 0 {
            config.accumulated_fees += transferable;
            **tournament.to_account_info().try_borrow_mut_lamports()? -= transferable;
            **config.to_account_info().try_borrow_mut_lamports()? += transferable;
        }

        tournament.payout_started_at = clock.unix_timestamp;
        tournament.state = TournamentState::Payout;
    } else {
        // Sort scores descending to find threshold
        let mut sorted_scores: Vec<u32> = tournament.scores.iter()
            .enumerate()
            .filter(|(i, _)| tournament.players[*i] != Pubkey::default())
            .map(|(_, &s)| s)
            .collect();
        sorted_scores.sort_by(|a, b| b.cmp(a));

        // Calculate winner count (top 25%, minimum 1)
        let target_winners = std::cmp::max(1, (active + 3) / 4); // ceil(n/4)

        // Set min_winning_score (threshold to be a winner)
        tournament.min_winning_score = sorted_scores
            .get((target_winners - 1) as usize)
            .copied()
            .unwrap_or(0);

        // Count actual winners (all players at or above threshold)
        tournament.winner_count = sorted_scores
            .iter()
            .filter(|&&s| s >= tournament.min_winning_score)
            .count() as u32;

        // Calculate house fee
        let house_fee = tournament.pool
            .checked_mul(tournament.house_fee_bps as u64)
            .ok_or(ArenaError::Overflow)?
            .checked_div(10000)
            .ok_or(ArenaError::Overflow)?;

        // Determine max distributable lamports (total - rent-exempt minimum)
        let rent = Rent::get()?;
        let tournament_info = tournament.to_account_info();
        let min_balance = rent.minimum_balance(tournament_info.data_len());
        let max_distributable = tournament_info.lamports()
            .saturating_sub(min_balance);

        // Winner pool is the lesser of (pool - fees) and max distributable
        let winner_pool_raw = (tournament.pool - house_fee).min(max_distributable);
        let per_winner = winner_pool_raw / tournament.winner_count as u64;
        let dust = winner_pool_raw - (per_winner * tournament.winner_count as u64);

        let fee_total = house_fee + dust;
        config.accumulated_fees += fee_total;

        // Transfer fee lamports from tournament account to config account
        **tournament.to_account_info().try_borrow_mut_lamports()? -= fee_total;
        **config.to_account_info().try_borrow_mut_lamports()? += fee_total;

        tournament.winner_pool = per_winner * tournament.winner_count as u64;
        tournament.payout_started_at = clock.unix_timestamp;
        tournament.state = TournamentState::Payout;
    }

    // Create next tournament with snapshotted config values
    config.current_tournament_id += 1;
    
    next_tournament.id = config.current_tournament_id;
    next_tournament.state = TournamentState::Registration;
    next_tournament.stake = config.stake;
    next_tournament.house_fee_bps = config.house_fee_bps;
    next_tournament.matches_per_player = config.matches_per_player;
    next_tournament.registration_duration = config.registration_duration;
    next_tournament.reveal_duration = config.reveal_duration;  // NEW v1.7
    next_tournament.pool = 0;
    next_tournament.participant_count = 0;
    next_tournament.registration_ends = clock.unix_timestamp + config.registration_duration;
    next_tournament.matches_completed = 0;
    next_tournament.matches_total = 0;
    next_tournament.randomness_seed = [0u8; 32];
    next_tournament.min_winning_score = 0;
    next_tournament.winner_count = 0;
    next_tournament.winner_pool = 0;
    next_tournament.claims_processed = 0;
    next_tournament.payout_started_at = 0;
    next_tournament.entries_remaining = 0;
    next_tournament.round_tier = 0;
    next_tournament.reveal_ends = 0;          // NEW v1.7
    next_tournament.reveals_completed = 0;    // NEW v1.7
    next_tournament.forfeits = 0;             // NEW v1.7
    next_tournament.players = Vec::new();
    next_tournament.scores = Vec::new();
    next_tournament.strategies = Vec::new();
    next_tournament.strategy_params = Vec::new();
    next_tournament.bump = ctx.bumps.next_tournament;

    msg!(
        "Tournament {} finalized. {} winners (min score: {}) will split {} lamports",
        tournament.id,
        tournament.winner_count,
        tournament.min_winning_score,
        tournament.winner_pool
    );
    msg!(
        "Tournament {} created, registration ends at {}",
        next_tournament.id,
        next_tournament.registration_ends
    );

    Ok(())
}

/// Close expired entry and recover unclaimed funds
#[derive(Accounts)]
pub struct CloseExpiredEntry<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump = config.bump,
        has_one = operator @ ArenaError::Unauthorized
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [b"tournament", tournament.id.to_le_bytes().as_ref()],
        bump = tournament.bump
    )]
    pub tournament: Account<'info, Tournament>,

    #[account(
        mut,
        seeds = [b"entry", tournament.key().as_ref(), entry.player.as_ref()],
        bump = entry.bump,
        has_one = tournament,
        close = operator
    )]
    pub entry: Account<'info, Entry>,

    #[account(mut)]
    pub operator: Signer<'info>,
}

pub fn close_expired_entry(ctx: Context<CloseExpiredEntry>) -> Result<()> {
    let config = &mut ctx.accounts.config;
    let tournament = &mut ctx.accounts.tournament;
    let entry = &ctx.accounts.entry;
    let clock = Clock::get()?;

    // Must be in Payout state
    require!(
        tournament.state == TournamentState::Payout,
        ArenaError::InvalidState
    );

    // Allow early closure when all winners have claimed, or after 30-day expiry
    let time_expired = clock.unix_timestamp >= tournament.payout_started_at + CLAIM_EXPIRY_SECONDS;
    let all_winners_claimed = tournament.claims_processed >= tournament.winner_count;
    require!(
        time_expired || all_winners_claimed,
        ArenaError::NotExpired
    );

    // If this was an unclaimed winner, add their share to accumulated fees
    if !entry.paid_out && tournament.winner_count > 0 && entry.score >= tournament.min_winning_score {
        let unclaimed_share = tournament.winner_pool / tournament.winner_count as u64;

        let rent = Rent::get()?;
        let min_balance = rent.minimum_balance(tournament.to_account_info().data_len());
        let max_transfer = tournament.to_account_info().lamports()
            .saturating_sub(min_balance);
        let transfer_amount = unclaimed_share.min(max_transfer);

        if transfer_amount > 0 {
            config.accumulated_fees += transfer_amount;
            **tournament.to_account_info().try_borrow_mut_lamports()? -= transfer_amount;
            **config.to_account_info().try_borrow_mut_lamports()? += transfer_amount;
        }

        msg!(
            "Added unclaimed prize {} lamports to accumulated fees (of {} owed)",
            transfer_amount,
            unclaimed_share
        );
    }

    // Decrement entries_remaining counter
    tournament.entries_remaining -= 1;

    msg!("Closed expired entry for player {}", entry.player);

    Ok(())
}

/// Close a tournament account and recover all lamports to accumulated_fees
#[derive(Accounts)]
pub struct CloseTournament<'info> {
    #[account(
        mut,
        seeds = [b"config"],
        bump = config.bump,
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [b"tournament", tournament.id.to_le_bytes().as_ref()],
        bump = tournament.bump,
    )]
    pub tournament: Account<'info, Tournament>,

    #[account(
        constraint = operator.key() == config.operator || operator.key() == config.admin @ ArenaError::Unauthorized
    )]
    pub operator: Signer<'info>,
}

pub fn close_tournament(ctx: Context<CloseTournament>) -> Result<()> {
    let config = &mut ctx.accounts.config;
    let tournament = &ctx.accounts.tournament;
    let clock = Clock::get()?;

    // Must be in Payout state
    require!(
        tournament.state == TournamentState::Payout,
        ArenaError::InvalidState
    );

    // Allow early closure when all winners have claimed, or after 30 days
    let time_expired = clock.unix_timestamp >= tournament.payout_started_at + TOURNAMENT_CLOSURE_SECONDS;
    let all_winners_claimed = tournament.claims_processed >= tournament.winner_count;
    require!(
        time_expired || all_winners_claimed,
        ArenaError::TournamentNotCloseable
    );

    // All entries must be closed (claimed, refunded, or expired)
    require!(
        tournament.entries_remaining == 0,
        ArenaError::EntriesRemaining
    );

    // Transfer ALL lamports (rent + any surplus) to config PDA → accumulated_fees
    let tournament_info = tournament.to_account_info();
    let total_lamports = tournament_info.lamports();
    if total_lamports > 0 {
        **tournament_info.try_borrow_mut_lamports()? = 0;
        **config.to_account_info().try_borrow_mut_lamports()? += total_lamports;
        config.accumulated_fees = config.accumulated_fees
            .checked_add(total_lamports)
            .ok_or(ArenaError::Overflow)?;
    }

    // Zero out account data to mark as closed (Solana GCs 0-lamport accounts)
    let mut data = tournament_info.try_borrow_mut_data()?;
    for byte in data.iter_mut() {
        *byte = 0;
    }

    msg!(
        "Closed tournament {} — {} lamports transferred to accumulated fees",
        tournament.id,
        total_lamports
    );

    Ok(())
}
