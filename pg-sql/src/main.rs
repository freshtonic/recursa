use std::io::{self, Read};
use std::process;

use clap::{Parser, Subcommand};
use recursa::Input;
use recursa_core::fmt::FormatStyle;

use pg_sql::ast::{parse_sql_file, parse_stats, FileItem, PsqlCommand, Statement};
use pg_sql::formatter::format_file;

#[derive(Parser)]
#[command(name = "pg-sql", about = "PostgreSQL SQL tools")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Format a SQL file
    Fmt {
        /// SQL file to format, or `-` for stdin
        file: String,
    },
    /// Report parse coverage for SQL files
    Coverage {
        /// SQL files to analyze
        files: Vec<String>,
    },
    /// Dump the first 200 chars of each Raw statement in a file
    DumpRaw {
        /// SQL file to analyze
        file: String,
        /// Maximum number of raw statements to print
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
}

fn read_sql(file: &str) -> String {
    if file == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).unwrap_or_else(|e| {
            eprintln!("error reading stdin: {e}");
            process::exit(1);
        });
        buf
    } else {
        std::fs::read_to_string(file).unwrap_or_else(|e| {
            eprintln!("error reading {file}: {e}");
            process::exit(1);
        })
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Fmt { file } => {
            let sql = read_sql(&file);
            let mut input = Input::new(&sql);
            let items = match parse_sql_file(&mut input) {
                Ok(items) => items,
                Err(e) => {
                    eprintln!("{e}");
                    process::exit(1);
                }
            };
            print!("{}", format_file(&items, FormatStyle::default()));
        }
        Command::DumpRaw { file, limit } => {
            let sql = read_sql(&file);
            let mut input = Input::new(&sql);
            let items = match parse_sql_file(&mut input) {
                Ok(items) => items,
                Err(e) => {
                    eprintln!("{e}");
                    process::exit(1);
                }
            };
            let mut count = 0usize;
            for item in &items {
                if let FileItem::Command(PsqlCommand::Statement(ts)) = item
                    && let Statement::Raw(r) = &ts.stmt
                {
                    let text = r.text.trim().replace('\n', " ");
                    let truncated: String = text.chars().take(200).collect();
                    println!("{truncated}");
                    count += 1;
                    if count >= limit {
                        break;
                    }
                }
            }
        }
        Command::Coverage { files } => {
            let mut total_structured = 0usize;
            let mut total_raw = 0usize;

            for file in &files {
                let sql = match std::fs::read_to_string(file) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("{file}: {e}");
                        continue;
                    }
                };
                let mut input = Input::new(&sql);
                let items = match parse_sql_file(&mut input) {
                    Ok(items) => items,
                    Err(e) => {
                        eprintln!("{file}: parse error: {e}");
                        continue;
                    }
                };

                let stats = parse_stats(&items);
                let total = stats.structured + stats.raw;
                if total > 0 {
                    println!(
                        "{file}: {}/{} ({:.0}%) structured, {} raw, {} directives",
                        stats.structured,
                        total,
                        stats.structured_pct(),
                        stats.raw,
                        stats.directives,
                    );
                }
                total_structured += stats.structured;
                total_raw += stats.raw;
            }

            if files.len() > 1 {
                let grand_total = total_structured + total_raw;
                let pct = if grand_total == 0 {
                    100.0
                } else {
                    (total_structured as f64 / grand_total as f64) * 100.0
                };
                println!(
                    "\nTotal: {total_structured}/{grand_total} ({pct:.0}%) structured, {total_raw} raw"
                );
            }
        }
    }
}
