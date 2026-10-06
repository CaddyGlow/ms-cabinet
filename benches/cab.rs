//! Reproducible release-mode CAB writing and streaming-reading benchmark.
use cabinet::{Cabinet, CabinetBuilder, WriteCompression};
use std::{
    env, fs,
    hint::black_box,
    io::{self, Cursor, Write},
    path::PathBuf,
    time::Instant,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
struct Options {
    size: usize,
    iterations: usize,
    input: Option<PathBuf>,
    csv: Option<PathBuf>,
}
fn options() -> Result<Options> {
    let mut options = Options {
        size: 1 << 20,
        iterations: 5,
        input: None,
        csv: None,
    };
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--bench" {
            continue;
        }
        if arg == "--help" {
            println!(
                "cargo bench -p cabinet --bench cab -- [--size BYTES] [--iterations N] [--input FILE] [--csv FILE]"
            );
            std::process::exit(0);
        }
        let value = args.next().ok_or("option requires a value")?;
        match arg.as_str() {
            "--size" => options.size = value.parse()?,
            "--iterations" => options.iterations = value.parse()?,
            "--input" => options.input = Some(value.into()),
            "--csv" => options.csv = Some(value.into()),
            _ => return Err(format!("unknown option: {arg}").into()),
        }
    }
    if options.size == 0 || options.iterations == 0 {
        return Err("size and iterations must be positive".into());
    }
    Ok(options)
}
fn fixtures(size: usize) -> Vec<(&'static str, Vec<u8>)> {
    let mut state = 0x1234_5678u32;
    let random: Vec<u8> = (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    let phrase = b"Microsoft Cabinet archive benchmark: repeating payload data\n";
    let repeated: Vec<u8> = (0..size)
        .map(|index| phrase[index % phrase.len()])
        .collect();
    let mixed = random
        .iter()
        .enumerate()
        .map(|(index, &byte)| {
            if index % 4096 < 2048 {
                phrase[index % phrase.len()]
            } else {
                byte
            }
        })
        .collect();
    vec![
        ("repetitive", repeated),
        ("mixed", mixed),
        ("random", random),
    ]
}
fn csv_cell(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
fn run(options: Options) -> Result<()> {
    let inputs = if let Some(path) = options.input {
        let bytes = fs::read(path)?;
        if bytes.is_empty() {
            return Err("benchmark input must not be empty".into());
        }
        vec![("file", bytes)]
    } else {
        fixtures(options.size)
    };
    let methods = [
        ("stored", WriteCompression::None),
        ("mszip", WriteCompression::MsZip),
        ("lzx-15", WriteCompression::Lzx { window_order: 15 }),
        ("lzx-21", WriteCompression::Lzx { window_order: 21 }),
        (
            "quantum-1-18",
            WriteCompression::Quantum {
                level: 1,
                window_order: 18,
            },
        ),
        (
            "quantum-7-18",
            WriteCompression::Quantum {
                level: 7,
                window_order: 18,
            },
        ),
    ];
    let mut csv = String::from(
        "corpus,method,input_bytes,cab_bytes,ratio,iterations,write_mib_s,read_mib_s\n",
    );
    println!(
        "{:<12} {:<16} {:>12} {:>12} {:>8} {:>12} {:>12}",
        "corpus", "method", "input bytes", "CAB bytes", "ratio", "write MiB/s", "read MiB/s"
    );
    for (name, data) in inputs {
        for (method, compression) in methods {
            let mut builder = CabinetBuilder::new(compression);
            builder.add_file("payload.bin", &data)?;
            // Validate and warm up before timing. Allocation and codec setup
            // inside write/read are included; corpus generation and equality checks are not.
            let mut output = Cursor::new(Vec::with_capacity(data.len() + 4096));
            builder.write(&mut output)?;
            let mut reader = Cabinet::new(Cursor::new(output.get_ref().as_slice()))?;
            if reader.read_file_bytes("payload.bin", data.len())? != data {
                return Err("CAB round-trip mismatch".into());
            }
            let start = Instant::now();
            for _ in 0..options.iterations {
                output.set_position(0);
                output.get_mut().clear();
                black_box(builder.write(black_box(&mut output))?);
            }
            let write_seconds = start.elapsed().as_secs_f64();
            let start = Instant::now();
            for _ in 0..options.iterations {
                let mut reader = Cabinet::new(Cursor::new(black_box(output.get_ref().as_slice())))?;
                let read = io::copy(&mut reader.read_file("payload.bin")?, &mut io::sink())?;
                if black_box(read) != data.len() as u64 {
                    return Err("short benchmark extraction".into());
                }
            }
            let read_seconds = start.elapsed().as_secs_f64();
            let mib = data.len() as f64 / 1048576.0 * options.iterations as f64;
            let write_rate = mib / write_seconds;
            let read_rate = mib / read_seconds;
            let ratio = output.get_ref().len() as f64 / data.len() as f64;
            println!(
                "{name:<12} {method:<16} {:>12} {:>12} {ratio:>8.4} {write_rate:>12.2} {read_rate:>12.2}",
                data.len(),
                output.get_ref().len()
            );
            csv.push_str(&format!(
                "{},{},{},{},{:.6},{},{:.3},{:.3}\n",
                csv_cell(name),
                csv_cell(method),
                data.len(),
                output.get_ref().len(),
                ratio,
                options.iterations,
                write_rate,
                read_rate
            ));
        }
    }
    if let Some(path) = options.csv {
        fs::File::create(path)?.write_all(csv.as_bytes())?;
    }
    Ok(())
}
fn main() -> Result<()> {
    run(options()?)
}
