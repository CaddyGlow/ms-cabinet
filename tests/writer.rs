//! Writer round trips and independent CAB interoperability.
use cabinet::{Cabinet, CabinetBuilder, Compression, WriteCompression};
use std::{
    io::{self, Cursor, Seek, SeekFrom, Write},
    process::Command,
};

fn methods() -> Vec<WriteCompression> {
    vec![
        WriteCompression::None,
        WriteCompression::MsZip,
        WriteCompression::Lzx { window_order: 15 },
        WriteCompression::Lzx { window_order: 21 },
        WriteCompression::Quantum {
            level: 1,
            window_order: 10,
        },
        WriteCompression::Quantum {
            level: 7,
            window_order: 21,
        },
    ]
}
fn payloads() -> Vec<Vec<u8>> {
    let mut state = 0x1234_5678u32;
    let random: Vec<_> = (0..100_001)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    vec![
        Vec::new(),
        b"CAB writer literal and repeating match data\n".repeat(2200),
        random,
        vec![0xe8; 65_537],
        b"final member".to_vec(),
    ]
}

#[test]
fn all_methods_round_trip_multiframe_multimember_cabinets() {
    let data = payloads();
    for method in methods() {
        let mut builder = CabinetBuilder::new(method);
        for (name, bytes) in [
            "empty",
            "nested/repeated.txt",
            "random.bin",
            "unicode-é.bin",
            "last",
        ]
        .into_iter()
        .zip(&data)
        {
            builder.add_file(name, bytes).unwrap();
        }
        let mut output = Cursor::new(Vec::new());
        let size = builder.write(&mut output).unwrap();
        assert_eq!(size, output.get_ref().len() as u64);
        assert_eq!(output.position(), size);
        let mut cabinet = Cabinet::new(Cursor::new(output.into_inner())).unwrap();
        let declared = match method {
            WriteCompression::None => Compression::None,
            WriteCompression::MsZip => Compression::MsZip,
            WriteCompression::Lzx { window_order } => Compression::Lzx { window_order },
            WriteCompression::Quantum {
                level,
                window_order,
            } => Compression::Quantum {
                level,
                window_order,
            },
        };
        // Reading a later member first exercises retained history and frame skipping.
        for index in (0..data.len()).rev() {
            let entry = &cabinet.entries()[index];
            assert_eq!(entry.compression, declared);
            let name = entry.name.clone();
            assert_eq!(
                cabinet.read_file_bytes(&name, data[index].len()).unwrap(),
                data[index],
                "{method:?}: {name}"
            );
        }
    }
}

#[test]
fn mszip_incompressible_full_frame_uses_bounded_final_stored_block() {
    let payload = &payloads()[2][..32768];
    let mut builder = CabinetBuilder::new(WriteCompression::MsZip);
    builder.add_file("random.bin", payload).unwrap();
    let mut output = Cursor::new(Vec::new());
    builder.write(&mut output).unwrap();
    let bytes = output.into_inner();
    let offset = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize;
    assert_eq!(u16::from_le_bytes(bytes[40..42].try_into().unwrap()), 1);
    assert_eq!(
        u16::from_le_bytes(bytes[offset + 4..offset + 6].try_into().unwrap()),
        32775
    );
    assert_eq!(&bytes[offset + 8..offset + 15], b"CK\x01\x00\x80\xff\x7f");
    assert_eq!(&bytes[offset + 15..], payload);
    let mut cabinet = Cabinet::new(Cursor::new(bytes)).unwrap();
    assert_eq!(
        cabinet
            .read_file_bytes("random.bin", payload.len())
            .unwrap(),
        payload
    );
}

