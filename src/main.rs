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
    /// Parse addresses from stdin or DuckDB
    Parse {
        /// Output format: "clean" (default), "full", or "tsv" (stdin mode only)
        #[arg(long, default_value = "clean")]
        format: String,
        /// Show timing information
        #[arg(long)]
        time: bool,
        /// DuckDB database file path
        #[cfg(feature = "duckdb")]
        #[arg(long)]
        duckdb: Option<PathBuf>,
        /// Input table name (required with --duckdb)
        #[cfg(feature = "duckdb")]
        #[arg(long)]
        input_table: Option<String>,
        /// Output table name (default: {input_table}_parsed)
        #[cfg(feature = "duckdb")]
        #[arg(long)]
        output_table: Option<String>,
        /// Address column name (default: "address")
        #[cfg(feature = "duckdb")]
        #[arg(long, default_value = "address")]
        column: String,
    },
    /// Generate a default .addrust.toml config file
    Init,
    /// List pipeline rules or dictionary tables
    List {
        #[command(subcommand)]
        what: ListCommands,
    },
    /// Interactive pipeline editor
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

    match cli.command {
        Some(Commands::Parse {
            format,
            time,
            #[cfg(feature = "duckdb")]
            duckdb,
            #[cfg(feature = "duckdb")]
            input_table,
            #[cfg(feature = "duckdb")]
            output_table,
            #[cfg(feature = "duckdb")]
            column,
        }) => {
            let config = load_config(&cli.config);

            #[cfg(feature = "duckdb")]
            if let Some(ref db_path) = duckdb {
                let input = match input_table {
                    Some(ref t) => t.clone(),
                    None => {
                        eprintln!("Error: --input-table is required when using --duckdb");
                        std::process::exit(1);
                    }
                };
                let output = output_table
                    .unwrap_or_else(|| format!("{}_parsed", input));
                let db_str = db_path.to_str().unwrap_or_else(|| {
                    eprintln!("Error: invalid database path");
                    std::process::exit(1);
                });
                if let Err(e) = addrust::duckdb_io::run_duckdb(
                    &config, db_str, &input, &output, &column,
                ) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }

            run_parse(&config, &format, time);
        }
        Some(Commands::Init) => {
            let path = std::path::Path::new(".addrust.toml");
            if path.exists() {
                eprint!(".addrust.toml already exists. Overwrite? (y/N) ");
                let _ = io::stderr().flush();
                let mut answer = String::new();
                io::stdin().read_line(&mut answer).unwrap();
                if !answer.trim().eq_ignore_ascii_case("y") {
                    eprintln!("Aborted.");
                    std::process::exit(0);
                }
            }
            let content = addrust::init::generate_default_config();
            std::fs::write(path, content).unwrap();
            println!("Created .addrust.toml");
        }
        Some(Commands::List { what }) => {
            let config = load_config(&cli.config);
            match what {
                ListCommands::Rules => {
                    let pipeline = Pipeline::from_config(&config);
                    for (i, rule) in pipeline.rule_summaries().iter().enumerate() {
                        let status = if rule.enabled { " " } else { "x" };
                        println!("{:>3}. [{}] {:30} {:8} {}", i + 1, status, rule.label, format!("{:?}", rule.action), rule.pattern_template);
                    }
                }
                ListCommands::Tables { name } => {
                    use addrust::tables::abbreviations::build_default_tables;

                    let tables = build_default_tables();
                    let tables = if config.dictionaries.is_empty() {
                        tables
                    } else {
                        tables.patch(&config.dictionaries)
                    };

                    match name {
                        None => {
                            for name in tables.table_names() {
                                let table = tables.get(name).unwrap();
                                println!("{:20} ({} entries)", name, table.entries.len());
                            }
                        }
                        Some(ref name) => {
                            match tables.get(name) {
                                Some(table) => {
                                    println!("{} ({} entries):", name, table.entries.len());
                                    for entry in &table.entries {
                                        println!("  {:20} -> {}", entry.short, entry.long);
                                    }
                                }
                                None => {
                                    eprintln!("Unknown table: {}", name);
                                    eprintln!("Available: {}", tables.table_names().join(", "));
                                    std::process::exit(1);
                                }
                            }
                        }
                    }
                }
            }
        }
        Some(Commands::Configure) => {
            let config_path = cli.config.unwrap_or_else(|| PathBuf::from(".addrust.toml"));
            if let Err(e) = addrust::tui::run(config_path) {
                eprintln!("TUI error: {}", e);
                std::process::exit(1);
            }
        }
        None => {
            let config = load_config(&cli.config);
            run_parse(&config, "clean", false);
        }
    }
}
