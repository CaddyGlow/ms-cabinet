//! Native, read-only Microsoft cabinet parsing and bounded member extraction.
//!
//! Stored, MSZIP, LZX, and Quantum folders and spanning cabinet sets are supported.
//! Members are
//! streamed into caller-owned buffers; archive names are never filesystem paths.
//! Inputs must remain stable while reading. CFDATA checksums are checked when
//! present, but do not authenticate the publisher or validate unread members.

use ms_compress::{lzx::CabinetLzxDecoder, quantum::QuantumDecoder};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// The compression method declared by a cabinet folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    /// Stored bytes, with no compression.
    None,
    /// Raw DEFLATE blocks with a persistent 32 KiB dictionary.
    MsZip,
    /// Arithmetic/LZ compression with a persistent dictionary and models.
    Quantum {
        /// Producer compression level (1 through 7).
        level: u8,
        /// Dictionary size is `1 << window_order` bytes (orders 10 through 21).
        window_order: u8,
    },
    /// LZX with a persistent dictionary of `1 << window_order` bytes.
    Lzx {
        /// Dictionary size is `1 << window_order` bytes (orders 15 through 21).
        window_order: u8,
    },
}

impl Compression {
    fn parse(bits: u16) -> io::Result<Self> {
        match bits & 15 {
            0 if bits == 0 => Ok(Self::None),
            1 if bits == 1 => Ok(Self::MsZip),
            2 if bits & 0xe000 == 0
                && (1..=7).contains(&((bits >> 4) & 15))
                && (10..=21).contains(&((bits >> 8) & 31)) =>
            {
                Ok(Self::Quantum {
                    level: ((bits >> 4) & 15) as u8,
                    window_order: ((bits >> 8) & 31) as u8,
                })
            }
            3 if bits & 0xe0f0 == 0 && (15..=21).contains(&((bits >> 8) & 31)) => Ok(Self::Lzx {
                window_order: ((bits >> 8) & 31) as u8,
            }),
            _ => Err(invalid("invalid or unsupported CAB compression parameters")),
        }
    }
}

/// Metadata about a member. Names are matched case-insensitively for ASCII,
/// treating `/` and `\` as equivalent separators, without basename fallback.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    /// Original archive member name.
    pub name: String,
    /// Size of the uncompressed member in bytes.
    pub size: u32,
    /// CAB file attribute bits, including the UTF-8 name flag.
    pub attributes: u16,
    /// Raw DOS modification date.
    pub dos_date: u16,
    /// Raw DOS modification time.
    pub dos_time: u16,
    /// Compression method of this member's folder.
    pub compression: Compression,
    folder: usize,
    offset: u32,
    #[serde(skip)]
    continuation: u16,
}

struct Folder {
    compression: Compression,
    blocks: Vec<DataBlock>,
    uncompressed_size: u64,
    from_previous: bool,
    to_next: bool,
}

struct DataBlock {
    source: usize,
    offset: u64,
    checksum: u32,
    compressed_size: u16,
    uncompressed_size: u16,
}

/// A validated cabinet or complete cabinet set and its member directory.
///
/// Opening validates tables, block boundaries, member ranges, and unambiguous
/// names without decompressing payloads. Signed cabinets may have trailing
/// bytes outside `cbCabinet`; all member reads stay within the declared archive.
pub struct Cabinet<R> {
    readers: Vec<R>,
    set_id: u16,
    cabinet_index: u16,
    previous: Option<String>,
    next: Option<String>,
    entries: Vec<Entry>,
    names: BTreeMap<String, usize>,
    folders: Vec<Folder>,
}

