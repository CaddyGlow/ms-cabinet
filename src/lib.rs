//! Read and write Microsoft Cabinet (CAB) archives.
//!
//! [`Cabinet`] reads stored, MSZIP, LZX, and Quantum members, including complete
//! spanning sets. [`CabinetBuilder`] writes single cabinets using any of these four
//! compression methods. Archive names are metadata, never extraction paths.
//!
//! # Example
//!
//! ```
//! use cabinet::{Cabinet, CabinetBuilder, WriteCompression};
//! use std::io::Cursor;
//!
//! let mut builder = CabinetBuilder::new(WriteCompression::MsZip);
//! builder.add_file("hello.txt", b"Hello, CAB!")?;
//! let mut output = Cursor::new(Vec::new());
//! builder.write(&mut output)?;
//! let mut cabinet = Cabinet::new(Cursor::new(output.into_inner()))?;
//! assert_eq!(cabinet.read_file_bytes("hello.txt", 1024)?, b"Hello, CAB!");
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! # Reader-based creation
//!
//! Register names and sizes first, then open each file only when the writer
//! requests it. Payload memory is limited to a 32 KiB frame plus codec buffers;
//! the frame and compression history continue across member boundaries.
//!
//! ```no_run
//! use cabinet::{CabinetBuilder, WriteCompression};
//! use std::{fs::File, io::Read};
//!
//! let names = ["first.bin", "second.bin"];
//! let mut builder = CabinetBuilder::new(WriteCompression::MsZip);
//! for name in names {
//!     builder.add_file_source(name, std::fs::metadata(name)?.len())?;
//! }
//! let mut output = File::create("output.cab")?;
//! builder.write_from_readers(&mut output, &mut |index| {
//!     Ok(Box::new(File::open(names[index])?) as Box<dyn Read>)
//! })?;
//! # Ok::<(), std::io::Error>(())
//! ```
#![deny(missing_docs)]

/// Version of this library, as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

mod reader;
mod writer;

pub use reader::{Cabinet, Compression, Entry, MemberReader};
pub use writer::{CabinetBuilder, WriteCompression};
