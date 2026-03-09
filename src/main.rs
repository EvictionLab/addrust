use std::io::{self, BufRead, Write};
use std::time::Instant;

use clap::Parser;

use addrust::address::Address;
use addrust::pipeline::{Pipeline, PipelineConfig};
use addrust::tables::build_rules;
use addrust::tables::abbreviations::ABBR;

#[derive(Parser)]
#[command(name = "addrust", about = "Parse and standardize US addresses")]
struct Cli {
    /// Disable specific rule groups (comma-separated)
    #[arg(long, value_delimiter = ',')]
    disable_groups: Vec<String>,

    /// Disable specific rules by label (comma-separated)
    #[arg(long, value_delimiter = ',')]
    disable_rules: Vec<String>,

    /// Output format: "clean" (default), "full", or "tsv"
    #[arg(long, default_value = "clean")]
    format: String,

    /// Show timing information
    #[arg(long)]
    time: bool,
}

fn format_address(addr: &Address, format: &str) -> String {
    match format {
        "full" => format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            addr.clean_address().unwrap_or_default(),
            addr.street_number.as_deref().unwrap_or(""),
            addr.pre_direction.as_deref().unwrap_or(""),
            addr.street_name.as_deref().unwrap_or(""),
            addr.suffix.as_deref().unwrap_or(""),
            addr.post_direction.as_deref().unwrap_or(""),
            addr.unit.as_deref().unwrap_or(""),
            addr.unit_type.as_deref().unwrap_or(""),
            addr.po_box.as_deref().unwrap_or(""),
            addr.building.as_deref().unwrap_or(""),
        ),
        "tsv" => {
            let parts = [
                addr.po_box.as_deref().unwrap_or(""),
                addr.street_number.as_deref().unwrap_or(""),
                addr.pre_direction.as_deref().unwrap_or(""),
                addr.street_name.as_deref().unwrap_or(""),
                addr.suffix.as_deref().unwrap_or(""),
                addr.post_direction.as_deref().unwrap_or(""),
                addr.unit_type.as_deref().unwrap_or(""),
                addr.unit.as_deref().unwrap_or(""),
                addr.building.as_deref().unwrap_or(""),
            ];
            parts.join("\t")
        }
        _ => addr.clean_address().unwrap_or_default(),
    }
}

fn main() {
    let cli = Cli::parse();

    let config = PipelineConfig {
        disabled_rules: cli.disable_rules,
        disabled_groups: cli.disable_groups,
    };

    let start = Instant::now();

    let rules = build_rules(&ABBR);
    let pipeline = Pipeline::new(rules, &config);

    if cli.time {
        eprintln!("Pipeline built in {:?}", start.elapsed());
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    let parse_start = Instant::now();
    let mut count = 0;

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let addr = pipeline.parse(trimmed);
        let _ = writeln!(out, "{}", format_address(&addr, &cli.format));
        count += 1;
    }

    if cli.time {
        let elapsed = parse_start.elapsed();
        eprintln!(
            "Parsed {} addresses in {:?} ({:.0} addr/sec)",
            count,
            elapsed,
            count as f64 / elapsed.as_secs_f64()
        );
    }
}
