//! List, verify, or extract CAB members into a new directory.
mod support;
use cabinet::Cabinet;
use clap::Parser;
use std::{
    collections::BTreeSet,
    fs,
    io::{self, BufWriter, Write},
    path::PathBuf,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Parser)]
#[command(
    about = "List, test, or extract a CAB archive; extraction requires a new output directory"
)]
struct Args {
    archive: PathBuf,
    /// List members without decoding.
    #[arg(short, long, conflicts_with_all = ["test", "output"])]
    list: bool,
    /// Decode selected members and verify their checksums without writing files.
    #[arg(short, long, conflicts_with = "output")]
    test: bool,
    /// New destination directory (must not exist).
    #[arg(short, long, required_unless_present_any = ["list", "test"])]
    output: Option<PathBuf>,
    /// Exact archive member names; omit to select all members.
    #[arg(short, long)]
    member: Vec<String>,
    /// Maximum total declared bytes of selected members.
    #[arg(long, default_value_t = 4u64 * 1024 * 1024 * 1024)]
    max_bytes: u64,
}
fn run(args: Args) -> Result<()> {
    let mut cabinet = Cabinet::open(&args.archive)?;
    let entries = if args.member.is_empty() {
        cabinet.entries().to_vec()
    } else {
        let mut entries = Vec::new();
        for member in &args.member {
            let entry = cabinet
                .entry(member)
                .ok_or_else(|| format!("member not found: {member}"))?;
            if !entries
                .iter()
                .any(|e: &cabinet::Entry| e.name == entry.name)
            {
                entries.push(entry.clone());
            }
        }
        entries
    };
    if args.list {
        for entry in &entries {
            println!(
                "{:>12}  {:<30?}  {}",
                entry.size, entry.compression, entry.name
            );
        }
        return Ok(());
    }
    let total: u64 = entries.iter().map(|entry| u64::from(entry.size)).sum();
    if total > args.max_bytes {
        return Err(format!(
            "selected members declare {total} bytes, exceeding --max-bytes {}",
            args.max_bytes
        )
        .into());
    }
    if args.test {
        for entry in &entries {
            let count = io::copy(&mut cabinet.read_file(&entry.name)?, &mut io::sink())?;
            if count != u64::from(entry.size) {
                return Err(format!("short member: {}", entry.name).into());
            }
        }
        println!("Verified {} files, {total} bytes", entries.len());
        return Ok(());
    }
    let output = args.output.ok_or("--output is required for extraction")?;
    if output.try_exists()? {
        return Err(format!("output directory already exists: {}", output.display()).into());
    }
    let mut paths = Vec::new();
    let mut names = BTreeSet::new();
    for entry in &entries {
        let path = support::member_path(&entry.name)?;
        let key = entry.name.replace('\\', "/").to_ascii_lowercase();
        names.insert(key);
        paths.push(path);
    }
    for name in &names {
        let mut ancestor = name.as_str();
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            if names.contains(parent) {
                return Err(format!("member is both a file and a directory: {parent}").into());
            }
            ancestor = parent;
        }
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let staging = tempfile::tempdir_in(parent)?;
    for (entry, relative) in entries.iter().zip(&paths) {
        let path = staging.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let mut writer = BufWriter::new(file);
        let count = io::copy(&mut cabinet.read_file(&entry.name)?, &mut writer)?;
        writer.flush()?;
        if count != u64::from(entry.size) {
            return Err(format!("short member: {}", entry.name).into());
        }
    }
    // Claim a new destination only after all members decode successfully.
    // The caller must have exclusive access to the destination's parent.
    fs::create_dir(&output)?;
    for entry in fs::read_dir(staging.path())? {
        let entry = entry?;
        fs::rename(entry.path(), output.join(entry.file_name()))?;
    }
    println!(
        "Extracted {} files, {total} bytes into {}",
        entries.len(),
        output.display()
    );
    Ok(())
}
pub(crate) fn main() -> Result<()> {
    run(Args::parse())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cabinet::{CabinetBuilder, WriteCompression};
    use std::io::Cursor;
    fn archive(path: &std::path::Path, name: &str) {
        let mut builder = CabinetBuilder::new(WriteCompression::MsZip);
        builder.add_file(name, b"decoded payload").unwrap();
        builder.write(&mut fs::File::create(path).unwrap()).unwrap();
    }
    #[test]
    fn selection_verification_and_extraction_preserve_existing_destinations() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("input.cab");
        let output = root.path().join("out");
        archive(&input, "nested/file.txt");
        let args = || {
            Args::try_parse_from([
                "cabextract",
                input.to_str().unwrap(),
                "--output",
                output.to_str().unwrap(),
                "--member",
                "NESTED\\FILE.TXT",
            ])
            .unwrap()
        };
        run(Args::try_parse_from(["cabextract", input.to_str().unwrap(), "--test"]).unwrap())
            .unwrap();
        run(args()).unwrap();
        assert_eq!(
            fs::read(output.join("nested/file.txt")).unwrap(),
            b"decoded payload"
        );
        fs::write(output.join("nested/file.txt"), b"retained").unwrap();
        assert!(run(args()).is_err());
        assert_eq!(
            fs::read(output.join("nested/file.txt")).unwrap(),
            b"retained"
        );
    }
    #[test]
    fn traversing_archive_names_fail_without_creating_output() {
        let root = tempfile::tempdir().unwrap();
        let mut builder = CabinetBuilder::new(WriteCompression::None);
        builder.add_file("safe.txt", b"data").unwrap();
        let mut bytes = Cursor::new(Vec::new());
        builder.write(&mut bytes).unwrap();
        let mut bytes = bytes.into_inner();
        bytes[60..68].copy_from_slice(b"../e.txt");
        let input = root.path().join("unsafe.cab");
        let output = root.path().join("out");
        fs::write(&input, bytes).unwrap();
        let args = Args::try_parse_from([
            "cabextract",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .unwrap();
        assert!(run(args).is_err());
        assert!(!output.exists());
        assert!(!root.path().join("e.txt").exists());
    }
    #[test]
    fn size_limits_and_failed_decoding_leave_no_destination() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("input.cab");
        let output = root.path().join("out");
        archive(&input, "file");
        let args = Args::try_parse_from([
            "cabextract",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--max-bytes",
            "1",
        ])
        .unwrap();
        assert!(run(args).is_err());
        assert!(!output.exists());
        let mut bytes = fs::read(&input).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&input, bytes).unwrap();
        let args = Args::try_parse_from([
            "cabextract",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .unwrap();
        assert!(run(args).is_err());
        assert!(!output.exists());
    }
}
