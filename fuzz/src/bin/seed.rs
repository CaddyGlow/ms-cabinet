use cabinet::{Cabinet, CabinetBuilder};
use cabinet_fuzz::{WRITER_LIMIT, encode_parts, roundtrip_compression};
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};
fn copy_fixtures(source: &Path, output: &Path) {
    for entry in fs::read_dir(source).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            copy_fixtures(&path, output);
        } else if path.extension().is_some_and(|ext| ext == "cab") {
            fs::copy(&path, output.join(path.file_name().unwrap())).unwrap();
        }
    }
}
fn overlap_seed(count: u16) -> Vec<u8> {
    let start = 36 + usize::from(count) * 8;
    let length = start + usize::from(count) * 9;
    let mut bytes = vec![0; length];
    bytes[..4].copy_from_slice(b"MSCF");
    bytes[8..12].copy_from_slice(&(length as u32).to_le_bytes());
    bytes[16..20].copy_from_slice(&(start as u32).to_le_bytes());
    bytes[24..26].copy_from_slice(&[3, 1]);
    bytes[26..28].copy_from_slice(&count.to_le_bytes());
    for i in 0..usize::from(count) {
        bytes[36 + i * 8..40 + i * 8].copy_from_slice(&(start as u32).to_le_bytes());
        bytes[40 + i * 8..42 + i * 8].copy_from_slice(&count.to_le_bytes());
        bytes[start + i * 9 + 4..start + i * 9 + 6].copy_from_slice(&1u16.to_le_bytes());
        bytes[start + i * 9 + 6..start + i * 9 + 8].copy_from_slice(&1u16.to_le_bytes());
    }
    bytes
}
fn main() {
    let output = PathBuf::from(std::env::args_os().nth(1).expect("corpus directory"));
    for target in ["cab", "spanning", "roundtrip"] {
        fs::create_dir_all(output.join(target)).unwrap();
    }
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures");
    copy_fixtures(&fixtures, &output.join("cab"));
    for count in [1000, 50000] {
        let bytes = overlap_seed(count);
        assert!(Cabinet::new(Cursor::new(bytes.as_slice())).is_err());
        fs::write(
            output.join(format!("cab/reused-folder-ranges-{count}.cab")),
            bytes,
        )
        .unwrap();
    }
    let parts: Vec<_> = (1..=5)
        .map(|index| fs::read(fixtures.join(format!("cab-spanning/split-{index}.cab"))).unwrap())
        .collect();
    let set = encode_parts(&parts);
    assert!(
        cabinet_fuzz::spanning(&set),
        "positive spanning corpus must assemble and decode"
    );
    fs::write(output.join("spanning/complete-five-part-set"), &set).unwrap();
    let incomplete = encode_parts(&parts[..4]);
    assert!(!cabinet_fuzz::spanning(&incomplete));
    fs::write(output.join("spanning/incomplete-set"), incomplete).unwrap();
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
                data.extend((0..1024).map(|i| (i % 251) as u8));
                cabinet_fuzz::roundtrip(&data);
                fs::write(
                    output.join(format!("roundtrip/config-{codec}-{window}-{level}")),
                    &data,
                )
                .unwrap();
            }
        }
        // Include exact boundaries and a four-frame payload for each family.
        for length in [0, 32767, 32768, 32769, 65537, WRITER_LIMIT] {
            let mut data = vec![codec, 0, 0];
            data.extend((0..length).map(|i| (i % 251) as u8));
            cabinet_fuzz::roundtrip(&data);
            fs::write(
                output.join(format!("roundtrip/boundary-{codec}-{length}")),
                &data,
            )
            .unwrap();
            let mut builder = CabinetBuilder::new(roundtrip_compression(&data).unwrap());
            builder.add_file("payload.bin", &data[3..]).unwrap();
            let mut bytes = Cursor::new(Vec::new());
            builder.write(&mut bytes).unwrap();
            let mut cabinet = Cabinet::new(Cursor::new(bytes.get_ref().as_slice())).unwrap();
            assert_eq!(
                cabinet
                    .read_file_bytes("payload.bin", WRITER_LIMIT)
                    .unwrap(),
                &data[3..]
            );
            fs::write(
                output.join(format!("cab/generated-{codec}-{length}.cab")),
                bytes.into_inner(),
            )
            .unwrap();
        }
    }
    println!(
        "Seeded all 23 retained CAB vectors, two allocation-attack regressions, complete spanning decode, 93 codec configurations and 24 frame-boundary cases"
    );
}
