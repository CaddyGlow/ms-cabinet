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
#![deny(missing_docs)]

/// Version of this library, as declared in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

mod reader;
mod writer;

pub use reader::{Cabinet, Compression, Entry, MemberReader};
pub use writer::{CabinetBuilder, WriteCompression};
