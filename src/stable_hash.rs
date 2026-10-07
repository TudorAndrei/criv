//! Unambiguous BLAKE3 input for published State hashes and partition
//! fingerprints.

/// Feeds typed values to BLAKE3 so that two different inputs never produce
/// the same bytes. A string carries its length, an optional value carries a
/// presence tag, and a list carries its item count.
pub struct StableHasher(blake3::Hasher);

impl StableHasher {
    /// Starts a hash whose first input names the thing it hashes.
    pub fn new(domain: &str) -> Self {
        let mut hasher = Self(blake3::Hasher::new());
        hasher.str(domain);
        hasher
    }

    pub fn str(&mut self, value: &str) -> &mut Self {
        self.usize(value.len());
        self.0.update(value.as_bytes());
        self
    }

    pub fn option_str(&mut self, value: Option<&str>) -> &mut Self {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.str(value);
        }
        self
    }

    pub fn strs<S: AsRef<str>>(&mut self, values: &[S]) -> &mut Self {
        self.usize(values.len());
        for value in values {
            self.str(value.as_ref());
        }
        self
    }

    pub fn usize(&mut self, value: usize) -> &mut Self {
        let value = u64::try_from(value).unwrap_or(u64::MAX);
        self.0.update(&value.to_le_bytes());
        self
    }

    pub fn option_usize(&mut self, value: Option<usize>) -> &mut Self {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.usize(value);
        }
        self
    }

    pub fn bool(&mut self, value: bool) -> &mut Self {
        self.0.update(&[u8::from(value)]);
        self
    }

    pub fn finish(&self) -> String {
        self.0.finalize().to_hex().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::StableHasher;

    fn hash(build: impl FnOnce(&mut StableHasher)) -> String {
        let mut hasher = StableHasher::new("test");
        build(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn missing_and_empty_values_hash_differently() {
        assert_ne!(
            hash(|hasher| {
                hasher.option_str(None);
            }),
            hash(|hasher| {
                hasher.option_str(Some(""));
            })
        );
        assert_ne!(
            hash(|hasher| {
                hasher.option_usize(None);
            }),
            hash(|hasher| {
                hasher.option_usize(Some(0));
            })
        );
    }

    #[test]
    fn separators_inside_values_do_not_collide() {
        assert_ne!(
            hash(|hasher| {
                hasher.str("a:b").str("c");
            }),
            hash(|hasher| {
                hasher.str("a").str("b:c");
            })
        );
        assert_ne!(
            hash(|hasher| {
                hasher.strs(&["a,b"]);
            }),
            hash(|hasher| {
                hasher.strs(&["a", "b"]);
            })
        );
    }

    #[test]
    fn domains_separate_equal_payloads() {
        let mut node = StableHasher::new("node");
        node.str("x");
        let mut edge = StableHasher::new("edge");
        edge.str("x");
        assert_ne!(node.finish(), edge.finish());
    }
}
