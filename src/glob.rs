use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};

use crate::{CrivError, Result};

#[derive(Debug, Clone)]
pub struct GlobMatcher {
    sets: Vec<(GlobSet, Vec<usize>)>,
}

/// The globs of a list, each compiled or with its syntax error, by index.
pub struct CompiledGlobs {
    pub(crate) matcher: GlobMatcher,
    pub(crate) errors: Vec<Option<String>>,
}

fn glob(pattern: &str) -> std::result::Result<Glob, globset::Error> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(true)
        .build()
}

impl GlobMatcher {
    pub(crate) fn new(patterns: &[String]) -> Result<Self> {
        let globs = patterns
            .iter()
            .map(|pattern| {
                glob(pattern)
                    .map_err(|err| CrivError::new(format!("invalid glob `{pattern}`: {err}")))
            })
            .collect::<Result<Vec<_>>>()?;
        Self::from_globs(globs, (0..patterns.len()).collect())
            .map_err(|err| CrivError::new(format!("failed to compile globs: {err}")))
    }

    /// Compiles every valid pattern, keeping its index, and reports the
    /// syntax error of each invalid one.
    pub(crate) fn compile_each(patterns: &[String]) -> CompiledGlobs {
        let mut errors = Vec::with_capacity(patterns.len());
        let mut valid = Vec::new();
        for (index, pattern) in patterns.iter().enumerate() {
            match glob(pattern) {
                Ok(glob) => {
                    valid.push((index, glob));
                    errors.push(None);
                }
                Err(err) => errors.push(Some(err.kind().to_string())),
            }
        }
        let (indices, globs): (Vec<_>, Vec<_>) = valid.iter().cloned().unzip();
        // A valid aggregate can exceed globset's automaton limit; then each
        // glob gets its own set.
        let matcher = Self::from_globs(globs, indices).unwrap_or_else(|_| Self {
            sets: valid
                .into_iter()
                .filter_map(|(index, glob)| Self::from_globs(vec![glob], vec![index]).ok())
                .flat_map(|matcher| matcher.sets)
                .collect(),
        });
        CompiledGlobs { matcher, errors }
    }

    fn from_globs(
        globs: Vec<Glob>,
        pattern_indices: Vec<usize>,
    ) -> std::result::Result<Self, globset::Error> {
        let mut builder = GlobSetBuilder::new();
        for glob in globs {
            builder.add(glob);
        }
        Ok(Self {
            sets: vec![(builder.build()?, pattern_indices)],
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
            into.extend(
                matched
                    .iter()
                    .filter_map(|index| pattern_indices.get(*index).copied()),
            );
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
