fn main() {
    loop {
        honggfuzz::fuzz!(|data: &[u8]| {
            cabinet_fuzz::spanning(data);
        });
    }
}
