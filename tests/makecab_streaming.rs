#![cfg(all(feature = "cli", target_os = "linux"))]

use std::{io::Read, process::Command};

#[test]
fn makecab_streams_payload_larger_than_process_address_limit() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("large");
    let output = root.path().join("large.cab");
    std::fs::File::create(&input)
        .unwrap()
        .set_len(128 << 20)
        .unwrap();
    // Run only the child with a constrained address space. Payload retention
    // would exceed this ceiling before any compression or output could start.
    let result = Command::new("sh")
        .args(["-c", "ulimit -v 98304 && exec \"$@\"", "makecab-limited"])
        .arg(env!("CARGO_BIN_EXE_makecab"))
        .args(["--compression", "none", "--output"])
        .arg(&output)
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let mut cabinet = cabinet::Cabinet::open(&output).unwrap();
    let mut reader = cabinet.read_file("large").unwrap();
    let mut buffer = [0u8; 32768];
    let mut total = 0u64;
    loop {
        let count = reader.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        assert!(buffer[..count].iter().all(|byte| *byte == 0));
        total += count as u64;
    }
    assert_eq!(total, 128 << 20);
}
