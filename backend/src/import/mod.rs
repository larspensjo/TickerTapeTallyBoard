use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::time::SystemTime;

pub mod sharesight;
pub mod text;

/// Lowercase hex SHA-256 of the raw file bytes, used as `raw_file_hash`.
pub fn raw_file_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Current instant as an RFC-3339 UTC string for `imported_at`.
pub fn now_iso8601() -> String {
    let now: DateTime<Utc> = SystemTime::now().into();
    now.to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::{now_iso8601, raw_file_hash};
    use chrono::DateTime;

    #[test]
    fn now_iso8601_looks_like_rfc3339() {
        let value = now_iso8601();
        DateTime::parse_from_rfc3339(&value).expect("timestamp parses as RFC3339");
    }

    #[test]
    fn hash_is_stable_64_char_hex() {
        let a = raw_file_hash(b"hello");
        assert_eq!(a.len(), 64);
        assert_eq!(a, raw_file_hash(b"hello"));
        assert_ne!(a, raw_file_hash(b"world"));
    }
}
