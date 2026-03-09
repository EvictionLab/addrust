pub mod address;
pub mod ops;
pub mod pipeline;
pub mod prepare;
pub mod config;
pub mod tables;

use address::Address;
use pipeline::{Pipeline, PipelineConfig};
use tables::build_rules;

/// Parse a single address string with default settings.
pub fn parse(input: &str) -> Address {
    let rules = build_rules();
    let config = PipelineConfig::default();
    let pipeline = Pipeline::new(rules, &config);
    pipeline.parse(input)
}

/// Parse a batch of address strings (parallel).
pub fn parse_batch(inputs: &[&str]) -> Vec<Address> {
    let rules = build_rules();
    let config = PipelineConfig::default();
    let pipeline = Pipeline::new(rules, &config);
    pipeline.parse_batch(inputs)
}
