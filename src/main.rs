use anyhow::Result;
use clap::{Parser, Subcommand};
use std::env::current_dir;
use std::path::Path;

use index::{Index, get_index_root_and_subdir, write_index_file};

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
    match args.command {
        Command::Check => {
            todo!();
        },
        Command::Init => {},  // handled above
        Command::List { recursive: bool }=> {
            todo!();
        },
        Command::Rename { recursive: bool }=> {
            todo!();
        },
        Command::Update => {
            todo!();
        }
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
