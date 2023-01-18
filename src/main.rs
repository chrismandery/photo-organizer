use anyhow::Result;
use clap::{Parser, Subcommand};
use std::collections::HashSet;
use std::env::current_dir;
use std::path::{Path, PathBuf};

use collection::{Photo, calc_photo_hash, scan_photo_collection};
use index::{Index, IndexEntry, get_index_root_and_subdir, read_index_file, write_index_file};

mod checks;
mod collection;
mod index;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
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

    match args.command {
        Command::Check => {
            todo!();
        },
        Command::Init => {},  // handled in main()
        Command::List { recursive } => {
            todo!();
        },
        Command::Rename { recursive } => {
            index_changed = rename_photos(root_dir, subdir, &mut index, &photos, recursive)?;
        },
        Command::Update => {
            index_changed = update_index(root_dir, &mut index, &photos)?;
        }
    }

    if index_changed {
        write_index_file(root_dir, &index)?;
        println!("Index file for {} has been updated.", root_dir.display());
    }

    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Get photo collection that the current working directory is a part of (required by all commands expect init)
    let found_collection = get_index_root_and_subdir(&current_dir()?)?;

    if args.command == Command::Init {
        // Specifically handle the init command since it is the only command that does not require an existing photo collection
        match found_collection {
            Some((ref root_dir, _)) => {
                println!("Cannot initialize a new photo collection here!");
                println!("This directory is already within the collection at: {}", root_dir.display());
            },
            None => {
                let wd = current_dir()?;
                write_index_file(&wd, &Index::default())?;
                println!("Empty index file created for directory {}.", wd.display());
                println!("Adjust configuration options in file if desired and then run the \"update\" command.");
            }
        }
    } else if let Some((ref root_dir, ref subdir)) = found_collection {
        // Handle all other commands
        handle_command(&args, root_dir, subdir)?;
    } else {
        println!("Working directory does not seem to be part of a photo collection!");
        println!("Please run \"init\" in this or the appropriate parent directory.");
    }

    Ok(())
}

/// Renames the files in the given directory (and potentially subdirectories) to follow the naming scheme configured in the index. Returns
/// whether the index has been changed by the function.
fn rename_photos(root_dir: &Path, subdir: &Path, index: &Index, photos: &Vec<Photo>, recursive: bool) -> Result<bool> {
    todo!();
}

/// Updates the index entries with the actual stored photos, detecting new, renamed and deleted photos. Returns whether the index has been
/// changed by the function.
fn update_index(root_dir: &Path, index: &mut Index, photos: &Vec<Photo>) -> Result<bool> {
    // Create index data structures for faster matching of index and photos
    let index_set: HashSet<PathBuf> = index.photos.iter().map(|p| p.filepath.clone()).collect();
    let photos_set: HashSet<PathBuf> = photos.iter().map(|p| p.relative_path.clone()).collect();

    // Check for photos in the index that do no longer exist and thus have been deleted or renamed
    let deleted_photos_paths: HashSet<_> = index_set.difference(&photos_set).collect();
    let deleted_photos: Vec<IndexEntry> = index.photos.iter().filter(|p| deleted_photos_paths.contains(&p.filepath)).cloned().collect();
    index.photos.retain_mut(|p| !deleted_photos_paths.contains(&p.filepath));

    // Check for new photos that are not part of the index yet
    let added_photos_paths = photos_set.difference(&index_set);
    let mut new_photo_found = false;

    for added_photo in added_photos_paths {
        new_photo_found = true;

        // Hash photo
        let hash = calc_photo_hash(&root_dir.join(added_photo))?;

        let new_index_entry = if let Some(renamed_photo) = deleted_photos.iter().find(|p| p.filehash == hash) {
            // Hash matches one of the deleted photos (this photo was just renamed)
            let mut new_entry = renamed_photo.clone();
            new_entry.filepath = added_photo.clone();
            new_entry
        } else {
            // Hash not found in the deleted photos (this photo is new)
            IndexEntry {
                filepath: added_photo.clone(),
                orig_filename: added_photo.file_name().unwrap_or_default().to_string_lossy().into(),
                filehash: hash
            }
        };

        index.photos.push(new_index_entry);
    }

    Ok(new_photo_found || !deleted_photos.is_empty())
}