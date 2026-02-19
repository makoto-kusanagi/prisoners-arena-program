//! Strategy definitions and execution

use serde::{Deserialize, Serialize};
use crate::random::SeededRng;
use crate::vm;

/// A move in the Prisoner's Dilemma
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Move {
    Cooperate,
    Defect,
}

/// Base strategy type
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrategyBase {
    /// Copy opponent's last move. Start with cooperate.
    TitForTat,
    /// Always defect, never cooperate.
    AlwaysDefect,
    /// Always cooperate, never defect.
    AlwaysCooperate,
    /// Cooperate until opponent defects once, then always defect.
    GrimTrigger,
    /// Win-stay, lose-switch. Repeat move if good outcome.
    Pavlov,
    /// Tit-for-Tat but start with defect.
    SuspiciousTitForTat,
    /// Random choice each round.
    Random,
    /// Defect only if opponent defected twice in a row.
    TitForTwoTats,
    /// Retaliate with increasing defection streaks, then forgive.
    Gradual,
}

/// Complete strategy with base type
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Strategy {
    pub base: StrategyBase,
}

impl Strategy {
    /// Create a new strategy
    pub fn new(base: StrategyBase) -> Self {
        Self { base }
    }
}

impl Default for Strategy {
    fn default() -> Self {
        Self::new(StrategyBase::TitForTat)
    }
}

/// Execute a strategy for one round
/// 
/// # Arguments
/// * `strategy` - The strategy to execute
/// * `opponent_history` - Opponent's past moves
/// * `my_history` - Our past moves  
/// * `round` - Current round number (0-indexed)
/// * `rng` - Random number generator for this round
pub fn execute_strategy(
    strategy: &Strategy,
    opponent_history: &[Move],
    my_history: &[Move],
    _round: u8,
    rng: &mut SeededRng,
) -> Move {
    match strategy.base {
        StrategyBase::TitForTat => {
            execute_tit_for_tat(opponent_history)
        }
        StrategyBase::AlwaysDefect => Move::Defect,
        StrategyBase::AlwaysCooperate => Move::Cooperate,
        StrategyBase::GrimTrigger => {
            execute_grim_trigger(opponent_history)
        }
        StrategyBase::Pavlov => {
            execute_pavlov(opponent_history, my_history)
        }
        StrategyBase::SuspiciousTitForTat => {
            execute_suspicious_tit_for_tat(opponent_history)
        }
        StrategyBase::Random => {
            execute_random(rng)
        }
        StrategyBase::TitForTwoTats => {
            execute_tit_for_two_tats(opponent_history)
        }
        StrategyBase::Gradual => {
            execute_gradual(opponent_history, my_history)
        }
    }
}

/// Tit-for-Tat: Copy opponent's last move, start with cooperate
fn execute_tit_for_tat(
    opponent_history: &[Move],
) -> Move {
    match opponent_history.last() {
        None => Move::Cooperate,
        Some(Move::Cooperate) => Move::Cooperate,
        Some(Move::Defect) => Move::Defect,
    }
}

/// Grim Trigger: Cooperate until opponent defects once, then always defect
fn execute_grim_trigger(
    opponent_history: &[Move],
) -> Move {
    let has_defection = opponent_history
        .iter()
        .any(|m| *m == Move::Defect);

    if has_defection {
        Move::Defect
    } else {
        Move::Cooperate
    }
}

/// Pavlov: Win-stay, lose-switch
/// - If last round was good (3+ points), repeat move
/// - If last round was bad (<3 points), switch move
fn execute_pavlov(
    opponent_history: &[Move],
    my_history: &[Move],
) -> Move {
    if my_history.is_empty() {
        return Move::Cooperate; // Start with cooperate
    }
    
    let my_last = my_history.last().unwrap();
    let opp_last = opponent_history.last().unwrap();
    
    // Calculate what we scored last round
    let (my_score, _) = crate::payoff(*my_last, *opp_last);
    
    // Good outcome (3+): stay with same move
    // Bad outcome (<3): switch
    if my_score >= 3 {
        *my_last
    } else {
        match my_last {
            Move::Cooperate => Move::Defect,
            Move::Defect => Move::Cooperate,
        }
    }
}