#[test]
fn independent_7z_extracts_all_writer_methods() {
    let tool = std::env::var_os("CABINET_7Z").unwrap_or_else(|| "7z".into());
    if Command::new(&tool).arg("i").output().is_err() {
        if std::env::var_os("CABINET_REQUIRE_7Z").is_some() {
            panic!("independent 7z reader required");
        }
        eprintln!("skipping independent reader: install 7z or set CABINET_7Z");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let data = payloads();
    for (index, method) in methods().into_iter().enumerate() {
        let mut builder = CabinetBuilder::new(method);
        builder.add_file("repeated.bin", &data[1]).unwrap();
        builder.add_file("random.bin", &data[2]).unwrap();
        builder.add_file("e8.bin", &data[3]).unwrap();
        let path = directory.path().join(format!("{index}.cab"));
        builder
            .write(&mut std::fs::File::create(&path).unwrap())
            .unwrap();
        for (name, bytes) in ["repeated.bin", "random.bin", "e8.bin"]
            .into_iter()
            .zip(&data[1..4])
        {
            let result = Command::new(&tool)
                .args(["x", "-so"])
                .arg(&path)
                .arg(name)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{method:?}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(&result.stdout, bytes, "{method:?}: {name}");
        }
    }
}

#[test]
fn invalid_names_duplicates_and_configuration_are_rejected_before_writing() {
    let mut builder = CabinetBuilder::new(WriteCompression::None);
    for name in [
        "",
        "/absolute",
        "../escape",
        "a/../b",
        "a//b",
        "C:\\file",
        "nul\0byte",
    ] {
        assert!(builder.add_file(name, b"data").is_err(), "{name:?}");
    }
    builder.add_file("Folder/File", b"data").unwrap();
    assert!(builder.add_file("folder\\file", b"data").is_err());
    for method in [
        WriteCompression::Lzx { window_order: 14 },
        WriteCompression::Quantum {
            level: 0,
            window_order: 10,
        },
        WriteCompression::Quantum {
            level: 1,
            window_order: 22,
        },
    ] {
        let mut output = Cursor::new(Vec::new());
        assert!(CabinetBuilder::new(method).write(&mut output).is_err());
        assert!(output.into_inner().is_empty());
    }
}

#[test]
fn empty_cabinets_and_empty_members_round_trip() {
    for method in methods() {
        let mut builder = CabinetBuilder::new(method);
        let mut output = Cursor::new(Vec::new());
        builder.write(&mut output).unwrap();
        assert!(
            Cabinet::new(Cursor::new(output.into_inner()))
                .unwrap()
                .entries()
                .is_empty()
        );
        builder.add_file("empty", b"").unwrap();
        let mut output = Cursor::new(Vec::new());
        builder.write(&mut output).unwrap();
        assert!(
            Cabinet::new(Cursor::new(output.into_inner()))
                .unwrap()
                .read_file_bytes("empty", 0)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn compression_methods_reduce_repetitive_payloads() {
    let data = b"compressible CAB data\n".repeat(4000);
    for method in methods()
        .into_iter()
        .filter(|m| *m != WriteCompression::None)
    {
        let mut builder = CabinetBuilder::new(method);
        builder.add_file("data", &data).unwrap();
        let size = builder.write(&mut Cursor::new(Vec::new())).unwrap();
        assert!(size < data.len() as u64 / 2, "{method:?}: {size}");
    }
}

#[test]
fn embedded_archive_offsets_are_relative_to_its_header() {
    let mut builder = CabinetBuilder::new(WriteCompression::MsZip);
    builder.add_file("file", b"payload").unwrap();
    let mut output = Cursor::new(vec![42; 13]);
    output.seek(SeekFrom::End(0)).unwrap();
    let size = builder.write(&mut output).unwrap();
    let bytes = output.into_inner();
    assert_eq!(&bytes[..13], &[42; 13]);
    assert_eq!(bytes.len(), 13 + size as usize);
    assert_eq!(
        Cabinet::new(Cursor::new(&bytes[13..]))
            .unwrap()
            .read_file_bytes("file", 7)
            .unwrap(),
        b"payload"
    );
}

struct FailingOutput(Cursor<Vec<u8>>);
impl Write for FailingOutput {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("injected write failure"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Seek for FailingOutput {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.0.seek(position)
    }
}
#[test]
fn output_failures_are_returned_to_the_caller() {
    let mut builder = CabinetBuilder::new(WriteCompression::MsZip);
    builder.add_file("file", b"payload").unwrap();
    assert!(
        builder
            .write(&mut FailingOutput(Cursor::new(Vec::new())))
            .is_err()
    );
}

#[test]
fn every_encoder_window_and_quantum_level_produces_valid_frames() {
    let payload = b"dictionary parameter coverage\n".repeat(1200);
    let methods = (15..=21)
        .map(|window_order| WriteCompression::Lzx { window_order })
        .chain((10..=21).flat_map(|window_order| {
            (1..=7).map(move |level| WriteCompression::Quantum {
                level,
                window_order,
            })
        }));
    for method in methods {
        let mut builder = CabinetBuilder::new(method);
        builder.add_file("data", &payload).unwrap();
        let mut output = Cursor::new(Vec::new());
        builder.write(&mut output).unwrap();
        assert_eq!(
            Cabinet::new(Cursor::new(output.into_inner()))
                .unwrap()
                .read_file_bytes("data", payload.len())
                .unwrap(),
            payload,
            "{method:?}"
        );
    }
}

#[test]
fn quantum_long_distance_matches_interoperate_with_independent_reader() {
    let mut state = 0xaabb_ccddu32;
    let mut payload: Vec<u8> = (0..150_000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    payload.extend_from_within(..);
    let mut builder = CabinetBuilder::new(WriteCompression::Quantum {
        level: 7,
        window_order: 21,
    });
    builder.add_file("long-distance.bin", &payload).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("quantum.cab");
    let size = builder
        .write(&mut std::fs::File::create(&path).unwrap())
        .unwrap();
    assert!(size < payload.len() as u64 * 3 / 4);
    assert_eq!(
        Cabinet::open(&path)
            .unwrap()
            .read_file_bytes("long-distance.bin", payload.len())
            .unwrap(),
        payload
    );
    let tool = std::env::var_os("CABINET_7Z").unwrap_or_else(|| "7z".into());
    match Command::new(&tool)
        .args(["x", "-so"])
        .arg(&path)
        .arg("long-distance.bin")
        .output()
    {
        Ok(result) => {
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(result.stdout, payload);
        }
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && std::env::var_os("CABINET_REQUIRE_7Z").is_none() =>
        {
            eprintln!("independent 7z not installed")
        }
        Err(error) => panic!("independent CAB extraction failed: {error}"),
    }
}
