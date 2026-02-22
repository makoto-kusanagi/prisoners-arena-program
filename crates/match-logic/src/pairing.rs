//! Deterministic pairing generation for tournament matches
//!
//! Uses a Feistel-network permutation for O(1)-memory per-match lookups
//! and direct pair computation (no heap allocation on the critical on-chain path).

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
/// Heap-allocating version for WASM/frontend use. On-chain code should use
/// `get_pairing_for_match` instead.
///
/// # Arguments
/// * `participant_count` - Total number of participants
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
    let n = participant_count;
    let k = matches_per_player as u32;

    if n < 2 {
        return Vec::new();
    }

    let total = calculate_match_count_inner(n, k);
    if total == 0 {
        return Vec::new();
    }

    let round_keys = derive_feistel_keys(seed);

    if n <= k + 1 {
        (0..total)
            .filter_map(|i| {
                let canonical = feistel_permute(i, total, &round_keys)?;
                Some(canonical_to_pair_round_robin(canonical, n))
            })
            .collect()
    } else {
        let (offsets, count) = select_offsets(n, k, seed);
        (0..total)
            .filter_map(|i| {
                let canonical = feistel_permute(i, total, &round_keys)?;
                Some(canonical_to_pair_circular(canonical, n, &offsets[..count]))
            })
            .collect()
    }
}

/// Get the pairing for a specific match index — O(1) memory
pub fn get_pairing_for_match(
    participant_count: u32,
    matches_per_player: u16,
    seed: &[u8; 32],
    match_index: u32,
) -> Option<(u32, u32)> {
    let n = participant_count;
    let k = matches_per_player as u32;

    let total = calculate_match_count_inner(n, k);
    if match_index >= total {
        return None;
    }

    let round_keys = derive_feistel_keys(seed);
    let canonical_idx = feistel_permute(match_index, total, &round_keys)?;

    if n <= k + 1 {
        Some(canonical_to_pair_round_robin(canonical_idx, n))
    } else {
        let (offsets, count) = select_offsets(n, k, seed);
        Some(canonical_to_pair_circular(canonical_idx, n, &offsets[..count]))
    }
}

/// Calculate total number of matches — O(1), no allocation
pub fn calculate_match_count(
    participant_count: u32,
    matches_per_player: u16,
    _seed: &[u8; 32],
) -> u32 {
    calculate_match_count_inner(participant_count, matches_per_player as u32)
}

// ──────────────────────────── Internal helpers ────────────────────────────

/// O(1) match count via formula.
///
/// Round-robin (n ≤ k+1): `C(n,2) × ⌈k/(n−1)⌉`
/// Circular  (n > k+1):  `offsets_to_use × n`
fn calculate_match_count_inner(n: u32, k: u32) -> u32 {
    if n < 2 || k == 0 {
        return 0;
    }

    if n <= k + 1 {
        let c_n_2 = n * (n - 1) / 2;
        let cycles = k.div_ceil(n - 1);
        c_n_2 * cycles
    } else {
        let available = if n % 2 == 0 { n / 2 - 1 } else { n / 2 };
        let offsets_to_use = k.div_ceil(2).min(available);
        offsets_to_use * n
    }
}

/// Integer ceiling square root (pure integer — no f64, BPF-safe).
fn isqrt_ceil(n: u32) -> u32 {
    if n <= 1 {
        return n;
    }
    // Newton's method for floor(sqrt(n))
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    // x = floor(sqrt(n))
    if (x as u64) * (x as u64) == n as u64 {
        x
    } else {
        x + 1
    }
}

/// Feistel round function — mixes `input` with `key`, result in [0, modulus).
fn feistel_round_fn(input: u32, key: u64, modulus: u32) -> u32 {
    (((input as u64).wrapping_mul(key | 1).wrapping_add(key >> 32)) >> 16) as u32 % modulus
}

/// Derive 6 Feistel round keys from seed (uses a separate RNG stream via u32::MAX).
fn derive_feistel_keys(seed: &[u8; 32]) -> [u64; 6] {
    let mut rng = SeededRng::new(seed, u32::MAX);
    let mut keys = [0u64; 6];
    for k in &mut keys {
        *k = rng.next_u64();
    }
    keys
}

