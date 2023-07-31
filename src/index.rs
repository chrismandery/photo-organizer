use anyhow::{Context, Result};
use log::debug;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::from_utf8;

const INDEX_FILE_NAME: &str = "photo_organizer_index.json";

#[derive(Clone, Deserialize, Serialize)]
pub struct UserConfig {
    pub file_naming_scheme: String,
    pub file_types: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct IndexEntry {
    pub filepath: PathBuf,
    pub orig_filename: String,
    pub filehash: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Index {
    pub user_config: UserConfig,
    pub photos: Vec<IndexEntry>,
}

impl Default for Index {
    fn default() -> Self {
        Index {
            user_config: UserConfig {
                file_naming_scheme: String::from("%Y%m%d_%H%M%S_%{type}.%{fileextension}"),
                file_types: BTreeMap::from([
                    ("IMG".into(), vec!["jpg".into(), "jpeg".into(), "png".into()]),
                    ("VID".into(), vec!["mp4".into()]),
                ]),
            },
            photos: vec![],
        }
    }
}

/// Checks whether the index file in the given root directory is put under version control using Git. Return false if Git is not installed,
/// no Git repository has been created, the index file is not versioned (determined using git ls-files) or any other error occured.
pub fn check_index_file_is_git_versioned(root_dir: &Path) -> bool {
    let res = Command::new("git").current_dir(root_dir).args(["ls-files"]).output();

    match res {
        Ok(output) => {
            output.status.success()
                && from_utf8(&output.stdout)
                    .map(|s| s == String::from(INDEX_FILE_NAME) + "\n")
                    .unwrap_or(false)
        }
        Err(_) => false,
    }
}

/// Determines for a given directory (usually the working directory) both the path of the root directory (containing the index file) and
/// the subdirectory (portion of the path from the root to the given directory). Returns Ok(None) is the given directory is not part of a
/// photo collection.
pub fn get_index_root_and_subdir(dir: &Path) -> Result<Option<(PathBuf, PathBuf)>> {
    for cur_dir in dir.ancestors() {
        if cur_dir.join(INDEX_FILE_NAME).exists() {
            // Index file found
            let subdir = dir.strip_prefix(cur_dir)?.to_owned();
            return Ok(Some((cur_dir.to_owned(), subdir)));
        }
    }

    // No index file found
    Ok(None)
}

/// Reads the index file for given root directory.
pub fn read_index_file(root_dir: &Path) -> Result<Index> {
    let filepath = root_dir.join(INDEX_FILE_NAME);
    debug!("Reading index file at {}...", filepath.display());
    let file = File::open(&filepath)
        .with_context(|| format!("Could not open index file at {} for reading!", filepath.display()))?;
    let reader = BufReader::new(file);
    let res = serde_json::from_reader(reader)
        .with_context(|| format!("Could not parse index file at {}!", filepath.display()))?;
    Ok(res)
}

/// Writes index file to given root directory.
pub fn write_index_file(root_dir: &Path, index: &mut Index) -> Result<()> {
    // Sort index file before writing to ensure file is stable for versioning it with Git
    index.photos.sort_unstable_by_key(|e| e.filepath.clone());

    let filepath = root_dir.join(INDEX_FILE_NAME);
    let file = File::create(&filepath)
        .with_context(|| format!("Could not open index file at {} for writing!", filepath.display()))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, index)?;
    writer.flush()?;
    Ok(())
}
