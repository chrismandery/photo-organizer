use anyhow::{Context, Result};
use hex::encode;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::copy;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::index::UserConfig;

pub struct Photo {
    relative_path: PathBuf
}

/// Hashes the given file and returns the hash as a hex-encoded string.
pub fn calc_photo_hash(filepath: &PathBuf) -> Result<String> {
    let mut file = File::open(&filepath).with_context(|| format!("Could not open {} for hashing!", filepath.display()))?;
    let mut hasher = Sha256::new();
    copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    Ok(encode(&hash))
}

/// Recursively walks the given root directory of a photo collection and returns all photos.
pub fn scan_photo_collection(config: &UserConfig, root_dir: &Path) -> Result<Vec<Photo>> {
    let filter_file_extensions: Vec<String> = config.file_types.values().flatten().cloned().collect();
    let mut res = vec!();

    for entry in WalkDir::new(root_dir) {
        let entry = entry.with_context(|| format!("Could not traverse directory structure below {}!", root_dir.display()))?;

        // Check if file has one of the file types that should be considered
        if entry.file_type().is_file() {
            let path = entry.path();
            if let Some(extension) = path.extension() {
                if filter_file_extensions.contains(&extension.to_string_lossy().to_string()) {
                    res.push(Photo {
                        relative_path: path.strip_prefix(root_dir)?.to_owned()
                    });
                }
            }
        }
    }

    Ok(res)
}
