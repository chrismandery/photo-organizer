use std::collections::HashMap;
use std::path::Path;

use crate::collection::calc_photo_hash;
use crate::index::{Index, IndexEntry};

/// Checks for duplicates (according to the hash) among the photos that are part of the index. Returns whether duplicates have been found.
pub fn check_for_duplicates(index: &Index) -> bool {
    let mut hashes_to_files: HashMap<&str, Vec<&IndexEntry>> = HashMap::new();
    for photo in index.photos.iter() {
        if let Some(list) = hashes_to_files.get_mut(photo.filehash.as_str()) {
            list.push(&photo);
        } else {
            hashes_to_files.insert(&photo.filehash, vec!(&photo));
        }
    }

    let mut found_duplicates = false;

    for (hash, photos) in hashes_to_files.iter().filter(|(_, photos)| photos.len() > 1) {
        found_duplicates = true;

        println!("These files seem to be duplicates (hash: {}):", hash);
        for photo in photos {
            println!("  {}", photo.filepath.display());
        }
    }

    found_duplicates
}

/// Rehashes all photos in the index and checks whether the actual hash matches the recorded one. Returns whether for any photo there was a
/// discrepancy (or any photo could not be re-hashed for checking).
pub fn check_hashes(root_dir: &Path, index: &Index) -> bool {
    let mut found_deviation = false;

    for photo in index.photos.iter() {
        let maybe_actual_hash = calc_photo_hash(&root_dir.join(&photo.filepath));
        match maybe_actual_hash {
            Ok(actual_hash) => {
                if photo.filehash != actual_hash {
                    found_deviation = true;
                    println!("{}: Hash does not match (recorded {} but was {})!", photo.filepath.display(), photo.filehash, actual_hash);
                }
            },
            Err(e) => {
                found_deviation = true;
                println!("Skipping file {}: Could not re-hash the file. The error was: {}", photo.filepath.display(), e);
            }
        }
    }

    found_deviation
}

/// Checks whether all photos in the index are compliant with the naming scheme set in the index. Returns whether the name of any photo
/// deviates from the naming scheme.
pub fn check_photo_naming(index: &Index) -> bool {
    false  // TODO: Implement later
}
