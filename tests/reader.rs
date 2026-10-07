//! CAB interoperability, malformed-input, and streaming gates.
use cabinet::{Cabinet, Compression};
use std::io::{Cursor, Read};

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn make_cab(compression: u16, files: &[(&str, u32, u32)], blocks: &[(Vec<u8>, u16)]) -> Vec<u8> {
    let mut bytes = vec![0; 44];
    bytes[..4].copy_from_slice(b"MSCF");
    bytes[24..26].copy_from_slice(&[3, 1]);
    put32(&mut bytes, 16, 44);
    put16(&mut bytes, 26, 1);
    put16(&mut bytes, 28, files.len() as u16);
    put16(&mut bytes, 40, blocks.len() as u16);
    put16(&mut bytes, 42, compression);
    for &(name, offset, size) in files {
        bytes.extend_from_slice(&size.to_le_bytes());
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
    }
    let data_offset = bytes.len() as u32;
    put32(&mut bytes, 36, data_offset);
    for (data, size) in blocks {
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&size.to_le_bytes());
        bytes.extend_from_slice(data);
    }
    let length = bytes.len() as u32;
    put32(&mut bytes, 8, length);
    bytes
}

fn stored_cab(name: &str, data: &[u8]) -> Vec<u8> {
    make_cab(
        0,
        &[(name, 0, data.len() as u32)],
        &data
            .chunks(32768)
            .map(|chunk| (chunk.to_vec(), chunk.len() as u16))
            .collect::<Vec<_>>(),
    )
}

fn pack_bits(fields: &[(u32, usize)]) -> Vec<u8> {
    let mut bits = Vec::new();
    for &(value, count) in fields {
        for shift in (0..count).rev() {
            bits.push(((value >> shift) & 1) as u16);
        }
    }
    let mut bytes = Vec::new();
    for chunk in bits.chunks(16) {
        let mut word = 0;
        for (index, &bit) in chunk.iter().enumerate() {
            word |= bit << (15 - index);
        }
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn lzx_stored_header(size: usize, intel_size: Option<u32>) -> Vec<u8> {
    let mut fields = vec![(u32::from(intel_size.is_some()), 1)];
    if let Some(size) = intel_size {
        fields.extend([(size >> 16, 16), (size & 0xffff, 16)]);
    }
    fields.extend([(3, 3), ((size >> 8) as u32, 16), ((size & 255) as u32, 8)]);
    let mut bytes = pack_bits(&fields);
    if fields
        .iter()
        .map(|(_, count)| count)
        .sum::<usize>()
        .is_multiple_of(16)
    {
        bytes.extend_from_slice(&[0, 0]);
    }
    for _ in 0..3 {
        bytes.extend_from_slice(&1u32.to_le_bytes());
    }
    bytes
}

fn linked_part(
    mut bytes: Vec<u8>,
    index: u16,
    previous: Option<&str>,
    next: Option<&str>,
    continuation: u16,
) -> Vec<u8> {
    let mut links = Vec::new();
    for name in [previous, next].into_iter().flatten() {
        links.extend_from_slice(name.as_bytes());
        links.extend_from_slice(b"\0disk\0");
    }
    let shift = links.len() as u32;
    let files = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    let data = u32::from_le_bytes(bytes[36..40].try_into().unwrap());
    put16(
        &mut bytes,
        30,
        u16::from(previous.is_some()) | (u16::from(next.is_some()) << 1),
    );
    put16(&mut bytes, 32, 123);
    put16(&mut bytes, 34, index);
    let mut position = files as usize;
    let count = u16::from_le_bytes(bytes[28..30].try_into().unwrap());
    for _ in 0..count {
        put16(&mut bytes, position + 8, continuation);
        position += 16;
        position += bytes[position..].iter().position(|&b| b == 0).unwrap() + 1;
    }
    put32(&mut bytes, 16, files + shift);
    put32(&mut bytes, 36, data + shift);
    bytes.splice(36..36, links);
    let size = bytes.len() as u32;
    put32(&mut bytes, 8, size);
    bytes
}

fn split_pair(
    compression: u16,
    files: &[(&str, u32, u32)],
    blocks: &[(Vec<u8>, u16)],
    split: usize,
) -> [Vec<u8>; 2] {
    let first = make_cab(compression, files, &[(blocks[0].0[..split].to_vec(), 0)]);
    let mut remaining = vec![(blocks[0].0[split..].to_vec(), blocks[0].1)];
    remaining.extend_from_slice(&blocks[1..]);
    let second = make_cab(compression, files, &remaining);
    [
        linked_part(first, 0, None, Some("two.cab"), 0xfffe),
        linked_part(second, 1, Some("one.cab"), None, 0xfffd),
    ]
}

#[test]
fn reused_frame_buffers_do_not_expose_old_bytes_after_a_short_invalid_block() {
    let first = vec![0x5a; 32768];
    let mut archive = make_cab(
        0,
        &[("data.bin", 0, 32771)],
        &[(first.clone(), 32768), (vec![1, 2, 3], 3)],
    );
    let last_header = archive.len() - 11;
    put32(&mut archive, last_header, 1); // Deliberately incorrect nonzero checksum.
    let mut cabinet = Cabinet::new(Cursor::new(archive)).unwrap();
    let mut reader = cabinet.read_file("data.bin").unwrap();
    let mut decoded = vec![0; first.len()];
    reader.read_exact(&mut decoded).unwrap();
    assert_eq!(decoded, first);
    assert!(reader.read(&mut [0; 3]).is_err());
    assert!(
        reader
            .read(&mut [0; 3])
            .unwrap_err()
            .to_string()
            .contains("invalidated")
    );
}

#[test]
fn stored_members_stream_across_blocks_and_preserve_archive_bytes() {
    let data: Vec<u8> = (0..70000).map(|index| (index % 251) as u8).collect();
    let original = stored_cab("nested\\data.bin", &data);
    let mut input = Cursor::new(original.clone());
    let mut cab = Cabinet::new(&mut input).unwrap();
    assert_eq!(cab.entries()[0].compression, Compression::None);
    let mut reader = cab.read_file("NESTED/data.bin").unwrap();
    let mut output = Vec::new();
    let mut buffer = [0; 17];
    loop {
        let read = reader.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..read]);
    }
    assert_eq!(output, data);
    drop(reader);
    assert_eq!(
        cab.read_file_bytes("nested/data.bin", data.len()).unwrap(),
        data
    );
    drop(cab);
    assert_eq!(input.into_inner(), original);
}

