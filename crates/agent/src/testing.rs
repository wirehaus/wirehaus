pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, limit: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(limit).unwrap()).unwrap()
    }

    pub fn bytes(&mut self, max: usize) -> Vec<u8> {
        let length = self.below(max + 1);
        (0..length)
            .map(|_| u8::try_from(self.next() & 0xFF).unwrap())
            .collect()
    }

    pub fn mutate(&mut self, valid: &[u8]) -> Vec<u8> {
        let mut bytes = valid.to_vec();
        match self.below(5) {
            0 if !bytes.is_empty() => {
                let at = self.below(bytes.len());
                bytes[at] ^= 1 << self.below(8);
            }
            1 if !bytes.is_empty() => bytes.truncate(self.below(bytes.len())),
            2 => bytes.extend(self.bytes(8)),
            3 if !bytes.is_empty() => {
                let at = self.below(bytes.len());
                bytes.remove(at);
            }
            _ if !bytes.is_empty() => {
                let at = self.below(bytes.len());
                bytes[at] = u8::try_from(self.next() & 0xFF).unwrap();
            }
            _ => {}
        }
        bytes
    }
}