impl Cabinet<BufReader<File>> {
    /// Open a file for read-only member listing and extraction.
    pub fn open(path: &Path) -> io::Result<Self> {
        let directory = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut paths = vec![path.to_path_buf()];
        let first = Self::parse(BufReader::new(File::open(path)?))?;
        let mut parts = vec![first];
        while let Some(name) = parts[0].previous.as_deref() {
            let previous = resolve_neighbor(directory, name)?;
            if paths.contains(&previous) || parts.len() >= 65536 {
                return Err(invalid("cyclic or oversized CAB set"));
            }
            let part = Self::parse(BufReader::new(File::open(&previous)?))?;
            validate_neighbors(&part, &parts[0])?;
            if part.next.as_deref().map(member_key)
                != paths[0]
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(member_key)
            {
                return Err(invalid("CAB set has inconsistent next cabinet name"));
            }
            paths.insert(0, previous);
            parts.insert(0, part);
        }
        while let Some(name) = parts.last().and_then(|part| part.next.as_deref()) {
            let next = resolve_neighbor(directory, name)?;
            if paths.contains(&next) || parts.len() >= 65536 {
                return Err(invalid("cyclic or oversized CAB set"));
            }
            let part = Self::parse(BufReader::new(File::open(&next)?))?;
            let last = parts.last().ok_or_else(|| invalid("empty CAB set"))?;
            validate_neighbors(last, &part)?;
            if part.previous.as_deref().map(member_key)
                != paths
                    .last()
                    .and_then(|path| path.file_name())
                    .and_then(|n| n.to_str())
                    .map(member_key)
            {
                return Err(invalid("CAB set has inconsistent previous cabinet name"));
            }
            paths.push(next);
            parts.push(part);
        }
        Self::assemble(parts)
    }
}

impl<R: Read + Seek> Cabinet<R> {
    /// Read and validate a cabinet from a seekable source starting at offset zero.
    pub fn new(reader: R) -> io::Result<Self> {
        let part = Self::parse(reader)?;
        if part.previous.is_some() || part.next.is_some() {
            return Err(unsupported(
                "CAB set requires Cabinet::open or Cabinet::from_parts",
            ));
        }
        Self::assemble(vec![part])
    }

    /// Validate an explicitly supplied complete set, ordered by cabinet index.
    /// Unlike `open`, this does not discover filenames on disk.
    pub fn from_parts(readers: impl IntoIterator<Item = R>) -> io::Result<Self> {
        let parts = readers
            .into_iter()
            .map(Self::parse)
            .collect::<io::Result<Vec<_>>>()?;
        Self::assemble(parts)
    }