#[test]
fn exact_member_selection_does_not_fall_back_to_a_nested_basename() {
    let mut cab = Cabinet::new(Cursor::new(stored_cab("nested/update.mum", b"payload"))).unwrap();
    assert!(cab.entry("update.mum").is_none());
    assert_eq!(
        cab.read_file("update.mum").err().unwrap().kind(),
        std::io::ErrorKind::NotFound
    );
}

#[test]
fn empty_stored_member_in_empty_folder_is_readable() {
    let mut cab = Cabinet::new(Cursor::new(stored_cab("empty", b""))).unwrap();
    assert!(cab.read_file_bytes("empty", 0).unwrap().is_empty());
}

#[test]
fn member_size_limit_is_checked_before_decoding() {
    let mut bytes = stored_cab("data", b"payload");
    // Damage data, but extraction must fail on the size limit first.
    let data = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize;
    bytes[data] = 1;
    let mut cab = Cabinet::new(Cursor::new(bytes)).unwrap();
    assert!(
        cab.read_file_bytes("data", 6)
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
}

#[test]
fn mszip_solid_dictionary_is_retained_when_selecting_a_later_member() {
    let source: Vec<u8> = (0..32768)
        .map(|index| ((index * 13 + index / 251) % 256) as u8)
        .collect();
    let mut compressor = flate2::Compress::new(flate2::Compression::best(), false);
    let mut blocks = Vec::new();
    for _ in 0..2 {
        let mut encoded = Vec::with_capacity(65535);
        compressor
            .compress_vec(&source, &mut encoded, flate2::FlushCompress::Sync)
            .unwrap();
        let mut frame = b"CK".to_vec();
        frame.extend(encoded);
        frame.extend_from_slice(&[3, 0]); // End this raw DEFLATE stream.
        blocks.push((frame, 32768));
    }
    let bytes = make_cab(1, &[("first", 0, 32768), ("second", 32768, 32768)], &blocks);
    let mut cab = Cabinet::new(Cursor::new(bytes)).unwrap();
    assert_eq!(cab.read_file_bytes("second", 32768).unwrap(), source);
    assert_eq!(cab.read_file_bytes("first", 32768).unwrap(), source);
    let mut isolated = Cabinet::new(Cursor::new(make_cab(
        1,
        &[("second", 0, 32768)],
        &blocks[1..],
    )))
    .unwrap();
    assert!(
        isolated.read_file_bytes("second", 32768).is_err(),
        "second frame must require the first frame's dictionary"
    );
}

#[test]
fn lzx_uncompressed_block_continues_across_cfdata_frames() {
    let source: Vec<u8> = (0..40000).map(|index| (index % 251) as u8).collect();
    let mut first = lzx_stored_header(source.len(), None);
    first.extend_from_slice(&source[..32768]);
    let bytes = make_cab(
        0x1003,
        &[("later", 32760, 7240)],
        &[(first, 32768), (source[32768..].to_vec(), 7232)],
    );
    let mut cab = Cabinet::new(Cursor::new(bytes)).unwrap();
    assert_eq!(cab.read_file_bytes("later", 7240).unwrap(), source[32760..]);
}

#[test]
fn lzx_intel_translation_uses_folder_position() {
    let mut source = vec![b'x'; 32768 + 40];
    source[32768 + 3] = 0xe8;
    source[32768 + 4..32768 + 8].copy_from_slice(&100000i32.to_le_bytes());
    let mut first = lzx_stored_header(source.len(), Some(200000));
    first.extend_from_slice(&source[..32768]);
    let bytes = make_cab(
        0x0f03,
        &[("second", 32768, 40)],
        &[(first, 32768), (source[32768..].to_vec(), 40)],
    );
    let mut cab = Cabinet::new(Cursor::new(bytes)).unwrap();
    let actual = cab.read_file_bytes("second", 40).unwrap();
    assert_eq!(
        i32::from_le_bytes(actual[4..8].try_into().unwrap()),
        100000 - 32771
    );
}

#[test]
fn signed_cabinet_reserves_and_trailing_bytes_are_accepted() {
    let base = stored_cab("file", b"payload");
    let mut bytes = base[..36].to_vec();
    put16(&mut bytes, 30, 4);
    bytes.extend_from_slice(&[3, 0, 2, 3]); // Header/folder/data reserve lengths.
    bytes.extend_from_slice(b"sig");
    bytes.extend_from_slice(&base[36..44]);
    bytes.extend_from_slice(&[9, 8]);
    let files = bytes.len() as u32;
    let old_data = u32::from_le_bytes(base[36..40].try_into().unwrap()) as usize;
    bytes.extend_from_slice(&base[44..old_data]);
    let data = bytes.len() as u32;
    bytes.extend_from_slice(&base[old_data..old_data + 8]);
    bytes.extend_from_slice(&[1, 2, 3]);
    bytes.extend_from_slice(&base[old_data + 8..]);
    let length = bytes.len() as u32;
    put32(&mut bytes, 8, length);
    put32(&mut bytes, 16, files);
    put32(&mut bytes, 43, data);
    // Independent XOR calculation for size fields + "payload" (reserve excluded).
    put32(&mut bytes, data as usize, 0x6c110013);
    bytes.extend_from_slice(b"trailing-signature");
    let mut cab = Cabinet::new(Cursor::new(bytes)).unwrap();
    assert_eq!(cab.read_file_bytes("file", 7).unwrap(), b"payload");
}

#[test]
fn malformed_tables_and_ranges_return_errors() {
    let base = stored_cab("file", b"payload");
    let data = u32::from_le_bytes(base[36..40].try_into().unwrap()) as usize;
    let mut cases = Vec::new();
    for (offset, value) in [(8, 200u32), (16, 2), (36, 44), (44, 8), (48, 1000000)] {
        let mut bytes = base.clone();
        put32(&mut bytes, offset, value);
        cases.push(bytes);
    }
    for (offset, value) in [(52, 2u16), (data + 4, 8), (data + 6, 32769), (data + 6, 0)] {
        let mut bytes = base.clone();
        put16(&mut bytes, offset, value);
        cases.push(bytes);
    }
    for bytes in cases {
        assert!(Cabinet::new(Cursor::new(bytes)).is_err());
    }
    for length in 0..base.len() {
        assert!(Cabinet::new(Cursor::new(&base[..length])).is_err());
    }
}

#[test]
fn duplicate_names_are_rejected_after_case_and_separator_normalization() {
    let bytes = make_cab(
        0,
        &[("dir\\FILE", 0, 1), ("DIR/file", 1, 1)],
        &[(b"ab".to_vec(), 2)],
    );
    assert!(
        Cabinet::new(Cursor::new(bytes))
            .err()
            .unwrap()
            .to_string()
            .contains("duplicate")
    );
}

#[test]
fn quantum_parameters_are_listed_and_invalid_data_fails_decoding() {
    let mut bytes = stored_cab("file", b"payload");
    put16(&mut bytes, 42, 0x1472);
    let mut cab = Cabinet::new(Cursor::new(bytes)).unwrap();
    assert_eq!(
        cab.entries()[0].compression,
        Compression::Quantum {
            level: 7,
            window_order: 20
        }
    );
    assert_eq!(
        cab.read_file_bytes("file", 7).err().unwrap().kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn incomplete_cabinet_sets_and_orphan_continuations_are_rejected() {
    for (offset, value) in [
        (30, 1),
        (30, 2),
        (34, 1),
        (52, 0xfffd),
        (52, 0xfffe),
        (52, 0xffff),
    ] {
        let mut bytes = stored_cab("file", b"payload");
        put16(&mut bytes, offset, value);
        assert!(Cabinet::new(Cursor::new(bytes)).is_err());
    }
}

#[test]
fn invalid_utf8_and_unspecified_legacy_code_pages_are_rejected() {
    for attributes in [0, 0x80] {
        let mut bytes = stored_cab("file", b"payload");
        put16(&mut bytes, 58, attributes);
        bytes[60] = 0xff;
        assert!(Cabinet::new(Cursor::new(bytes)).is_err());
    }
}

#[test]
fn invalid_checksum_and_failed_reader_cannot_return_success_on_retry() {
    let mut bytes = stored_cab("file", b"payload");
    let data = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize;
    put32(&mut bytes, data, 1);
    let mut cab = Cabinet::new(Cursor::new(bytes)).unwrap();
    let mut reader = cab.read_file("file").unwrap();
    assert!(
        reader
            .read(&mut [0; 7])
            .unwrap_err()
            .to_string()
            .contains("checksum")
    );
    assert!(
        reader
            .read(&mut [0; 7])
            .unwrap_err()
            .to_string()
            .contains("invalidated")
    );
}

#[test]
fn invalid_mszip_signature_and_lzx_payload_are_not_accepted_as_members() {
    for compression in [1, 0x0f03] {
        let mut bytes = stored_cab("file", b"payload");
        put16(&mut bytes, 42, compression);
        let mut cab = Cabinet::new(Cursor::new(bytes)).unwrap();
        assert!(cab.read_file_bytes("file", 7).is_err());
    }
}

#[test]
fn native_reader_decodes_independent_cab_lzx_sample() {
    // cab 0.6.0 cabinet.rs MIT sample; notice: docs/licenses/cab-MIT.txt.
    let binary: &[u8] = b"\x4d\x53\x43\x46\x00\x00\x00\x00\x97\x00\x00\x00\x00\x00\x00\
        \x00\x2c\x00\x00\x00\x00\x00\x00\x00\x03\x01\x01\x00\x02\x00\
        \x00\x00\x2d\x05\x00\x00\x5b\x00\x00\x00\x01\x00\x03\x13\x0f\
        \x00\x00\x00\x00\x00\x00\x00\x00\x00\x21\x53\x0d\xb2\x20\x00\
        \x68\x69\x2e\x74\x78\x74\x00\x10\x00\x00\x00\x0f\x00\x00\x00\
        \x00\x00\x21\x53\x0b\xb2\x20\x00\x62\x79\x65\x2e\x74\x78\x74\
        \x00\x5c\xef\x2a\xc7\x34\x00\x1f\x00\x5b\x80\x80\x8d\x00\x30\
        \xf0\x01\x10\x00\x00\x00\x01\x00\x00\x00\x01\x00\x00\x00\x48\
        \x65\x6c\x6c\x6f\x2c\x20\x77\x6f\x72\x6c\x64\x21\x0d\x0a\x53\
        \x65\x65\x20\x79\x6f\x75\x20\x6c\x61\x74\x65\x72\x21\x0d\x0a\
        \x00\x00\x00\x00";
    let mut cab = Cabinet::new(Cursor::new(binary)).unwrap();
    assert_eq!(
        cab.read_file_bytes("hi.txt", 100).unwrap(),
        b"Hello, world!\r\n"
    );
    assert_eq!(
        cab.read_file_bytes("bye.txt", 100).unwrap(),
        b"See you later!\r\n"
    );
}

#[test]
fn makecab_five_part_fixture_opens_from_every_volume() {
    use sha2::{Digest, Sha256};
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cab-spanning");
    let expected = [
        (
            "small1.bin",
            "416e95ff9e088dca5fa43eeb41acb104852a6c812f3762ac72d6801d1da0ccc2",
        ),
        (
            "small2.bin",
            "1b1366101b3cd6297c0852d133686887b4539c4d5d4e7a96eb944d04c2d9deb0",
        ),
        (
            "medium1.bin",
            "35a052709780ba369567875f644a0cc97059298f0724429a191d066fb27f05c4",
        ),
        (
            "medium2.bin",
            "998ef19336dd0c9e953b33c109c943fa362a1b5f6aa7649d6f33a6a31a6f4e6e",
        ),
        (
            "small3.bin",
            "b536a2d99a7df05436cdaa5d73467d2fc180239b75c946e2110a8226670aaaa7",
        ),
        (
            "medium3.bin",
            "bdf7ca7b9e81e4833cea630fde20e16420285eafc53b41b3487dff027c9e0894",
        ),
    ]
    .map(|(name, hash)| (name.to_owned(), hash.to_owned()));
    for index in 1..=5 {
        let mut cab = Cabinet::open(&directory.join(format!("split-{index}.cab"))).unwrap();
        assert_eq!(cab.entries().len(), 6);
        let names: Vec<_> = cab.entries().iter().map(|e| e.name.clone()).collect();
        let hashes: Vec<_> = names
            .iter()
            .map(|name| {
                let bytes = cab.read_file_bytes(name, 50000).unwrap();
                (name.clone(), format!("{:x}", Sha256::digest(bytes)))
            })
            .collect();
        assert_eq!(expected.as_slice(), hashes);
    }
}

// Add on-disk links to the single-folder builder without relying on this reader
// to produce the fixture. Offsets in CFFILE remain relative to the whole folder.
#[test]
fn split_stored_and_lzx_frames_are_reassembled_before_decoding() {
    let data: Vec<_> = (0..40000).map(|i| (i % 251) as u8).collect();
    for compression in [0, 0x1003] {
        let mut first = if compression == 0 {
            Vec::new()
        } else {
            lzx_stored_header(data.len(), None)
        };
        first.extend_from_slice(&data[..32768]);
        let blocks = [(first, 32768), (data[32768..].to_vec(), 7232)];
        let parts = split_pair(compression, &[("later", 32760, 7240)], &blocks, 13);
        let mut cab = Cabinet::from_parts(parts.map(Cursor::new)).unwrap();
        assert_eq!(cab.read_file_bytes("later", 7240).unwrap(), data[32760..]);
    }
}

#[test]
fn mszip_dictionary_survives_split_blocks_and_cabinet_boundaries() {
    let data: Vec<_> = (0..32768)
        .map(|i| ((i * 13 + i / 251) % 256) as u8)
        .collect();
    let mut compressor = flate2::Compress::new(flate2::Compression::best(), false);
    let blocks: Vec<_> = (0..2)
        .map(|_| {
            let mut compressed = Vec::with_capacity(65535);
            compressor
                .compress_vec(&data, &mut compressed, flate2::FlushCompress::Sync)
                .unwrap();
            let mut frame = b"CK".to_vec();
            frame.extend(compressed);
            frame.extend_from_slice(&[3, 0]);
            (frame, 32768)
        })
        .collect();
    let parts = split_pair(
        1,
        &[("first", 0, 32768), ("later", 32768, 32768)],
        &blocks,
        1,
    );
    let mut cab = Cabinet::from_parts(parts.map(Cursor::new)).unwrap();
    assert_eq!(cab.read_file_bytes("later", 32768).unwrap(), data);
}

#[test]
fn invalid_set_ids_indices_members_compression_and_fragments_are_rejected() {
    let base = split_pair(0, &[("data", 0, 7)], &[(b"payload".to_vec(), 7)], 3);
    for case in 0..7 {
        let mut parts = base.clone();
        let files = u32::from_le_bytes(parts[1][16..20].try_into().unwrap()) as usize;
        let folder = 36 + b"one.cab\0disk\0".len();
        match case {
            0 => put16(&mut parts[1], 32, 124),
            1 => put16(&mut parts[1], 34, 2),
            2 => put32(&mut parts[1], files, 6),
            3 => parts[1][files + 16] = b'x',
            4 => put16(&mut parts[1], folder + 6, 1),
            5 => put16(&mut parts[1], files + 8, 0),
            _ => {
                let data =
                    u32::from_le_bytes(parts[1][folder..folder + 4].try_into().unwrap()) as usize;
                put16(&mut parts[1], data + 6, 0);
            }
        }
        assert!(
            Cabinet::from_parts(parts.map(Cursor::new)).is_err(),
            "case {case}"
        );
    }
    assert!(Cabinet::from_parts([Cursor::new(base[0].clone())]).is_err());
    assert!(Cabinet::from_parts([Cursor::new(base[1].clone())]).is_err());
}

#[test]
fn filesystem_set_resolution_rejects_missing_ambiguous_and_unsafe_neighbors() {
    let directory = tempfile::tempdir().unwrap();
    let mut parts = split_pair(0, &[("data", 0, 7)], &[(b"payload".to_vec(), 7)], 3);
    let one = directory.path().join("one.cab");
    std::fs::write(&one, &parts[0]).unwrap();
    assert_eq!(
        Cabinet::open(&one).err().unwrap().kind(),
        std::io::ErrorKind::NotFound
    );
    std::fs::write(directory.path().join("two.cab"), &parts[1]).unwrap();
    assert_eq!(
        Cabinet::open(&one)
            .unwrap()
            .read_file_bytes("data", 7)
            .unwrap(),
        b"payload"
    );
    // A reciprocal name must actually identify the cabinet on disk.
    parts[1][36..43].copy_from_slice(b"bad.cab");
    std::fs::write(directory.path().join("two.cab"), &parts[1]).unwrap();
    assert!(Cabinet::open(&one).is_err());
    parts[0][36..43].copy_from_slice(b"../evil");
    std::fs::write(&one, &parts[0]).unwrap();
    assert!(Cabinet::open(&one).is_err());
}

#[cfg(unix)]
#[test]
fn case_ambiguous_and_symlink_neighbors_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let parts = split_pair(0, &[("data", 0, 7)], &[(b"payload".to_vec(), 7)], 3);
    let one = directory.path().join("one.cab");
    std::fs::write(&one, &parts[0]).unwrap();
    std::fs::write(directory.path().join("two.cab"), &parts[1]).unwrap();
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.path().join("TWO.CAB"))
    {
        Ok(mut file) => {
            std::io::Write::write_all(&mut file, &parts[1]).unwrap();
            assert!(Cabinet::open(&one).is_err());
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // Case-insensitive filesystems cannot contain this ambiguity.
            assert_eq!(
                Cabinet::open(&one)
                    .unwrap()
                    .read_file_bytes("data", 7)
                    .unwrap(),
                b"payload"
            );
        }
        Err(error) => panic!("cannot create case-ambiguous fixture: {error}"),
    }
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("one.cab"), &parts[0]).unwrap();
    std::os::unix::fs::symlink(
        directory.path().join("two.cab"),
        other.path().join("two.cab"),
    )
    .unwrap();
    assert!(Cabinet::open(&other.path().join("one.cab")).is_err());
}

