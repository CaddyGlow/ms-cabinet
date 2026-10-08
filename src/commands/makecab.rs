//! Create a cabinet from files and directory trees using any CAB codec.
mod support;
use cabinet::{CabinetBuilder, WriteCompression};
use clap::{Parser, ValueEnum};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Clone, Copy, ValueEnum)]
enum Method {
    None,
    Mszip,
    Lzx,
    Quantum,
}
#[derive(Parser)]
#[command(about = "Create a CAB archive from files and directories (existing output is preserved)")]
struct Args {
    #[arg(short, long)]
    output: PathBuf,
    #[arg(short, long, value_enum, default_value = "mszip")]
    compression: Method,
    /// Dictionary order: LZX 15..21 (default 15), Quantum 10..21 (default 18).
    #[arg(long)]
    window: Option<u8>,
    /// Quantum match-search level 1..7 (default 7).
    #[arg(long)]
    level: Option<u8>,
    /// Input files or directories; directories retain their root basename.
    #[arg(required = true)]
    inputs: Vec<PathBuf>,
}
fn compression(args: &Args) -> Result<WriteCompression> {
    if args.level.is_some() && !matches!(args.compression, Method::Quantum) {
        return Err("--level requires Quantum compression".into());
    }
    Ok(match args.compression {
        Method::None | Method::Mszip if args.window.is_some() => {
            return Err("--window requires LZX or Quantum compression".into());
        }
        Method::None => WriteCompression::None,
        Method::Mszip => WriteCompression::MsZip,
        Method::Lzx => {
            let window_order = args.window.unwrap_or(15);
            if !(15..=21).contains(&window_order) {
                return Err("LZX window must be 15..21".into());
            }
            WriteCompression::Lzx { window_order }
        }
        Method::Quantum => {
            let window_order = args.window.unwrap_or(18);
            let level = args.level.unwrap_or(7);
            if !(10..=21).contains(&window_order) || !(1..=7).contains(&level) {
                return Err("Quantum window must be 10..21 and level must be 1..7".into());
            }
            WriteCompression::Quantum {
                level,
                window_order,
            }
        }
    })
}
struct Source {
    name: String,
    path: PathBuf,
    size: u64,
}

impl Source {
    fn open(&self) -> io::Result<Box<dyn io::Read>> {
        // Recheck the source at the time its payload is consumed. Enumeration
        // deliberately retains neither bytes nor an open handle per member.
        if !fs::symlink_metadata(&self.path)?.is_file() {
            return Err(io::Error::other("input is no longer a regular file"));
        }
        let file = fs::File::open(&self.path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() != self.size {
            return Err(io::Error::other("input changed during creation"));
        }
        Ok(Box::new(file))
    }
}

fn collect(
    path: &Path,
    name: String,
    files: &mut Vec<Source>,
    names: &mut BTreeSet<String>,
) -> Result<()> {
    support::member_path(&name)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        let mut entries = fs::read_dir(path)?.collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let child = entry
                .file_name()
                .into_string()
                .map_err(|_| "input names must be UTF-8")?;
            collect(&entry.path(), format!("{name}/{child}"), files, names)?;
        }
    } else if metadata.is_file() {
        let key = name.to_ascii_lowercase();
        if !names.insert(key) {
            return Err(format!("duplicate archive member: {name}").into());
        }
        files.push(Source {
            name,
            path: path.to_owned(),
            size: metadata.len(),
        });
    } else {
        return Err(format!(
            "input must be a regular file or directory, not a link or special file: {}",
            path.display()
        )
        .into());
    }
    Ok(())
}
fn run(args: Args) -> Result<()> {
    let compression = compression(&args)?;
    if args.output.try_exists()? {
        return Err(format!("output already exists: {}", args.output.display()).into());
    }
    let mut files = Vec::new();
    let mut names = BTreeSet::new();
    for input in &args.inputs {
        let name = input
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("input must have a UTF-8 basename")?;
        collect(input, name.to_owned(), &mut files, &mut names)?;
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    let mut builder = CabinetBuilder::new(compression);
    for source in &files {
        builder.add_file_source(&source.name, source.size)?;
    }
    let parent = args
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut output = BufWriter::new(temporary.as_file_mut());
    let size = builder.write_from_readers(&mut output, &mut |index| files[index].open())?;
    output.flush()?;
    drop(output);
    temporary.as_file().sync_all()?;
    temporary.persist_noclobber(&args.output)?;
    println!(
        "Created {}: {} files, {size} bytes, {compression:?}",
        args.output.display(),
        files.len()
    );
    Ok(())
}
pub(crate) fn main() -> Result<()> {
    run(Args::parse())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cabinet::Cabinet;
    #[test]
    fn directory_trees_round_trip_with_every_method_and_existing_output_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("tree");
        fs::create_dir_all(input.join("nested")).unwrap();
        fs::write(input.join("nested/data.txt"), b"example payload").unwrap();
        for method in ["none", "mszip", "lzx", "quantum"] {
            let output = root.path().join(format!("{method}.cab"));
            let args = || {
                Args::try_parse_from([
                    "makecab",
                    "--output",
                    output.to_str().unwrap(),
                    "--compression",
                    method,
                    input.to_str().unwrap(),
                ])
                .unwrap()
            };
            run(args()).unwrap();
            assert_eq!(
                Cabinet::open(&output)
                    .unwrap()
                    .read_file_bytes("tree/nested/data.txt", 100)
                    .unwrap(),
                b"example payload"
            );
            let original = fs::read(&output).unwrap();
            assert!(run(args()).is_err());
            assert_eq!(fs::read(output).unwrap(), original);
        }
    }
    #[test]
    fn invalid_codec_configuration_fails_before_creating_output() {
        let args = Args::try_parse_from([
            "makecab",
            "--output",
            "unused.cab",
            "--compression",
            "lzx",
            "--window",
            "14",
            "input",
        ])
        .unwrap();
        assert!(compression(&args).is_err());
        let args = Args::try_parse_from([
            "makecab",
            "--output",
            "unused.cab",
            "--compression",
            "mszip",
            "--level",
            "7",
            "input",
        ])
        .unwrap();
        assert!(compression(&args).is_err());
    }
    #[test]
    fn changed_source_is_rejected_when_opened() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        fs::write(&path, b"initial").unwrap();
        let mut sources = Vec::new();
        collect(&path, "file".to_owned(), &mut sources, &mut BTreeSet::new()).unwrap();
        fs::write(&path, b"changed size").unwrap();
        assert!(sources[0].open().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn input_symlinks_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("data"), b"preserved").unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink("data", &link).unwrap();
        assert!(
            collect(
                &link,
                "link".to_owned(),
                &mut Vec::new(),
                &mut BTreeSet::new()
            )
            .is_err()
        );
    }
}
