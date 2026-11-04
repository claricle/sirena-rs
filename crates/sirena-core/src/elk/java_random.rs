//! `java.util.Random`'s 48-bit linear congruential generator, as
//! elkrb's `JavaRandom` (lib/elkrb/layout/java_random.rb) has it.

const MULTIPLIER: u64 = 0x5DEECE66D;
const ADDEND: u64 = 0xB;
const MASK: u64 = (1 << 48) - 1;

#[derive(Debug, Clone)]
pub struct JavaRandom {
    state: u64,
}

impl JavaRandom {
    pub fn new(seed: i64) -> Self {
        JavaRandom { state: (seed as u64 ^ MULTIPLIER) & MASK }
    }

    /// Restarts the sequence, like `Random#setSeed`.
    pub fn set_seed(&mut self, seed: i64) {
        self.state = (seed as u64 ^ MULTIPLIER) & MASK;
    }

    /// The top `bits` bits of the next state, as a Java int (bits <= 32).
    pub fn next_bits(&mut self, bits: u32) -> i64 {
        self.state = (self.state.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND)) & MASK;
        let value = (self.state >> (48 - bits)) as i64;
        if value >= 1 << 31 { value - (1 << 32) } else { value }
    }

    /// A signed 64-bit integer, like `Random#nextLong` (wrapping).
    pub fn next_long(&mut self) -> i64 {
        let hi = self.next_bits(32);
        let lo = self.next_bits(32);
        (hi << 32).wrapping_add(lo)
    }

    /// A float in [0, 1) with 24 random bits, like `Random#nextFloat`.
    pub fn next_float(&mut self) -> f64 {
        self.next_bits(24) as f64 / 16_777_216.0
    }

    /// A double in [0, 1) with 53 random bits, like `Random#nextDouble`.
    pub fn next_double(&mut self) -> f64 {
        let hi = self.next_bits(26);
        let lo = self.next_bits(27);
        ((hi << 27) + lo) as f64 / 9_007_199_254_740_992.0
    }

    /// An integer in [0, bound), like `Random#nextInt(bound)`.
    pub fn next_int(&mut self, bound: i64) -> i64 {
        assert!(bound > 0, "bound must be positive");
        if bound & (bound - 1) == 0 {
            return (bound * self.next_bits(31)) >> 31;
        }
        loop {
            let bits = self.next_bits(31);
            let value = bits % bound;
            if bits - value + (bound - 1) < 1 << 31 {
                return value;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REF_BITS32: [i64; 6] = [-1155869325, 431529176, 1761283695, 1749940626, 892128508, 155629808];
    const REF_LONG: i64 = -4964420948893066024;
    const REF_BIT: i64 = 0;
    const REF_DOUBLE: f64 = 0.7275636800328681;
    const REF_FLOAT: f64 = 0.6832234263420105;
    const REF_INT10: i64 = 4;
    const REF_INT16: i64 = 4;
    const REF_LONG7: i64 = -4967725919621401576;

    // Reference values computed with elkrb 2.0.1's Elkrb::Layout::JavaRandom.
    #[test]
    fn matches_ruby_reference() {
        let mut r = JavaRandom::new(1);
        let bits: Vec<i64> = (0..6).map(|_| r.next_bits(32)).collect();
        assert_eq!(bits, REF_BITS32);
        let mut r = JavaRandom::new(1);
        assert_eq!(r.next_long(), REF_LONG);
        assert_eq!(r.next_bits(1), REF_BIT);
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next_double().to_bits(), REF_DOUBLE.to_bits());
        assert_eq!(r.next_float().to_bits(), REF_FLOAT.to_bits());
        assert_eq!(r.next_int(10), REF_INT10);
        assert_eq!(r.next_int(16), REF_INT16);
        r.set_seed(7);
        assert_eq!(r.next_long(), REF_LONG7);
    }
}
