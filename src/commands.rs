use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use log::{debug, info, warn};
use std::fs;
use std::path::{Path, PathBuf};

use crate::checks::{check_for_duplicates, check_hashes, check_photo_naming};
use crate::collection::{calc_photo_hash, Photo, get_canonical_photo_filename, get_photos_in_subdir, read_exif_data};
use crate::index::{Index, IndexEntry};

/// Runs all checks and returns whether any of the checks has generated a warning.
pub fn check(root_dir: &Path, index: &Index) -> bool {
    // Run all checks (TODO: should be configurable later)
    check_for_duplicates(&index) ||
        check_hashes(root_dir, &index) ||
        check_photo_naming(root_dir, &index)
}

/// Show meta data from EXIF tags and the index file for image files within the current directory.
pub fn list(root_dir: &Path, subdir: &Path, index: &Index, photos: &Vec<Photo>, recursive: bool) -> Result<()> {
    let cur_photos = get_photos_in_subdir(photos, subdir, recursive);

    // Create HashMap from index for efficient lookup
    let index_map: HashMap<PathBuf, &IndexEntry> = index.photos.iter().map(|p| (p.filepath.clone(), p)).collect();

    for photo in cur_photos {
        let path = photo.relative_path;
        let rel_path = path.strip_prefix(subdir).expect("Path not in subdir! (should never happen)");

        // Read EXIF data of photo
        let exif_str = match read_exif_data(&root_dir.join(&path)) {
            Ok(pmd) => {
                format!("{} / {} / {} / loc: {},{},{}",
                    pmd.make.as_deref().unwrap_or("<unknown make>"),
                    pmd.model.as_deref().unwrap_or("<unknown model>"),
                    pmd.timestamp_local.map(|ts| ts.format("%d.%m.%Y %H:%M").to_string()).as_deref().unwrap_or("unknown time"),
                    pmd.location.map(|l| format!("{:.4}", l.0)).as_deref().unwrap_or("?"),
                    pmd.location.map(|l| format!("{:.4}", l.1)).as_deref().unwrap_or("?"),
                    pmd.altitude.map(|a| a.to_string() + "m").as_deref().unwrap_or("?"))
            },
            Err(_) => "Could not read EXIF data".into()
        };

        // Check original filename from index ()
        let index_str = match index_map.get(&path) {
            Some(ie) => format!("orig name: {}", ie.orig_filename),
            None => "photo not indexed!".into()
        };

        info!("{}: {} / {}", rel_path.display(), exif_str, index_str);
    }

    Ok(())
}

/// Renames the files in the given directory (and potentially subdirectories) to follow the naming scheme configured in the index. Returns
/// how many files have been renamed by the function.
pub fn rename(root_dir: &Path, subdir: &Path, index: &Index, photos: &Vec<Photo>, recursive: bool, dry_run: bool) -> Result<usize> {
    // TODO: Maybe ask for additional confirmation? (if not in dry-run mode)
    let cur_photos = get_photos_in_subdir(photos, subdir, recursive);

    // Check for each file whether it should be renamed
    let mut renamed_photo_count = 0;
    for filepath in cur_photos.into_iter().map(|p| p.relative_path) {
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
