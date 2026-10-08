//! Single-cabinet creation with bounded compression buffers.
use crate::reader::block_checksum;
use ms_compress::{lzx_encode::CabinetLzxEncoder, quantum::QuantumEncoder};
use std::{
    collections::BTreeSet,
    io::{self, Read, Seek, SeekFrom, Write},
};

const FRAME_SIZE: usize = 32768;
const MAX_CAB_SIZE: u64 = 0x7fff_ffff;

/// Compression to use when creating a cabinet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteCompression {
    /// Store the original bytes without compression.
    None,
    /// Compress each frame with raw DEFLATE and the CAB MSZIP signature.
    MsZip,
    /// LZX verbatim Huffman blocks; dictionary orders are 15 through 21.
    Lzx {
        /// Base-two dictionary size in bytes.
        window_order: u8,
    },
    /// Quantum adaptive arithmetic/LZ compression.
    Quantum {
        /// Match-search level, from 1 through 7.
        level: u8,
        /// Base-two dictionary size in bytes, from 10 through 21.
        window_order: u8,
    },
}
impl WriteCompression {
    fn bits(self) -> io::Result<u16> {
        match self {
            Self::None => Ok(0),
            Self::MsZip => Ok(1),
            Self::Lzx { window_order } if (15..=21).contains(&window_order) => {
                Ok(3 | u16::from(window_order) << 8)
            }
            Self::Quantum {
                level,
                window_order,
            } if (1..=7).contains(&level) && (10..=21).contains(&window_order) => {
                Ok(2 | u16::from(level) << 4 | u16::from(window_order) << 8)
            }
            _ => Err(invalid("invalid CAB compression configuration")),
        }
    }
}

struct File<'a> {
    name: &'a str,
    data: Option<&'a [u8]>,
    size: u32,
    offset: u32,
    dos_date: u16,
    dos_time: u16,
    attributes: u16,
}

/// Build one unsigned CAB containing a single solid folder.
///
/// Names and optional byte inputs are borrowed, not copied. Reader-based inputs
/// are opened one at a time. Compression uses bounded frame buffers and
/// codec history. At most 65535 files and 65535 frames are supported. Files have
/// deterministic DOS timestamps (1980-01-01) by default; explicit metadata can
/// retain DOS modification dates and read-only/hidden/system/archive attributes.
/// Names use UTF-8.
/// Cabinet signing and splitting into spanning sets are not supported.
pub struct CabinetBuilder<'a> {
    compression: WriteCompression,
    files: Vec<File<'a>>,
    names: BTreeSet<String>,
    size: u32,
    table_size: u64,
}
impl<'a> CabinetBuilder<'a> {
    /// Start a builder. Compression parameters are checked when writing.
    pub fn new(compression: WriteCompression) -> Self {
        Self {
            compression,
            files: Vec::new(),
            names: BTreeSet::new(),
            size: 0,
            table_size: 0,
        }
    }

    /// Add borrowed file contents with a relative archive member name.
    ///
    /// # Errors
    /// Rejects empty, absolute, traversal, NUL-containing, or duplicate names,
    /// and contents or directory sizes exceeding single-cabinet format limits.
    /// Duplicate matching is ASCII-insensitive and treats both separators alike.
    pub fn add_file(&mut self, name: &'a str, data: &'a [u8]) -> io::Result<&mut Self> {
        self.add_file_inner(name, data.len() as u64, Some(data), 0x21, 0, 0xa0)
    }

    /// Register a file whose contents will be opened by [`Self::write_from_readers`].
    ///
    /// The size must match the reader exactly, including for empty files. Names
    /// and format limits are checked now, before any source is opened.
    pub fn add_file_source(&mut self, name: &'a str, size: u64) -> io::Result<&mut Self> {
        self.add_file_inner(name, size, None, 0x21, 0, 0xa0)
    }

    /// Register a reader-based file with DOS modification time and CAB attributes.
    /// See [`Self::add_file_with_metadata`] for supported attribute bits.
    pub fn add_file_source_with_metadata(
        &mut self,
        name: &'a str,
        size: u64,
        dos_date: u16,
        dos_time: u16,
        attributes: u16,
    ) -> io::Result<&mut Self> {
        self.add_file_inner(name, size, None, dos_date, dos_time, attributes)
    }

