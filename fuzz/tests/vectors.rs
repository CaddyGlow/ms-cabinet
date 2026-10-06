use std::path::Path;
fn replay_tree(path: &Path, count: &mut usize) {
    for entry in std::fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            replay_tree(&path, count);
        } else if path.extension().is_some_and(|ext| ext == "cab") {
            let bytes = std::fs::read(&path).unwrap();
            cabinet_fuzz::cab(&bytes);
            cabinet_fuzz::spanning(&bytes);
            *count += 1;
        }
    }
}
#[test]
fn all_retained_cab_vectors() {
    let mut count = 0;
    replay_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures"),
        &mut count,
    );
    assert!(
        count >= 23,
        "retained CAB corpus unexpectedly shrank: {count}"
    );
}
#[test]
fn every_writer_family_and_window() {
    for selector in 0..=255 {
        let mut data = vec![selector; 257];
        data.extend(0..=255);
        cabinet_fuzz::roundtrip(&data);
    }
}
