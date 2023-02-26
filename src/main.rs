use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use log::{debug, error, info, warn};
use std::collections::HashSet;
use std::env::current_dir;
use std::fs::rename;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use checks::{check_for_duplicates, check_hashes, check_photo_naming};
use collection::{Photo, calc_photo_hash, get_canonical_photo_filename, scan_photo_collection};
use index::{Index, IndexEntry, check_index_file_is_git_versioned, get_index_root_and_subdir, read_index_file, write_index_file};

mod checks;
mod collection;
mod index;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[clap(flatten)]
    verbose: clap_verbosity_flag::Verbosity<clap_verbosity_flag::InfoLevel>,

    /// Do not make any writing changes to the filesystem, but just print what would be done
    #[arg(long)]
    dry_run: bool,

    #[command(subcommand)]
    command: Command
}

#[derive(Debug, PartialEq, Subcommand)]
enum Command {
    /// Verifies integrity of the photo collection by ensuring the index file is up-to-date and all photo hashes match their recorded hash
    Check,

    /// Initialize new photo collection by creating an index file in the current directory
    Init,

    /// Show meta information for image files within the current directory
    List {
        #[arg(long, short)]
        recursive: bool
    },

    /// Renames the files in the current directory (and potentially subdirectories) to follow the configured naming scheme
    Rename {
        #[arg(long, short)]
        recursive: bool
    },

    /// Update index file adding, renaming and deleting entries as image files have been changed
    Update
}

/// Handles execution of all commands except the init command.
fn handle_command(args: &Args, root_dir: &Path, subdir: &Path) -> Result<()> {
    // Read index file and scan photo collection
    let mut index = read_index_file(root_dir)?;
    let mut index_changed = false;
    let photos = scan_photo_collection(&index.user_config, root_dir)?;

    // Check whether the index file is versioned using Git and hint user to do so if that is not the case
    if args.command != Command::Init && !check_index_file_is_git_versioned(root_dir) {
        warn!("Warning: Index file in {} does not seem to be versioned using Git.", root_dir.display());
        warn!("It is recommended to setting up a Git repository for tracking changes of the index file.");
    }

    match args.command {
        Command::Check => {
            // Print warning is index is not up to date
            let index_not_up_to_date = update_index(root_dir, &mut index.clone(), &photos)?;
            if index_not_up_to_date {
                warn!("Index file is not up-to-date! Consider running \"update\" before \"check\" to get accurate results.");
            }

            // Run all checks (TODO: should be configurable later)
            check_for_duplicates(&index);
            check_hashes(root_dir, &index);
            check_photo_naming(root_dir, &index);
        },
        Command::Init => {},  // handled in main()
        Command::List { recursive } => {
            dbg!(recursive);
            todo!();
        },
        Command::Rename { recursive } => {
            // Print warning is index is not up to date
            let index_not_up_to_date = update_index(root_dir, &mut index.clone(), &photos)?;
            if index_not_up_to_date {
                warn!("Index file is not up-to-date! Consider running \"update\" before \"check\" to get accurate results.");
            }

            let renamed_file_count = rename_photos(root_dir, subdir, &index, &photos, recursive, args.dry_run)?;

            if renamed_file_count > 0 {
                info!("{} photos have been renamed. Run \"update\" to update the index file.", renamed_file_count);
            } else {
                info!("No photos renamed.");
            }
        },
        Command::Update => {
            index_changed = update_index(root_dir, &mut index, &photos)?;
        }
    }

    if index_changed {
        if args.dry_run {
            info!("Index file for {} would have been updated but changes not written (running in dry-run mode).", root_dir.display());
        } else {
            write_index_file(root_dir, &mut index)?;
            info!("Index file for {} has been updated.", root_dir.display());
        }
    } else {
        debug!("No changes, index file not being updated.");
    }

    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Configure logger for verbosity
    env_logger::Builder::new()
        .filter_level(args.verbose.log_level_filter())
        .format_target(false)
        .format_timestamp(None)
        .init();

    // Get photo collection that the current working directory is a part of (required by all commands expect init)
    let found_collection = get_index_root_and_subdir(&current_dir()?)?;

    if args.command == Command::Init {
        // Specifically handle the init command since it is the only command that does not require an existing photo collection
        match found_collection {
            Some((ref root_dir, _)) => {
                error!("Cannot initialize a new photo collection here!");
                error!("This directory is already within the collection at: {}", root_dir.display());
            },
            None => {
                let wd = current_dir()?;
                write_index_file(&wd, &mut Index::default())?;
                info!("Empty index file created for directory {}.", wd.display());
                info!("Adjust configuration options in file if desired and then run the \"update\" command.");
            }
        }
    } else if let Some((ref root_dir, ref subdir)) = found_collection {
        // Handle all other commands
        handle_command(&args, root_dir, subdir)?;
    } else {
        error!("Working directory does not seem to be part of a photo collection!");
        error!("Please run \"init\" in this or the appropriate parent directory.");
    }

    Ok(())
}

/// Renames the files in the given directory (and potentially subdirectories) to follow the naming scheme configured in the index. Returns
/// how many files have been renamed by the function.
fn rename_photos(root_dir: &Path, subdir: &Path, index: &Index, photos: &Vec<Photo>, recursive: bool, dry_run: bool) -> Result<usize> {
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

                        rename(full_old_path, full_new_path)?;
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
fn update_index(root_dir: &Path, index: &mut Index, photos: &Vec<Photo>) -> Result<bool> {
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
