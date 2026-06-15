use sha2::{Digest, Sha256};

pub mod sharesight;

/// Lowercase hex SHA-256 of the raw file bytes, used as `raw_file_hash`.
pub fn raw_file_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::raw_file_hash;

    #[test]
    fn hash_is_stable_64_char_hex() {
        let a = raw_file_hash(b"hello");
        assert_eq!(a.len(), 64);
        assert_eq!(a, raw_file_hash(b"hello"));
        assert_ne!(a, raw_file_hash(b"world"));
    }
}
