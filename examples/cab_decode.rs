//! Benchmark adapter: extract a single CAB member to stdout without publication.
use cabinet::Cabinet;
use std::{
    env,
    io::{self, BufWriter, Write},
    path::Path,
};

fn main() -> io::Result<()> {
    let mut arguments = env::args_os().skip(1);
    let path = arguments
        .next()
        .ok_or_else(|| io::Error::other("usage: cab_decode CAB MEMBER"))?;
    let member = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| io::Error::other("member name must be UTF-8"))?;
    if arguments.next().is_some() {
        return Err(io::Error::other("unexpected argument"));
    }
    let mut cabinet = Cabinet::open(Path::new(&path))?;
    let expected = cabinet
        .entry(&member)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "CAB member missing"))?
        .size;
    let stdout = io::stdout();
    let mut output = BufWriter::with_capacity(32768, stdout.lock());
    let count = io::copy(&mut cabinet.read_file(&member)?, &mut output)?;
    if count != u64::from(expected) {
        return Err(io::Error::other("CAB decoded size mismatch"));
    }
    output.flush()
}
