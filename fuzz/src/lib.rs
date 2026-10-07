use cabinet::{Cabinet, CabinetBuilder, WriteCompression};
use std::io::{Cursor, Read};
/// Maximum CAB input and cumulative decoded member size.
pub const ARCHIVE_LIMIT: usize = 1 << 20;
/// Writer payload bound permits four full frames and one final partial frame.
pub const WRITER_LIMIT: usize = 4 * 32768 + 1;
/// Bound metadata work for explicit spanning inputs.
pub const PART_LIMIT: usize = 64;

fn decode_bounded<R: Read + std::io::Seek>(cabinet: &mut Cabinet<R>) -> bool {
    let names: Vec<_> = cabinet
        .entries()
        .iter()
        .take(64)
        .map(|e| e.name.clone())
        .collect();
    let mut budget = ARCHIVE_LIMIT as u64;
    for name in names {
        if budget == 0 {
            break;
        }
        let Ok(member) = cabinet.read_file(&name) else {
            return false;
        };
        let mut limited = member.take(budget);
        let mut buffer = [0; 4096];
        loop {
            match limited.read(&mut buffer) {
                Ok(0) => break,
                Err(_) => return false,
                Ok(n) => budget -= n as u64,
            }
        }
    }
    true
}

/// Parse a CAB and stream bounded member bytes without filesystem extraction.
pub fn cab(data: &[u8]) {
    if data.len() > ARCHIVE_LIMIT {
        return;
    }
    if let Ok(mut cabinet) = Cabinet::new(Cursor::new(data)) {
        decode_bounded(&mut cabinet);
    }
}

/// Encode a complete set as a part count followed by u32 lengths and bytes.
/// Intended for trusted corpus generation, not parsing fuzz input.
pub fn encode_parts(parts: &[Vec<u8>]) -> Vec<u8> {
    assert!(!parts.is_empty() && parts.len() <= PART_LIMIT);
    let mut data = vec![parts.len() as u8];
    for part in parts {
        data.extend_from_slice(&u32::try_from(part.len()).unwrap().to_le_bytes());
        data.extend_from_slice(part);
    }
    assert!(data.len() <= ARCHIVE_LIMIT);
    data
}

/// Assemble up to 64 explicit parts, then decode bounded member bytes.
/// Returns whether assembly and all attempted member reads succeeded.
pub fn spanning(data: &[u8]) -> bool {
    if data.len() > ARCHIVE_LIMIT {
        return false;
    }
    let Some((&count, mut rest)) = data.split_first() else {
        return false;
    };
    if count == 0 || usize::from(count) > PART_LIMIT {
        return false;
    }
    let mut parts = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let Some(length) = rest.get(..4) else {
            return false;
        };
        let length = u32::from_le_bytes(length.try_into().unwrap()) as usize;
        rest = &rest[4..];
        let Some(part) = rest.get(..length) else {
            return false;
        };
        parts.push(Cursor::new(part));
        rest = &rest[length..];
    }
    if !rest.is_empty() {
        return false;
    }
    let Ok(mut cabinet) = Cabinet::from_parts(parts) else {
        return false;
    };
    decode_bounded(&mut cabinet)
}

/// Decode independently selected codec, dictionary and level bytes.
pub fn roundtrip_compression(data: &[u8]) -> Option<WriteCompression> {
    let config = data.get(..3)?;
    Some(match config[0] % 4 {
        0 => WriteCompression::None,
        1 => WriteCompression::MsZip,
        2 => WriteCompression::Lzx {
            window_order: 15 + config[1] % 7,
        },
        _ => WriteCompression::Quantum {
            level: 1 + config[2] % 7,
            window_order: 10 + config[1] % 12,
        },
    })
}

/// Assert multiframe, multimember writer/reader round trips for every codec.
pub fn roundtrip(data: &[u8]) {
    let Some(compression) = roundtrip_compression(data) else {
        return;
    };
    let payload = &data[3..data.len().min(WRITER_LIMIT + 3)];
    // A second member crosses frame boundaries and exercises dictionary reuse.
    let split = payload.len() / 2;
    let mut builder = CabinetBuilder::new(compression);
    builder.add_file("first.bin", &payload[..split]).unwrap();
    builder.add_file("second.bin", &payload[split..]).unwrap();
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output).unwrap();
    let mut cabinet = Cabinet::new(Cursor::new(output.into_inner())).unwrap();
    // Read later member first to exercise skipped solid-folder frames too.
    assert_eq!(
        cabinet.read_file_bytes("second.bin", WRITER_LIMIT).unwrap(),
        &payload[split..]
    );
    assert_eq!(
        cabinet.read_file_bytes("first.bin", WRITER_LIMIT).unwrap(),
        &payload[..split]
    );
}