    fn parse(mut reader: R) -> io::Result<Self> {
        let actual_size = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;
        let header = read_array::<36>(&mut reader)?;
        if &header[..4] != b"MSCF" {
            return Err(invalid("invalid CAB signature"));
        }
        let size = u64::from(u32_at(&header, 8));
        if size < 36 || size > actual_size || size > 0x7fff_ffff {
            return Err(invalid("invalid or truncated CAB size"));
        }
        if header[24..26] != [3, 1] {
            return Err(invalid("unsupported CAB version (expected 1.3)"));
        }
        let flags = u16_at(&header, 30);
        if flags & !7 != 0 {
            return Err(invalid("unknown CAB header flags"));
        }
        let (folder_reserve, data_reserve) = if flags & 4 != 0 {
            checked_range(reader.stream_position()?, 4, size)?;
            let reserve = read_array::<4>(&mut reader)?;
            let header_reserve = u64::from(u16_at(&reserve, 0));
            let position = reader.stream_position()?;
            checked_range(position, header_reserve, size)?;
            reader.seek(SeekFrom::Start(position + header_reserve))?;
            (usize::from(reserve[2]), usize::from(reserve[3]))
        } else {
            (0, 0)
        };
        let mut read_link = |present: bool| -> io::Result<Option<String>> {
            if !present {
                return Ok(None);
            }
            let name = read_name(&mut reader, size, false)?;
            validate_link(&name)?;
            // The disk label is informational and never used as a path.
            skip_disk_label(&mut reader, size)?;
            Ok(Some(name))
        };
        let previous = read_link(flags & 1 != 0)?;
        let next = read_link(flags & 2 != 0)?;
        let mut folders = Vec::new();
        let mut locations = Vec::new();
        for _ in 0..u16_at(&header, 26) {
            checked_range(reader.stream_position()?, 8 + folder_reserve as u64, size)?;
            let folder = read_array::<8>(&mut reader)?;
            let position = reader.stream_position()?;
            reader.seek(SeekFrom::Start(position + folder_reserve as u64))?;
            locations.push((u64::from(u32_at(&folder, 0)), u16_at(&folder, 4)));
            folders.push(Folder {
                compression: Compression::parse(u16_at(&folder, 6))?,
                blocks: Vec::new(),
                uncompressed_size: 0,
                from_previous: false,
                to_next: false,
            });
        }
        let files_offset = u64::from(u32_at(&header, 16));
        if files_offset < reader.stream_position()? || files_offset > size {
            return Err(invalid(
                "CAB file table overlaps header or lies outside cabinet",
            ));
        }
        reader.seek(SeekFrom::Start(files_offset))?;
        let mut entries = Vec::new();
        let mut names = BTreeMap::new();
        for _ in 0..u16_at(&header, 28) {
            checked_range(reader.stream_position()?, 16, size)?;
            let file = read_array::<16>(&mut reader)?;
            let raw_index = u16_at(&file, 8);
            let folder_index = match raw_index {
                0xfffd | 0xffff => 0,
                0xfffe => folders
                    .len()
                    .checked_sub(1)
                    .ok_or_else(|| invalid("continued CAB member without folder"))?,
                index => usize::from(index),
            };
            if raw_index == 0xffff && folders.len() != 1 {
                return Err(invalid(
                    "CAB member continued both ways requires one folder",
                ));
            }
            let folder = folders
                .get_mut(folder_index)
                .ok_or_else(|| invalid("CAB member folder index out of bounds"))?;
            folder.from_previous |= matches!(raw_index, 0xfffd | 0xffff);
            folder.to_next |= matches!(raw_index, 0xfffe | 0xffff);
            let attributes = u16_at(&file, 14);
            let name = read_name(&mut reader, size, attributes & 0x80 != 0)?;
            if names.insert(member_key(&name), entries.len()).is_some() {
                return Err(invalid("ambiguous duplicate CAB member name"));
            }
            entries.push(Entry {
                name,
                size: u32_at(&file, 0),
                offset: u32_at(&file, 4),
                continuation: raw_index,
                folder: folder_index,
                dos_date: u16_at(&file, 10),
                dos_time: u16_at(&file, 12),
                attributes,
                compression: folder.compression,
            });
        }
        let table_end = reader.stream_position()?;
        let mut ranges = Vec::new();
        for (folder, (start, count)) in folders.iter_mut().zip(locations) {
            if count == 0 {
                continue;
            }
            if start < table_end {
                return Err(invalid("CAB data overlaps its member table"));
            }
            reader.seek(SeekFrom::Start(start))?;
            for index in 0..count {
                checked_range(reader.stream_position()?, 8 + data_reserve as u64, size)?;
                let block = read_array::<8>(&mut reader)?;
                let compressed_size = u16_at(&block, 4);
                let uncompressed_size = u16_at(&block, 6);
                if uncompressed_size == 0 && (index + 1 != count || !folder.to_next) {
                    return Err(invalid("split CAB block must end a continued folder"));
                }
                if uncompressed_size > 32768 || compressed_size == 0 {
                    return Err(invalid("invalid CAB data block size"));
                }
                let mut reserve = vec![0; data_reserve];
                reader.read_exact(&mut reserve)?;
                let offset = reader.stream_position()?;
                checked_range(offset, u64::from(compressed_size), size)?;
                reader.seek(SeekFrom::Start(offset + u64::from(compressed_size)))?;
                folder.uncompressed_size += u64::from(uncompressed_size);
                folder.blocks.push(DataBlock {
                    source: 0,
                    offset,
                    checksum: u32_at(&block, 0),
                    compressed_size,
                    uncompressed_size,
                });
            }
            ranges.push((start, reader.stream_position()?));
        }
        ranges.sort_unstable();
        if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return Err(invalid("CAB folder data ranges overlap"));
        }
        Ok(Self {
            readers: vec![reader],
            set_id: u16_at(&header, 32),
            cabinet_index: u16_at(&header, 34),
            previous,
            next,
            entries,
            names,
            folders,
        })
    }

    fn assemble(mut parts: Vec<Self>) -> io::Result<Self> {
        if parts.is_empty() {
            return Err(invalid("empty CAB set"));
        }
        if parts[0].previous.is_some()
            || parts[0].cabinet_index != 0
            || parts.last().is_some_and(|part| part.next.is_some())
        {
            return Err(invalid("incomplete CAB set"));
        }
        for pair in parts.windows(2) {
            validate_neighbors(&pair[0], &pair[1])?;
        }
        let mut result = parts.remove(0);
        for mut part in parts {
            let source = result.readers.len();
            for folder in &mut part.folders {
                for block in &mut folder.blocks {
                    block.source = source;
                }
            }
            let left_continues = result.folders.last().is_some_and(|f| f.to_next);
            let right_continues = part.folders.first().is_some_and(|f| f.from_previous);
            if left_continues != right_continues {
                return Err(invalid("CAB folder continuation mismatch"));
            }
            let folder_base = result.folders.len();
            if left_continues {
                let left_index = folder_base - 1;
                let left = &mut result.folders[left_index];
                let right = part.folders.remove(0);
                if left.compression != right.compression {
                    return Err(invalid("continued CAB folder compression mismatch"));
                }
                // Only continued members repeat in the next directory;
                // completed files in the same solid folder need not repeat.
                let old: Vec<_> = result
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| {
                        e.folder == left_index && matches!(e.continuation, 0xfffe | 0xffff)
                    })
                    .map(|(index, _)| index)
                    .collect();
                let new: Vec<_> = part
                    .entries
                    .iter()
                    .filter(|e| e.folder == 0 && matches!(e.continuation, 0xfffd | 0xffff))
                    .collect();
                if old.len() != new.len()
                    || old
                        .iter()
                        .zip(&new)
                        .any(|(&index, entry)| !same_entry(&result.entries[index], entry))
                {
                    return Err(invalid("continued CAB member directories disagree"));
                }
                for (&index, entry) in old.iter().zip(new) {
                    result.entries[index].continuation = if entry.continuation == 0xffff {
                        0xfffe
                    } else {
                        0
                    };
                }
                left.blocks.extend(right.blocks);
                left.uncompressed_size += right.uncompressed_size;
                left.to_next = right.to_next;
                part.entries
                    .retain(|entry| !matches!(entry.continuation, 0xfffd | 0xffff));
            }
            for mut entry in part.entries {
                entry.folder += folder_base - usize::from(left_continues);
                if result
                    .names
                    .insert(member_key(&entry.name), result.entries.len())
                    .is_some()
                {
                    return Err(invalid("ambiguous duplicate CAB member name"));
                }
                result.entries.push(entry);
            }
            result.folders.extend(part.folders);
            result.readers.extend(part.readers);
        }
        for folder in &result.folders {
            if folder.from_previous || folder.to_next {
                return Err(invalid("incomplete CAB folder continuation"));
            }
            let mut compressed_size = 0usize;
            for (index, block) in folder.blocks.iter().enumerate() {
                compressed_size += usize::from(block.compressed_size);
                if compressed_size > 38912 {
                    return Err(invalid("CAB compressed block exceeds 38912 bytes"));
                }
                if block.uncompressed_size == 0 {
                    let next = folder
                        .blocks
                        .get(index + 1)
                        .ok_or_else(|| invalid("unfinished split CAB block"))?;
                    if next.source != block.source + 1 {
                        return Err(invalid("split CAB block must continue in the next cabinet"));
                    }
                    continue;
                }
                if folder.compression == Compression::None
                    && compressed_size != usize::from(block.uncompressed_size)
                {
                    return Err(invalid("stored CAB block sizes disagree"));
                }
                if matches!(
                    folder.compression,
                    Compression::Lzx { .. } | Compression::Quantum { .. }
                ) && index + 1 < folder.blocks.len()
                    && block.uncompressed_size != 32768
                {
                    return Err(invalid(
                        "non-final CAB LZX/Quantum frame must contain 32768 bytes",
                    ));
                }
                compressed_size = 0;
            }
        }
        for entry in &result.entries {
            if u64::from(entry.offset) + u64::from(entry.size)
                > result.folders[entry.folder].uncompressed_size
            {
                return Err(invalid("CAB member extends beyond its folder data"));
            }
        }
        result.previous = None;
        result.next = None;
        Ok(result)
    }

    /// List all members without decompressing their data.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Return metadata for an exact, normalized archive member name.
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.names
            .get(&member_key(name))
            .map(|&index| &self.entries[index])
    }

    /// Stream one member. Earlier blocks in a solid folder are decoded and
    /// discarded to build the correct dictionary; other folders are not decoded.
    pub fn read_file(&mut self, name: &str) -> io::Result<MemberReader<'_, R>> {
        let &index = self.names.get(&member_key(name)).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("CAB member not found: {name}"),
            )
        })?;
        let entry = &self.entries[index];
        let folder = &self.folders[entry.folder];
        Ok(MemberReader {
            readers: &mut self.readers,
            blocks: &folder.blocks,
            decoder: Decoder::new(folder.compression)?,
            index: 0,
            data: Vec::new(),
            compressed: Vec::new(),
            position: 0,
            skip: u64::from(entry.offset),
            remaining: u64::from(entry.size),
            failed: false,
        })
    }

    /// Read one member into memory, rejecting its declared size before decoding
    /// if it exceeds `limit`. Short decoded data is an error, never a valid file.
    pub fn read_file_bytes(&mut self, name: &str, limit: usize) -> io::Result<Vec<u8>> {
        let entry = self.entry(name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("CAB member not found: {name}"),
            )
        })?;
        let size = usize::try_from(entry.size)
            .map_err(|_| invalid("CAB member size exceeds address space"))?;
        if size > limit {
            return Err(invalid("CAB member exceeds configured extraction limit"));
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(size).map_err(io::Error::other)?;
        self.read_file(name)?
            .take(size as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() != size {
            return Err(invalid("CAB member decoded size disagrees with directory"));
        }
        Ok(bytes)
    }
}