    fn add_file_inner(
        &mut self,
        name: &'a str,
        length: u64,
        data: Option<&'a [u8]>,
        dos_date: u16,
        dos_time: u16,
        attributes: u16,
    ) -> io::Result<&mut Self> {
        validate_metadata(dos_date, dos_time, attributes)?;
        if name.is_empty()
            || name.len() > 255
            || name.contains('\0')
            || name.contains(':')
            || name
                .split(['/', '\\'])
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(invalid(
                "CAB member must have a relative, non-traversing name of at most 255 UTF-8 bytes",
            ));
        }
        let key = name.replace('\\', "/").to_ascii_lowercase();
        if self.names.contains(&key) {
            return Err(invalid("duplicate CAB member name"));
        }
        let length = u32::try_from(length).map_err(|_| invalid("CAB member exceeds 4 GiB"))?;
        let size = self
            .size
            .checked_add(length)
            .ok_or_else(|| invalid("CAB folder exceeds 4 GiB"))?;
        let table_size = self.table_size + 17 + name.len() as u64;
        if self.files.len() >= u16::MAX as usize
            || u64::from(size).div_ceil(FRAME_SIZE as u64) > u64::from(u16::MAX)
            || table_size + 44 > MAX_CAB_SIZE
        {
            return Err(invalid(
                "single-cabinet file, frame, or directory limit exceeded",
            ));
        }
        self.files.push(File {
            name,
            data,
            size: length,
            offset: self.size,
            dos_date,
            dos_time,
            attributes: attributes | 0x80,
        });
        self.names.insert(key);
        self.size = size;
        self.table_size = table_size;
        Ok(self)
    }

    /// Add a file with raw DOS modification time and supported CAB attribute bits.
    /// UTF-8 naming is always retained; callers supply read-only/hidden/system/archive bits.
    pub fn add_file_with_metadata(
        &mut self,
        name: &'a str,
        data: &'a [u8],
        dos_date: u16,
        dos_time: u16,
        attributes: u16,
    ) -> io::Result<&mut Self> {
        self.add_file_inner(
            name,
            data.len() as u64,
            Some(data),
            dos_date,
            dos_time,
            attributes,
        )
    }

    /// Write the cabinet at the output's current position and return its byte size.
    ///
    /// The header is patched using seeking; success leaves the cursor at the
    /// end of the cabinet. Existing trailing output is not truncated. Pass a new
    /// file or empty cursor when producing a standalone archive.
    ///
    /// # Errors
    /// Returns configuration, format-size, codec, or underlying I/O errors.
    /// Output may contain a partial cabinet on error; discard it.
    pub fn write<W: Write + Seek>(&self, output: &mut W) -> io::Result<u64> {
        if self.files.iter().any(|file| file.data.is_none()) {
            return Err(invalid("reader-based CAB files require write_from_readers"));
        }
        self.write_from_readers(output, &mut |index| {
            let bytes = self.files[index]
                .data
                .ok_or_else(|| invalid("missing CAB file data"))?;
            Ok(Box::new(io::Cursor::new(bytes)))
        })
    }

    /// Write using one lazily opened reader per file, in registration order.
    ///
    /// The callback receives the zero-based file index, including for empty
    /// files. Each reader is dropped before the next is opened. Readers must
    /// return exactly the registered file size. A single 32 KiB frame spans
    /// file boundaries, preserving solid compression for every codec.
    ///
    /// Output positioning and partial-output error handling match [`Self::write`].
    pub fn write_from_readers<'r, W: Write + Seek>(
        &self,
        output: &mut W,
        open: &mut impl FnMut(usize) -> io::Result<Box<dyn Read + 'r>>,
    ) -> io::Result<u64> {
        let compression = self.compression.bits()?;
        let mut encoder = Encoder::new(self.compression)?;
        let start = output.stream_position()?;
        let data_offset = 44 + self.table_size;
        let mut header = [0u8; 44];
        header[..4].copy_from_slice(b"MSCF");
        header[16..20].copy_from_slice(&44u32.to_le_bytes());
        header[24..26].copy_from_slice(&[3, 1]);
        header[26..28].copy_from_slice(&1u16.to_le_bytes());
        header[28..30].copy_from_slice(&(self.files.len() as u16).to_le_bytes());
        header[36..40].copy_from_slice(&(data_offset as u32).to_le_bytes());
        header[42..44].copy_from_slice(&compression.to_le_bytes());
        output.write_all(&header)?;
        for file in &self.files {
            output.write_all(&file.size.to_le_bytes())?;
            output.write_all(&file.offset.to_le_bytes())?;
            output.write_all(&0u16.to_le_bytes())?; // Folder index.
            output.write_all(&file.dos_date.to_le_bytes())?;
            output.write_all(&file.dos_time.to_le_bytes())?;
            output.write_all(&file.attributes.to_le_bytes())?;
            output.write_all(file.name.as_bytes())?;
            output.write_all(&[0])?;
        }
        let mut frame = [0; FRAME_SIZE];
        let mut filled = 0;
        let mut count = 0u16;
        let mut size = data_offset;
        for (index, file) in self.files.iter().enumerate() {
            let mut source = open(index)?;
            let mut remaining = u64::from(file.size);
            while remaining != 0 {
                let available = remaining.min((FRAME_SIZE - filled) as u64) as usize;
                let n = read_retry(&mut *source, &mut frame[filled..filled + available])?;
                if n == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "CAB source is shorter than its declared size",
                    ));
                }
                filled += n;
                remaining -= n as u64;
                if filled == FRAME_SIZE {
                    size += encoder.write_frame(&frame, output, size)?;
                    count += 1;
                    filled = 0;
                }
            }
            if read_retry(&mut *source, &mut [0; 1])? != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CAB source exceeds its declared size",
                ));
            }
        }
        if filled != 0 {
            size += encoder.write_frame(&frame[..filled], output, size)?;
            count += 1;
        }
        let end = output.stream_position()?;
        header[8..12].copy_from_slice(&(size as u32).to_le_bytes());
        header[40..42].copy_from_slice(&count.to_le_bytes());
        output.seek(SeekFrom::Start(start))?;
        output.write_all(&header)?;
        output.seek(SeekFrom::Start(end))?;
        Ok(size)
    }
}

