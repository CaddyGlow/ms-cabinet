//! One-process-per-case CAB reader/byte API benchmark. See scripts/benchmark-streaming.py.
use cabinet::{CabinetBuilder, WriteCompression};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::OpenOptions,
    io::{self, Read, Seek},
    path::PathBuf,
    time::Instant,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Options {
    api: String,
    method: String,
    corpus: String,
    size: usize,
    output: PathBuf,
}
fn options() -> Result<Options> {
    let mut options = Options {
        api: "reader".into(),
        method: "stored".into(),
        corpus: "repetitive".into(),
        size: 1 << 20,
        output: PathBuf::new(),
    };
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == "--bench" {
            continue;
        }
        if argument == "--help" {
            println!(
                "streaming --api bytes|reader --method stored|mszip|lzx|quantum --corpus repetitive|random --size BYTES --output FILE"
            );
            std::process::exit(0);
        }
        let value = args.next().ok_or("option requires a value")?;
        match argument.as_str() {
            "--api" => options.api = value,
            "--method" => options.method = value,
            "--corpus" => options.corpus = value,
            "--size" => options.size = value.parse()?,
            "--output" => options.output = value.into(),
            _ => return Err(format!("unknown option: {argument}").into()),
        }
    }
    if options.size == 0 || options.output.as_os_str().is_empty() {
        return Err("positive --size and --output are required".into());
    }
    if !matches!(options.api.as_str(), "bytes" | "reader")
        || !matches!(options.corpus.as_str(), "repetitive" | "random")
    {
        return Err("unsupported API or corpus".into());
    }
    Ok(options)
}

struct Generated {
    remaining: usize,
    position: usize,
    state: u32,
    random: bool,
}
impl Generated {
    fn new(size: usize, corpus: &str) -> Self {
        Self {
            remaining: size,
            position: 0,
            state: 0x1234_5678,
            random: corpus == "random",
        }
    }
}
impl Read for Generated {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = output.len().min(self.remaining).min(32768);
        let phrase = b"CAB streaming benchmark: repeating deterministic payload data\n";
        for byte in &mut output[..count] {
            *byte = if self.random {
                self.state ^= self.state << 13;
                self.state ^= self.state >> 17;
                self.state ^= self.state << 5;
                self.state as u8
            } else {
                phrase[self.position % phrase.len()]
            };
            self.position += 1;
        }
        self.remaining -= count;
        Ok(count)
    }
}
fn main() -> Result<()> {
    let options = options()?;
    let compression = match options.method.as_str() {
        "stored" => WriteCompression::None,
        "mszip" => WriteCompression::MsZip,
        "lzx" => WriteCompression::Lzx { window_order: 21 },
        "quantum" => WriteCompression::Quantum {
            level: 6,
            window_order: 21,
        },
        _ => return Err("unsupported method".into()),
    };
    let mut output = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&options.output)?;
    let started = Instant::now();
    let mut builder = CabinetBuilder::new(compression);
    let encoded_bytes = if options.api == "bytes" {
        // Generation and the full payload allocation are inside the timing/RSS scope.
        let mut data = vec![0; options.size];
        Generated::new(options.size, &options.corpus).read_exact(&mut data)?;
        builder.add_file("payload.bin", &data)?;
        builder.write(&mut output)?
    } else {
        builder.add_file_source("payload.bin", options.size as u64)?;
        builder.write_from_readers(&mut output, &mut |_| {
            Ok(Box::new(Generated::new(options.size, &options.corpus)))
        })?
    };
    let seconds = started.elapsed().as_secs_f64();
    // Hash outside throughput timing using fixed storage. GNU time's peak RSS
    // includes this phase, process startup, allocator state, and codec history.
    output.rewind()?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = output.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    println!(
        "{}",
        serde_json::json!({
            "api": options.api,
            "method": options.method,
            "corpus": options.corpus,
            "input_bytes": options.size,
            "cab_bytes": encoded_bytes,
            "seconds": seconds,
            "mib_per_second": options.size as f64 / (1024.0 * 1024.0) / seconds,
            "sha256": format!("{:x}", hash.finalize()),
        })
    );
    Ok(())
}
