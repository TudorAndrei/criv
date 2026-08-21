//! Documentation asset projection and validation.

use std::collections::BTreeSet;

use criv_state_wire::AssetIndexEntry;

use super::{EditorAssetEntry, source::safe_source_path};

const MAX_ASSET_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ASSET_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

pub(super) fn take_assets(entries: Vec<AssetIndexEntry>) -> Vec<EditorAssetEntry> {
    let mut candidates = entries
        .into_iter()
        .filter_map(|entry| {
            let path = safe_source_path(&entry.path)?;
            valid_asset(&entry, &path).then_some(EditorAssetEntry {
                path,
                mime: entry.mime,
                bytes: entry.bytes,
                hash: entry.hash,
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.path.cmp(&right.path));

    let mut assets = Vec::new();
    let mut seen = BTreeSet::new();
    let mut total_bytes = 0_u64;
    for entry in candidates {
        if !seen.insert(entry.path.clone()) {
            continue;
        }
        let next = total_bytes.saturating_add(entry.bytes);
        if next > MAX_ASSET_TOTAL_BYTES {
            break;
        }
        total_bytes = next;
        assets.push(entry);
    }
    assets
}

fn valid_asset(entry: &AssetIndexEntry, path: &str) -> bool {
    entry.bytes <= MAX_ASSET_BYTES
        && entry.hash.len() == 64
        && entry.hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        && expected_mime(path) == Some(entry.mime.as_str())
}

fn expected_mime(path: &str) -> Option<&'static str> {
    match path.rsplit_once('.')?.1 {
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

    fn entry(path: &str, mime: &str, bytes: u64) -> AssetIndexEntry {
        AssetIndexEntry {
            path: path.into(),
            mime: mime.into(),
            bytes,
            hash: "a".repeat(64),
        }
    }

    #[test]
    fn keeps_safe_verified_assets_in_lexical_order() {
        let assets = take_assets(vec![
            entry("docs/z.pdf", "application/pdf", 4),
            entry("../secret.png", "image/png", 4),
            entry("docs/a.png", "image/png", 3),
            entry("docs/a.png", "image/png", 3),
            entry("docs/wrong.png", "image/jpeg", 3),
            entry("docs/upper.PNG", "image/png", 3),
            entry("docs/too-large.png", "image/png", MAX_ASSET_BYTES + 1),
            AssetIndexEntry {
                hash: "bad".into(),
                ..entry("docs/bad-hash.png", "image/png", 3)
            },
        ]);

        assert_eq!(
            assets
                .iter()
                .map(|asset| asset.path.as_str())
                .collect::<Vec<_>>(),
            ["docs/a.png", "docs/z.pdf"]
        );
    }

    #[test]
    fn keeps_the_total_size_bound_in_lexical_order() {
        let mut entries = (0..7)
            .map(|index| entry(&format!("docs/{index}.png"), "image/png", MAX_ASSET_BYTES))
            .collect::<Vec<_>>();
        entries.push(entry("docs/7.png", "image/png", MAX_ASSET_BYTES / 2));
        entries.push(entry(
            "docs/overflow.pdf",
            "application/pdf",
            MAX_ASSET_BYTES,
        ));
        entries.push(entry(
            "docs/z-after-overflow.png",
            "image/png",
            MAX_ASSET_BYTES / 2,
        ));
        let assets = take_assets(entries);

        assert_eq!(assets.len(), 8);
        assert_eq!(assets[0].path, "docs/0.png");
        assert_eq!(assets[7].path, "docs/7.png");
    }
}
