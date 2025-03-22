use std::error::Error;
use clap::Parser;
use std::fs::OpenOptions;
use std::io::{stdout, Cursor, Read, Seek, SeekFrom, Write};
use gpt::{GptConfig, partition_types};
use gpt::mbr::ProtectiveMBR;
use sysinfo::{Disks, System};
use uuid::Uuid;
use std::fs::File;
use std::os::fd::AsRawFd;
use exfat_fs::format::{Exfat, FormatVolumeOptionsBuilder, Label};
use fatfs::{format_volume, FatType, FormatVolumeOptions};
use fatfs::FatType::{Fat12, Fat16, Fat32};
use libc::{bind, ioctl, BLKSSZGET};
use iso9660_simple::ISO9660;
use iso9660_simple::{helpers, Read as ISORead, *};
struct FileDevice(File);
impl ISORead for FileDevice {
    fn read(&mut self, position: usize, size: usize, buffer: &mut [u8]) -> Option<()> {
        if self.0.seek(SeekFrom::Start(position as u64)).is_err() { return None; }
        if self.0.read_exact(&mut buffer[..size]).is_ok() { Some(()) } else { None }
    }
}

/// This function uses the `gpt` crate to create a new GPT table
fn new_gpt(device_path: &str, iso_size: u64) -> Result<(), Box<dyn Error>> {
    let mut disk = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&device_path)?;

    // Initialize a new GPT partition table
    let mut gpt = GptConfig::new()
        .writable(true)
        .create_from_device(&mut disk, Some(Uuid::new_v4()))?; // Creates a new GPT with a unique disk GUID

    // use iso_size to make the partition size be the same as the iso.
    gpt.add_partition(
        "temporary",
        iso_size+512,
        partition_types::BASIC,
        0,
        None, // no guid
    )?;
    // Write the GPT table back to the disk
    gpt.write()?; // This writes the GPT partition table
    let protective_mbr = ProtectiveMBR::new();
    protective_mbr.overwrite_lba0(&mut disk)?; // This writes protection MBR.

    Ok(()) // Success
}

/// This function writes a new MBR [dos] table to a disk drive.
fn new_dos_mbr(device_path: &str, iso_size: u64) -> Result<(), Box<dyn Error>> {
    let ss = 512;
    let iso_size = iso_size+512u64;
    let mut disk = OpenOptions::new().write(true).read(true).open(&device_path)?;
    let mut mbr = mbrman::MBR::new_from(&mut disk, ss as u32, [0xff;4])?;
    mbr.write_into(&mut disk)?;
    let free_part_number = mbr.iter().find(|(i,p)| p.is_unused()).map(|(i,_)| i)
        .expect("No free partition");
    let sectors = (iso_size / ss) as u32;
    let starting_lba = mbr.find_optimal_place(sectors)
        .expect("Couldn't find a place to put the partition.");

        mbr[free_part_number] = mbrman::MBRPartitionEntry {
            boot: mbrman::BOOT_INACTIVE,
            first_chs: mbrman::CHS::empty(),
            sys: 0x83,
            last_chs: mbrman::CHS::empty(),
            starting_lba,
            sectors
        };
    mbr.write_into(&mut disk)?;
    Ok(())
}

