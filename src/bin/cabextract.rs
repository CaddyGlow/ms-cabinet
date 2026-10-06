//! cabextract command-line entry point.
#[path = "../commands/cabextract.rs"]
mod command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    command::main()
}
