use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};

use addrust::address::Address;
use addrust::config::Config;
use addrust::pipeline::Pipeline;

#[derive(Parser)]
#[command(name = "addrust", about = "Parse and standardize US addresses")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to config file (default: .addrust.toml)
    #[arg(long, global = true)]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse addresses from stdin
    Parse {
        /// Output format: "clean" (default), "full", or "tsv"
        #[arg(long, default_value = "clean")]
        format: String,
        /// Show timing information
        #[arg(long)]
        time: bool,
    },
    /// Generate a default .addrust.toml config file
    Init,
    /// List pipeline rules or dictionary tables
    List {
        #[command(subcommand)]
        what: ListCommands,
    },
    /// Interactive pipeline editor (coming soon)
    Configure,
}

#[derive(Subcommand)]
enum ListCommands {
    /// List all pipeline rules in order
    Rules,
    /// List dictionary tables (optionally show entries for a specific table)
    Tables {
        /// Table name to show entries for
        name: Option<String>,
    },
}

fn load_config(cli_config: &Option<PathBuf>) -> Config {
    let path = cli_config.clone().unwrap_or_else(|| PathBuf::from(".addrust.toml"));
    Config::load(&path)
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

fn run_parse(config: &Config, format: &str, time: bool) {
    let start = Instant::now();
    let pipeline = Pipeline::from_config(config);

    if time {
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
        let _ = writeln!(out, "{}", format_address(&addr, format));
        count += 1;
    }

    if time {
        let elapsed = parse_start.elapsed();
        eprintln!(
            "Parsed {} addresses in {:?} ({:.0} addr/sec)",
            count,
            elapsed,
            count as f64 / elapsed.as_secs_f64()
        );
    }
}

fn main() {
    let cli = Cli::parse();
    let config = load_config(&cli.config);

    match cli.command {
        Some(Commands::Parse { format, time }) => {
            run_parse(&config, &format, time);
        }
        Some(Commands::Init) => {
            eprintln!("addrust init: coming soon");
        }
        Some(Commands::List { what }) => match what {
            ListCommands::Rules => {
                eprintln!("addrust list rules: coming soon");
            }
            ListCommands::Tables { .. } => {
                eprintln!("addrust list tables: coming soon");
            }
        },
        Some(Commands::Configure) => {
            eprintln!("addrust configure: coming soon (interactive TUI)");
        }
        None => {
            // Backwards compat: bare `addrust` with stdin behaves like `parse`
            run_parse(&config, "clean", false);
        }
    }
}
