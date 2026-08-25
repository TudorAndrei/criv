use std::path::Path;

use crate::Result;
use crate::repository::RepositoryFiles;

pub fn read_source_to_string_from(files: &RepositoryFiles, source_path: &str) -> Result<String> {
    let contents = read_source_bytes(files, source_path)?;
    Ok(String::from_utf8_lossy(&contents).into_owned())
}

pub(super) fn read_source_bytes(files: &RepositoryFiles, source_path: &str) -> Result<Vec<u8>> {
    files.read(Path::new(source_path))
}
