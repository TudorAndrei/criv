//! Paths of the generated `.criv/` directory, relative to the vault root.

pub const DIR: &str = ".criv";
/// The published State document and its commit record.
pub const STATE: &str = ".criv/state.json";
/// The hash of the newest published snapshot.
pub const LATEST: &str = ".criv/latest";
pub const SNAPSHOTS: &str = ".criv/snapshots";
pub const SNAPSHOT_INDEX: &str = ".criv/snapshots/index.json";
pub const SOURCE_GRAPH: &str = ".criv/source-graph.json";
pub const WATCH_LOCK: &str = ".criv/watch.lock";
pub const PUBLICATION_LOCK: &str = ".criv/state-publication.lock";
pub const PUBLICATION_TRANSACTION: &str = ".criv/state-transaction.json";
pub const QUARANTINE: &str = ".criv/state-quarantine";
pub const ADR_RECEIPT: &str = ".criv/adr-reconcile.json";
pub const SOURCE_RECEIPT: &str = ".criv/source-reconcile.json";

#[must_use]
pub fn snapshot(hash: &str) -> String {
    format!("{SNAPSHOTS}/{hash}.json")
}

#[must_use]
pub fn quarantined_snapshot(hash: &str) -> String {
    format!("{QUARANTINE}/{hash}.json")
}

#[cfg(test)]
mod tests {
    #[test]
    fn state_path_matches_the_editor_fixture() {
        let paths: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/editor/vault-paths.v1.json"))
                .unwrap();
        assert_eq!(paths["state"], super::STATE);
    }
}
