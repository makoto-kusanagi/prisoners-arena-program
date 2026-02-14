//! Strategy definitions and execution

use serde::{Deserialize, Serialize};
use crate::random::SeededRng;

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

/// Strategy parameters for fine-tuning behavior
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategyParams {
    /// Percentage chance to cooperate after opponent defects (0-100)
    pub forgiveness: u8,
    /// Rounds to wait before retaliating (0-10)
    pub retaliation_delay: u8,
    /// Number of defections to ignore before retaliating (0-5)
    pub noise_tolerance: u8,
    /// Bitmask of first 8 moves (1 = defect, 0 = use strategy)
    pub initial_moves: u8,
    /// Bias toward cooperation for Random strategy (0-100)
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

/// Complete strategy with base type and parameters
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Strategy {
    pub base: StrategyBase,
    pub params: StrategyParams,
}

impl Strategy {
    /// Create a new strategy with default parameters
    pub fn new(base: StrategyBase) -> Self {
        Self {
            base,
            params: StrategyParams::default(),
        }
    }
    
    /// Create with custom parameters
    pub fn with_params(base: StrategyBase, params: StrategyParams) -> Self {
        Self { base, params }
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
    round: u8,
    rng: &mut SeededRng,
) -> Move {
    // Check initial_moves override (first 8 rounds)
    if round < 8 {
        let bit = (strategy.params.initial_moves >> round) & 1;
        if bit == 1 {
            return Move::Defect;
        }
    }
    
    match strategy.base {
        StrategyBase::TitForTat => {
            execute_tit_for_tat(opponent_history, &strategy.params, rng)
        }
        StrategyBase::AlwaysDefect => Move::Defect,
        StrategyBase::AlwaysCooperate => Move::Cooperate,
        StrategyBase::GrimTrigger => {
            execute_grim_trigger(opponent_history, &strategy.params)
        }
        StrategyBase::Pavlov => {
            execute_pavlov(opponent_history, my_history)
        }
        StrategyBase::SuspiciousTitForTat => {
            execute_suspicious_tit_for_tat(opponent_history, &strategy.params, rng)
        }
        StrategyBase::Random => {
            execute_random(&strategy.params, rng)
        }
        StrategyBase::TitForTwoTats => {
            execute_tit_for_two_tats(opponent_history)
        }
        StrategyBase::Gradual => {
            execute_gradual(opponent_history, my_history, &strategy.params)
        }
    }
}

/// Tit-for-Tat: Copy opponent's last move, start with cooperate
fn execute_tit_for_tat(
    opponent_history: &[Move],
    params: &StrategyParams,
    rng: &mut SeededRng,
) -> Move {
    match opponent_history.last() {
        None => Move::Cooperate,
        Some(Move::Cooperate) => Move::Cooperate,
        Some(Move::Defect) => {
            // Retaliation delay: wait N rounds after seeing defection
            if params.retaliation_delay > 0 {
                let last_defect_pos = opponent_history.iter().rposition(|m| *m == Move::Defect);
                if let Some(pos) = last_defect_pos {
                    let rounds_since = opponent_history.len() - 1 - pos;
                    if rounds_since < params.retaliation_delay as usize {
                        return Move::Cooperate;
                    }
                }
            }
            // Forgiveness: chance to cooperate anyway
            if params.forgiveness > 0 && rng.next_percent() < params.forgiveness {
                Move::Cooperate
            } else {
                Move::Defect
            }
        }
    }
}

/// Grim Trigger: Cooperate until opponent defects, then always defect
fn execute_grim_trigger(
    opponent_history: &[Move],
    params: &StrategyParams,
) -> Move {
    // Count opponent defections
    let defection_count = opponent_history
        .iter()
        .filter(|m| **m == Move::Defect)
        .count();
    
    // Trigger if defections exceed noise tolerance
    if defection_count > params.noise_tolerance as usize {
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
    params: &StrategyParams,
    rng: &mut SeededRng,
) -> Move {
    if opponent_history.is_empty() {
        return Move::Defect; // Start suspicious
    }
    
    match opponent_history.last() {
        Some(Move::Cooperate) => Move::Cooperate,
        Some(Move::Defect) => {
            // Retaliation delay: wait N rounds after seeing defection
            if params.retaliation_delay > 0 {
                let last_defect_pos = opponent_history.iter().rposition(|m| *m == Move::Defect);
                if let Some(pos) = last_defect_pos {
                    let rounds_since = opponent_history.len() - 1 - pos;
                    if rounds_since < params.retaliation_delay as usize {
                        return Move::Cooperate;
                    }
                }
            }
            if params.forgiveness > 0 && rng.next_percent() < params.forgiveness {
                Move::Cooperate
            } else {
                Move::Defect
            }
        }
        None => Move::Defect,
    }
}

/// Random: Random choice with configurable bias
fn execute_random(
    params: &StrategyParams,
    rng: &mut SeededRng,
) -> Move {
    let bias = params.cooperate_bias;
    
    if rng.next_percent() < bias {
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
    _params: &StrategyParams,
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

/// Get a human-readable description of a strategy (used by WASM module)
#[allow(dead_code)]
pub fn describe_strategy(strategy: &Strategy) -> String {
    let base_desc = match strategy.base {
        StrategyBase::TitForTat => "Copies opponent's last move. Starts by cooperating.",
        StrategyBase::AlwaysDefect => "Never cooperates. Always defects.",
        StrategyBase::AlwaysCooperate => "Never defects. Always cooperates.",
        StrategyBase::GrimTrigger => "Cooperates until betrayed, then always defects.",
        StrategyBase::Pavlov => "Repeats move if outcome was good, switches if bad.",
        StrategyBase::SuspiciousTitForTat => "Like Tit-for-Tat, but starts with defect.",
        StrategyBase::Random => "Randomly cooperates or defects each round.",
        StrategyBase::TitForTwoTats => "Only retaliates after two consecutive defections.",
        StrategyBase::Gradual => "Retaliates with increasing severity, then forgives.",
    };
    
    let mut desc = base_desc.to_string();
    
    if strategy.params.forgiveness > 0 {
        desc.push_str(&format!(" {}% chance to forgive.", strategy.params.forgiveness));
    }
    
    if strategy.params.noise_tolerance > 0 {
        desc.push_str(&format!(" Tolerates {} accidental defections.", strategy.params.noise_tolerance));
    }
    
    desc
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
    fn test_grim_trigger_noise_tolerance() {
        let strategy = Strategy::with_params(
            StrategyBase::GrimTrigger,
            StrategyParams { noise_tolerance: 1, ..Default::default() }
        );
        let mut rng = make_rng();
        
        // Tolerate one defection
        let m = execute_strategy(&strategy, &[Move::Defect], &[], 1, &mut rng);
        assert_eq!(m, Move::Cooperate);
        
        // But not two
        let m = execute_strategy(&strategy, &[Move::Defect, Move::Defect], &[], 2, &mut rng);
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
    
    #[test]
    fn test_initial_moves_override() {
        let strategy = Strategy::with_params(
            StrategyBase::AlwaysCooperate,
            StrategyParams { initial_moves: 0b00000101, ..Default::default() }
        );
        let mut rng = make_rng();
        
        // Round 0: bit 0 is 1, should defect
        assert_eq!(execute_strategy(&strategy, &[], &[], 0, &mut rng), Move::Defect);
        
        // Round 1: bit 1 is 0, should cooperate (strategy default)
        assert_eq!(execute_strategy(&strategy, &[], &[], 1, &mut rng), Move::Cooperate);
        
        // Round 2: bit 2 is 1, should defect
        assert_eq!(execute_strategy(&strategy, &[], &[], 2, &mut rng), Move::Defect);
    }

    #[test]
    fn test_cooperate_bias_zero_means_always_defect() {
        let strategy = Strategy::with_params(
            StrategyBase::Random,
            StrategyParams { cooperate_bias: 0, ..Default::default() }
        );
        let mut rng = make_rng();
        // With bias=0, should always defect
        for round in 0..20 {
            assert_eq!(execute_strategy(&strategy, &[], &[], round, &mut rng), Move::Defect);
        }
    }

    #[test]
    fn test_cooperate_bias_100_means_always_cooperate() {
        let strategy = Strategy::with_params(
            StrategyBase::Random,
            StrategyParams { cooperate_bias: 100, ..Default::default() }
        );
        let mut rng = make_rng();
        for round in 0..20 {
            assert_eq!(execute_strategy(&strategy, &[], &[], round, &mut rng), Move::Cooperate);
        }
    }

    #[test]
    fn test_default_cooperate_bias_is_50() {
        let params = StrategyParams::default();
        assert_eq!(params.cooperate_bias, 50);
    }

    #[test]
    fn test_retaliation_delay_tft() {
        // With delay=2, TFT should wait 2 rounds after seeing defection
        let strategy = Strategy::with_params(
            StrategyBase::TitForTat,
            StrategyParams { retaliation_delay: 2, ..Default::default() }
        );
        let mut rng = make_rng();
        
        // Opponent defected on last move — round_since=0, delay=2, should cooperate
        let m = execute_strategy(
            &strategy,
            &[Move::Cooperate, Move::Defect],
            &[Move::Cooperate, Move::Cooperate],
            2, &mut rng
        );
        assert_eq!(m, Move::Cooperate);
        
        // Opponent defected 3 rounds ago — rounds_since=2, delay=2, should defect
        let m = execute_strategy(
            &strategy,
            &[Move::Defect, Move::Cooperate, Move::Cooperate],
            &[Move::Cooperate, Move::Cooperate, Move::Cooperate],
            3, &mut rng
        );
        // Last move is Cooperate, so TFT would cooperate anyway
        assert_eq!(m, Move::Cooperate);
    }

    #[test]
    fn test_forgiveness_statistical() {
        // With 100% forgiveness, TFT should always cooperate even after defection
        let strategy = Strategy::with_params(
            StrategyBase::TitForTat,
            StrategyParams { forgiveness: 100, ..Default::default() }
        );
        let mut rng = make_rng();
        
        for _ in 0..20 {
            let m = execute_strategy(
                &strategy,
                &[Move::Defect],
                &[Move::Cooperate],
                1, &mut rng
            );
            assert_eq!(m, Move::Cooperate);
        }
    }
}
