use anyhow::{anyhow, Result};
use std::collections::HashSet;
use log::{debug, info, warn};
use std::fs;
use std::path::{Path, PathBuf};

use crate::checks::{check_for_duplicates, check_hashes, check_photo_naming};
use crate::collection::{calc_photo_hash, Photo, get_canonical_photo_filename};
use crate::index::{Index, IndexEntry};

/// Runs all checks and returns whether any of the checks has generated a warning.
pub fn check(root_dir: &Path, index: &Index) -> bool {
    // Run all checks (TODO: should be configurable later)
    check_for_duplicates(&index) ||
        check_hashes(root_dir, &index) ||
        check_photo_naming(root_dir, &index)
}

/// Renames the files in the given directory (and potentially subdirectories) to follow the naming scheme configured in the index. Returns
/// how many files have been renamed by the function.
pub fn rename(root_dir: &Path, subdir: &Path, index: &Index, photos: &Vec<Photo>, recursive: bool, dry_run: bool) -> Result<usize> {
    // TODO: Maybe ask for additional confirmation? (if not in dry-run mode)

    // Get all files that should by renamed
    let filepaths: Vec<PathBuf> = photos
        .into_iter()
        .filter(|photo| {
            if recursive {
                photo.relative_path.starts_with(subdir)
            } else {
                photo.relative_path.parent().map(|d| d == subdir).unwrap_or(false)
            }
        })
        .map(|photo| photo.relative_path.clone())
        .collect();

    // Check for each file whether it should be renamed
    let mut renamed_photo_count = 0;
    for filepath in filepaths {
        let full_old_path = root_dir.join(&filepath);

        match get_canonical_photo_filename(&full_old_path, &index.user_config) {
            Ok(canonical_name) => {
                let cur_name = filepath.file_name().ok_or(anyhow!("Could not file component of photo path!"))?;
                let canonical_name = PathBuf::from(&canonical_name);

                // Rename is necessary if a photo does not already have its canonical name
                if cur_name == canonical_name {
                    debug!("{}: Rename not necessary", filepath.display());
                } else {
                    if dry_run {
                        info!("{}: Would rename file to {} (running in dry-run mode)", filepath.display(), canonical_name.display());
                    } else {
                        info!("{}: Renaming file to {}", filepath.display(), canonical_name.display());

                        let full_new_path = root_dir
                            .join(filepath.parent().expect("Could not get directory component of photo path!"))
                            .join(canonical_name);

                        fs::rename(full_old_path, full_new_path)?;
                        renamed_photo_count += 1;
                    }
                }
            },
            Err(e) => {
                warn!("{}: Could not process file - {}", filepath.display(), e);
            }
        }
    }

    Ok(renamed_photo_count)
}

/// Updates the index entries with the actual stored photos, detecting new, renamed and deleted photos. Returns whether the index has been
/// changed by the function.
pub fn update(root_dir: &Path, index: &mut Index, photos: &Vec<Photo>) -> Result<bool> {
    // Create index data structures for faster matching of index and photos
    let index_set: HashSet<PathBuf> = index.photos.iter().map(|p| p.filepath.clone()).collect();
    let photos_set: HashSet<PathBuf> = photos.iter().map(|p| p.relative_path.clone()).collect();

    // Check for photos in the index that do no longer exist and thus have been deleted or renamed
    let deleted_photos_paths: HashSet<_> = index_set.difference(&photos_set).collect();
    let mut deleted_photos: Vec<IndexEntry> = index.photos.iter().filter(|p| deleted_photos_paths.contains(&p.filepath)).cloned().collect();
    index.photos.retain_mut(|p| !deleted_photos_paths.contains(&p.filepath));

    // Check for new photos that are not part of the index yet
    let added_photos_paths = photos_set.difference(&index_set);
    let mut new_photo_found = false;

    for added_photo in added_photos_paths {
        new_photo_found = true;

        // Hash photo
        let hash = calc_photo_hash(&root_dir.join(added_photo))?;

        let new_index_entry = if let Some(renamed_photo_index) = deleted_photos.iter().position(|p| p.filehash == hash) {
            // Remove entry in deleted_photos so it does not show up when we are logging all deleted photos below
            let renamed_photo = deleted_photos.swap_remove(renamed_photo_index);

            info!("Renamed: {} -> {}", renamed_photo.filepath.display(), added_photo.display());

            // Hash matches one of the deleted photos (this photo was just renamed)
            let mut new_entry = renamed_photo.clone();
            new_entry.filepath = added_photo.clone();
            new_entry
        } else {
            info!("Added: {}", added_photo.display());

            // Hash not found in the deleted photos (this photo is new)
            IndexEntry {
                filepath: added_photo.clone(),
                orig_filename: added_photo.file_name().unwrap_or_default().to_string_lossy().into(),
                filehash: hash
            }
        };

        index.photos.push(new_index_entry);
    }

    // Log deleted photos (note: apparent deletions that correspond to renamed files have already been removed from the vec)
    for dp in deleted_photos.iter() {
        info!("Deleted: {}", dp.filepath.display());
    }

    Ok(new_photo_found || !deleted_photos.is_empty())
}
