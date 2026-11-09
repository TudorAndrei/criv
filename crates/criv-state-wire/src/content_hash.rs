//! Content hashes published in State: snapshot names and asset hashes.

/// The lowercase hex BLAKE3 hash of `bytes`.
#[must_use]
pub fn of_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// The snapshot hash of a published State document.
///
/// The hash covers the JSON text without its final newline.
#[must_use]
pub fn of_document(published: &str) -> String {
    of_bytes(published.strip_suffix('\n').unwrap_or(published).as_bytes())
}

/// Whether `value` has the published content-hash form: 64 lowercase hex digits.
#[must_use]
pub fn is_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_hash_ignores_only_the_final_newline() {
        assert_eq!(of_document("{}\n"), of_bytes(b"{}"));
        assert_eq!(of_document("{}"), of_bytes(b"{}"));
        assert_ne!(of_document("{}\n\n"), of_bytes(b"{}"));
    }

    #[test]
    fn only_lowercase_hex_of_hash_length_is_valid() {
        let hash = of_bytes(b"x");
        assert!(is_valid(&hash));
        assert!(!is_valid(&hash.to_uppercase()));
        assert!(!is_valid(&hash.chars().skip(1).collect::<String>()));
    }
}
