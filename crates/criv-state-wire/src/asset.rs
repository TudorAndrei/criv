//! Documentation asset limits and media types published in State.

/// Largest single asset that State publishes.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Largest sum of asset sizes that State publishes, in path order.
pub const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

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
}
