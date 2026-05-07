pub mod address;
pub mod config;
pub mod init;
pub mod ops;
pub mod pattern;
pub mod pipeline;
pub mod prepare;
pub mod step;
pub mod tables;
// `collapsible_match` triggers heavily on the keyboard-handler match/if pattern
// throughout tabs.rs and panel.rs. The explicit form is clearer than match guards
// for multi-statement arms. Revisit during the 0.1.5 TUI rewrite.
#[cfg(feature = "cli")]
#[allow(clippy::collapsible_match)]
pub mod tui;

#[cfg(feature = "duckdb")]
pub mod duckdb_io;

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
