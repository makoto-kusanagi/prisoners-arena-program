//! Match Logic for Prisoner's Arena
//!
//! Core game logic for the Iterated Prisoner's Dilemma tournament.
//! This crate is compiled to:
//! - Native (for contract and operator)
//! - WASM (for frontend match replay)

mod random;
mod strategy;
mod game;
mod pairing;
pub mod vm;

#[cfg(feature = "wasm")]
mod wasm;

pub use random::SeededRng;
pub use strategy::{Move, Strategy, StrategyBase, PlayerStrategy, execute_player_strategy};
pub use game::{run_match, MatchResult, RoundResult, RoundConfig, expected_rounds};
pub use pairing::{generate_all_pairings, get_pairing_for_match, calculate_match_count, effective_k};
pub use vm::{validate_bytecode, execute_bytecode, BytecodeError, MAX_BYTECODE_LEN};

/// Payoff matrix for the Prisoner's Dilemma
/// Returns (score_a, score_b)
pub fn payoff(a: Move, b: Move) -> (u8, u8) {
    match (a, b) {
        (Move::Cooperate, Move::Cooperate) => (3, 3),
        (Move::Cooperate, Move::Defect) => (0, 5),
        (Move::Defect, Move::Cooperate) => (5, 0),
        (Move::Defect, Move::Defect) => (1, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payoff_matrix() {
        assert_eq!(payoff(Move::Cooperate, Move::Cooperate), (3, 3));
        assert_eq!(payoff(Move::Cooperate, Move::Defect), (0, 5));
        assert_eq!(payoff(Move::Defect, Move::Cooperate), (5, 0));
        assert_eq!(payoff(Move::Defect, Move::Defect), (1, 1));
    }
}
