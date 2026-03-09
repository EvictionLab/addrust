pub mod address;
pub mod config;
pub mod init;
pub mod ops;
pub mod pipeline;
pub mod prepare;
pub mod tables;
pub mod tui;

use address::Address;
use pipeline::Pipeline;

/// Parse a single address string with default settings.
pub fn parse(input: &str) -> Address {
    let pipeline = Pipeline::default();
    pipeline.parse(input)
}

/// Parse a batch of address strings (parallel).
pub fn parse_batch(inputs: &[&str]) -> Vec<Address> {
    let pipeline = Pipeline::default();
    pipeline.parse_batch(inputs)
}
