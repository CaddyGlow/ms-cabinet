//! Lazy source creation preserves solid CAB framing without retaining payloads.
use cabinet::{Cabinet, CabinetBuilder, WriteCompression};
use std::{
    cell::Cell,
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    rc::Rc,
};

fn methods() -> [WriteCompression; 4] {
    [
        WriteCompression::None,
        WriteCompression::MsZip,
        WriteCompression::Lzx { window_order: 15 },
        WriteCompression::Quantum {
            level: 6,
            window_order: 15,
        },
    ]
}
struct ShortReader<'a> {
    bytes: &'a [u8],
    position: usize,
    active: Rc<Cell<usize>>,
    interrupted: bool,
}
impl Read for ShortReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        assert!(output.len() <= 32768);
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let count = output.len().min(113).min(self.bytes.len() - self.position);
        output[..count].copy_from_slice(&self.bytes[self.position..self.position + count]);
        self.position += count;
        Ok(count)
    }
}
impl Drop for ShortReader<'_> {
    fn drop(&mut self) {
        self.active.set(self.active.get() - 1);
    }
}
#[test]
fn short_reads_match_byte_api_across_file_and_frame_boundaries_for_every_codec() {
    let payloads: Vec<Vec<u8>> = [0, 32767, 2, 65536 + 123]
        .into_iter()
        .map(|len| (0..len).map(|i| (i % 251) as u8).collect())
        .collect();
    let names = ["empty", "first", "second", "last"];
    for method in methods() {
        let mut bytes_builder = CabinetBuilder::new(method);
        let mut readers_builder = CabinetBuilder::new(method);
        for (name, data) in names.iter().zip(&payloads) {
            bytes_builder
                .add_file_with_metadata(name, data, 0x5821, 0x6042, 0x21)
                .unwrap();
            readers_builder
                .add_file_source_with_metadata(name, data.len() as u64, 0x5821, 0x6042, 0x21)
                .unwrap();
        }
        let mut expected = Cursor::new(vec![42; 13]);
        expected.set_position(13);
        bytes_builder.write(&mut expected).unwrap();
        let mut actual = Cursor::new(vec![42; 13]);
        actual.set_position(13);
        let active = Rc::new(Cell::new(0));
        let mut opened = 0;
        let mut open = |i: usize| -> io::Result<Box<dyn Read + '_>> {
            assert_eq!(i, opened);
            assert_eq!(active.get(), 0);
            opened += 1;
            active.set(1);
            Ok(Box::new(ShortReader {
                bytes: &payloads[i],
                position: 0,
                active: active.clone(),
                interrupted: false,
            }))
        };
        let size = readers_builder
            .write_from_readers(&mut actual, &mut open)
            .unwrap();
        assert_eq!(opened, payloads.len());
        assert_eq!(active.get(), 0);
        assert_eq!(actual.position(), 13 + size);
        assert_eq!(actual.get_ref(), expected.get_ref(), "{method:?}");
        let mut cabinet = Cabinet::new(Cursor::new(&actual.get_ref()[13..])).unwrap();
        for (name, data) in names.iter().zip(&payloads) {
            assert_eq!(cabinet.read_file_bytes(name, data.len()).unwrap(), *data);
        }
    }
}
#[test]
fn declared_sizes_reject_short_long_and_nonempty_empty_sources() {
    for method in methods() {
        for (declared, actual, kind) in [
            (10, 9, io::ErrorKind::UnexpectedEof),
            (10, 11, io::ErrorKind::InvalidData),
            (0, 1, io::ErrorKind::InvalidData),
        ] {
            let mut builder = CabinetBuilder::new(method);
            builder.add_file_source("file", declared).unwrap();
            let error = builder
                .write_from_readers(&mut Cursor::new(Vec::new()), &mut |_| {
                    Ok(Box::new(Cursor::new(vec![0; actual])))
                })
                .unwrap_err();
            assert_eq!(error.kind(), kind, "{method:?}");
        }
    }
}
#[test]
fn invalid_registration_does_not_reserve_name_and_byte_write_rejects_missing_data() {
    let mut builder = CabinetBuilder::new(WriteCompression::None);
    assert!(builder.add_file_source("file", u64::MAX).is_err());
    assert!(
        builder
            .add_file_source_with_metadata("file", 1, 0, 0, 0)
            .is_err()
    );
    builder.add_file_source("file", 0).unwrap();
    assert!(builder.add_file_source("FILE", 0).is_err());
    assert!(builder.add_file_source("../outside", 0).is_err());
    let mut output = Cursor::new(Vec::new());
    assert!(builder.write(&mut output).is_err());
    assert!(output.get_ref().is_empty());
}
struct Discard {
    position: u64,
    written: Rc<Cell<u64>>,
    fail_at: Option<u64>,
}
impl Write for Discard {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .fail_at
            .is_some_and(|end| self.position + bytes.len() as u64 > end)
        {
            return Err(io::Error::other("injected write failure"));
        }
        self.position += bytes.len() as u64;
        self.written.set(self.written.get().max(self.position));
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Seek for Discard {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.position = match position {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(n) => self.position.checked_add_signed(n).unwrap(),
            SeekFrom::End(n) => self.written.get().checked_add_signed(n).unwrap(),
        };
        Ok(self.position)
    }
}
struct Generated {
    position: u64,
    size: u64,
    written: Rc<Cell<u64>>,
}
impl Read for Generated {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        assert!(output.len() <= 32768);
        assert!(
            self.written.get() + 32768 >= self.position,
            "payload retained instead of streamed"
        );
        let n = output.len().min((self.size - self.position) as usize);
        output[..n].fill(17);
        self.position += n as u64;
        Ok(n)
    }
}
#[test]
fn generated_large_payload_reaches_output_before_input_is_exhausted() {
    let size = 16 << 20;
    let written = Rc::new(Cell::new(0));
    let mut output = Discard {
        position: 0,
        written: written.clone(),
        fail_at: None,
    };
    let mut builder = CabinetBuilder::new(WriteCompression::None);
    builder.add_file_source("large", size).unwrap();
    let result = builder
        .write_from_readers(&mut output, &mut |_| {
            Ok(Box::new(Generated {
                position: 0,
                size,
                written: written.clone(),
            }))
        })
        .unwrap();
    assert!(result > size);
    assert_eq!(output.position, result);
}
struct FailRead;
impl Read for FailRead {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("injected read failure"))
    }
}
#[test]
fn opener_reader_and_frame_write_failures_are_propagated() {
    let mut builder = CabinetBuilder::new(WriteCompression::MsZip);
    builder.add_file_source("file", 32768).unwrap();
    let open_error = builder
        .write_from_readers(&mut Cursor::new(Vec::new()), &mut |_| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "open denied",
            ))
        })
        .unwrap_err();
    assert_eq!(open_error.kind(), io::ErrorKind::PermissionDenied);
    let read_error = builder
        .write_from_readers(&mut Cursor::new(Vec::new()), &mut |_| {
            Ok(Box::new(FailRead))
        })
        .unwrap_err();
    assert_eq!(read_error.to_string(), "injected read failure");
    let mut output = Discard {
        position: 0,
        written: Rc::new(Cell::new(0)),
        fail_at: Some(65),
    };
    let write_error = builder
        .write_from_readers(&mut output, &mut |_| {
            Ok(Box::new(Cursor::new(vec![1; 32768])))
        })
        .unwrap_err();
    assert_eq!(write_error.to_string(), "injected write failure");
}
