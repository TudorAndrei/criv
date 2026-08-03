use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "criv.performance-workload.v1";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkloadManifest {
    pub schema: String,
    pub id: String,
    pub tier: String,
    pub observed_repository: String,
    pub observed_revision: String,
    pub observed_date: String,
    pub notes: usize,
    pub decisions: usize,
    pub docs: usize,
    pub source_files: usize,
    pub source_bytes: usize,
    pub symbols: usize,
    pub note_links: usize,
    pub source_references: usize,
    pub policies: usize,
    pub c4_artifacts: usize,
    pub changed_source_files: usize,
    pub changed_file_fraction: f64,
    pub extensions: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct LoadedManifest {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub digest: String,
    pub manifest: WorkloadManifest,
}

impl LoadedManifest {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path)
            .map_err(|error| format!("failed to read manifest {}: {error}", path.display()))?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|error| format!("manifest {} is not UTF-8: {error}", path.display()))?;
        let manifest = toml::from_str::<WorkloadManifest>(text)
            .map_err(|error| format!("failed to parse manifest {}: {error}", path.display()))?;
        manifest.validate(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            digest: blake3::hash(&bytes).to_hex().to_string(),
            bytes,
            manifest,
        })
    }
}

impl WorkloadManifest {
    fn validate(&self, path: &Path) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!(
                "manifest {} has schema {}, expected {SCHEMA}",
                path.display(),
                self.schema
            ));
        }
        if self.id.trim().is_empty() || self.tier.trim().is_empty() {
            return Err(format!(
                "manifest {} id and tier must not be empty",
                path.display()
            ));
        }
        if self.notes != self.decisions + self.docs {
            return Err(format!(
                "manifest {} notes must equal decisions + docs",
                path.display()
            ));
        }
        let extension_files = self.extensions.values().sum::<usize>();
        if extension_files != self.source_files {
            return Err(format!(
                "manifest {} extension counts total {extension_files}, expected {} source files",
                path.display(),
                self.source_files
            ));
        }
        if self.source_files == 0
            || self.source_bytes == 0
            || self.changed_source_files == 0
            || self.changed_source_files > self.source_files
        {
            return Err(format!(
                "manifest {} must have positive source data and a valid changed source count",
                path.display()
            ));
        }
        let fraction = self.changed_source_files as f64 / self.source_files as f64;
        if (fraction - self.changed_file_fraction).abs() > 1e-12 {
            return Err(format!(
                "manifest {} changed_file_fraction does not match changed_source_files/source_files",
                path.display()
            ));
        }
        if self.policies > self.decisions || self.source_references > self.source_files {
            return Err(format!(
                "manifest {} cannot place its policies or unique source references",
                path.display()
            ));
        }
        if !self.extensions.contains_key("rs") {
            return Err(format!(
                "manifest {} needs at least one Rust source for deterministic symbols and policies",
                path.display()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_manifests_are_internally_consistent() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for name in ["barrs-small.toml", "criv-medium.toml"] {
            LoadedManifest::load(&root.join("fixtures/performance").join(name)).unwrap();
        }
    }
}
