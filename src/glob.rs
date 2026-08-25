use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::{CrivError, Result};

#[derive(Debug, Clone)]
pub struct GlobMatcher {
    sets: Vec<(GlobSet, Vec<usize>)>,
}

impl GlobMatcher {
    pub(crate) fn new(patterns: &[String]) -> Result<Self> {
        Self::from_patterns(patterns, (0..patterns.len()).collect())
    }

    /// Compiles every valid pattern and preserves its original index. This is
    /// for legacy matching paths where an invalid glob has always meant
    /// "does not match", rather than a validation error.
    pub(crate) fn from_valid_patterns(patterns: &[String]) -> Self {
        let mut valid = Vec::new();
        for (index, pattern) in patterns.iter().enumerate() {
            if GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(true)
                .build()
                .is_ok()
            {
                valid.push((index, pattern.clone()));
            }
        }
        match Self::from_patterns(
            &valid
                .iter()
                .map(|(_, pattern)| pattern.clone())
                .collect::<Vec<_>>(),
            valid.iter().map(|(index, _)| *index).collect(),
        ) {
            Ok(matcher) => matcher,
            // A valid aggregate can exceed globset's automaton limit. Keep the
            // tolerant contract by compiling each valid pattern independently.
            Err(_) => Self {
                sets: valid
                    .iter()
                    .filter_map(|(index, pattern)| {
                        Self::from_patterns(std::slice::from_ref(pattern), vec![*index]).ok()
                    })
                    .flat_map(|matcher| matcher.sets)
                    .collect(),
            },
        }
    }

    fn from_patterns(patterns: &[String], pattern_indices: Vec<usize>) -> Result<Self> {
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            builder.add(
                GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .backslash_escape(true)
                    .build()
                    .map_err(|err| CrivError::new(format!("invalid glob `{pattern}`: {err}")))?,
            );
        }
        Ok(Self {
            sets: vec![(
                builder
                    .build()
                    .map_err(|err| CrivError::new(format!("failed to compile globs: {err}")))?,
                pattern_indices,
            )],
        })
    }

    pub(crate) fn is_match(&self, value: &str) -> bool {
        self.sets.iter().any(|(set, _)| set.is_match(value))
    }

    pub(crate) fn matching_pattern_indices_into(&self, value: &str, into: &mut Vec<usize>) {
        into.clear();
        let mut matched = Vec::new();
        for (set, pattern_indices) in &self.sets {
            // globset clears `matched` before every call, so it is safe to
            // reuse this scratch allocation while accumulating all sets.
            set.matches_into(value, &mut matched);
            into.extend(matched.iter().map(|index| pattern_indices[*index]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob_matches(pattern: &str, value: &str) -> bool {
        let patterns = [pattern.to_string()];
        GlobMatcher::new(&patterns).is_ok_and(|matcher| matcher.is_match(value))
    }

    #[test]
    fn simple_globs_match_repo_paths() {
        assert!(glob_matches("src/**", "src/auth/verify.rs"));
        assert!(glob_matches("src/*.rs", "src/lib.rs"));
        assert!(!glob_matches("src/*.rs", "src/auth/lib.rs"));
    }
}
