use anyhow::{anyhow, bail, Context, Result};
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
    pub make: Option<String>,
    pub model: Option<String>,
    pub timestamp_local: Option<NaiveDateTime>,
    pub location: Option<(f64, f64)>,
    pub altitude: Option<f64>
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

    match exif_data.timestamp_local {
        Some(timestamp_local) => {
            match filepath.extension() {
                Some(file_extension) => {
                    let file_extension = file_extension.to_string_lossy().to_lowercase();

                    // Replace file type identifier in the name
                    let (file_type_name, _) = user_config.file_types
                        .iter()
                        .find(|(_, allowed_file_extensions)| allowed_file_extensions.contains(&file_extension))
                        .ok_or_else(|| anyhow!("File extension not defined in configuration."))?;
                    let cur_name = user_config.file_naming_scheme.replace("%{type}", file_type_name);

                    // Replace file extension in template: Lower case existing file extension (which a hardcoded special rule to rewrite
                    // "jpeg" to "jpg" though)
                    let cur_name = cur_name.replace("%{fileextension}", if file_extension == "jpeg" { "jpg" } else { &file_extension });

                    // Replace datetime fields with timestamp
                    Ok(timestamp_local.format(&cur_name).to_string())
                },
                None => {
                    bail!("Could not determine file extension.")
                }
            }
        },
        None => {
            bail!("EXIF timestamp not set.");
        }
    }
}

/// Get all photos that are in a specific subdirectory (and possibly its subdirectories).
pub fn get_photos_in_subdir(photos: &[Photo], subdir: &Path, recursive: bool) -> Vec<Photo> {
    photos
        .iter()
        .filter(|photo| {
            if recursive {
                photo.relative_path.starts_with(subdir)
            } else {
                photo.relative_path.parent().map(|d| d == subdir).unwrap_or(false)
            }
        })
        .cloned()
        .collect()
}

pub fn read_exif_data(filepath: &PathBuf) -> Result<PhotoMetaData> {
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

    let make_value = exif.get_field(exif::Tag::Make, exif::In::PRIMARY).map(|e| &e.value);
    let make = if let Some(exif::Value::Ascii(s)) = make_value {
        let s = s.first().context("EXIF make name has no entry!")?;
        Some(from_utf8(s).context("Could not parse EXIF make name as utf8!")?.to_string())
    } else {
        None
    };

    let latitude_value = exif.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY).map(|e| &e.value);
    let longitude_value = exif.get_field(exif::Tag::GPSLongitude, exif::In::PRIMARY).map(|e| &e.value);

    // {Latitude,Longitude}Ref specificy North/South resp. East/West
    let latitude_ref_value = exif.get_field(exif::Tag::GPSLatitudeRef, exif::In::PRIMARY).map(|e| &e.value);
    let longitude_ref_value = exif.get_field(exif::Tag::GPSLongitudeRef, exif::In::PRIMARY).map(|e| &e.value);

    let location = if let (
        Some(exif::Value::Rational(lat_vec)),
        Some(exif::Value::Rational(long_vec)),
        Some(exif::Value::Ascii(lat_ref_vec)),
        Some(exif::Value::Ascii(long_ref_vec))
        ) = (latitude_value, longitude_value, latitude_ref_value, longitude_ref_value) {
        let mut it = lat_vec.iter();
        let lat_degrees = it.next().context("Could not pop degrees from EXIF GPSLatitude!")?;
        let lat_minutes = it.next().context("Could not pop minutes from EXIF GPSLatitude!")?;
        let lat_seconds = it.next().context("Could not pop seconds from EXIF GPSLatitude!")?;

        let mut it = long_vec.iter();
        let long_degrees = it.next().context("Could not pop degrees from EXIF GPSLongitude!")?;
        let long_minutes = it.next().context("Could not pop minutes from EXIF GPSLongitude!")?;
        let long_seconds = it.next().context("Could not pop seconds from EXIF GPSLongitude!")?;

        let lat_ref = lat_ref_vec.first().context("EXIF GPSLatitudeRef has no entry!")?;
        let lat_ref = from_utf8(lat_ref).context("Could not parse EXIF GPSLatitudeRef value as utf8!")?;

        let long_ref = long_ref_vec.first().context("EXIF GPSLongitudeRef has no entry!")?;
        let long_ref = from_utf8(long_ref).context("Could not parse EXIF GPSLongitudeRef value as utf8!")?;

        // Calculate decimal latitude/longitude values from degrees/minutes/seconds
        let lat = (lat_degrees.to_f64() + lat_minutes.to_f64() / 60.0 + lat_seconds.to_f64() / 3600.0) * match lat_ref {
            "N" => { 1.0 },
            "S" => { -1.0 },
            _ => { bail!("GPSLatitudeRef was \"{}\", expected \"N\" or \"S\"!", lat_ref); }
        };
        let long = (long_degrees.to_f64() + long_minutes.to_f64() / 60.0 + long_seconds.to_f64() / 3600.0) * match long_ref {
            "E" => { 1.0 },
            "W" => { -1.0 },
            _ => { bail!("GPSLongitudeRef was \"{}\", expected \"E\" or \"W\"!", long_ref); }
        };

        Some((lat, long))
    } else {
        None
    };

    let altitude_value = exif.get_field(exif::Tag::GPSAltitude, exif::In::PRIMARY).map(|e| &e.value);
    let altitude = if let Some(exif::Value::Rational(v)) = altitude_value {
        let v = v.first().context("EXIF GPSAltitude has no entry!")?;
        Some(v.to_f64())
    } else {
        None
    };

    Ok(PhotoMetaData {
        model: model,
        make: make,
        timestamp_local: timestamp,
        location: location,
        altitude: altitude
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
                if filter_file_extensions.contains(&extension.to_string_lossy().to_lowercase()) {
                    res.push(Photo {
                        relative_path: path.strip_prefix(root_dir)?.to_owned()
                    });
                }
            }
        }
    }

    // Sort photo list by path to declutter the output of various commands that work with this list in its order (e.g., rename and list)
    res.sort_unstable_by_key(|e| e.relative_path.clone());

    Ok(res)
}
