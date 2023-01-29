use anyhow::{Context, Result};
use chrono::prelude::*;
use hex::encode;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, copy};
use std::path::{Path, PathBuf};
use std::str::from_utf8;
use walkdir::WalkDir;

use crate::index::UserConfig;

#[derive(Clone)]
pub struct Photo {
    pub relative_path: PathBuf
}

/// Holds photo meta data that are extracted from the EXIF data. This struct contains only the subset of the EXIF data that is used within
/// this project right now.
pub struct PhotoMetaData {
    make: Option<String>,
    model: Option<String>,
    timestamp_local: Option<NaiveDateTime>
}

/// Hashes the given file and returns the hash as a hex-encoded string.
pub fn calc_photo_hash(filepath: &PathBuf) -> Result<String> {
    let mut file = File::open(&filepath).with_context(|| format!("Could not open {} for hashing!", filepath.display()))?;
    let mut hasher = Sha256::new();
    copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    Ok(encode(&hash))
}

/// Determines the "correct" filename for a given photo, using the provided user config with its file naming scheme.
pub fn get_canonical_photo_filename(filepath: &PathBuf, user_config: &UserConfig) -> Result<String> {
    let exif_data = read_exif_data(filepath)?;

    Ok("...".into())  // TODO
}

fn read_exif_data(filepath: &PathBuf) -> Result<PhotoMetaData> {
    let file = File::open(&filepath)
        .with_context(|| format!("Could not open {} for reading EXIF data!", filepath.display()))?;
    let mut bufreader = BufReader::new(&file);
    let exifreader = exif::Reader::new();
    let exif = exifreader.read_from_container(&mut bufreader)
        .with_context(|| format!("Could not read EXIF data from {}!", filepath.display()))?;

    // Print all EXIF fields for debugging
    /* for f in exif.fields() {
        println!("{} {} {}", f.tag, f.ifd_num, f.display_value().with_unit(&exif));
    } */

    // Note: The timestamp is stored as a string with the format "2022:05:07 12:32:10"
    // Unfortunately, the standard does not mandate whether this timestamp is stored in local time or in UTC. For now, we are just
    // assuming it is already in local time, which seems to be true at least for Android mobile phones. Later, this can be revisited.
    let timestamp_value = exif.get_field(exif::Tag::DateTime, exif::In::PRIMARY).map(|e| &e.value);
    let timestamp = if let Some(exif::Value::Ascii(s)) = timestamp_value {
        let s = s.first().context("EXIF DateTime has no entry!")?;
        let s = from_utf8(s).context("Could not parse EXIF DateTime value as utf8!")?;
        let ts = NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").
            with_context(|| format!("Could not parse EXIF DateTime value: {}", s))?;
        Some(ts)
    } else {
        None
    };

    let model_value = exif.get_field(exif::Tag::Model, exif::In::PRIMARY).map(|e| &e.value);
    let model = if let Some(exif::Value::Ascii(s)) = model_value {
        let s = s.first().context("EXIF model name has no entry!")?;
        Some(from_utf8(s).context("Could not parse EXIF model name as utf8!")?.to_string())
    } else {
        None
    };

    let make_value = exif.get_field(exif::Tag::Model, exif::In::PRIMARY).map(|e| &e.value);
    let make = if let Some(exif::Value::Ascii(s)) = make_value {
        let s = s.first().context("EXIF make name has no entry!")?;
        Some(from_utf8(s).context("Could not parse EXIF make name as utf8!")?.to_string())
    } else {
        None
    };

    Ok(PhotoMetaData {
        model: model,
        make: make,
        timestamp_local: timestamp
    })
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