#[test]
fn split_block_fragment_checksums_are_verified_individually() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cab-spanning");
    for damaged in [0, 1] {
        let mut parts: Vec<_> = (1..=5)
            .map(|i| std::fs::read(root.join(format!("split-{i}.cab"))).unwrap())
            .collect();
        // First half of small2.bin's split block is in volume 1's last
        // folder; the completing fragment is volume 2's first block.
        let bytes = &parts[damaged];
        let mut folder = 40 + usize::from(u16::from_le_bytes(bytes[36..38].try_into().unwrap()));
        let link_count = if damaged == 0 { 2 } else { 4 };
        for _ in 0..link_count {
            folder += bytes[folder..].iter().position(|&b| b == 0).unwrap() + 1;
        }
        if damaged == 0 {
            folder += 8 + usize::from(bytes[38]);
        }
        let data =
            u32::from_le_bytes(parts[damaged][folder..folder + 4].try_into().unwrap()) as usize;
        parts[damaged][data + 8 + 10] ^= 1;
        let mut cab = Cabinet::from_parts(parts.into_iter().map(Cursor::new)).unwrap();
        let error = cab.read_file_bytes("small2.bin", 8000).unwrap_err();
        assert!(error.to_string().contains("checksum"));
    }
}

#[test]
fn compressed_lzx_huffman_block_spans_three_cabinets() {
    let source = b"cabinet native codec sharing ".repeat(500);
    let wim = ms_compress::lzx_encode::compress_lzx(&source, 32768, 32768)
        .unwrap()
        .unwrap();
    let bits: Vec<_> = wim
        .chunks_exact(2)
        .flat_map(|b| {
            let word = u16::from_le_bytes([b[0], b[1]]);
            (0..16).rev().map(move |shift| (word >> shift) & 1)
        })
        .collect();
    assert_eq!(&bits[..4], &[0, 0, 1, 0]);
    let mut fields = vec![(0, 1), (1, 3), (source.len() as u32, 24)];
    fields.extend(bits[20..].iter().map(|&bit| (u32::from(bit), 1)));
    let encoded = pack_bits(&fields);
    let files = [("later", 100, (source.len() - 100) as u32)];
    let parts = [
        linked_part(
            make_cab(0x0f03, &files, &[(encoded[..1].to_vec(), 0)]),
            0,
            None,
            Some("two.cab"),
            0xfffe,
        ),
        linked_part(
            make_cab(0x0f03, &files, &[(encoded[1..13].to_vec(), 0)]),
            1,
            Some("one.cab"),
            Some("three.cab"),
            0xffff,
        ),
        linked_part(
            make_cab(
                0x0f03,
                &files,
                &[(encoded[13..].to_vec(), source.len() as u16)],
            ),
            2,
            Some("two.cab"),
            None,
            0xfffd,
        ),
    ];
    let mut cab = Cabinet::from_parts(parts.map(Cursor::new)).unwrap();
    assert_eq!(
        cab.read_file_bytes("later", source.len()).unwrap(),
        source[100..]
    );
}

