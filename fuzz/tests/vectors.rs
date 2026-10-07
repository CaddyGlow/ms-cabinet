use cabinet::{Cabinet, WriteCompression};
use cabinet_fuzz::{WRITER_LIMIT, encode_parts, roundtrip_compression};
use std::{io::Cursor, path::Path};
fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures")
}
fn replay_tree(path: &Path, count: &mut usize) {
    for entry in std::fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            replay_tree(&path, count);
        } else if path.extension().is_some_and(|ext| ext == "cab") {
            let bytes = std::fs::read(&path).unwrap();
            cabinet_fuzz::cab(&bytes);
            *count += 1;
        }
    }
}
#[test]
fn all_retained_cab_vectors() {
    let mut count = 0;
    replay_tree(&fixtures(), &mut count);
    assert!(
        count >= 23,
        "retained CAB corpus unexpectedly shrank: {count}"
    );
}
#[test]
fn complete_spanning_seed_assembles_and_decodes_every_member() {
    let parts: Vec<_> = (1..=5)
        .map(|i| std::fs::read(fixtures().join(format!("cab-spanning/split-{i}.cab"))).unwrap())
        .collect();
    let mut cabinet = Cabinet::from_parts(parts.iter().map(|p| Cursor::new(p.as_slice()))).unwrap();
    assert_eq!(cabinet.entries().len(), 6);
    let names: Vec<_> = cabinet.entries().iter().map(|e| e.name.clone()).collect();
    for name in names {
        let size = cabinet.entry(&name).unwrap().size as usize;
        assert_eq!(
            cabinet
                .read_file_bytes(&name, cabinet_fuzz::ARCHIVE_LIMIT)
                .unwrap()
                .len(),
            size
        );
    }
    assert!(cabinet_fuzz::spanning(&encode_parts(&parts)));
    assert!(!cabinet_fuzz::spanning(&encode_parts(&parts[..4])));
    let mut reordered = parts.clone();
    reordered.swap(0, 1);
    assert!(!cabinet_fuzz::spanning(&encode_parts(&reordered)));
}
#[test]
fn spanning_framing_rejects_truncation_oversized_counts_and_trailing_bytes() {
    for data in [
        &[][..],
        &[0],
        &[65],
        &[1],
        &[1, 255, 255, 255, 255],
        &[1, 0, 0, 0, 0, 42],
    ] {
        assert!(!cabinet_fuzz::spanning(data));
    }
}
#[test]
fn every_writer_family_window_and_level_is_reachable() {
    for codec in 0..4u8 {
        let windows = match codec {
            2 => 7,
            3 => 12,
            _ => 1,
        };
        let levels = if codec == 3 { 7 } else { 1 };
        for window in 0..windows {
            for level in 0..levels {
                let mut data = vec![codec, window, level];
                let expected = match codec {
                    0 => WriteCompression::None,
                    1 => WriteCompression::MsZip,
                    2 => WriteCompression::Lzx {
                        window_order: 15 + window,
                    },
                    _ => WriteCompression::Quantum {
                        window_order: 10 + window,
                        level: 1 + level,
                    },
                };
                assert_eq!(roundtrip_compression(&data), Some(expected));
                data.extend((0..1024).map(|i| (i % 251) as u8));
                cabinet_fuzz::roundtrip(&data);
            }
        }
    }
}
#[test]
fn every_codec_crosses_frame_and_member_boundaries() {
    for codec in 0..4u8 {
        for length in [0, 1, 32767, 32768, 32769, 65535, 65536, 65537, WRITER_LIMIT] {
            let mut data = vec![codec, 0, 0];
            data.extend((0..length).map(|i| (i % 251) as u8));
            cabinet_fuzz::roundtrip(&data);
        }
    }
}