/// Suspicious Tit-for-Tat: TFT but start with defect
fn execute_suspicious_tit_for_tat(
    opponent_history: &[Move],
) -> Move {
    if opponent_history.is_empty() {
        return Move::Defect; // Start suspicious
    }

    match opponent_history.last() {
        Some(Move::Cooperate) => Move::Cooperate,
        Some(Move::Defect) => Move::Defect,
        None => Move::Defect,
    }
}

/// Random: Random choice with 50% bias
fn execute_random(
    rng: &mut SeededRng,
) -> Move {
    if rng.next_percent() < 50 {
        Move::Cooperate
    } else {
        Move::Defect
    }
}

/// Tit-for-Two-Tats: Only defect if opponent defected twice in a row
fn execute_tit_for_two_tats(
    opponent_history: &[Move],
) -> Move {
    if opponent_history.len() < 2 {
        return Move::Cooperate;
    }
    
    let last_two = &opponent_history[opponent_history.len() - 2..];
    if last_two[0] == Move::Defect && last_two[1] == Move::Defect {
        Move::Defect
    } else {
        Move::Cooperate
    }
}

/// Gradual: Escalating retaliation
/// After N opponent defections, player should have made N(N+1)/2 total defections
fn execute_gradual(
    opponent_history: &[Move],
    my_history: &[Move],
) -> Move {
    // Count opponent defections
    let their_defections = opponent_history
        .iter()
        .filter(|m| **m == Move::Defect)
        .count();
    
    // Count our defections
    let my_defections = my_history
        .iter()
        .filter(|m| **m == Move::Defect)
        .count();
    
    // Expected total defections: 1 + 2 + ... + N = N(N+1)/2
    let expected = their_defections * (their_defections + 1) / 2;
    
    if my_defections < expected {
        Move::Defect
    } else {
        Move::Cooperate
    }
}

/// A strategy that is either a built-in type or custom bytecode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerStrategy {
    /// One of the 9 built-in strategies.
    Builtin(Strategy),
    /// Custom bytecode program (max 64 bytes).
    Custom(Vec<u8>),
}

/// Execute a player strategy for one round, dispatching to native or VM.
pub fn execute_player_strategy(
    strategy: &PlayerStrategy,
    opponent_history: &[Move],
    my_history: &[Move],
    round: u8,
    rng: &mut SeededRng,
) -> Move {
    match strategy {
        PlayerStrategy::Builtin(s) => execute_strategy(s, opponent_history, my_history, round, rng),
        PlayerStrategy::Custom(bytecode) => {
            vm::execute_bytecode(bytecode, opponent_history, my_history, round, rng)
        }
    }
}