#[test]
fn completed_members_in_a_continued_folder_need_not_repeat_in_the_next_directory() {
    let first = make_cab(
        0,
        &[("first", 0, 3), ("continued", 3, 4)],
        &[(b"abc".to_vec(), 3), (b"de".to_vec(), 0)],
    );
    let mut first = linked_part(first, 0, None, Some("two.cab"), 0xfffe);
    let files = u32::from_le_bytes(first[16..20].try_into().unwrap()) as usize;
    put16(&mut first, files + 8, 0); // The first member ends in volume one.
    let second = linked_part(
        make_cab(
            0,
            &[("continued", 3, 4), ("last", 7, 2)],
            &[(b"fg".to_vec(), 4), (b"hi".to_vec(), 2)],
        ),
        1,
        Some("one.cab"),
        None,
        0xfffd,
    );
    let mut second = second;
    let files = u32::from_le_bytes(second[16..20].try_into().unwrap()) as usize;
    put16(&mut second, files + 16 + "continued".len() + 1 + 8, 0);
    let mut cab = Cabinet::from_parts([Cursor::new(first), Cursor::new(second)]).unwrap();
    assert_eq!(cab.entries().len(), 3);
    assert_eq!(cab.read_file_bytes("first", 3).unwrap(), b"abc");
    assert_eq!(cab.read_file_bytes("continued", 4).unwrap(), b"defg");
    assert_eq!(cab.read_file_bytes("last", 2).unwrap(), b"hi");
}