/// Read-only streaming decoder for one member. An error invalidates this reader;
/// open a new member reader to retry from the beginning of its folder.
pub struct MemberReader<'a, R> {
    readers: &'a mut [R],
    blocks: &'a [DataBlock],
    decoder: Decoder,
    index: usize,
    data: Vec<u8>,
    compressed: Vec<u8>,
    position: usize,
    skip: u64,
    remaining: u64,
    failed: bool,
}

impl<R: Read + Seek> MemberReader<'_, R> {
    fn read_next(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() || self.remaining == 0 {
            return Ok(0);
        }
        while self.position == self.data.len() {
            let mut compressed_size = 0;
            let uncompressed_size = loop {
                let block = self
                    .blocks
                    .get(self.index)
                    .ok_or_else(|| invalid("CAB member ended before its declared size"))?;
                let reader = &mut self.readers[block.source];
                reader.seek(SeekFrom::Start(block.offset))?;
                let start = compressed_size;
                compressed_size += usize::from(block.compressed_size);
                if self.compressed.len() < compressed_size {
                    self.compressed.resize(compressed_size, 0);
                }
                reader.read_exact(&mut self.compressed[start..compressed_size])?;
                if block.checksum != 0
                    && block_checksum(
                        block.compressed_size,
                        block.uncompressed_size,
                        &self.compressed[start..compressed_size],
                    ) != block.checksum
                {
                    return Err(invalid("CAB data block checksum mismatch"));
                }
                self.index += 1;
                if block.uncompressed_size != 0 {
                    break usize::from(block.uncompressed_size);
                }
            };
            self.compressed.truncate(compressed_size);
            self.decoder
                .decode_into(&self.compressed, uncompressed_size, &mut self.data)?;
            let skip = self.skip.min(self.data.len() as u64) as usize;
            self.skip -= skip as u64;
            self.position = skip;
        }
        let count = output
            .len()
            .min(self.data.len() - self.position)
            .min(self.remaining as usize);
        output[..count].copy_from_slice(&self.data[self.position..self.position + count]);
        self.position += count;
        self.remaining -= count as u64;
        Ok(count)
    }
}

