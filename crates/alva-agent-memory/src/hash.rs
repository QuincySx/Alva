// INPUT:  &str
// OUTPUT: pub fn compute_hash
// POS:    Stable content hash for change detection — shared by sync and service.

/// FNV-1a (64-bit) content hash.
///
/// Stable across Rust versions, unlike `DefaultHasher` which is documented
/// as not guaranteed stable across releases. Used for change detection
/// (not security). If a stronger hash is needed later, change this one function.
pub fn compute_hash(content: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in content.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        assert_eq!(compute_hash("hello"), compute_hash("hello"));
    }

    #[test]
    fn different_inputs_differ() {
        assert_ne!(compute_hash("hello"), compute_hash("world"));
    }
}
