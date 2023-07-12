use anyhow::Result;
use clap::{Parser, Subcommand};
use log::{debug, error, info, warn};
use std::env::current_dir;
use std::path::Path;
use std::process::ExitCode;

use collection::scan_photo_collection;
use index::{Index, check_index_file_is_git_versioned, get_index_root_and_subdir, read_index_file, write_index_file};

mod checks;
mod collection;
mod commands;
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

    /// Show meta data from EXIF tags and the index file for image files within the current directory
    List {
        #[arg(long, short)]
        recursive: bool
    },

    /// Exports the GPS locations of the image files within the current directory in the GPX format and shows them on a map
    Map {
        /// Command to run for visualizing the GPX file (if empty, just save the GPX file)
        #[arg(long, short, default_value="gpxsee")]
        command: Option<String>,

        #[arg(long, short)]
        recursive: bool
    },

    /// Renames the files in the current directory (and potentially subdirectories) to follow the configured naming scheme
    Rename {
        #[arg(long, short)]
        recursive: bool
    },

    /// Creates a thumbnail catalogue that shows all photos within the current directory in a size-optimized thumbnail format in a
    /// self-contained HTML file. This is useful for previewing the photos, e.g., in a bandwidth-constrained setting where downloading all
    /// the photos would not be feasible.
    ThumbCat {
        /// Filename for the thumbnail catalogue
        #[arg(long, default_value="000_thumbnails.html")]
        filename: String,

        /// Force re-generation of the thumbnail catalogue even if it seems to be up-to-date (containing exactly the filenames of the
        /// photos that are stored in the directory).
        #[arg(long, short)]
        force: bool,

        /// Create thumbnail catalogues recursively in all subdirectories that contain at least one photo. Thumbnail catalogues will always
        /// only preview photos within the current directory, not photos contained in a subdirectory.
        #[arg(long, short)]
        recursive: bool,

        /// Width to resize images to
        #[arg(long, default_value="300")]
        resize_width: u32
    },

    /// Update index file adding, renaming and deleting entries as image files have been changed
    Update
}

/// Handles execution of all commands except the init command.
fn handle_command(args: &Args, root_dir: &Path, subdir: &Path) -> Result<ExitCode> {
    // Read index file and scan photo collection
    let mut index = read_index_file(root_dir)?;
    let mut index_changed = false;
    let photos = scan_photo_collection(&index.user_config, root_dir)?;

    // Check whether the index file is versioned using Git and hint user to do so if that is not the case
    if args.command != Command::Init && !check_index_file_is_git_versioned(root_dir) {
        warn!("Warning: Index file in {} does not seem to be versioned using Git.", root_dir.display());
        warn!("It is recommended to setting up a Git repository for tracking changes of the index file.");
    }

    let mut exit_code = ExitCode::SUCCESS;

    match &args.command {
        Command::Check => {
            // Print warning is index is not up to date
            let index_not_up_to_date = commands::update(root_dir, &mut index.clone(), &photos)?;
            if index_not_up_to_date {
                warn!("Index file is not up-to-date! Consider running \"update\" before \"check\" to get accurate results.");
            }

            if !commands::check(root_dir, &index) {
                exit_code = ExitCode::FAILURE;
            }
        },
        Command::Init => {},  // handled in main()
        Command::List { recursive } => {
            // Print warning is index is not up to date
            let index_not_up_to_date = commands::update(root_dir, &mut index.clone(), &photos)?;
            if index_not_up_to_date {
                warn!("Index file is not up-to-date! Consider running \"update\" before \"list\" to get accurate results.");
            }

            commands::list(root_dir, subdir, &index, &photos, *recursive)?;
        },
        Command::Map { command, recursive } => {
            // TODO: Check index up-to-date (once refactored)
            commands::map(root_dir, subdir, &photos, *recursive, command.as_deref())?;
        }
        Command::Rename { recursive } => {
            // Print warning is index is not up to date
            let index_not_up_to_date = commands::update(root_dir, &mut index.clone(), &photos)?;
            if index_not_up_to_date {
                warn!("Index file is not up-to-date! Consider running \"update\" before \"rename\" to get accurate results.");
            }

            let renamed_file_count = commands::rename(root_dir, subdir, &index, &photos, *recursive, args.dry_run)?;

            if renamed_file_count > 0 {
                info!("{} photos have been renamed. Run \"update\" to update the index file.", renamed_file_count);
            } else {
                info!("No photos renamed.");
            }
        },
        Command::ThumbCat { filename, force, recursive, resize_width } => {
            commands::thumbcat(root_dir, subdir, &photos, filename, *force, *recursive, *resize_width)?;
        },
        Command::Update => {
            index_changed = commands::update(root_dir, &mut index, &photos)?;
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

    Ok(exit_code)
}

fn main() -> Result<ExitCode> {
    let args = Args::parse();

    // Configure logger for verbosity
    env_logger::Builder::new()
        .filter_level(args.verbose.log_level_filter())
        .format_target(false)
        .format_timestamp(None)
        .init();

    // Get photo collection that the current working directory is a part of (required by all commands expect init)
    let found_collection = get_index_root_and_subdir(&current_dir()?)?;

    let exit_code = if args.command == Command::Init {
        // Specifically handle the init command since it is the only command that does not require an existing photo collection
        match found_collection {
            Some((ref root_dir, _)) => {
                error!("Cannot initialize a new photo collection here!");
                error!("This directory is already within the collection at: {}", root_dir.display());
                ExitCode::FAILURE
            },
            None => {
                let wd = current_dir()?;
                write_index_file(&wd, &mut Index::default())?;
                info!("Empty index file created for directory {}.", wd.display());
                info!("Adjust configuration options in file if desired and then run the \"update\" command.");
                ExitCode::SUCCESS
            }
        }
    } else if let Some((ref root_dir, ref subdir)) = found_collection {
        // Handle all other commands
        handle_command(&args, root_dir, subdir)?
    } else {
        error!("Working directory does not seem to be part of a photo collection!");
        error!("Please run \"init\" in this or the appropriate parent directory.");
        ExitCode::FAILURE
    };

    Ok(exit_code)
}
