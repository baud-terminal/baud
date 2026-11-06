//! CPython's `random.Random`, for a search a Python generator ran: the
//! multiplier it found is found again, from the same seed, so moving the
//! search into a build changes nothing it chose. `build/tcap.rs` searches
//! with it, and `src/vt/iterm.rs`' tests check its two indexes' with it.
//! `core` alone, so a test of the `no_std` crate can include it.

/// **CPython's `random.Random`**, the Mersenne Twister MT19937 as its
/// `_randommodule.c` seeds and draws it, so a multiplier searched for with
/// Python's generator is found again: an integer seed is `init_by_array`
/// over its 32-bit words, low first, and `getrandbits(64)` takes two draws,
/// the first the low word.
pub struct PyRandom {
    mt: [u32; 624],
    i: usize,
}

impl PyRandom {
    /// `random.Random(seed)` for a seed below 2^32: the key is the seed
    /// alone.
    pub fn new(seed: u32) -> PyRandom {
        let mut mt = [0u32; 624];
        mt[0] = 19_650_218;
        for i in 1..624 {
            mt[i] = 1_812_433_253u32
                .wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        let key = [seed];
        let (mut i, mut j) = (1usize, 0usize);
        for _ in 0..624.max(key.len()) {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= 624 {
                mt[0] = mt[623];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..623 {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1_566_083_941))
                .wrapping_sub(i as u32);
            i += 1;
            if i >= 624 {
                mt[0] = mt[623];
                i = 1;
            }
        }
        mt[0] = 0x8000_0000;
        PyRandom { mt, i: 624 }
    }

    /// `genrand_uint32`.
    pub fn next_u32(&mut self) -> u32 {
        if self.i >= 624 {
            for k in 0..624 {
                let y = (self.mt[k] & 0x8000_0000) | (self.mt[(k + 1) % 624] & 0x7fff_ffff);
                let mag = if y & 1 == 0 { 0 } else { 0x9908_b0df };
                self.mt[k] = self.mt[(k + 397) % 624] ^ (y >> 1) ^ mag;
            }
            self.i = 0;
        }
        let mut y = self.mt[self.i];
        self.i += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// `getrandbits(64)`.
    pub fn bits64(&mut self) -> u64 {
        let lo = u64::from(self.next_u32());
        lo | (u64::from(self.next_u32()) << 32)
    }
}
