//! makecab command-line entry point.
#[path = "../commands/makecab.rs"]
mod command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    command::main()
}
