//! WASM bindings for frontend match replay

#![cfg(feature = "wasm")]

use wasm_bindgen::prelude::*;
use crate::{run_match, Strategy, StrategyBase, PlayerStrategy};
use crate::{effective_k, expected_rounds, RoundConfig};
use crate::strategy::describe_strategy;
use crate::pairing::{generate_all_pairings, get_pairing_for_match, calculate_match_count};
use crate::vm::validate_bytecode;

/// Parse a strategy JSON string into a PlayerStrategy.
///
/// Accepts two formats:
/// - Builtin: `{"base": "TitForTat", "params": {...}}` (legacy Strategy JSON)
/// - Custom:  `{"Custom": [2, 24]}` (PlayerStrategy::Custom bytecode array)
/// - Full:    `{"Builtin": {"base": "TitForTat", "params": {...}}}` (PlayerStrategy JSON)
fn parse_player_strategy(json: &str) -> Result<PlayerStrategy, String> {
    // Try PlayerStrategy first (handles both Builtin and Custom variants)
    if let Ok(ps) = serde_json::from_str::<PlayerStrategy>(json) {
        return Ok(ps);
    }
    // Fall back to legacy Strategy format → wrap in Builtin
    let s: Strategy = serde_json::from_str(json)
        .map_err(|e| format!("Invalid strategy: {}", e))?;
    Ok(PlayerStrategy::Builtin(s))
}

/// Replay a match with full round-by-round details
///
/// # Arguments
/// * `strategy_a_json` - JSON serialized PlayerStrategy or Strategy for player A
/// * `strategy_b_json` - JSON serialized PlayerStrategy or Strategy for player B
/// * `seed` - 32-byte tournament randomness seed
/// * `match_index` - Index of this match
/// * `participant_count` - Number of tournament participants (determines round config)
///
/// # Returns
/// JSON serialized MatchResult
#[wasm_bindgen]
pub fn replay_match(
    strategy_a_json: &str,
    strategy_b_json: &str,
    seed: &[u8],
    match_index: u32,
    participant_count: u32,
) -> Result<JsValue, JsError> {
    let strategy_a = parse_player_strategy(strategy_a_json)
        .map_err(|e| JsError::new(&format!("Invalid strategy A: {}", e)))?;
    let strategy_b = parse_player_strategy(strategy_b_json)
        .map_err(|e| JsError::new(&format!("Invalid strategy B: {}", e)))?;

    let seed_arr: [u8; 32] = seed.try_into()
        .map_err(|_| JsError::new("Seed must be exactly 32 bytes"))?;

    let result = run_match(&strategy_a, &strategy_b, &seed_arr, match_index, participant_count);

    serde_wasm_bindgen::to_value(&result)
        .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
}

/// Get human-readable description of a strategy
#[wasm_bindgen]
pub fn get_strategy_description(strategy_json: &str) -> Result<String, JsError> {
    let strategy: Strategy = serde_json::from_str(strategy_json)
        .map_err(|e| JsError::new(&format!("Invalid strategy: {}", e)))?;
    
    Ok(describe_strategy(&strategy))
}

/// Get all available strategy base types
#[wasm_bindgen]
pub fn get_strategy_types() -> Result<JsValue, JsError> {
    let types = vec![
        StrategyInfo { 
            id: "TitForTat".to_string(),
            name: "Tit for Tat".to_string(),
            description: "Copies opponent's last move. Starts by cooperating.".to_string(),
        },
        StrategyInfo {
            id: "AlwaysDefect".to_string(),
            name: "Always Defect".to_string(),
            description: "Never cooperates. Always defects.".to_string(),
        },
        StrategyInfo {
            id: "AlwaysCooperate".to_string(),
            name: "Always Cooperate".to_string(),
            description: "Never defects. Always cooperates.".to_string(),
        },
        StrategyInfo {
            id: "GrimTrigger".to_string(),
            name: "Grim Trigger".to_string(),
            description: "Cooperates until betrayed, then always defects.".to_string(),
        },
        StrategyInfo {
            id: "Pavlov".to_string(),
            name: "Pavlov".to_string(),
            description: "Repeats move if outcome was good, switches if bad.".to_string(),
        },
        StrategyInfo {
            id: "SuspiciousTitForTat".to_string(),
            name: "Suspicious Tit for Tat".to_string(),
            description: "Like Tit for Tat, but starts with defect.".to_string(),
        },
        StrategyInfo {
            id: "Random".to_string(),
            name: "Random".to_string(),
            description: "Randomly cooperates or defects each round.".to_string(),
        },
        StrategyInfo {
            id: "TitForTwoTats".to_string(),
            name: "Tit for Two Tats".to_string(),
            description: "Only retaliates after two consecutive defections.".to_string(),
        },
        StrategyInfo {
            id: "Gradual".to_string(),
            name: "Gradual".to_string(),
            description: "Retaliates with increasing severity, then forgives.".to_string(),
        },
    ];
    
    serde_wasm_bindgen::to_value(&types)
        .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
}

