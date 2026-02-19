//! Custom error codes per architecture spec

use anchor_lang::prelude::*;

#[error_code]
pub enum ArenaError {
    #[msg("Invalid tournament state for this action")]
    InvalidState = 6000,

    #[msg("Registration has closed")]
    RegistrationClosed = 6001,

    #[msg("Player has already entered this tournament")]
    AlreadyEntered = 6002,

    #[msg("Score below min_winning_score — not a winner")]
    NotWinner = 6003,

    #[msg("Payout already claimed")]
    AlreadyPaid = 6004,

    #[msg("Not authorized to perform this action")]
    Unauthorized = 6005,

    #[msg("Insufficient funds for operation")]
    InsufficientFunds = 6006,

    #[msg("Matches not yet complete")]
    MatchesIncomplete = 6007,

    #[msg("No fees to withdraw")]
    NoFeesToWithdraw = 6008,

    #[msg("Claim period has expired (30 days)")]
    ClaimExpired = 6009,

    #[msg("Entry has not expired yet")]
    NotExpired = 6010,

    #[msg("SlotHashes sysvar unavailable")]
    SlotHashUnavailable = 6011,

    #[msg("Tournament has reached maximum participants")]
    TournamentFull = 6012,

    #[msg("Invalid entry account in remaining_accounts")]
    InvalidEntryAccount = 6013,

    #[msg("min_participants must be even and >= 2")]
    InvalidMinParticipants = 6014,

    #[msg("Arithmetic overflow")]
    Overflow = 6015,

    #[msg("Invalid match index")]
    InvalidMatch = 6016,

    #[msg("Registration is still open")]
    RegistrationOpen = 6017,

    #[msg("Previous tournament still active")]
    TournamentActive = 6018,

    #[msg("Tournament still has open entries")]
    EntriesRemaining = 6019,

    #[msg("Tournament closure period not yet reached (30 days after payout)")]
    TournamentNotCloseable = 6020,

    // v1.7 Commit-Reveal errors
    #[msg("Commitment does not match revealed strategy")]
    CommitmentMismatch = 6022,

    #[msg("Strategy already revealed")]
    AlreadyRevealed = 6023,

    #[msg("Reveal period has ended")]
    RevealPeriodEnded = 6024,

    #[msg("Reveal period has not ended yet")]
    RevealPeriodNotEnded = 6025,

    #[msg("Unrevealed entries must be forfeited before closing reveal")]
    UnprocessedForfeits = 6026,

    #[msg("Unrevealed strategy in active player slot")]
    UnrevealedStrategy = 6027,

    #[msg("Minimum participants not reached")]
    MinParticipantsNotReached = 6028,

    #[msg("Invalid custom strategy bytecode")]
    InvalidBytecode = 6029,
}
