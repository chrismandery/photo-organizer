use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

const INDEX_FILE_NAME: &str = "photo_organizer_index.json";

#[derive(Deserialize, Serialize)]
pub struct UserConfig {
    pub file_naming_scheme: String,
    pub file_types: HashMap<String, Vec<String>>
}

#[derive(Deserialize, Serialize)]
pub struct IndexEntry {
    filepath: String,
    orig_filename: String,
    filehash: String
}

#[derive(Deserialize, Serialize)]
pub struct Index {
    user_config: UserConfig,
    photos: Vec<IndexEntry>
}

impl Default for Index {
    fn default() -> Self {
        Index {
            user_config: UserConfig {
                file_naming_scheme: String::from("%Y%m%d_%H%M%S_%{type}.%{fileextension}"),  // TODO
                file_types: HashMap::from([
                    ("IMG".into(), vec!("jpg".into(), "jpeg".into(), "png".into())),
                    ("VID".into(), vec!("mp4".into()))
                ])
            },
            photos: vec!()
        }
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
    let file = File::open(&filepath).with_context(|| format!("Could not open index file at {} for reading!", filepath.display()))?;
    let reader = BufReader::new(file);
    let res = serde_json::from_reader(reader).with_context(|| format!("Could not parse index file at {}!", filepath.display()))?;
    Ok(res)
}

/// Writes index file to given root directory.
pub fn write_index_file(root_dir: &Path, index: &Index) -> Result<()> {
    // TODO: Maybe sort filelist if necessary to make index file deterministic/stable for versioning with Git
    let filepath = root_dir.join(INDEX_FILE_NAME);
    let file = File::open(&filepath).with_context(|| format!("Could not open index file at {} for writing!", filepath.display()))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, index)?;
    writer.flush()?;
    Ok(())
}
