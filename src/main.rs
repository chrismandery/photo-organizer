use anyhow::Result;
use clap::{Parser, Subcommand};
use std::env::current_dir;

use index::{get_index_root_and_subdir, write_index_file};

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
        recursive: bool, 
    },

    /// Update index file adding, renaming and deleting entries as image files have been changed
    Update
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Get photo collection that the current working directory is a part of (required by all commands expect init)
    let root_dir = get_index_root_and_subdir(&current_dir()?)?;

    if args.command == Command::Init {
        match root_dir {
            Some((root_dir, _)) => {
                println!("Cannot initialize a new photo collection here!");
                println!("This directory is already within the collection at: {}", root_dir.display());
            },
            None => {
                let wd = current_dir()?;
                write_index_file(&wd, &vec!())?;
                println!("Index file created for directory {}.", wd.display());
            }
        }
    }

    match args.command {
        Command::Check => {
            todo!();
        },
        Command::Init => {},  // handled above
        Command::List { recursive: bool }=> {
            todo!();
        },
        Command::Update => {
            todo!();
        }
    }

    Ok(())
}