#[derive(serde::Serialize)]
struct StrategyInfo {
    id: String,
    name: String,
    description: String,
}

/// Create a strategy JSON from base type
#[wasm_bindgen]
pub fn create_strategy(
    base: &str,
) -> Result<String, JsError> {
    let base = match base {
        "TitForTat" => StrategyBase::TitForTat,
        "AlwaysDefect" => StrategyBase::AlwaysDefect,
        "AlwaysCooperate" => StrategyBase::AlwaysCooperate,
        "GrimTrigger" => StrategyBase::GrimTrigger,
        "Pavlov" => StrategyBase::Pavlov,
        "SuspiciousTitForTat" => StrategyBase::SuspiciousTitForTat,
        "Random" => StrategyBase::Random,
        "TitForTwoTats" => StrategyBase::TitForTwoTats,
        "Gradual" => StrategyBase::Gradual,
        _ => return Err(JsError::new(&format!("Unknown strategy: {}", base))),
    };

    let strategy = Strategy::new(base);

    serde_json::to_string(&strategy)
        .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
}

/// Get pairings for a tournament
#[wasm_bindgen]
pub fn get_tournament_pairings(
    participant_count: u32,
    opponents_per_agent: u16,
    seed: &[u8],
) -> Result<JsValue, JsError> {
    let seed_arr: [u8; 32] = seed.try_into()
        .map_err(|_| JsError::new("Seed must be exactly 32 bytes"))?;
    
    let pairings = generate_all_pairings(participant_count, opponents_per_agent, &seed_arr);
    
    serde_wasm_bindgen::to_value(&pairings)
        .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
}

/// Get pairing for a specific match
#[wasm_bindgen]
pub fn get_match_pairing(
    participant_count: u32,
    opponents_per_agent: u16,
    seed: &[u8],
    match_index: u32,
) -> Result<JsValue, JsError> {
    let seed_arr: [u8; 32] = seed.try_into()
        .map_err(|_| JsError::new("Seed must be exactly 32 bytes"))?;
    
    let pairing = get_pairing_for_match(participant_count, opponents_per_agent, &seed_arr, match_index);
    
    serde_wasm_bindgen::to_value(&pairing)
        .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
}

#[derive(serde::Serialize)]
struct ValidationResult {
    valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Validate custom bytecode program
///
/// Returns `{valid: true}` or `{valid: false, error: "..."}`.
/// Never throws — validation errors are returned as structured data.
#[wasm_bindgen]
pub fn validate_custom_bytecode(bytecode: &[u8]) -> JsValue {
    let result = match validate_bytecode(bytecode) {
        Ok(()) => ValidationResult { valid: true, error: None },
        Err(e) => ValidationResult { valid: false, error: Some(e.to_string()) },
    };
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

/// Get total match count for a tournament
#[wasm_bindgen]
pub fn get_match_count(
    participant_count: u32,
    opponents_per_agent: u16,
    seed: &[u8],
) -> Result<u32, JsError> {
    let seed_arr: [u8; 32] = seed.try_into()
        .map_err(|_| JsError::new("Seed must be exactly 32 bytes"))?;

    Ok(calculate_match_count(participant_count, opponents_per_agent, &seed_arr))
}

/// Calculate effective K (matches per player) for given tournament parameters
#[wasm_bindgen]
pub fn get_effective_k(participant_count: u32, config_k: u16) -> u16 {
    effective_k(participant_count, config_k)
}

#[derive(serde::Serialize)]
struct MatchmakingStats {
    effective_k: u16,
    tier: String,
    min_rounds: u8,
    max_rounds: u8,
    end_probability: u8,
    avg_rounds: f64,
    total_matches: u32,
}

/// Get matchmaking statistics for a tournament configuration.
///
/// Returns a JSON object with: effective_k, tier, min_rounds, max_rounds,
/// end_probability, avg_rounds, total_matches.
#[wasm_bindgen]
pub fn get_matchmaking_stats(participant_count: u32, config_k: u16) -> Result<JsValue, JsError> {
    let k = effective_k(participant_count, config_k);
    let (tier, config) = if participant_count <= 1000 {
        ("Standard", RoundConfig::standard())
    } else {
        ("Compressed", RoundConfig::compressed())
    };
    let avg_rounds = expected_rounds(&config);
    let total_matches = participant_count * k as u32 / 2;

    let stats = MatchmakingStats {
        effective_k: k,
        tier: tier.to_string(),
        min_rounds: config.min_rounds,
        max_rounds: config.max_rounds,
        end_probability: config.end_probability,
        avg_rounds,
        total_matches,
    };

    serde_wasm_bindgen::to_value(&stats)
        .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
}
