use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use geo_types::Point;
use gpx::{write, Gpx, GpxVersion, Waypoint};
use html_escape::encode_safe;
use log::{debug, error, info, warn};
use rayon::prelude::*;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs::{self, read_dir, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::checks::{check_for_duplicates, check_hashes, check_photo_naming};
use crate::collection::{calc_photo_hash, get_canonical_photo_filename, get_photos_in_subdir, read_exif_data, Photo};
use crate::index::{Index, IndexEntry};

/// Runs all checks and returns whether any of the checks has generated a warning.
pub fn check(root_dir: &Path, index: &Index) -> bool {
    // Run checks without short-circuit evaluation (i.e., always run all checks)
    // TODO: Should be configurable later which checks should be run
    check_for_duplicates(index) | check_hashes(root_dir, index) | check_photo_naming(root_dir, index)
}

/// Reads a thumbnail catalogue (HTML file) and extracts the filenames of all contained photos. This function is used to avoid
/// re-generating thumbnail catalogue for directories where nothing has changed.
fn extract_entries_from_thumbcat(html_path: &Path) -> Result<Vec<String>> {
    let re = Regex::new(r"^<h1>(.+)</h1>$").unwrap();

    let f = File::open(html_path).with_context(|| format!("Could not open {} for reading!", html_path.display()))?;
    let reader = BufReader::new(&f);
    let mut entries = vec![];

    for line in reader.lines() {
        let line = line?;
        if let Some(cap) = re.captures(&line) {
            let entry = cap.get(1).unwrap().as_str().to_string();
            entries.push(entry);
        }
    }

    Ok(entries)
}

/// Show meta data from EXIF tags and the index file for image files within the current directory.
pub fn list(root_dir: &Path, subdir: &Path, index: &Index, photos: &[Photo], recursive: bool) -> Result<()> {
    let cur_photos = get_photos_in_subdir(photos, subdir, recursive);

    // Create HashMap from index for efficient lookup
    let index_map: HashMap<PathBuf, &IndexEntry> = index.photos.iter().map(|p| (p.filepath.clone(), p)).collect();

    for photo in cur_photos {
        let path = photo.relative_path;
        let rel_path = path
            .strip_prefix(subdir)
            .expect("Path not in subdir! (should never happen)");

        // Read EXIF data of photo
        let exif_str = match read_exif_data(&root_dir.join(&path)) {
            Ok(pmd) => {
                format!(
                    "{} / {} / {} / loc: {},{},{}",
                    pmd.make.as_deref().unwrap_or("<unknown make>"),
                    pmd.model.as_deref().unwrap_or("<unknown model>"),
                    pmd.timestamp_local
                        .map(|ts| ts.format("%d.%m.%Y %H:%M").to_string())
                        .as_deref()
                        .unwrap_or("unknown time"),
                    pmd.location.map(|l| format!("{:.4}", l.0)).as_deref().unwrap_or("?"),
                    pmd.location.map(|l| format!("{:.4}", l.1)).as_deref().unwrap_or("?"),
                    pmd.altitude.map(|a| a.to_string() + "m").as_deref().unwrap_or("?")
                )
            }
            Err(_) => "Could not read EXIF data".into(),
        };

        // Check original filename from index ()
        let index_str = match index_map.get(&path) {
            Some(ie) => format!("orig name: {}", ie.orig_filename),
            None => "photo not indexed!".into(),
        };

        info!("{}: {} / {}", rel_path.display(), exif_str, index_str);
    }

    Ok(())
}

/// Exports the GPS locations of the image files within the current directory in the GPX format and shows them on a map
pub fn map(root_dir: &Path, subdir: &Path, photos: &[Photo], recursive: bool, command: Option<&str>) -> Result<()> {
    let cur_photos = get_photos_in_subdir(photos, subdir, recursive);

    // Create GPX data structure for writing
    let mut gpx_data = Gpx {
        version: GpxVersion::Gpx11,
        ..Default::default()
    };

    for photo in cur_photos {
        let path = photo.relative_path;
        let full_path = &root_dir.join(&path);

        // Read EXIF data of photo
        match read_exif_data(full_path) {
            Ok(pmd) => {
                if let Some(location) = pmd.location {
                    let mut wp = Waypoint::new(Point::new(location.1, location.0));
                    wp.elevation = pmd.altitude;
                    wp.name = Some(path.to_string_lossy().to_string());
                    // TODO: Add time

                    gpx_data.waypoints.push(wp);
                } else {
                    warn!("No location found in EXIF data from {}!", path.display());
                }
            }
            Err(e) => {
                warn!("Could not read EXIF data from {}: {}", path.display(), e);
            }
        };
    }

    // Write GPX data to file
    // TODO: Use proper temporary file instead of hardcoded one
    let file = File::create("/tmp/photo_locations.gpx").context("Could not open GPX file for writing!")?;
    write(&gpx_data, &file)?;
    file.sync_all()?;
    drop(file);

    // Invoke external tool to visualize the GPX data using a map
    if let Some(command) = command {
        info!("Invoking external command {}...", command);
        Command::new(command).args(["/tmp/photo_locations.gpx"]).spawn()?;
    }

    Ok(())
}

/// Renames the files in the given directory (and potentially subdirectories) to follow the naming scheme configured in the index. Returns
/// how many files have been renamed by the function.
pub fn rename(
    root_dir: &Path,
    subdir: &Path,
    index: &Index,
    photos: &[Photo],
    recursive: bool,
    dry_run: bool,
) -> Result<usize> {
    // TODO: Maybe ask for additional confirmation? (if not in dry-run mode)
    let cur_photos = get_photos_in_subdir(photos, subdir, recursive);

    // Check for each file whether it should be renamed
    let mut renamed_photo_count = 0;
    for filepath in cur_photos.into_iter().map(|p| p.relative_path) {
        let full_old_path = root_dir.join(&filepath);

        match get_canonical_photo_filename(&full_old_path, &index.user_config) {
            Ok(canonical_name) => {
                let cur_name = filepath
                    .file_name()
                    .ok_or(anyhow!("Could not file component of photo path!"))?;
                let canonical_name = PathBuf::from(&canonical_name);

                // Rename is necessary if a photo does not already have its canonical name
                if cur_name == canonical_name {
                    debug!("{}: Rename not necessary", filepath.display());
                } else if dry_run {
                    info!(
                        "{}: Would rename file to {} (running in dry-run mode)",
                        filepath.display(),
                        canonical_name.display()
                    );
                } else {
                    info!("{}: Renaming file to {}", filepath.display(), canonical_name.display());

                    let full_new_path = root_dir
                        .join(
                            filepath
                                .parent()
                                .expect("Could not get directory component of photo path!"),
                        )
                        .join(canonical_name);

                    // Check if file already exists and refuse to overwrite already existing file
                    // Note: Since we just check before rename here, this is not free of race conditions (good enough for now though)
                    // See: https://internals.rust-lang.org/t/rename-file-without-overriding-existing-target/17637
                    if full_new_path.exists() {
                        error!("Cannot rename: Target already exists.");
                        continue;
                    }

                    fs::rename(full_old_path, full_new_path)?;
                    renamed_photo_count += 1;
                }
            }
            Err(e) => {
                warn!("{}: Could not process file - {}", filepath.display(), e);
            }
        }
    }

    Ok(renamed_photo_count)
}

/// Creates a thumbnail catalogue in a HTML file (see description of thumbcat CLI command).
pub fn thumbcat(
    root_dir: &Path,
    subdir: &Path,
    photos: &Vec<Photo>,
    output_filename: &str,
    force: bool,
    recursive: bool,
    resize_width: u32,
) -> Result<()> {
    let root_plus_sub_dir = root_dir.join(subdir);

    // When running in recursive mode, recurse into subdirectories (sorted) before processing this one
    if recursive {
        let mut recurse_subdirs: Vec<PathBuf> = read_dir(&root_plus_sub_dir)?
            .filter_map(|e| {
                let e = e.unwrap().file_name();
                if root_plus_sub_dir.join(&e).is_dir() {
                    Some(subdir.join(e))
                } else {
                    None
                }
            })
            .collect();

        recurse_subdirs.sort_unstable();

        for d in recurse_subdirs {
            thumbcat(root_dir, &d, photos, output_filename, force, recursive, resize_width)?;
        }
    }

    // Get photos in current directory
    let cur_photos = get_photos_in_subdir(photos, subdir, false);

    // Check if entries in the existing thumbnail catalogue seems up-to-date for this directory
    let html_path = root_plus_sub_dir.join(output_filename);
    if !force && html_path.is_file() {
        // Print warning if we found a thumbnail catalogue in a directory without photos
        if cur_photos.is_empty() {
            warn!(
                "Existing thumbnail catalogue found in directory without photos: {}",
                root_plus_sub_dir.display()
            );
        }

        let cur_tc_entries = extract_entries_from_thumbcat(&html_path)?;
        let tc_up_to_date = cur_photos
            .iter()
            .map(|p| {
                p.relative_path
                    .strip_prefix(subdir)
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .eq(cur_tc_entries.into_iter());
        if tc_up_to_date {
            info!(
                "Thumbnail catalogue in {} seems up-to-date, skipping directory.",
                html_path.display()
            );
            return Ok(());
        }
    }

    // Abort if the directory does not contain any photos
    if cur_photos.is_empty() {
        info!(
            "No photos found in {}, skipping directory.",
            root_plus_sub_dir.display()
        );
        return Ok(());
    }

    info!("Creating thumbnail catalogue in {}...", root_plus_sub_dir.display());

    // Generate thumbnails
    let thumbnails: Vec<_> = cur_photos
        .par_iter()
        .map(|photo| {
            (
                photo.relative_path.strip_prefix(subdir).unwrap(),
                photo.get_thumbnail(root_dir, resize_width),
            )
        })
        .collect();

    // Write HTML file
    let mut f = File::create(&html_path).with_context(|| format!("Could not write to {}!", html_path.display()))?;
    writeln!(&mut f, "<!DOCTYPE html>")?;
    writeln!(&mut f, "<html lang=\"en\">")?;
    writeln!(&mut f, "<head>")?;
    writeln!(&mut f, "<meta charset=\"utf-8\">")?;
    writeln!(
        &mut f,
        "<title>Thumbnail Catalogue for Directory {}</title>",
        encode_safe(&subdir.display().to_string())
    )?;
    writeln!(&mut f, "<style>h1 {{ font-size: large }}</style>")?;
    writeln!(&mut f, "</head>")?;
    writeln!(&mut f, "<body>")?;

    for (photo_path, data) in thumbnails {
        writeln!(&mut f, "<h1>{}</h1>", &photo_path.display())?;

        match data {
            Ok(bytes) => {
                writeln!(
                    &mut f,
                    "<p><img src=\"data:image/jpeg;base64,{}\" style=\"width: 100%\" /></p>",
                    STANDARD_NO_PAD.encode(&bytes)
                )?;
            }
            Err(e) => {
                writeln!(&mut f, "<p>{}</p>", encode_safe(&e.to_string()))?;
            }
        }
    }

    writeln!(&mut f, "</body>")?;
    writeln!(&mut f, "</html>")?;

    info!("File {} generated successfully.", html_path.display());

    Ok(())
}

/// Updates the index entries with the actual stored photos, detecting new, renamed and deleted photos. Returns whether the index has been
/// changed by the function.
pub fn update(root_dir: &Path, index: &mut Index, photos: &[Photo]) -> Result<bool> {
    // Create index data structures for faster matching of index and photos
    let index_set: HashSet<PathBuf> = index.photos.iter().map(|p| p.filepath.clone()).collect();
    let photos_set: HashSet<PathBuf> = photos.iter().map(|p| p.relative_path.clone()).collect();

    // Check for photos in the index that do no longer exist and thus have been deleted or renamed
    let deleted_photos_paths: HashSet<_> = index_set.difference(&photos_set).collect();
    let mut deleted_photos: Vec<IndexEntry> = index
        .photos
        .iter()
        .filter(|p| deleted_photos_paths.contains(&p.filepath))
        .cloned()
        .collect();
    index.photos.retain_mut(|p| !deleted_photos_paths.contains(&p.filepath));

    // Check for new photos that are not part of the index yet
    let added_photos_paths = photos_set.difference(&index_set);
    let mut new_photo_found = false;

    for added_photo in added_photos_paths {
        new_photo_found = true;

        // Hash photo
        let hash = calc_photo_hash(&root_dir.join(added_photo))?;

        let new_index_entry = if let Some(renamed_photo_index) = deleted_photos.iter().position(|p| p.filehash == hash)
        {
            // Remove entry in deleted_photos so it does not show up when we are logging all deleted photos below
            let renamed_photo = deleted_photos.swap_remove(renamed_photo_index);

            info!(
                "Renamed: {} -> {}",
                renamed_photo.filepath.display(),
                added_photo.display()
            );

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
                filehash: hash,
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