/// Bijective permutation on [0, domain_size) via a Feistel network with cycle-walking.
fn feistel_permute(idx: u32, domain_size: u32, round_keys: &[u64; 6]) -> Option<u32> {
    if domain_size <= 1 {
        return Some(0);
    }
    let half = isqrt_ceil(domain_size);

    let mut val = idx;
    for _ in 0..1000 {
        let mut left = val / half;
        let mut right = val % half;

        for (i, &key) in round_keys.iter().enumerate() {
            if i % 2 == 0 {
                right = (right + feistel_round_fn(left, key, half)) % half;
            } else {
                left = (left + feistel_round_fn(right, key, half)) % half;
            }
        }

        val = left * half + right;
        if val < domain_size {
            return Some(val);
        }
        // cycle-walk: re-enter with the out-of-range value
    }
    None
}

/// Floyd's algorithm: sample `offsets_to_use` distinct offsets from [1, available].
/// Stack-allocated, zero heap. Max 50 entries (k ≤ 99 → ⌈99/2⌉ = 50).
fn select_offsets(n: u32, k: u32, seed: &[u8; 32]) -> ([u32; 50], usize) {
    let available = if n % 2 == 0 { n / 2 - 1 } else { n / 2 };
    debug_assert!(k.div_ceil(2).min(available) <= 50, "select_offsets: k too large");
    let m = k.div_ceil(2).min(available).min(50) as usize;
    let big_n = available as usize;

    let mut rng = SeededRng::new(seed, 0);
    let mut result = [0u32; 50];
    let mut count = 0usize;

    for j in (big_n - m + 1)..=big_n {
        let t = rng.next_range(j as u32) + 1; // uniform in [1, j]

        let mut found = false;
        for i in 0..count {
            if result[i] == t {
                found = true;
                break;
            }
        }

        result[count] = if found { j as u32 } else { t };
        count += 1;
    }

    result[..count].sort_unstable();
    (result, count)
}

/// Colexicographic combination unranking: rank → (a, b) with a < b.
///
/// rank = C(b,2) + a = b*(b−1)/2 + a
fn unrank_pair(rank: u32) -> (u32, u32) {
    // Estimate b via integer floor(sqrt(1 + 8·rank))
    let val = 1u64 + 8 * rank as u64;
    let mut s = val;
    let mut t = (s + 1) / 2;
    while t < s {
        s = t;
        t = (s + val / s) / 2;
    }
    // s = floor(sqrt(val))
    let mut b = ((1 + s) / 2) as u32;

    // Correct estimate
    while b > 0 && b * (b - 1) / 2 > rank {
        b -= 1;
    }
    while (b + 1) * b / 2 <= rank {
        b += 1;
    }

    let a = rank - b * (b - 1) / 2;
    (a, b)
}

/// Round-robin mode: canonical index → pair via modular unranking.
fn canonical_to_pair_round_robin(canonical_idx: u32, n: u32) -> (u32, u32) {
    let c_n_2 = n * (n - 1) / 2;
    let pair_rank = canonical_idx % c_n_2;
    unrank_pair(pair_rank)
}

