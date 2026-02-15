//! Deterministic pairing generation for tournament matches

use crate::random::SeededRng;

/// Calculate effective K (matches per player) based on participant count
///
/// - n < 2: return 0
/// - n ≤ 200: full round-robin (n-1)
/// - n > 200: clamp config_k to [49, 99]
pub fn effective_k(participant_count: u32, config_k: u16) -> u16 {
    if participant_count < 2 {
        return 0;
    }
    if participant_count <= 200 {
        (participant_count - 1) as u16
    } else {
        config_k.clamp(49, 99)
    }
}

/// Generate all match pairings for a tournament
/// 
/// Each participant plays exactly K matches (deduplicated — each match counted once).
/// Total matches = n×K/2 where n = participant count.
/// 
/// # Arguments
/// * `participant_count` - Total number of participants (should be even for exact K matches)
/// * `matches_per_player` - Number of matches per player (K)
/// * `seed` - Tournament randomness seed
/// 
/// # Returns
/// Vector of (index_a, index_b) pairs, where index_a < index_b
pub fn generate_all_pairings(
    participant_count: u32,
    matches_per_player: u16,
    seed: &[u8; 32],
) -> Vec<(u32, u32)> {
    if participant_count < 2 {
        return Vec::new();
    }
    
    let n = participant_count as usize;
    let k = matches_per_player as usize;
    
    // For small tournaments where n-1 <= k, use repeated round-robins
    if n <= k + 1 {
        return generate_repeated_round_robin(n, k, seed);
    }
    
    // Use circular pairing method:
    // - For offset d, each player i is paired with (i + d) mod n
    // - Each offset produces n unique pairs, giving each player 2 matches
    // - Offsets d and (n-d) produce identical pair sets
    // - So we use offsets from 1 to n/2 (distinct offsets only)
    // - For K matches per player, we need K/2 offsets
    
    let mut rng = SeededRng::new(seed, 0);
    
    // Generate available distinct offsets: 1 to n/2
    let max_offset = n / 2;
    let mut available_offsets: Vec<usize> = (1..=max_offset).collect();
    shuffle_usize(&mut available_offsets, &mut rng);
    
    // We need k/2 offsets for k matches per player
    // Each offset gives 2 matches per player (they appear twice in offset's pairs)
    let offsets_needed = k.div_ceil(2);
    let offsets_to_use = offsets_needed.min(available_offsets.len());
    
    let selected_offsets = &available_offsets[..offsets_to_use];
    
    // Generate matches from selected offsets
    let mut matches: Vec<(u32, u32)> = Vec::with_capacity(n * offsets_to_use);
    
    for &offset in selected_offsets {
        for i in 0..n {
            let j = (i + offset) % n;
            // Always add in canonical order (smaller, larger)
            let pair = if i < j { 
                (i as u32, j as u32) 
            } else { 
                (j as u32, i as u32) 
            };
            matches.push(pair);
        }
    }
    
    // Remove duplicates (shouldn't be any with distinct offsets, but be safe)
    matches.sort();
    matches.dedup();
    
    // Shuffle match order for unpredictable execution
    shuffle_pairs(&mut matches, &mut rng);
    
    matches
}

/// Generate repeated round-robin pairings for small tournaments
/// 
/// When n-1 < k (fewer unique opponents than desired matches), repeat full
/// round-robins until every player has >= k matches. Each repeated pair gets
/// a distinct match_index, producing a unique game seed via SeededRng.
fn generate_repeated_round_robin(n: usize, k: usize, seed: &[u8; 32]) -> Vec<(u32, u32)> {
    // Base round-robin: all unique pairs
    let mut base_pairs = Vec::with_capacity(n * (n - 1) / 2);
    for i in 0..n {
        for j in (i + 1)..n {
            base_pairs.push((i as u32, j as u32));
        }
    }
    
    // Each round-robin gives n-1 matches per player
    // Need ceil(k / (n-1)) cycles (n >= 2 guaranteed by caller)
    let matches_per_cycle = n - 1;
    let cycles = k.div_ceil(matches_per_cycle);
    
    let mut pairings = Vec::with_capacity(base_pairs.len() * cycles);
    for _ in 0..cycles {
        pairings.extend_from_slice(&base_pairs);
    }
    
    let mut rng = SeededRng::new(seed, 0);
    shuffle_pairs(&mut pairings, &mut rng);
    
    pairings
}

/// Get the pairing for a specific match index
pub fn get_pairing_for_match(
    participant_count: u32,
    matches_per_player: u16,
    seed: &[u8; 32],
    match_index: u32,
) -> Option<(u32, u32)> {
    let pairings = generate_all_pairings(participant_count, matches_per_player, seed);
    pairings.get(match_index as usize).copied()
}

/// Calculate total number of matches for a tournament
pub fn calculate_match_count(
    participant_count: u32,
    matches_per_player: u16,
    seed: &[u8; 32],
) -> u32 {
    generate_all_pairings(participant_count, matches_per_player, seed).len() as u32
}

