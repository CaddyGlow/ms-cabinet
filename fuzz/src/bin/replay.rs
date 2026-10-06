fn main() {
    let mut args = std::env::args().skip(1);
    let target = args.next().expect("target: cab, spanning, roundtrip");
    for path in args {
        let data = std::fs::read(path).unwrap();
        match target.as_str() {
            "cab" => cabinet_fuzz::cab(&data),
            "spanning" => cabinet_fuzz::spanning(&data),
            "roundtrip" => cabinet_fuzz::roundtrip(&data),
            _ => panic!("unknown target"),
        }
    }
}