#[test]
fn empty_and_non_ascii_disk_labels_do_not_affect_neighbor_discovery() {
    let parts = split_pair(0, &[("data", 0, 7)], &[(b"payload".to_vec(), 7)], 3);
    let mut parts = parts;
    // OEM label bytes are informational; interpret neither text nor paths.
    parts[0][36 + b"two.cab\0".len()] = 0xff;
    // An empty label: remove four label bytes and adjust absolute offsets.
    let mut second = parts[1].clone();
    let label = 36 + b"one.cab\0".len();
    second.drain(label..label + 4);
    let folder = 36 + b"one.cab\0\0".len();
    let files = u32::from_le_bytes(second[16..20].try_into().unwrap()) - 4;
    let data = u32::from_le_bytes(second[folder..folder + 4].try_into().unwrap()) - 4;
    put32(&mut second, 16, files);
    put32(&mut second, folder, data);
    let size = second.len() as u32;
    put32(&mut second, 8, size);
    let mut cab =
        Cabinet::from_parts([Cursor::new(parts[0].clone()), Cursor::new(second)]).unwrap();
    assert_eq!(cab.read_file_bytes("data", 7).unwrap(), b"payload");
}

#[test]
fn native_quantum_extracts_the_independent_mixed_compression_fixture() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cab-quantum/mszip_lzx_qtm.cab");
    let mut cab = Cabinet::open(&path).unwrap();
    assert_eq!(
        cab.read_file_bytes("qtm.txt", 100).unwrap(),
        b"If you can read this, the Quantum decompressor is working!\n"
    );
}