/// Get a human-readable description of a strategy (used by WASM module)
#[allow(dead_code)]
pub fn describe_strategy(strategy: &Strategy) -> String {
    match strategy.base {
        StrategyBase::TitForTat => "Copies opponent's last move. Starts by cooperating.",
        StrategyBase::AlwaysDefect => "Never cooperates. Always defects.",
        StrategyBase::AlwaysCooperate => "Never defects. Always cooperates.",
        StrategyBase::GrimTrigger => "Cooperates until betrayed, then always defects.",
        StrategyBase::Pavlov => "Repeats move if outcome was good, switches if bad.",
        StrategyBase::SuspiciousTitForTat => "Like Tit-for-Tat, but starts with defect.",
        StrategyBase::Random => "Randomly cooperates or defects each round (50/50).",
        StrategyBase::TitForTwoTats => "Only retaliates after two consecutive defections.",
        StrategyBase::Gradual => "Retaliates with increasing severity, then forgives.",
    }.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rng() -> SeededRng {
        SeededRng::new(&[42u8; 32], 0)
    }

    #[test]
    fn test_tit_for_tat_first_move() {
        let strategy = Strategy::new(StrategyBase::TitForTat);
        let mut rng = make_rng();
        
        let m = execute_strategy(&strategy, &[], &[], 0, &mut rng);
        assert_eq!(m, Move::Cooperate);
    }
    
    #[test]
    fn test_tit_for_tat_copies() {
        let strategy = Strategy::new(StrategyBase::TitForTat);
        let mut rng = make_rng();
        
        // Opponent cooperated
        let m = execute_strategy(&strategy, &[Move::Cooperate], &[Move::Cooperate], 1, &mut rng);
        assert_eq!(m, Move::Cooperate);
        
        // Opponent defected
        let m = execute_strategy(&strategy, &[Move::Defect], &[Move::Cooperate], 1, &mut rng);
        assert_eq!(m, Move::Defect);
    }
    
    #[test]
    fn test_always_defect() {
        let strategy = Strategy::new(StrategyBase::AlwaysDefect);
        let mut rng = make_rng();
        
        for round in 0..10 {
            let m = execute_strategy(&strategy, &[], &[], round, &mut rng);
            assert_eq!(m, Move::Defect);
        }
    }
    
    #[test]
    fn test_always_cooperate() {
        let strategy = Strategy::new(StrategyBase::AlwaysCooperate);
        let mut rng = make_rng();
        
        for round in 0..10 {
            let m = execute_strategy(&strategy, &[], &[], round, &mut rng);
            assert_eq!(m, Move::Cooperate);
        }
    }
    
    #[test]
    fn test_grim_trigger() {
        let strategy = Strategy::new(StrategyBase::GrimTrigger);
        let mut rng = make_rng();
        
        // Cooperate while opponent cooperates
        let m = execute_strategy(&strategy, &[Move::Cooperate, Move::Cooperate], &[], 2, &mut rng);
        assert_eq!(m, Move::Cooperate);
        
        // Defect forever after opponent defects
        let m = execute_strategy(&strategy, &[Move::Cooperate, Move::Defect], &[], 2, &mut rng);
        assert_eq!(m, Move::Defect);
    }
    
    #[test]
    fn test_pavlov_win_stay() {
        let strategy = Strategy::new(StrategyBase::Pavlov);
        let mut rng = make_rng();
        
        // Both cooperated (3 points) - stay with cooperate
        let m = execute_strategy(
            &strategy, 
            &[Move::Cooperate], 
            &[Move::Cooperate], 
            1, 
            &mut rng
        );
        assert_eq!(m, Move::Cooperate);
        
        // We defected, they cooperated (5 points) - stay with defect
        let m = execute_strategy(
            &strategy,
            &[Move::Cooperate],
            &[Move::Defect],
            1,
            &mut rng
        );
        assert_eq!(m, Move::Defect);
    }
    
    #[test]
    fn test_pavlov_lose_switch() {
        let strategy = Strategy::new(StrategyBase::Pavlov);
        let mut rng = make_rng();
        
        // We cooperated, they defected (0 points) - switch to defect
        let m = execute_strategy(
            &strategy,
            &[Move::Defect],
            &[Move::Cooperate],
            1,
            &mut rng
        );
        assert_eq!(m, Move::Defect);
        
        // Both defected (1 point) - switch to cooperate
        let m = execute_strategy(
            &strategy,
            &[Move::Defect],
            &[Move::Defect],
            1,
            &mut rng
        );
        assert_eq!(m, Move::Cooperate);
    }
    
    #[test]
    fn test_suspicious_tft_starts_defect() {
        let strategy = Strategy::new(StrategyBase::SuspiciousTitForTat);
        let mut rng = make_rng();
        
        let m = execute_strategy(&strategy, &[], &[], 0, &mut rng);
        assert_eq!(m, Move::Defect);
    }
    
    #[test]
    fn test_tit_for_two_tats() {
        let strategy = Strategy::new(StrategyBase::TitForTwoTats);
        let mut rng = make_rng();
        
        // Single defection - forgive
        let m = execute_strategy(&strategy, &[Move::Cooperate, Move::Defect], &[], 2, &mut rng);
        assert_eq!(m, Move::Cooperate);
        
        // Two consecutive defections - retaliate
        let m = execute_strategy(&strategy, &[Move::Defect, Move::Defect], &[], 2, &mut rng);
        assert_eq!(m, Move::Defect);
    }
    
}