/// This function checks the permissions to read the source file and write to the destination file.
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
#[command(author = "namnam1105", version = "0.0.1", name = "burn-rs")]
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
fn main() -> Result<(), Box<dyn Error>> {
    // Check if the system is POSIX
    if !cfg!(target_os = "linux") && !cfg!(target_os = "macos") && !cfg!(target_os = "freebsd") && !cfg!(target_os = "openbsd") && !cfg!(target_os = "netbsd") && !cfg!(target_os = "dragonfly") {
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



    if !std::path::Path::new(file_path).file_name().unwrap().to_str().unwrap().ends_with(".iso") {
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
        println!("2. \x1b[1mFAT16\x1b[0m");
        println!("3. \x1b[1mexFAT\x1b[0m");
        println!("4. \x1b[1mCancel\x1b[0m");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).expect("Error reading input");
        let input = input.trim();
        match input.to_lowercase().as_str() {
            "1" | "fat32" => {
                fs = "fat32".to_string();
                break;
            }
            "2" | "fat16" => {
                fs = "fat16".to_string();
                break;
            }
            "3" | "exfat" => {
                fs = "exfat".to_string();
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
    use std::os::unix::fs::MetadataExt;
    let iso_size = std::path::Path::new(file_path).metadata()?.size();
    let iso_file = File::open(file_path)?;
    let mut read = ISO9660::from_device(FileDevice(iso_file));
    let iso = read.read_root();
    let mut label: &str = iso[2].name.as_ref();
    if label.is_empty() {
        label = "NO_NAME";
    }
    let mut binding = label.replace(" ", "").replace(".", "").replace("-","");
    if binding.len() > 11 {
        // if length is more than 11 chars then make it ten [chop them off]
        binding = binding.chars().take(10).collect::<String>();
    }
    let label = binding.as_str().as_ref();

    // Summary
    println!("\x1b[1mSummary:\x1b[0m");
    println!("Writing \x1b[1m{}\x1b[0m to \x1b[1m{}.\x1b[0m", file_path.split("/").last().unwrap(), dest_path);
    println!("Partitioning table: \x1b[1m{}\x1b[0m", table);
    println!("Filesystem: \x1b[1m{}\x1b[0m", fs);
    println!("Label: \x1b[1m{}\x1b[0m", label);
    println!("\x1b[1m\x1b[33mWarning!\x1b[39m This will \x1b[31mDESTROY\x1b[39m all data on the destination drive.\x1b[0m");
    let mut confirmation = String::new();
    println!("\x1b[1mAre you sure you want to continue? [Y/n]\x1b[0m");
    std::io::stdin().read_line(&mut confirmation).expect("Error reading input");
    let confirmation = confirmation.trim();
    if confirmation.to_lowercase() != "y" {
        eprintln!("\x1b[1mExiting...\x1b[0m");
        std::process::exit(0);
    }
    eprint!("\x1b[1m[ .... ] Creating a {} partition table...\x1b[0m", table);
    stdout().flush()?;
    let mut result: Result<(), Box<dyn Error>>;
    match table.as_str() {
        "dos" => {
            result = new_dos_mbr(dest_path, iso_size);
        }
        "gpt" => {
            result = new_gpt(dest_path, iso_size);
        }
        _ => {
            eprint!("\r\x1b[1m[\x1b[31m FAILED \x1b[39m] Creating a {} partition table...\x1b[0m", table);
            stdout().flush()?;
            println!();
            eprintln!("\x1b[1m\x1b[31mFatal. \x1b[39mInvalid partition table.\x1b[0m");
            std::process::exit(1);
        }
    }
    if result.is_err() {
        eprint!("\r\x1b[1m[\x1b[31m FAILED \x1b[39m] Creating a {} partition table...\x1b[0m", table);
        stdout().flush()?;
        println!();
        eprintln!("\x1b[1m\x1b[31mFatal. \x1b[39mError creating partition table.\x1b[0m");
        std::process::exit(1);
    }
    eprint!("\r\x1b[1m[\x1b[32m DONE \x1b[39m] Creating a {} partition table...\x1b[0m", table);
    stdout().flush()?;
    println!();
    eprint!("\x1b[1m[ .... ] Formatting the volume as {}...\x1b[0m", fs);
    let mut result: Result<(), Box<dyn Error>>;
    match fs.as_str() {
        "fat32" => result = make_fat(dest_path, label, 32),
        "fat16" => result = make_fat(dest_path, label, 16),
        "exfat" => result = make_exfat(dest_path, label, iso_size),
        _ => {
            eprint!("\r\x1b[1m[\x1b[31m FAILED \x1b[39m] Formatting the volume as {}...\x1b[0m", fs);
            stdout().flush()?;
            println!();
            eprintln!("\x1b[1m\x1b[31mFatal. \x1b[39mInvalid filesystem.\x1b[0m");
            std::process::exit(1);
        }
    }
    if result.is_err() {
        eprint!("\r\x1b[1m[\x1b[31m FAILED \x1b[39m] Formatting the volume as {}...\x1b[0m", fs);
        stdout().flush()?;
        println!();
        eprintln!("\x1b[1m\x1b[31mFatal. \x1b[39mError formatting volume.\x1b[0m");
        eprintln!("\x1b[1m\x1b[31mError: {}\x1b[0m", result.unwrap_err());
        std::process::exit(1);
    }
    eprint!("\r\x1b[1m[\x1b[32m DONE \x1b[39m] Formatting the volume as {}...\x1b[0m", fs);
    println!();
    eprint!("\x1b[1m[{}] Writing the iso to the volume...\x1b[0m", " ".repeat(15));
    stdout().flush()?;
    // start writing...
    // write_image(file_path, dest_path)?;
    eprint!("\r\x1b[1m[\x1b[32m DONE \x1b[39m] Writing the iso to the volume...{}\x1b[0m", "‎".repeat(32));
    println!();
    println!("Btw nothing happened..."); // UNFINISHED...
    println!("\x1b[1m\x1b[32mSuccessfully written an image to disk!\x1b[0m");




    Ok(())
}

/// Writes an image to the disk drive.
/// TODO: Fix this function like what the hell it doesnt work as intended.
fn write_image(file_path: &str, dest_path: &str) -> Result<(), Box<dyn Error>> {
    let dest_path = format!("{}1", dest_path);
    let mut file = OpenOptions::new().read(true).write(true).open(file_path)?;
    let mut dest = OpenOptions::new().read(true).write(true).open(dest_path)?;
    let file_size = file.metadata()?.len();
    let mut bytes_written: u64 = 0;

    let mut buffer = [0u8; 65536]; // allocate a 64kb

    loop {
        let bytes_read = match file.read(&mut buffer) {
            Ok(0) => break, // End of file
            Ok(n) => n,
            Err(e) => return Err(Box::new(e)),
        };
        dest.write_all(&buffer[..bytes_read])?;
        bytes_written += bytes_read as u64;
        let progress = (bytes_written as f64 / file_size as f64) * 100.0;
        let fill = progress.round() as f32 * 14.0_f32.round() / 100.0;
        let empty_fill = 15_i32-fill.round() as i32;
        let fill_chars = format!("{}>{}", "=".repeat(fill.round() as usize), " ".repeat(empty_fill as usize));
        eprint!("\r[{}] {:.2}% ({}/{} mb) Writing the iso to the volume...", fill_chars, progress, bytes_written/1024/1024, file_size/1024/1024);
        stdout().flush()?;
    }
    dest.flush()?;


    

    Ok(())
}

fn make_exfat(drive_path: &str, label: &str,iso_size: u64) -> Result<(), Box<dyn Error>> {
    let drive_path = format!("{}1", drive_path);
    let mut file = OpenOptions::new().read(true).write(true).open(drive_path)?;
    let label = Label::new(label.to_string());
    // println!("{:?}", label); // debugging
    let format_options = FormatVolumeOptionsBuilder::default()
        .pack_bitmap(false)
        .full_format(false)
        .label(label.unwrap())
        .dev_size(iso_size+512)
        .bytes_per_sector(512)
        .build()?;

    let mut formatter = Exfat::try_from(format_options)?;
    formatter.write(&mut file)?;

    Ok(())
}

/// Use the fatfs crate to format the volume as fat.
fn make_fat(drive_path: &str, label: &str, fat: u8) -> Result<(), Box<dyn Error>> {
    let path_to_volume = format!("{}1", drive_path);
    let mut file = OpenOptions::new().read(true).write(true).open(path_to_volume)?;
    let mut fat_type: FatType;
    match fat {
        16 => {
            fat_type = Fat16;
        },
        32 => {
            fat_type = Fat32;
        },
        _ => {
            return Err("Coder is stupid.".into());
            // A case that should never happen.
        }
    }
    let mut volume_label = [0u8; 11];
    for (i, &b) in label.as_bytes().iter().take(11).enumerate() {
        volume_label[i] = b;
    }

    format_volume(&mut file, FormatVolumeOptions::new().fat_type(fat_type).volume_label(volume_label))?;

    Ok(())
}