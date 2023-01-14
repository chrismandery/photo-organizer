use anyhow::Result;
use std::path::{Path, PathBuf};

const INDEX_FILE_NAME: &str = "photo_organizer_index.csv";

pub struct IndexEntry {
    filepath: String,
    orig_filename: String,
    filehash: String  // TODO: Change type
}

pub type Index = Vec<IndexEntry>;

/// Determines for a given directory (usually the working directory) both the path of the root directory (containing the index file) and
/// the subdirectory (portion of the path from the root to the given directory). Returns Ok(None) is the given directory is not part of a
/// photo collection.
pub fn get_index_root_and_subdir(dir: &Path) -> Result<Option<(PathBuf, PathBuf)>> {
    todo!();
}

/// Reads the index file for given root directory.
pub fn read_index_file(root_dir: &Path) -> Result<Index> {
    todo!();
}

/// Writes index file to given root directory.
pub fn write_index_file(root_dir: &Path, index: &Index) -> Result<()> {
    todo!();
}
