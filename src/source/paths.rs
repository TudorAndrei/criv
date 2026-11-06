use std::path::Path;

use crate::Result;
use crate::repository::RepositoryFiles;

pub fn read_source_to_string_from(files: &RepositoryFiles, source_path: &str) -> Result<String> {
    let contents = files.read(Path::new(source_path))?;
    Ok(String::from_utf8_lossy(&contents).into_owned())
}
