use std::io::{self, Read};
use std::process;

use clap::{Parser, Subcommand};
use recursa::Input;
use recursa_core::fmt::FormatStyle;

use pg_sql::ast::parse_sql_file;
use pg_sql::formatter::format_tokens_sql;

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
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Fmt { file } => {
            let sql = if file == "-" {
                let mut buf = String::new();
                io::stdin().read_to_string(&mut buf).unwrap_or_else(|e| {
                    eprintln!("error reading stdin: {e}");
                    process::exit(1);
                });
                buf
            } else {
                std::fs::read_to_string(&file).unwrap_or_else(|e| {
                    eprintln!("error reading {file}: {e}");
                    process::exit(1);
                })
            };

            let mut input = Input::new(&sql);
            let commands = match parse_sql_file(&mut input) {
                Ok(cmds) => cmds,
                Err(e) => {
                    eprintln!("{e}");
                    process::exit(1);
                }
            };

            let style = FormatStyle::default();
            for cmd in &commands {
                println!("{}", format_tokens_sql(cmd, style.clone()));
            }
        }
    }
}
