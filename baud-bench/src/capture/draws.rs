//! The draws `random.Random` makes over `baud-domain`'s generator, so a
//! seeded generator here writes what the Python one did: `random`,
//! `_randbelow` (which `choice` and `randint` go through) and `gauss`, whose
//! second value is kept for the next call.

#[path = "../../../baud-domain/build/py_random.rs"]
#[allow(dead_code)]
mod py_random;

use py_random::PyRandom;

/// A seeded `random.Random`.
pub struct Draws {
    mt: PyRandom,
    spare: Option<f64>,
}

impl Draws {
    /// `random.Random(seed)`.
    pub fn new(seed: u32) -> Draws {
        Draws {
            mt: PyRandom::new(seed),
            spare: None,
        }
    }

    /// `random()`: 53 bits, as `_randommodule.c` makes them of two words.
    pub fn random(&mut self) -> f64 {
        let a = f64::from(self.mt.next_u32() >> 5);
        let b = f64::from(self.mt.next_u32() >> 6);
        (a * 67_108_864.0 + b) * (1.0 / 9_007_199_254_740_992.0)
    }

    /// `_randbelow(n)`: as many bits as `n` has, drawn again while too large.
    pub fn below(&mut self, n: u32) -> u32 {
        let k = 32 - n.leading_zeros();
        loop {
            let r = self.mt.next_u32() >> (32 - k);
            if r < n {
                return r;
            }
        }
    }

    /// `choice(seq)`.
    pub fn choice<'a, T>(&mut self, seq: &'a [T]) -> &'a T {
        let n = u32::try_from(seq.len()).expect("a short sequence");
        &seq[usize::try_from(self.below(n)).expect("an index")]
    }

    /// `randint(a, b)`.
    pub fn randint(&mut self, a: u32, b: u32) -> u32 {
        a + self.below(b - a + 1)
    }

    /// `gauss(mu, sigma)`.
    pub fn gauss(&mut self, mu: f64, sigma: f64) -> f64 {
        let z = match self.spare.take() {
            Some(z) => z,
            None => {
                let x2pi = self.random() * core::f64::consts::TAU;
                let g2rad = (-2.0 * (1.0 - self.random()).ln()).sqrt();
                self.spare = Some(x2pi.sin() * g2rad);
                x2pi.cos() * g2rad
            }
        };
        mu + z * sigma
    }
}
