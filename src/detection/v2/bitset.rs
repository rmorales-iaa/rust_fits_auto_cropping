//! Compact fixed-size bit set for full-frame pixel masks (1 bit per pixel).

#[derive(Clone, Debug)]
pub struct Bitset {
    words: Vec<u64>,
    len: usize,
}

impl Bitset {
    pub fn new(len: usize) -> Self {
        Self {
            words: vec![0; len.div_ceil(64)],
            len,
        }
    }

    pub fn get(&self, index: usize) -> bool {
        debug_assert!(index < self.len);
        self.words[index / 64] >> (index % 64) & 1 == 1
    }

    pub fn set(&mut self, index: usize) {
        debug_assert!(index < self.len);
        self.words[index / 64] |= 1 << (index % 64);
    }

    pub fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::Bitset;

    #[test]
    fn set_get_and_count() {
        let mut a = Bitset::new(130);
        a.set(0);
        a.set(129);
        a.set(64);
        assert!(a.get(0) && a.get(64) && a.get(129) && !a.get(1));
        assert_eq!(a.count(), 3);
    }
}