impl<R: Read + Seek> Read for MemberReader<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.failed {
            return Err(invalid("CAB member reader invalidated by a previous error"));
        }
        let result = self.read_next(output);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}

enum Decoder {
    Stored,
    MsZip {
        inflater: Box<flate2::Decompress>,
        history: Vec<u8>,
    },
    Lzx(Box<CabinetLzxDecoder>),
    Quantum(Box<QuantumDecoder>),
}

impl Decoder {
    fn new(compression: Compression) -> io::Result<Self> {
        match compression {
            Compression::None => Ok(Self::Stored),
            Compression::MsZip => Ok(Self::MsZip {
                inflater: Box::new(flate2::Decompress::new(false)),
                history: Vec::new(),
            }),
            Compression::Lzx { window_order } => Ok(Self::Lzx(Box::new(
                CabinetLzxDecoder::new(window_order).map_err(io::Error::other)?,
            ))),
            Compression::Quantum { window_order, .. } => Ok(Self::Quantum(Box::new(
                QuantumDecoder::new(window_order).map_err(io::Error::other)?,
            ))),
        }
    }

    fn decode_into(&mut self, input: &[u8], size: usize, output: &mut Vec<u8>) -> io::Result<()> {
        match self {
            Self::Stored => {
                if input.len() != size {
                    return Err(invalid("stored CAB block size mismatch"));
                }
                output.clear();
                output.extend_from_slice(input);
                Ok(())
            }
            Self::Quantum(decoder) => {
                output.resize(size, 0);
                decoder
                    .decompress_frame(input, output)
                    .map_err(|e| invalid(e.to_string()))?;
                Ok(())
            }
            Self::Lzx(decoder) => {
                output.resize(size, 0);
                decoder
                    .decompress_frame(input, output)
                    .map_err(|error| invalid(error.to_string()))?;
                Ok(())
            }
            Self::MsZip { inflater, history } => {
                if !input.starts_with(b"CK") {
                    return Err(invalid("invalid MSZIP CK signature"));
                }
                inflater.reset(false);
                // Raw DEFLATE has no dictionary API with the Rust backend. Seed
                // it with a non-final stored block containing previous output.
                if !history.is_empty() {
                    let length = history.len() as u16;
                    let mut seed = vec![0];
                    seed.extend_from_slice(&length.to_le_bytes());
                    seed.extend_from_slice(&(!length).to_le_bytes());
                    seed.extend_from_slice(history);
                    let mut discard = vec![0; history.len()];
                    let status = inflater
                        .decompress(&seed, &mut discard, flate2::FlushDecompress::Sync)
                        .map_err(|error| invalid(error.to_string()))?;
                    if status != flate2::Status::Ok
                        || inflater.total_in() != seed.len() as u64
                        || inflater.total_out() != discard.len() as u64
                    {
                        return Err(invalid("MSZIP dictionary initialization failed"));
                    }
                }
                let before_in = inflater.total_in();
                let before_out = inflater.total_out();
                // One extra byte detects a block expanding beyond cbUncomp.
                output.resize(size + 1, 0);
                let status = inflater
                    .decompress(&input[2..], output, flate2::FlushDecompress::Finish)
                    .map_err(|error| invalid(error.to_string()))?;
                if status != flate2::Status::StreamEnd
                    || inflater.total_out() - before_out != size as u64
                    || inflater.total_in() - before_in != (input.len() - 2) as u64
                {
                    return Err(invalid("MSZIP stream length or termination mismatch"));
                }
                output.truncate(size);
                history.extend_from_slice(output);
                if history.len() > 32768 {
                    history.drain(..history.len() - 32768);
                }
                Ok(())
            }
        }
    }
}