fn read_retry(source: &mut dyn Read, output: &mut [u8]) -> io::Result<usize> {
    loop {
        match source.read(output) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}
fn validate_metadata(dos_date: u16, dos_time: u16, attributes: u16) -> io::Result<()> {
    if attributes & !0xa7 != 0
        || dos_date & 31 == 0
        || (dos_date >> 5) & 15 == 0
        || (dos_date >> 5) & 15 > 12
        || dos_time & 31 > 29
        || (dos_time >> 5) & 63 > 59
        || dos_time >> 11 > 23
    {
        return Err(invalid("invalid CAB file metadata"));
    }
    Ok(())
}

enum Encoder {
    Stored,
    MsZip,
    Lzx(Box<CabinetLzxEncoder>),
    Quantum(Box<QuantumEncoder>),
}
impl Encoder {
    fn new(compression: WriteCompression) -> io::Result<Self> {
        match compression {
            WriteCompression::None => Ok(Self::Stored),
            WriteCompression::MsZip => Ok(Self::MsZip),
            WriteCompression::Lzx { window_order } => Ok(Self::Lzx(Box::new(
                CabinetLzxEncoder::new(window_order).map_err(io::Error::other)?,
            ))),
            WriteCompression::Quantum {
                level,
                window_order,
            } => Ok(Self::Quantum(Box::new(
                QuantumEncoder::new(window_order, level).map_err(io::Error::other)?,
            ))),
        }
    }
    fn write_frame<W: Write>(
        &mut self,
        frame: &[u8],
        output: &mut W,
        offset: u64,
    ) -> io::Result<u64> {
        let owned;
        let encoded = match self {
            Self::Stored => frame,
            Self::MsZip => {
                let mut deflater =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                deflater.write_all(frame)?;
                let compressed = deflater.finish()?;
                // Backend-specific stored-block splitting can exceed MSZIP's
                // expansion allowance. A single final stored block also avoids
                // an empty final block after the full 32 KiB output boundary.
                let mut encoded = Vec::with_capacity(2 + compressed.len().min(frame.len() + 5));
                encoded.extend_from_slice(b"CK");
                if compressed.len() >= frame.len() + 5 {
                    let length = u16::try_from(frame.len())
                        .map_err(|_| invalid("MSZIP frame exceeds stored-block length"))?;
                    encoded.push(1); // BFINAL=1, BTYPE=00, zero alignment padding.
                    encoded.extend_from_slice(&length.to_le_bytes());
                    encoded.extend_from_slice(&(!length).to_le_bytes());
                    encoded.extend_from_slice(frame);
                } else {
                    encoded.extend_from_slice(&compressed);
                }
                owned = encoded;
                &owned
            }
            Self::Lzx(encoder) => encoder.compress_frame(frame).map_err(io::Error::other)?,
            Self::Quantum(encoder) => {
                owned = encoder.compress_frame(frame).map_err(io::Error::other)?;
                &owned
            }
        };
        let compressed = u16::try_from(encoded.len())
            .map_err(|_| invalid("compressed CAB frame exceeds 65535 bytes"))?;
        let uncompressed = frame.len() as u16;
        let length = 8 + encoded.len() as u64;
        if offset + length > MAX_CAB_SIZE {
            return Err(invalid("cabinet exceeds 2 GiB format limit"));
        }
        output.write_all(&block_checksum(compressed, uncompressed, encoded).to_le_bytes())?;
        output.write_all(&compressed.to_le_bytes())?;
        output.write_all(&uncompressed.to_le_bytes())?;
        output.write_all(encoded)?;
        Ok(length)
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