#[test]
fn quantum_multiframe_models_and_dictionary_survive_small_member_reads() {
    use sha2::{Digest, Sha256};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cab-quantum");
    let records: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("generated-expected.json")).unwrap())
            .unwrap();
    for record in records.as_array().unwrap() {
        let mut cab = Cabinet::open(&root.join(record["cab"].as_str().unwrap())).unwrap();
        // Select the later member first: the earlier frame must initialize both
        // its arithmetic models and dictionary even though it is discarded.
        for expected in record["members"].as_array().unwrap().iter().rev() {
            let mut member = cab.read_file(expected["name"].as_str().unwrap()).unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [1; 17];
            loop {
                let count = member.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..count]);
            }
            assert_eq!(bytes.len() as u64, expected["size"].as_u64().unwrap());
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                expected["sha256"].as_str().unwrap()
            );
        }
    }
}

#[test]
fn quantum_known_malformed_archives_fail_instead_of_hanging_or_returning_garbage() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cab-quantum");
    for name in [
        "cve-2018-18584-qtm-max-size-block.cab",
        "cve-2014-9556-qtm-infinite-loop.cab",
        "cve-2010-2801-qtm-flush.cab",
    ] {
        if let Ok(mut cab) = Cabinet::open(&root.join(name)) {
            let names: Vec<_> = cab.entries().iter().map(|e| e.name.clone()).collect();
            for name in names {
                assert!(cab.read_file_bytes(&name, 1024 * 1024).is_err());
            }
        }
    }
}

