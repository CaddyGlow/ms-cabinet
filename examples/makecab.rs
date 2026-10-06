//! Run the installable makecab command as a Cargo example.
#[path = "../src/commands/makecab.rs"]
mod command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    command::main()
}
