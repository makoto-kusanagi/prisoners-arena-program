//! Account state definitions

use anchor_lang::prelude::*;

/// Claim expiry in seconds (30 days in production, 2s in testing)
#[cfg(not(feature = "testing"))]
pub const CLAIM_EXPIRY_SECONDS: i64 = 2_592_000;
#[cfg(feature = "testing")]
pub const CLAIM_EXPIRY_SECONDS: i64 = 2;

/// Tournament closure delay in seconds (30 days after payout start, 2s in testing)
#[cfg(not(feature = "testing"))]
pub const TOURNAMENT_CLOSURE_SECONDS: i64 = 2_592_000;
#[cfg(feature = "testing")]
pub const TOURNAMENT_CLOSURE_SECONDS: i64 = 2;

/// Winner percentage (top 25%) — used in finalize_tournament logic
#[allow(dead_code)]
pub const WINNER_PERCENTAGE: u32 = 25;

/// Matches per transaction batch
pub const MATCHES_PER_TX: u32 = 5;

/// Maximum participants per tournament — enforced via config.max_participants
#[allow(dead_code)]
pub const MAX_PARTICIPANTS: usize = 5000;

/// Global configuration account
#[account]
#[derive(Default)]
pub struct Config {
    /// Admin who can update config and withdraw fees
    pub admin: Pubkey,
    /// Operator who can run tournament lifecycle (separate from admin)
    pub operator: Pubkey,
    /// House fee in basis points (0-10000, where 100 = 1%)
    pub house_fee_bps: u16,
    /// Fixed stake for all players (lamports)
    pub stake: u64,
    /// Minimum participants to start tournament (must be even, >= 2)
    pub min_participants: u16,
    /// Maximum participants per tournament
    pub max_participants: u16,
    /// Registration duration in seconds
    pub registration_duration: i64,
    /// Number of matches each player plays (K)
    pub matches_per_player: u16,
    /// Accumulated fees pending withdrawal
    pub accumulated_fees: u64,
    /// Current tournament ID (increments each tournament)
    pub current_tournament_id: u32,
    /// Reveal duration in seconds (e.g., 172800 = 48h)
    pub reveal_duration: i64,
    /// PDA bump seed
    pub bump: u8,
    /// Per-tx reimbursement amount for operator (lamports, 0 = off)
    pub operator_tx_fee: u64,
}

impl Config {
    pub const LEN: usize = 8 + // discriminator
        32 +  // admin
        32 +  // operator
        2 +   // house_fee_bps
        8 +   // stake
        2 +   // min_participants
        2 +   // max_participants
        8 +   // registration_duration
        2 +   // matches_per_player
        8 +   // accumulated_fees
        4 +   // current_tournament_id
        8 +   // reveal_duration (NEW v1.7)
        1 +   // bump
        8 +   // operator_tx_fee (NEW v1.8)
        16;   // padding for future fields (was 24, used 8 for operator_tx_fee)
}

/// Tournament state machine
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Default)]
pub enum TournamentState {
    #[default]
    Registration,
    Reveal,    // NEW v1.7
    Running,
    Payout,
}

/// Tournament account
/// 
/// Sized dynamically based on max_participants at creation.
#[account]
#[derive(Default)]
pub struct Tournament {
    /// Tournament ID
    pub id: u32,
    /// Current state
    pub state: TournamentState,
    
    // Snapshotted from Config at creation (immutable after)
    /// Fixed stake (snapshotted from config)
    pub stake: u64,
    /// House fee in basis points (snapshotted from config)
    pub house_fee_bps: u16,
    /// Matches per player K (snapshotted from config)
    pub matches_per_player: u16,
    /// Registration duration in seconds (snapshotted from config)
    pub registration_duration: i64,
    
