use cabinet::Cabinet;
use std::io::{Cursor, Read};
/// Maximum CAB input and cumulative decoded member size.
pub const ARCHIVE_LIMIT: usize = 1 << 20;

/// Parse a CAB and stream bounded member bytes without filesystem extraction.
pub fn cab(data: &[u8]) {
    if data.len() > ARCHIVE_LIMIT {
        return;
    }
    let Ok(mut cabinet) = Cabinet::new(Cursor::new(data)) else {
        return;
    };
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
        if let Ok(member) = cabinet.read_file(&name) {
            let mut limited = member.take(budget);
            let mut buffer = [0; 4096];
            loop {
                match limited.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => budget -= n as u64,
                }
            }
        }
    }
}

/// Exercise explicit spanning inputs with bounded decoding.
pub fn spanning(data: &[u8]) {
    if data.len() > ARCHIVE_LIMIT || data.len() < 2 {
        return;
    }
    let split = u16::from_le_bytes([data[0], data[1]]) as usize % (data.len() - 1);
    let parts = [
        Cursor::new(&data[2..2 + split]),
        Cursor::new(&data[2 + split..]),
    ];
    if let Ok(mut cabinet) = Cabinet::from_parts(parts) {
        let names: Vec<_> = cabinet
            .entries()
            .iter()
            .take(64)
            .map(|e| e.name.clone())
            .collect();
        let mut budget = ARCHIVE_LIMIT as u64;
        for name in names {
            if let Ok(member) = cabinet.read_file(&name) {
                let mut limited = member.take(budget);
                let mut buffer = [0; 4096];
                loop {
                    match limited.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => budget -= n as u64,
                    }
                }
            }
            if budget == 0 {
                break;
            }
        }
    }
}

/// Assert writer/reader round trips for every supported compression family.
pub fn roundtrip(data: &[u8]) {
    use cabinet::{CabinetBuilder, WriteCompression};
    if data.is_empty() {
        return;
    }
    let payload = &data[1..data.len().min(8193)];
    let compression = match data[0] % 4 {
        0 => WriteCompression::None,
        1 => WriteCompression::MsZip,
        2 => WriteCompression::Lzx {
            window_order: 15 + data[0] % 7,
        },
        _ => WriteCompression::Quantum {
            level: 1 + data[0] % 7,
            window_order: 10 + data[0] % 12,
        },
    };
    let mut builder = CabinetBuilder::new(compression);
    builder.add_file("payload.bin", payload).unwrap();
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output).unwrap();
    let mut cabinet = Cabinet::new(Cursor::new(output.into_inner())).unwrap();
    assert_eq!(
        cabinet.read_file_bytes("payload.bin", 8192).unwrap(),
        payload
    );
}
