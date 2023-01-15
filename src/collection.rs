use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::index::UserConfig;

pub struct Photo {
    relative_path: PathBuf
}

/// Recursively walks the given root directory of a photo collection and returns all photos.
pub fn scan_photos(config: &UserConfig, root_dir: &Path) -> Result<Vec<Photo>> {
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