    /// Total prize pool (lamports)
    pub pool: u64,
    /// Number of active participants (excludes refunded)
    pub participant_count: u32,
    /// Registration deadline (Unix timestamp)
    pub registration_ends: i64,
    /// Number of matches completed
    pub matches_completed: u32,
    /// Total matches to run
    pub matches_total: u32,
    /// Randomness seed (set at registration close)
    pub randomness_seed: [u8; 32],
    /// Minimum score to be a winner (set at finalization)
    pub min_winning_score: u32,
    /// Number of winners (top 25%)
    pub winner_count: u32,
    /// Prize pool after house fee (for payout calculation)
    pub winner_pool: u64,
    /// Number of payouts claimed
    pub claims_processed: u32,
    /// Timestamp when payout state started (for claim expiry)
    pub payout_started_at: i64,
    /// Number of open entry accounts (inc on enter, dec on claim/refund/expire)
    pub entries_remaining: u32,
    /// Round tier: 0 = standard (20-50 rounds), 1 = compressed (10-30 rounds)
    pub round_tier: u8,
    /// Reveal phase deadline (Unix timestamp, set when registration closes)
    pub reveal_ends: i64,             // NEW v1.7
    /// Reveal duration in seconds (snapshotted from config)
    pub reveal_duration: i64,         // NEW v1.7
    /// Number of players who have revealed
    pub reveals_completed: u32,       // NEW v1.7
    /// Number of players who forfeited (didn't reveal in time)
    pub forfeits: u32,                // NEW v1.7
    /// Ordered list of player pubkeys (index = entry order, default = refunded)
    pub players: Vec<Pubkey>,
    /// Scores indexed by entry.index (source of truth for finalization)
    pub scores: Vec<u32>,
    /// Strategies indexed by entry.index (persists after entry closure; 255 = refunded/invalid)
    pub strategies: Vec<u8>,
    /// Strategy parameters indexed by entry.index
    pub strategy_params: Vec<StrategyParams>,
    /// PDA bump seed
    pub bump: u8,
    /// Accumulated operator costs for reimbursement at finalization
    pub operator_costs: u64,
}

/// Bytes added per player (32-byte pubkey + 4-byte score + 1-byte strategy + 5-byte params)
pub const BYTES_PER_PLAYER: usize = 42;

impl Tournament {
    /// Base space for tournament with empty vecs (used for initial allocation)
    pub const BASE_SPACE: usize = 8 + // discriminator
        4 +   // id
        1 +   // state
        8 +   // stake (snapshotted)
        2 +   // house_fee_bps (snapshotted)
        2 +   // matches_per_player (snapshotted)
        8 +   // registration_duration (snapshotted)
        8 +   // pool
        4 +   // participant_count
        8 +   // registration_ends
        4 +   // matches_completed
        4 +   // matches_total
        32 +  // randomness_seed
        4 +   // min_winning_score
        4 +   // winner_count
        8 +   // winner_pool
        4 +   // claims_processed
        8 +   // payout_started_at
        4 +   // entries_remaining
        1 +   // round_tier
        8 +   // reveal_ends (NEW v1.7)
        8 +   // reveal_duration (NEW v1.7)
        4 +   // reveals_completed (NEW v1.7)
        4 +   // forfeits (NEW v1.7)
        4 +   // players vec len (empty)
        4 +   // scores vec len (empty)
        4 +   // strategies vec len (empty)
        4 +   // strategy_params vec len (empty)
        1 +   // bump
        8 +   // operator_costs (NEW v1.8)
        32;   // padding (was 8, expanded for future fields)

    /// Calculate space needed for a tournament with given number of participants
    pub fn space(participant_count: u16) -> usize {
        Self::BASE_SPACE + (participant_count as usize * BYTES_PER_PLAYER)
    }
    
    /// Calculate space needed to add one more player
    pub fn space_for_next_player(&self) -> usize {
        Self::BASE_SPACE + ((self.players.len() + 1) * BYTES_PER_PLAYER)
    }
}

/// Strategy types
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Strategy {
    #[default]
    TitForTat,
    AlwaysDefect,
    AlwaysCooperate,
    GrimTrigger,
    Pavlov,
    SuspiciousTitForTat,
    Random,
    TitForTwoTats,
    Gradual,
}