/// Fisher-Yates shuffle for usize array
fn shuffle_usize(arr: &mut [usize], rng: &mut SeededRng) {
    let len = arr.len();
    if len <= 1 {
        return;
    }
    
    for i in (1..len).rev() {
        let j = rng.next_range((i + 1) as u32) as usize;
        arr.swap(i, j);
    }
}

/// Fisher-Yates shuffle for match pairs
fn shuffle_pairs(pairs: &mut [(u32, u32)], rng: &mut SeededRng) {
    let len = pairs.len();
    if len <= 1 {
        return;
    }
    
    for i in (1..len).rev() {
        let j = rng.next_range((i + 1) as u32) as usize;
        pairs.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tournament() {
        let seed = [42u8; 32];
        let pairings = generate_all_pairings(0, 5, &seed);
        assert!(pairings.is_empty());
        
        let pairings = generate_all_pairings(1, 5, &seed);
        assert!(pairings.is_empty());
    }
    
    #[test]
    fn test_two_players() {
        let seed = [42u8; 32];
        let pairings = generate_all_pairings(2, 5, &seed);
        
        // 2 players, K=5: ceil(5/1) = 5 cycles × 1 pair = 5 matches
        assert_eq!(pairings.len(), 5);
        
        // All pairs should be (0, 1)
        for p in &pairings {
            assert_eq!(*p, (0, 1));
        }
    }
    
    #[test]
    fn test_small_tournament_round_robin() {
        let seed = [42u8; 32];
        // 4 players, K=5: n-1=3, ceil(5/3)=2 cycles × 6 pairs = 12 matches
        let pairings = generate_all_pairings(4, 5, &seed);
        assert_eq!(pairings.len(), 12);
        
        // Each unique pair should appear exactly 2 times
        let mut counts = std::collections::HashMap::new();
        for p in &pairings {
            *counts.entry(*p).or_insert(0u32) += 1;
        }
        assert_eq!(counts.len(), 6); // 6 unique pairs
        for (_, count) in &counts {
            assert_eq!(*count, 2);
        }
    }
    
    #[test]
    fn test_pairing_determinism() {
        let seed = [42u8; 32];
        
        let pairings1 = generate_all_pairings(20, 6, &seed);
        let pairings2 = generate_all_pairings(20, 6, &seed);
        
        assert_eq!(pairings1, pairings2);
    }
    
    #[test]
    fn test_different_seeds_different_pairings() {
        let seed1 = [1u8; 32];
        let seed2 = [2u8; 32];
        
        let pairings1 = generate_all_pairings(20, 6, &seed1);
        let pairings2 = generate_all_pairings(20, 6, &seed2);
        
        // Order should be different
        assert_ne!(pairings1, pairings2);
    }
    
    #[test]
    fn test_no_self_pairings() {
        let seed = [42u8; 32];
        let pairings = generate_all_pairings(50, 10, &seed);
        
        for (a, b) in &pairings {
            assert_ne!(a, b, "Self-pairing found: {} vs {}", a, b);
        }
    }
    
    #[test]
    fn test_no_duplicate_pairings() {
        let seed = [42u8; 32];
        let pairings = generate_all_pairings(50, 10, &seed);
        
        let mut sorted = pairings.clone();
        sorted.sort();
        
        for i in 1..sorted.len() {
            assert_ne!(sorted[i], sorted[i-1], "Duplicate pairing found: {:?}", sorted[i]);
        }
    }
    
    #[test]
    fn test_index_ordering() {
        let seed = [42u8; 32];
        let pairings = generate_all_pairings(50, 10, &seed);
        
        for (a, b) in &pairings {
            assert!(a < b, "Pairing not ordered: {} >= {}", a, b);
        }
    }
    
    #[test]
    fn test_get_pairing_for_match() {
        let seed = [42u8; 32];
        let pairings = generate_all_pairings(10, 4, &seed);
        
        for (i, expected) in pairings.iter().enumerate() {
            let actual = get_pairing_for_match(10, 4, &seed, i as u32);
            assert_eq!(actual, Some(*expected));
        }
        
        // Out of bounds
        let out_of_bounds = get_pairing_for_match(10, 4, &seed, 1000);
        assert_eq!(out_of_bounds, None);
    }
    
    #[test]
    fn test_match_count_round_robin() {
        let seed = [42u8; 32];
        
        // 4 players, K=10: n-1=3, ceil(10/3)=4 cycles × 6 pairs = 24 matches
        let count = calculate_match_count(4, 10, &seed);
        assert_eq!(count, 24);
    }
    
    #[test]
    fn test_large_tournament_match_count() {
        let seed = [42u8; 32];
        let n = 100u32;
        let k = 14u16; // Even k for clean division
        let pairings = generate_all_pairings(n, k, &seed);
        
        // Expected: n*K/2 = 100*14/2 = 700
        let expected = (n as usize) * (k as usize) / 2;
        assert_eq!(pairings.len(), expected, "Got {} matches, expected {}", pairings.len(), expected);
        
        // All indices in valid range
        for (a, b) in &pairings {
            assert!(*a < n);
            assert!(*b < n);
        }
    }
    
    #[test]
    fn test_each_player_has_k_matches() {
        let seed = [42u8; 32];
        let n = 20u32;
        let k = 8u16; // Even k
        let pairings = generate_all_pairings(n, k, &seed);
        
        // Count matches per player
        let mut counts = vec![0u32; n as usize];
        for (a, b) in &pairings {
            counts[*a as usize] += 1;
            counts[*b as usize] += 1;
        }
        
        // Each player should have exactly K matches
        for (i, count) in counts.iter().enumerate() {
            assert_eq!(*count, k as u32, "Player {} has {} matches, expected {}", i, count, k);
        }
    }
    
    #[test]
    fn test_total_matches_formula() {
        let seed = [42u8; 32];
        
        // Test the n*K/2 formula for various sizes (use even K for clean math)
        for (n, k) in [(10, 4), (20, 10), (50, 14), (100, 14)].iter() {
            let pairings = generate_all_pairings(*n, *k, &seed);
            let expected = (*n as usize) * (*k as usize) / 2;
            assert_eq!(
                pairings.len(), 
                expected,
                "n={}, k={}: got {} matches, expected {}",
                n, k, pairings.len(), expected
            );
        }
    }
    
    #[test]
    fn test_odd_k_handled() {
        let seed = [42u8; 32];
        let n = 20u32;
        let k = 15u16; // Odd K - rounds up to use 8 offsets = 16 matches/player
        let pairings = generate_all_pairings(n, k, &seed);
        
        // Count matches per player
        let mut counts = vec![0u32; n as usize];
        for (a, b) in &pairings {
            counts[*a as usize] += 1;
            counts[*b as usize] += 1;
        }
        
        // Each player should have >= K matches (might be K or K+1 due to rounding)
        for (i, count) in counts.iter().enumerate() {
            assert!(*count >= k as u32, "Player {} has {} matches, expected >= {}", i, count, k);
        }
    }

    #[test]
    fn test_two_players_k15() {
        let seed = [42u8; 32];
        let pairings = generate_all_pairings(2, 15, &seed);
        
        // N=2, K=15: ceil(15/1) = 15 cycles × 1 pair = 15 matches
        assert_eq!(pairings.len(), 15);
        for p in &pairings {
            assert_eq!(*p, (0, 1));
        }
    }

    #[test]
    fn test_small_n_each_player_has_geq_k_matches() {
        let seed = [42u8; 32];
        let k = 15u16;
        
        for n in 2..=6u32 {
            let pairings = generate_all_pairings(n, k, &seed);
            
            let mut counts = vec![0u32; n as usize];
            for (a, b) in &pairings {
                counts[*a as usize] += 1;
                counts[*b as usize] += 1;
            }
            
            for (i, count) in counts.iter().enumerate() {
                assert!(
                    *count >= k as u32,
                    "N={}, Player {} has {} matches, expected >= {}",
                    n, i, count, k
                );
            }
        }
    }

    #[test]
    fn test_repeated_pairings_have_distinct_indices() {
        let seed = [42u8; 32];
        // N=2, K=15: 15 matches, all (0,1) but at different indices
        let pairings = generate_all_pairings(2, 15, &seed);
        assert_eq!(pairings.len(), 15);
        
        // Each match_index produces a different game via SeededRng::new(seed, match_index)
        // Verify indices 0..15 all map to valid pairings
        for i in 0..15u32 {
            let p = get_pairing_for_match(2, 15, &seed, i);
            assert_eq!(p, Some((0, 1)));
        }
        assert_eq!(get_pairing_for_match(2, 15, &seed, 15), None);
    }

    #[test]
    fn test_effective_k_tier_a() {
        assert_eq!(effective_k(0, 99), 0);
        assert_eq!(effective_k(1, 99), 0);
        assert_eq!(effective_k(2, 99), 1);
        assert_eq!(effective_k(10, 99), 9);
        assert_eq!(effective_k(200, 99), 199);
    }

    #[test]
    fn test_effective_k_tier_b_c() {
        assert_eq!(effective_k(201, 99), 99);
        assert_eq!(effective_k(500, 99), 99);
        assert_eq!(effective_k(1000, 99), 99);
        assert_eq!(effective_k(5000, 99), 99);
        // config_k < 49 gets clamped up
        assert_eq!(effective_k(500, 10), 49);
        // config_k > 99 gets clamped down
        assert_eq!(effective_k(500, 150), 99);
        assert_eq!(effective_k(500, 50), 50);
    }

    #[test]
    fn test_full_round_robin_all_pairs() {
        let seed = [42u8; 32];
        let n = 50u32;
        let k = 49u16; // n-1 = full round-robin
        let pairings = generate_all_pairings(n, k, &seed);
        
        let mut unique: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
        for p in &pairings {
            unique.insert(*p);
        }
        
        let expected = (n * (n - 1) / 2) as usize;
        assert_eq!(unique.len(), expected, "Expected {} unique pairs, got {}", expected, unique.len());
    }
}
