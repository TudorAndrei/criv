//! Documentation asset limits and media types published in State.

/// Largest single asset that State publishes.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Largest sum of asset sizes that State publishes, in path order.
pub const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// The running sum of asset sizes that State publishes, in path order.
#[derive(Debug, Default)]
pub struct TotalBudget {
    used: u64,
}

impl TotalBudget {
    /// Counts `bytes` and returns true when they fit within
    /// [`MAX_TOTAL_BYTES`]; otherwise counts nothing and returns false.
    pub const fn admit(&mut self, bytes: u64) -> bool {
        let next = self.used.saturating_add(bytes);
        if next > MAX_TOTAL_BYTES {
            return false;
        }
        self.used = next;
        true
    }
}

/// The media type of a supported asset, from its exact lowercase extension.
#[must_use]
pub fn mime_for_path(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let (stem, extension) = name.rsplit_once('.')?;
    if stem.is_empty() {
        return None;
    }
    match extension {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "pdf" => Some("application/pdf"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_only_exact_lowercase_extensions_of_named_files() {
        assert_eq!(mime_for_path("docs/a.png"), Some("image/png"));
        assert_eq!(mime_for_path("docs/a.jpeg"), Some("image/jpeg"));
        assert_eq!(mime_for_path("docs/a.PNG"), None);
        assert_eq!(mime_for_path("docs/.png"), None);
        assert_eq!(mime_for_path("docs.png/readme"), None);
    }

    #[test]
    fn total_budget_admits_up_to_the_bound_and_rejects_without_counting() {
        let mut budget = TotalBudget::default();
        assert!(budget.admit(MAX_TOTAL_BYTES - 1));
        assert!(!budget.admit(2));
        assert!(budget.admit(1));
        assert!(!budget.admit(1));
    }
}