fn read_array<const N: usize>(reader: &mut impl Read) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn checked_range(start: u64, length: u64, limit: u64) -> io::Result<()> {
    if start > limit || length > limit - start {
        Err(invalid(
            "CAB structure extends beyond declared cabinet size",
        ))
    } else {
        Ok(())
    }
}

fn read_name(reader: &mut (impl Read + Seek), limit: u64, utf8: bool) -> io::Result<String> {
    let mut bytes = Vec::new();
    loop {
        checked_range(reader.stream_position()?, 1, limit)?;
        let [byte] = read_array(reader)?;
        if byte == 0 {
            break;
        }
        if bytes.len() == 255 {
            return Err(invalid("CAB member name exceeds 255 bytes"));
        }
        bytes.push(byte);
    }
    if bytes.is_empty() {
        return Err(invalid("empty CAB member name"));
    }
    if !utf8 && !bytes.is_ascii() {
        return Err(unsupported(
            "non-ASCII legacy CAB names require an explicit code page",
        ));
    }
    String::from_utf8(bytes).map_err(|_| invalid("invalid UTF-8 CAB member name"))
}

fn member_key(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn unsupported(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}

// CAB's final one-to-three bytes are folded in big-endian order, unlike its
// little-endian full words. See MS-CAB section 2.3.1 and cab 0.6.0 checksum.rs
// (Matthew D. Steele, MIT; notice retained in LICENSE-cab-MIT.txt).
pub(crate) fn block_checksum(compressed: u16, uncompressed: u16, data: &[u8]) -> u32 {
    let mut value = u32::from(compressed) | (u32::from(uncompressed) << 16);
    let mut words = data.chunks_exact(4);
    for word in &mut words {
        value ^= u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
    }
    let mut tail = 0;
    for &byte in words.remainder() {
        tail = (tail << 8) | u32::from(byte);
    }
    value ^ tail
}

fn same_entry(a: &Entry, b: &Entry) -> bool {
    member_key(&a.name) == member_key(&b.name)
        && a.size == b.size
        && a.offset == b.offset
        && a.attributes == b.attributes
        && a.dos_date == b.dos_date
        && a.dos_time == b.dos_time
}

fn validate_neighbors<R>(left: &Cabinet<R>, right: &Cabinet<R>) -> io::Result<()> {
    if left.set_id != right.set_id
        || left.cabinet_index.checked_add(1) != Some(right.cabinet_index)
        || left.next.is_none()
        || right.previous.is_none()
    {
        return Err(invalid("CAB set ID, index, or links disagree"));
    }
    Ok(())
}

fn validate_link(name: &str) -> io::Result<()> {
    if name == "." || name == ".." || name.contains(['/', '\\', ':']) {
        return Err(invalid(
            "CAB neighbor must be a filename in the same directory",
        ));
    }
    Ok(())
}

fn resolve_neighbor(directory: &Path, name: &str) -> io::Result<PathBuf> {
    validate_link(name)?;
    let mut matches = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.eq_ignore_ascii_case(name))
        {
            if entry.file_type()?.is_symlink() || !entry.file_type()?.is_file() {
                return Err(invalid(
                    "CAB neighbor must be a regular file, not a symlink",
                ));
            }
            matches.push(entry.path());
        }
    }
    match matches.len() {
        0 => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing CAB set member: {name}"),
        )),
        1 => Ok(matches.remove(0)),
        _ => Err(invalid("ambiguous CAB neighbor filename")),
    }
}