/// Circular mode: canonical index → pair via offset groups.
fn canonical_to_pair_circular(canonical_idx: u32, n: u32, offsets: &[u32]) -> (u32, u32) {
    let group = canonical_idx / n;
    let i = canonical_idx % n;
    let d = offsets[group as usize];
    let j = (i + d) % n;
    if i < j {
        (i, j)
    } else {
        (j, i)
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
            assert_ne!(
                sorted[i],
                sorted[i - 1],
                "Duplicate pairing found: {:?}",
                sorted[i]
            );
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
        assert_eq!(
            pairings.len(),
            expected,
            "Got {} matches, expected {}",
            pairings.len(),
            expected
        );

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
            assert_eq!(
                *count, k as u32,
                "Player {} has {} matches, expected {}",
                i, count, k
            );
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
                n,
                k,
                pairings.len(),
                expected
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
            assert!(
                *count >= k as u32,
                "Player {} has {} matches, expected >= {}",
                i, count, k
            );
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
        assert_eq!(
            unique.len(),
            expected,
            "Expected {} unique pairs, got {}",
            expected,
            unique.len()
        );
    }

    // ──────────────── New tests for Feistel & helpers ────────────────

    #[test]
    fn test_feistel_is_bijection() {
        let seed = [42u8; 32];
        let keys = derive_feistel_keys(&seed);

        for domain in [1, 2, 3, 5, 10, 50, 100, 250, 1000] {
            let mut seen = vec![false; domain];
            for i in 0..domain as u32 {
                let out = feistel_permute(i, domain as u32, &keys).unwrap();
                assert!(out < domain as u32, "out of range: {} >= {}", out, domain);
                assert!(
                    !seen[out as usize],
                    "duplicate at domain={}, idx={}, out={}",
                    domain, i, out
                );
                seen[out as usize] = true;
            }
        }
    }

    #[test]
    fn test_feistel_different_seeds() {
        let keys1 = derive_feistel_keys(&[1u8; 32]);
        let keys2 = derive_feistel_keys(&[2u8; 32]);
        let domain = 100u32;

        let perm1: Vec<u32> = (0..domain)
            .map(|i| feistel_permute(i, domain, &keys1).unwrap())
            .collect();
        let perm2: Vec<u32> = (0..domain)
            .map(|i| feistel_permute(i, domain, &keys2).unwrap())
            .collect();

        assert_ne!(perm1, perm2);
    }

    #[test]
    fn test_floyd_sample_properties() {
        let seed = [42u8; 32];
        for (n, k) in [(20u32, 8u32), (100, 50), (201, 99)] {
            let (offsets, count) = select_offsets(n, k, &seed);
            let available = if n % 2 == 0 { n / 2 - 1 } else { n / 2 };
            let expected = k.div_ceil(2).min(available) as usize;
            assert_eq!(count, expected, "n={}, k={}", n, k);

            for i in 0..count {
                assert!(
                    offsets[i] >= 1 && offsets[i] <= available,
                    "offset {} out of range [1, {}]",
                    offsets[i],
                    available
                );
            }

            // No duplicates (sorted, so check adjacent)
            for i in 1..count {
                assert!(
                    offsets[i] > offsets[i - 1],
                    "duplicate offset at n={}, k={}",
                    n,
                    k
                );
            }
        }
    }

    #[test]
    fn test_unrank_pair_covers_all() {
        for n in [2u32, 5, 10, 20] {
            let c_n_2 = n * (n - 1) / 2;
            let mut seen = std::collections::HashSet::new();
            for r in 0..c_n_2 {
                let (a, b) = unrank_pair(r);
                assert!(a < b, "not ordered: {} >= {}", a, b);
                assert!(b < n, "out of range: b={} >= n={}", b, n);
                assert!(seen.insert((a, b)), "duplicate pair at rank {}", r);
            }
            assert_eq!(seen.len(), c_n_2 as usize);
        }
    }

    #[test]
    fn test_calculate_match_count_consistency() {
        let seed = [42u8; 32];
        for (n, k) in [(2, 5), (4, 5), (10, 4), (20, 10), (50, 14), (100, 14)] {
            let formula = calculate_match_count(n, k, &seed);
            let actual = generate_all_pairings(n, k, &seed).len() as u32;
            assert_eq!(
                formula, actual,
                "n={}, k={}: formula={}, actual={}",
                n, k, formula, actual
            );
        }
    }

    #[test]
    fn test_circular_no_duplicates_large() {
        let seed = [42u8; 32];
        for n in [201u32, 300, 500] {
            let k = 99u16;
            let pairings = generate_all_pairings(n, k, &seed);

            let mut sorted = pairings.clone();
            sorted.sort();
            for i in 1..sorted.len() {
                assert_ne!(
                    sorted[i],
                    sorted[i - 1],
                    "Duplicate at n={}: {:?}",
                    n,
                    sorted[i]
                );
            }
        }
    }

    #[test]
    fn test_effective_k_integration() {
        let seed = [42u8; 32];
        for n in [201u32, 300, 500, 1000] {
            let k = effective_k(n, 99);
            let pairings = generate_all_pairings(n, k, &seed);
            let count = calculate_match_count(n, k, &seed);
            assert_eq!(pairings.len() as u32, count, "n={n}, k={k}");

            // Verify get_pairing_for_match matches generate_all_pairings
            for (i, expected) in pairings.iter().enumerate() {
                let actual = get_pairing_for_match(n, k, &seed, i as u32);
                assert_eq!(actual, Some(*expected), "n={n}, k={k}, i={i}");
            }
        }
    }

    #[test]
    fn test_isqrt_ceil() {
        assert_eq!(isqrt_ceil(0), 0);
        assert_eq!(isqrt_ceil(1), 1);
        assert_eq!(isqrt_ceil(2), 2);
        assert_eq!(isqrt_ceil(3), 2);
        assert_eq!(isqrt_ceil(4), 2);
        assert_eq!(isqrt_ceil(5), 3);
        assert_eq!(isqrt_ceil(9), 3);
        assert_eq!(isqrt_ceil(10), 4);
        assert_eq!(isqrt_ceil(100), 10);
        assert_eq!(isqrt_ceil(101), 11);
        assert_eq!(isqrt_ceil(10000), 100);
        assert_eq!(isqrt_ceil(250000), 500);
    }
}
