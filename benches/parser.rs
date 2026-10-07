//! Parser work for valid blocks and adversarial overlapping folder ranges.
use cabinet::Cabinet;
use std::{
    cell::Cell,
    hint::black_box,
    io::{self, Cursor, Read, Seek, SeekFrom},
    rc::Rc,
    time::Instant,
};
struct Counting {
    inner: Cursor<Vec<u8>>,
    reads: Rc<Cell<usize>>,
}
impl Read for Counting {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.reads.set(self.reads.get() + 1);
        self.inner.read(output)
    }
}
impl Seek for Counting {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}
fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn fixture(folders: u16, blocks: u16, overlap: bool) -> Vec<u8> {
    let start = 36 + usize::from(folders) * 8;
    let data_folders = if overlap { 1 } else { usize::from(folders) };
    let length = start + data_folders * usize::from(blocks) * 9;
    let mut bytes = vec![0; length];
    bytes[..4].copy_from_slice(b"MSCF");
    put32(&mut bytes, 8, length as u32);
    put32(&mut bytes, 16, start as u32);
    bytes[24..26].copy_from_slice(&[3, 1]);
    put16(&mut bytes, 26, folders);
    for i in 0..usize::from(folders) {
        let location = start
            + if overlap {
                0
            } else {
                i * usize::from(blocks) * 9
            };
        put32(&mut bytes, 36 + i * 8, location as u32);
        put16(&mut bytes, 40 + i * 8, blocks);
    }
    for i in 0..data_folders * usize::from(blocks) {
        put16(&mut bytes, start + i * 9 + 4, 1);
        put16(&mut bytes, start + i * 9 + 6, 1);
    }
    bytes
}
fn main() {
    println!("case,input_bytes,iterations,read_calls,parse_us");
    for (name, folders, blocks, overlap, iterations) in [
        ("valid-1x1000", 1, 1000, false, 100),
        ("valid-100x10", 100, 10, false, 100),
        ("overlap-100x100", 100, 100, true, 30),
        ("overlap-1000x1000", 1000, 1000, true, 10),
    ] {
        let bytes = fixture(folders, blocks, overlap);
        let mut elapsed = 0.0;
        let mut read_calls = 0;
        for _ in 0..iterations {
            let reads = Rc::new(Cell::new(0));
            let input = Counting {
                inner: Cursor::new(bytes.clone()),
                reads: reads.clone(),
            };
            let start = Instant::now();
            let result = Cabinet::new(black_box(input));
            elapsed += start.elapsed().as_secs_f64();
            assert_eq!(result.is_err(), overlap);
            let _ = black_box(result);
            read_calls = reads.get();
        }
        println!(
            "{name},{},{iterations},{read_calls},{:.3}",
            bytes.len(),
            elapsed * 1e6 / f64::from(iterations)
        );
    }
}
