//! Seeded pseudo-random number generator
//!
//! Deterministic PRNG for reproducible match execution.
//! Uses a simple but effective xorshift algorithm.

/// Seeded random number generator
/// 
/// Deterministic: same seed + index = same sequence
#[derive(Clone, Debug)]
pub struct SeededRng {
    state: u64,
}

impl SeededRng {
    /// Create a new RNG from a 32-byte seed and match index
    pub fn new(seed: &[u8; 32], match_index: u32) -> Self {
        // Combine seed bytes into initial state
        let mut state = 0u64;
        for (i, chunk) in seed.chunks(8).enumerate() {
            let mut bytes = [0u8; 8];
            bytes[..chunk.len()].copy_from_slice(chunk);
            state ^= u64::from_le_bytes(bytes).wrapping_add(i as u64);
        }
        
        // Mix in match index
        state ^= (match_index as u64).wrapping_mul(0x517cc1b727220a95);
        
        // Warm up the generator
        let mut rng = Self { state };
        for _ in 0..8 {
            rng.next_u64();
        }
        
        rng
    }
    
    /// Create RNG for a specific round within a match
    pub fn for_round(&self, round: u8) -> Self {
        let mut new_state = self.state;
        new_state ^= (round as u64).wrapping_mul(0x9e3779b97f4a7c15);
        
        let mut rng = Self { state: new_state };
        rng.next_u64(); // Mix
        rng
    }
    
    /// Generate next u64
    pub fn next_u64(&mut self) -> u64 {
        // xorshift64*
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state.wrapping_mul(0x2545f4914f6cdd1d)
    }
    
    /// Generate next u32
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }
    
    /// Generate a value 0-99 (for percentage checks)
    pub fn next_percent(&mut self) -> u8 {
        (self.next_u32() % 100) as u8
    }
    
    /// Generate a value in range [0, max)
    pub fn next_range(&mut self, max: u32) -> u32 {
        if max == 0 {
            return 0;
        }
        self.next_u32() % max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determinism() {
        let seed = [42u8; 32];
        let rng1 = SeededRng::new(&seed, 0);
        let rng2 = SeededRng::new(&seed, 0);
        
        let mut r1 = rng1.clone();
        let mut r2 = rng2.clone();
        
        for _ in 0..100 {
            assert_eq!(r1.next_u64(), r2.next_u64());
        }
    }
    
    #[test]
    fn test_different_seeds() {
        let seed1 = [1u8; 32];
        let seed2 = [2u8; 32];
        
        let mut rng1 = SeededRng::new(&seed1, 0);
        let mut rng2 = SeededRng::new(&seed2, 0);
        
        // Should produce different sequences
        let vals1: Vec<_> = (0..10).map(|_| rng1.next_u64()).collect();
        let vals2: Vec<_> = (0..10).map(|_| rng2.next_u64()).collect();
        
        assert_ne!(vals1, vals2);
    }
    
    #[test]
    fn test_different_match_index() {
        let seed = [42u8; 32];
        
        let mut rng1 = SeededRng::new(&seed, 0);
        let mut rng2 = SeededRng::new(&seed, 1);
        
        assert_ne!(rng1.next_u64(), rng2.next_u64());
    }
    
    #[test]
    fn test_percent_range() {
        let seed = [42u8; 32];
        let mut rng = SeededRng::new(&seed, 0);
        
        for _ in 0..1000 {
            let p = rng.next_percent();
            assert!(p < 100);
        }
    }
    
    #[test]
    fn test_next_range() {
        let seed = [42u8; 32];
        let mut rng = SeededRng::new(&seed, 0);
        
        // Test various ranges
        for max in [1, 10, 100, 1000].iter() {
            for _ in 0..100 {
                let val = rng.next_range(*max);
                assert!(val < *max, "next_range({}) returned {}", max, val);
            }
        }
        
        // Test edge case: max = 0
        assert_eq!(rng.next_range(0), 0);
    }
}
