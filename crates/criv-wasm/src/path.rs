//! Repository-relative path safety shared by the Source and asset projections.

/// The normalized repository-relative form of `path`, or `None` when the path
/// is empty, absolute, drive-qualified, contains NUL, or escapes the root.
pub fn safe_relative_path(path: &str) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\0')
        || path.as_bytes().get(1) == Some(&b':')
    {
        return None;
    }
    let mut normalized = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            segment => normalized.push(segment),
        }
    }
    (!normalized.is_empty()).then(|| normalized.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_shared_editor_fixture() {
        let fixture = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../fixtures/editor/safe-vault-path.v1.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            assert_eq!(
                safe_relative_path(input).as_deref(),
                case["expected"].as_str(),
                "{input:?}"
            );
        }
    }
}