// Disk labels are opaque OEM strings and may be empty. They do not influence
// discovery, so no code-page interpretation or path conversion is necessary.
fn skip_disk_label(reader: &mut (impl Read + Seek), limit: u64) -> io::Result<()> {
    for _ in 0..=255 {
        checked_range(reader.stream_position()?, 1, limit)?;
        if read_array::<1>(reader)?[0] == 0 {
            return Ok(());
        }
    }
    Err(invalid("CAB disk label exceeds 255 bytes"))
}

#[cfg(test)]
mod checksum_regression {
    use super::block_checksum;

    #[test]
    fn word_checksum_preserves_little_endian_words_and_big_endian_tails() {
        assert_eq!(block_checksum(0, 0, &[1, 2, 3, 4]), 0x0403_0201);
        assert_eq!(block_checksum(0, 0, &[1, 2, 3]), 0x0001_0203);
        assert_eq!(block_checksum(0, 0, &[1, 2]), 0x0000_0102);
        assert_eq!(block_checksum(0, 0, &[1]), 1);
        let data: Vec<u8> = (0..40000)
            .map(|index| (index * 37 + index / 256) as u8)
            .collect();
        for start in 0..16 {
            for length in (0..=257).chain([32767, 32768, 38912]) {
                let bytes = &data[start..start + length];
                let mut expected = 0xabcd_1234;
                let mut word = 0;
                for (index, &byte) in bytes.iter().enumerate() {
                    word |= u32::from(byte) << (8 * (index % 4));
                    if index % 4 == 3 {
                        expected ^= word;
                        word = 0;
                    }
                }
                let mut tail = 0;
                for &byte in &bytes[length / 4 * 4..] {
                    tail = (tail << 8) | u32::from(byte);
                }
                assert_eq!(
                    block_checksum(0x1234, 0xabcd, bytes),
                    expected ^ tail,
                    "start={start}, length={length}"
                );
            }
        }
    }
}
