//! Stored, independent media cabinets without MSI table or grouping policy.
use cabinet::{Cabinet, CabinetBuilder, Compression, WriteCompression};
use std::io::{self, Cursor, Seek, SeekFrom, Write};

fn stored_members(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = CabinetBuilder::new(WriteCompression::None);
    for &(name, payload) in members {
        builder.add_file(name, payload).unwrap();
    }
    let mut output = Cursor::new(Vec::new());
    let size = builder.write(&mut output).unwrap();
    assert_eq!(size, output.position());
    output.into_inner()
}

#[test]
fn borrowed_members_keep_registration_order_and_frame_boundary_payloads() {
    let payloads: Vec<Vec<u8>> = [0, 1, 32767, 32768, 32769, 65536]
        .into_iter()
        .map(|length| (0..length).map(|index| (index % 251) as u8).collect())
        .collect();
    let names = ["F_z", "F_a", "F_30", "F_20", "F_10", "F_last"];
    let members: Vec<_> = names
        .iter()
        .zip(&payloads)
        .map(|(name, data)| (*name, data.as_slice()))
        .collect();
    let bytes = stored_members(&members);
    let mut cabinet = Cabinet::new(Cursor::new(bytes.as_slice())).unwrap();
    assert_eq!(
        cabinet
            .entries()
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        names
    );
    for &(name, payload) in members.iter().rev() {
        assert_eq!(cabinet.entry(name).unwrap().compression, Compression::None);
        assert_eq!(
            cabinet.read_file_bytes(name, payload.len()).unwrap(),
            payload
        );
    }
}

#[test]
fn multiple_cabinets_are_independent_and_can_reuse_file_identifiers() {
    for payload in [b"first medium".as_slice(), b"second medium".as_slice()] {
        let bytes = stored_members(&[("F_shared_identifier", payload), ("F_empty", b"")]);
        // Independent cabinets have no previous/next-cabinet flags or set indices.
        assert_eq!(&bytes[30..36], &[0; 6]);
        let mut cabinet = Cabinet::new(Cursor::new(bytes)).unwrap();
        assert_eq!(cabinet.entries().len(), 2);
        assert_eq!(
            cabinet
                .read_file_bytes("F_shared_identifier", payload.len())
                .unwrap(),
            payload
        );
        assert!(cabinet.read_file_bytes("F_empty", 0).unwrap().is_empty());
    }
}

#[test]
fn stored_media_bytes_are_deterministic_across_repeated_completion() {
    let payload = vec![0xa7; 32769];
    let members = [
        ("F_payload", payload.as_slice()),
        ("F_empty", b"".as_slice()),
    ];
    let mut builder = CabinetBuilder::new(WriteCompression::None);
    for &(name, data) in &members {
        builder.add_file(name, data).unwrap();
    }
    let expected = stored_members(&members);
    for _ in 0..3 {
        let mut output = Cursor::new(Vec::new());
        builder.write(&mut output).unwrap();
        assert_eq!(output.into_inner(), expected);
    }
}

#[derive(Clone, Copy)]
enum Failure {
    None,
    HeaderSeek,
    HeaderWrite,
    EndSeek,
}

struct PartialOutput {
    bytes: Cursor<Vec<u8>>,
    failure: Failure,
    finalizing: bool,
}

impl Write for PartialOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.finalizing && matches!(self.failure, Failure::HeaderWrite) {
            return Err(io::Error::other("injected header rewrite failure"));
        }
        self.bytes.write(&bytes[..bytes.len().min(3)])
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for PartialOutput {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        if matches!(position, SeekFrom::Start(0)) {
            self.finalizing = true;
            if matches!(self.failure, Failure::HeaderSeek) {
                return Err(io::Error::other("injected header seek failure"));
            }
        } else if self.finalizing
            && matches!(position, SeekFrom::Start(_))
            && matches!(self.failure, Failure::EndSeek)
        {
            return Err(io::Error::other("injected end seek failure"));
        }
        self.bytes.seek(position)
    }
}

#[test]
fn partial_writes_produce_the_same_completed_stored_cabinet() {
    let payload = vec![0x45; 32769];
    let mut builder = CabinetBuilder::new(WriteCompression::None);
    builder.add_file("F_partial", &payload).unwrap();
    let mut output = PartialOutput {
        bytes: Cursor::new(Vec::new()),
        failure: Failure::None,
        finalizing: false,
    };
    let size = builder.write(&mut output).unwrap();
    assert_eq!(output.bytes.position(), size);
    assert_eq!(
        output.bytes.into_inner(),
        stored_members(&[("F_partial", &payload)])
    );
}

#[test]
fn finalization_seek_and_header_write_failures_reach_the_caller() {
    let mut builder = CabinetBuilder::new(WriteCompression::None);
    builder.add_file("F_payload", b"payload").unwrap();
    for (failure, message) in [
        (Failure::HeaderSeek, "injected header seek failure"),
        (Failure::HeaderWrite, "injected header rewrite failure"),
        (Failure::EndSeek, "injected end seek failure"),
    ] {
        let mut output = PartialOutput {
            bytes: Cursor::new(Vec::new()),
            failure,
            finalizing: false,
        };
        let error = builder.write(&mut output).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(error.to_string(), message);
    }
}

#[test]
fn bounded_member_extraction_rejects_large_members_and_allows_the_exact_limit() {
    let payload = vec![0x69; 32769];
    let bytes = stored_members(&[("F_payload", &payload)]);
    let mut cabinet = Cabinet::new(Cursor::new(bytes)).unwrap();
    assert!(
        cabinet
            .read_file_bytes("F_payload", payload.len() - 1)
            .is_err()
    );
    assert_eq!(
        cabinet.read_file_bytes("F_payload", payload.len()).unwrap(),
        payload
    );
}
