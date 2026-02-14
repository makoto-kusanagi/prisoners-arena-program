//! WASM bindings for frontend match replay

#![cfg(feature = "wasm")]

use wasm_bindgen::prelude::*;
use crate::{run_match, Strategy, MatchResult, StrategyBase, StrategyParams};
use crate::strategy::describe_strategy;
use crate::pairing::{generate_all_pairings, get_pairing_for_match, calculate_match_count};

/// Replay a match with full round-by-round details
/// 
/// # Arguments
/// * `strategy_a_json` - JSON serialized Strategy for player A
/// * `strategy_b_json` - JSON serialized Strategy for player B  
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
    let strategy_a: Strategy = serde_json::from_str(strategy_a_json)
        .map_err(|e| JsError::new(&format!("Invalid strategy A: {}", e)))?;
    let strategy_b: Strategy = serde_json::from_str(strategy_b_json)
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

/// Create a strategy JSON from base type and parameters
#[wasm_bindgen]
pub fn create_strategy(
    base: &str,
    forgiveness: u8,
    retaliation_delay: u8,
    noise_tolerance: u8,
    initial_moves: u8,
    cooperate_bias: u8,
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
    
    let strategy = Strategy {
        base,
        params: StrategyParams {
            forgiveness,
            retaliation_delay,
            noise_tolerance,
            initial_moves,
            cooperate_bias,
        },
    };
    
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