#[test]
fn quantum_models_and_dictionary_persist_across_spanning_cabinet_fragments() {
    use sha2::{Digest, Sha256};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cab-quantum");
    let original = std::fs::read(root.join("quantum-10.cab")).unwrap();
    let mut offset = u32::from_le_bytes(original[36..40].try_into().unwrap()) as usize;
    let mut blocks = Vec::new();
    for _ in 0..2 {
        let compressed =
            u16::from_le_bytes(original[offset + 4..offset + 6].try_into().unwrap()) as usize;
        let expanded = u16::from_le_bytes(original[offset + 6..offset + 8].try_into().unwrap());
        blocks.push((
            original[offset + 8..offset + 8 + compressed].to_vec(),
            expanded,
        ));
        offset += 8 + compressed;
    }
    let parts = split_pair(0x0a72, &[("later", 32768, 20739)], &blocks, 1);
    let mut cab = Cabinet::from_parts(parts.map(Cursor::new)).unwrap();
    let bytes = cab.read_file_bytes("later", 20739).unwrap();
    let records: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("generated-expected.json")).unwrap())
            .unwrap();
    let expected = records
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["cab"] == "quantum-10.cab")
        .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        expected["members"][1]["sha256"].as_str().unwrap()
    );
}

#[test]
fn quantum_levels_are_metadata_and_invalid_levels_or_windows_are_rejected() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cab-quantum/quantum-package.cab");
    let original = std::fs::read(path).unwrap();
    for level in 1..=7 {
        let mut bytes = original.clone();
        put16(&mut bytes, 42, 0x1202 | (level << 4));
        let mut cab = Cabinet::new(Cursor::new(bytes)).unwrap();
        assert!(!cab.read_file_bytes("update.mum", 1024).unwrap().is_empty());
    }
    for parameters in [0x1202, 0x1282, 0x0972, 0x1672, 0xf272] {
        let mut bytes = original.clone();
        put16(&mut bytes, 42, parameters);
        assert!(Cabinet::new(Cursor::new(bytes)).is_err());
    }
}

