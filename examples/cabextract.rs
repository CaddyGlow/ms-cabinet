//! Run the installable cabextract command as a Cargo example.
#[path = "../src/commands/cabextract.rs"]
mod command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    command::main()
}
