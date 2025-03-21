use std::error::Error;
use clap::Parser;

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use clap::builder::Str;
use gpt::disk::LogicalBlockSize;
use gpt::GptConfig;

fn make_gpt(path: &str) -> Result<(), Box<dyn Error>> {
    let mut disk = OpenOptions::new().write(true).open(path)?;
    let block_size = LogicalBlockSize::Lb512;
    let mut gpt = GptConfig::new().writable(true).create_from_device(&mut disk, block_size)?;
    gpt.add_partition("ROOT", 1024*1024*1024, gpt::partition_types::EFI,0,None)?;
    gpt.write()?;
    Ok(())
}

fn check_permissions(file_path: &str, dest_path: &str) -> Result<(bool, bool), Box<dyn Error>> {
    // Check read permission for the file and write permissions for the destination
    let file_perm = match OpenOptions::new().read(true).open(file_path) {
        Ok(_) => true,
        Err(_) => false,
    };
    let dest_perm = match OpenOptions::new().write(true).open(dest_path) {
        Ok(_) => true,
        Err(_) => false,
    };
    Ok((file_perm, dest_perm))
}

#[derive(Parser)]
#[command(author = "namnam", version = "0.0.1", name = "burn-rs")]
/// A POSIX TUI/CLI program to burn an image to a drive written in rust.
struct Args {
    /// Path to a file (an iso) you want to burn to a drive.
    file: String,
    /// Path to a drive you want to burn your image to
    destination: String
}

fn is_block(path: &str) -> bool {
    use std::os::unix::fs::FileTypeExt;
    match std::fs::metadata(path) {
        Ok(metadata) => metadata.file_type().is_block_device(),
        Err(_) => false,
    }
}


/// Entry point.
fn main() {
    // Check if the system is POSIX
    if !cfg!(target_os = "linux") && !cfg!(target_os = "macos") && !cfg!(target_os = "freebsd") && !cfg!(target_os = "openbsd") && !cfg!(target_os = "netbsd") && !cfg!(target_os = "dragonflybsd") {
        eprintln!("\x1b[1m\x1b[31mFatal. \x1b[39mThis program is only supported on POSIX, MacOS, [Free,Open,Net,Dragonfly] BSD systems.\x1b[0m");
        eprintln!("\x1b[1mPlease \x1b[31mUninstall the program. \x1b[39mRun: \x1b[33mcargo uninstall burn-rs\x1b[0m");
        std::process::exit(1);
    }

    let args = Args::parse();

    let file_path = &args.file;
    let dest_path = &args.destination;

    // Check for file path
    if !std::path::Path::new(file_path).exists() {
        eprintln!("\x1b[1m\x1b[31mFatal. \x1b[39mFile does not exist.\x1b[0m");
        std::process::exit(1);
    }

    // Check for destination path
    if !std::path::Path::new(dest_path).exists() {
        eprintln!("\x1b[1m\x1b[31mFatal. \x1b[39mDestination does not exist.\x1b[0m");
        std::process::exit(1);
    }

    // Check for file is actually being an iso

    if !std::path::Path::new(file_path).ends_with(".iso") {
        eprintln!("\x1b[1m\x1b[31mFatal. \x1b[39mFile is not an iso disk image.\x1b[0m");
        std::process::exit(1);
    }

    // Check for destination is actually being a drive

    if !is_block(dest_path) {
        eprintln!("\x1b[1m\x1b[31mFatal. \x1b[39mDestination is not a block (disk) device.\x1b[0m");
        std::process::exit(1);
    }

    // Check for permissions on the file and destination
    let (file_perm, dest_perm) = check_permissions(file_path, dest_path).unwrap();
    if !file_perm {
        eprintln!("\x1b[1m\x1b[31mFatal. \x1b[39mNo read permission on the source file.\x1b[0m");
        std::process::exit(1);
    }
    if !dest_perm {
        eprintln!("\x1b[1m\x1b[31mFatal. \x1b[39mNo write permission on the destination.\x1b[0m");
        std::process::exit(1);
    }

    println!("\x1b[1mChoose partition table:\x1b[0m");
    let mut table = String::new();
    loop {
        println!("1. \x1b[1mMBR [dos]\x1b[0m");
        println!("2. \x1b[1mGPT\x1b[0m");
        println!("3. \x1b[1mCancel\x1b[0m");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).expect("Error reading input");
        let input = input.trim();
        match input.to_lowercase().as_str() {
            "1" | "dos" | "mbr" => {
                table = "dos".to_string();
                break;
            }
            "2" | "gpt" => {
                table = "gpt".to_string();
                break;
            }
            "3" | "cancel" => {
                eprintln!("\x1b[1mExiting...\x1b[0m");
                std::process::exit(0);
            }
            _ => {
                eprintln!("\x1b[1m\x1b[31mInvalid input.\x1b[0m");
                continue;
            }
        }

    }
    // eprintln!("\x1b[1mPartitioning table: {}\x1b[0m", table);
    println!("\x1b[1mChoose filesystem:\x1b[0m");
    let mut fs = String::new();
    loop {
        println!("1. \x1b[1mFAT32\x1b[0m");
        println!("2. \x1b[1mexFAT\x1b[0m");
        println!("3. \x1b[1mext4\x1b[0m");
        println!("4. \x1b[1mCancel\x1b[0m");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).expect("Error reading input");
        let input = input.trim();
        match input.to_lowercase().as_str() {
            "1" | "fat32" => {
                fs = "fat32".to_string();
                break;
            }
            "2" | "exfat" => {
                fs = "exfat".to_string();
                break;
            }
            "3" | "ext4" => {
                fs = "ext4".to_string();
                break;
            }
            "4" | "cancel" => {
                eprintln!("\x1b[1mExiting...\x1b[0m");
                std::process::exit(0);
            }
            _ => {
                eprintln!("\x1b[1m\x1b[31mInvalid input.\x1b[0m");
                continue;
            }
        }

    }

    // Summary
    println!("\x1b[1mSummary:\x1b[0m");
    println!("\x1b[1mWriting {} to {}.\x1b[0m", file_path, dest_path);
    println!("\x1b[1mPartitioning table: {}\x1b[0m", table);
    println!("\x1b[1mFilesystem: {}\x1b[0m", fs);
    println!("\x1b[1m\x1b[33mWarning!\x1b[39m This will erase all data on the destination.\x1b[0m");
    let mut confirmation = String::new();
    println!("\x1b[1mAre you sure you want to continue? [Y/n]\x1b[0m");
    std::io::stdin().read_line(&mut confirmation).expect("Error reading input");
    let confirmation = confirmation.trim();
    if confirmation.to_lowercase() != "y" {
        eprintln!("\x1b[1mExiting...\x1b[0m");
        std::process::exit(0);
    }
    eprintln!("\x1b[1mCreating a partition table...\x1b[0m");







}