impl Strategy {
    /// Map a u8 index (0–8) to the corresponding Strategy variant.
    pub fn from_index(index: u8) -> Option<Strategy> {
        match index {
            0 => Some(Strategy::TitForTat),
            1 => Some(Strategy::AlwaysDefect),
            2 => Some(Strategy::AlwaysCooperate),
            3 => Some(Strategy::GrimTrigger),
            4 => Some(Strategy::Pavlov),
            5 => Some(Strategy::SuspiciousTitForTat),
            6 => Some(Strategy::Random),
            7 => Some(Strategy::TitForTwoTats),
            8 => Some(Strategy::Gradual),
            _ => None,
        }
    }
}

/// Strategy parameters for fine-tuning behavior
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrategyParams {
    pub forgiveness: u8,
    pub retaliation_delay: u8,
    pub noise_tolerance: u8,
    pub initial_moves: u8,
    pub cooperate_bias: u8,
}

impl Default for StrategyParams {
    fn default() -> Self {
        Self {
            forgiveness: 0,
            retaliation_delay: 0,
            noise_tolerance: 0,
            initial_moves: 0,
            cooperate_bias: 50,
        }
    }
}

/// Player entry in a tournament
#[account]
#[derive(Default)]
pub struct Entry {
    /// Parent tournament
    pub tournament: Pubkey,
    /// Player wallet
    pub player: Pubkey,
    /// Anonymous index for matching (position in tournament.players[])
    pub index: u32,
    /// SHA256 commitment hash (NEW v1.7)
    pub commitment: [u8; 32],
    /// Player's strategy (zeroed until reveal)
    pub strategy: Strategy,
    /// Strategy parameters (zeroed until reveal)
    pub strategy_params: StrategyParams,
    /// Has player revealed? (NEW v1.7)
    pub revealed: bool,
    /// Accumulated score (synced with tournament.scores[index] during run_matches)
    pub score: u32,
    /// Number of matches played
    pub matches_played: u16,
    /// Whether payout has been claimed
    pub paid_out: bool,
    /// Timestamp when entry was created
    pub created_at: i64,
    /// PDA bump seed
    pub bump: u8,
}

impl Entry {
    pub const LEN: usize = 8 + // discriminator
        32 +  // tournament
        32 +  // player
        4 +   // index
        32 +  // commitment (NEW v1.7)
        1 +   // strategy (enum)
        5 +   // strategy_params
        1 +   // revealed (NEW v1.7)
        4 +   // score
        2 +   // matches_played
        1 +   // paid_out
        8 +   // created_at
        1 +   // bump
        16;   // padding
}

/// Convert on-chain strategy + params to match-logic types
pub fn to_match_strategy(strategy: Strategy, params: &StrategyParams) -> match_logic::Strategy {
    let base = match strategy {
        Strategy::TitForTat => match_logic::StrategyBase::TitForTat,
        Strategy::AlwaysDefect => match_logic::StrategyBase::AlwaysDefect,
        Strategy::AlwaysCooperate => match_logic::StrategyBase::AlwaysCooperate,
        Strategy::GrimTrigger => match_logic::StrategyBase::GrimTrigger,
        Strategy::Pavlov => match_logic::StrategyBase::Pavlov,
        Strategy::SuspiciousTitForTat => match_logic::StrategyBase::SuspiciousTitForTat,
        Strategy::Random => match_logic::StrategyBase::Random,
        Strategy::TitForTwoTats => match_logic::StrategyBase::TitForTwoTats,
        Strategy::Gradual => match_logic::StrategyBase::Gradual,
    };
    match_logic::Strategy::with_params(base, match_logic::StrategyParams {
        forgiveness: params.forgiveness,
        retaliation_delay: params.retaliation_delay,
        noise_tolerance: params.noise_tolerance,
        initial_moves: params.initial_moves,
        cooperate_bias: params.cooperate_bias,
    })
}
