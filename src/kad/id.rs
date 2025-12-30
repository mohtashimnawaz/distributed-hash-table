use rand::RngCore;
use std::cmp::Ordering;

/// 256-bit NodeId
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct NodeId(pub [u8; 32]);

impl NodeId {
    pub fn random() -> Self {
        let mut buf = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut buf);
        Self(buf)
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn xor(&self, other: &NodeId) -> [u8; 32] {
        let mut out = [0u8; 32];
        for i in 0..32 {
            out[i] = self.0[i] ^ other.0[i];
        }
        out
    }

    /// Returns the index of the bucket this id would fall into, based on XOR distance.
    /// If ids are equal, returns None.
    pub fn bucket_index(&self, other: &NodeId) -> Option<usize> {
        let xor = self.xor(other);
        for (i, byte) in xor.iter().enumerate() {
            if *byte != 0 {
                // leading zero bits in byte
                let lz = byte.leading_zeros() as usize;
                let bit_index = i * 8 + lz; // 0-based
                return Some(255 - bit_index);
            }
        }
        None
    }
}

impl Ord for NodeId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.iter().cmp(other.0.iter())
    }
}

impl PartialOrd for NodeId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_and_distance() {
        let a = NodeId::random();
        let b = NodeId::random();
        assert!(a != b || true); // just ensure it constructs

        if a != b {
            let idx = a.bucket_index(&b);
            assert!(idx.is_some());
            let idx_rev = b.bucket_index(&a);
            assert_eq!(idx, idx_rev);
        }
    }

    #[test]
    fn bucket_index_equal() {
        let a = NodeId::from_bytes([1u8; 32]);
        let b = NodeId::from_bytes([1u8; 32]);
        assert_eq!(a.bucket_index(&b), None);
    }
}
