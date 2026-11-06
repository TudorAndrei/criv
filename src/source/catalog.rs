use std::sync::Arc;

/// The selected Source paths in lexical order, for partial path lookup.
#[derive(Debug, Clone)]
pub(super) struct SourceCatalog {
    paths: Arc<[String]>,
}

impl SourceCatalog {
    pub(super) fn new(paths: Vec<String>) -> Self {
        Self {
            paths: paths.into(),
        }
    }

    pub(super) fn paths(&self) -> &[String] {
        &self.paths
    }

    pub(super) fn resolve_partial_path(&self, query: &str) -> Option<(String, bool)> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        if self
            .paths
            .binary_search_by(|path| path.as_str().cmp(query))
            .is_ok()
        {
            return Some((query.to_string(), false));
        }

        let matches = self
            .paths
            .iter()
            .filter(|path| {
                path.strip_suffix(query)
                    .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('/'))
                    || path.rsplit('/').next() == Some(query)
            })
            .cloned()
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => None,
            [only] => Some((only.clone(), false)),
            [first, ..] => Some((first.clone(), true)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_uses_exact_suffix_basename_and_stable_ambiguity() {
        let catalog = SourceCatalog::new(vec!["src/lib.rs".into(), "src/nested/lib.rs".into()]);

        assert_eq!(
            catalog.resolve_partial_path("src/lib.rs"),
            Some(("src/lib.rs".into(), false))
        );
        assert_eq!(
            catalog.resolve_partial_path("nested/lib.rs"),
            Some(("src/nested/lib.rs".into(), false))
        );
        assert_eq!(
            catalog.resolve_partial_path("lib.rs"),
            Some(("src/lib.rs".into(), true))
        );
    }
}