// Resource exhaustion regression: thousands of folders must not expand the same
// physical block table repeatedly before overlap rejection.
#[test]
fn repeated_folder_ranges_are_rejected_before_block_expansion() {
    use std::{
        cell::Cell,
        io::{self, Seek, SeekFrom},
        rc::Rc,
    };
    struct Counting {
        bytes: Cursor<Vec<u8>>,
        reads: Rc<Cell<usize>>,
    }
    impl Read for Counting {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            self.reads.set(self.reads.get() + 1);
            self.bytes.read(output)
        }
    }
    impl Seek for Counting {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.bytes.seek(position)
        }
    }
    for count in [1000u16, 50000] {
        let start = 36 + usize::from(count) * 8;
        let length = start + usize::from(count) * 9;
        let mut bytes = vec![0; length];
        bytes[..4].copy_from_slice(b"MSCF");
        put32(&mut bytes, 8, length as u32);
        put32(&mut bytes, 16, start as u32);
        bytes[24..26].copy_from_slice(&[3, 1]);
        put16(&mut bytes, 26, count);
        for index in 0..usize::from(count) {
            put32(&mut bytes, 36 + index * 8, start as u32);
            put16(&mut bytes, 40 + index * 8, count);
            put16(&mut bytes, start + index * 9 + 4, 1);
            put16(&mut bytes, start + index * 9 + 6, 1);
        }
        let reads = Rc::new(Cell::new(0));
        let input = Counting {
            bytes: Cursor::new(bytes),
            reads: reads.clone(),
        };
        let error = Cabinet::new(input).err().expect("overlap must fail");
        assert!(error.to_string().contains("block count"));
        assert!(
            reads.get() <= usize::from(count) + 1,
            "expanded a reused block table"
        );
    }
}

#[test]
fn physically_reversed_folder_order_is_supported() {
    let mut bytes = vec![0; 72];
    bytes[..4].copy_from_slice(b"MSCF");
    put32(&mut bytes, 8, 72);
    put32(&mut bytes, 16, 52);
    bytes[24..26].copy_from_slice(&[3, 1]);
    put16(&mut bytes, 26, 2);
    put32(&mut bytes, 36, 62); // Folder 0 is physically after folder 1.
    put16(&mut bytes, 40, 1);
    put32(&mut bytes, 44, 52);
    put16(&mut bytes, 48, 1);
    for start in [52, 62] {
        put16(&mut bytes, start + 4, 2);
        put16(&mut bytes, start + 6, 2);
    }
    Cabinet::new(Cursor::new(bytes)).unwrap();
}

#[test]
fn folder_blocks_cannot_cross_the_next_physical_folder() {
    // Aggregate block count fits, but the first folder's block crosses the
    // second start. Padding ensures this exercises per-folder range validation.
    let mut bytes = vec![0; 90];
    bytes[..4].copy_from_slice(b"MSCF");
    put32(&mut bytes, 8, 90);
    put32(&mut bytes, 16, 52);
    bytes[24..26].copy_from_slice(&[3, 1]);
    put16(&mut bytes, 26, 2);
    put32(&mut bytes, 36, 52);
    put16(&mut bytes, 40, 1);
    put32(&mut bytes, 44, 61);
    put16(&mut bytes, 48, 1);
    put16(&mut bytes, 56, 10);
    put16(&mut bytes, 58, 10);
    put16(&mut bytes, 65, 1);
    put16(&mut bytes, 67, 1);
    assert!(Cabinet::new(Cursor::new(bytes)).is_err());
}
