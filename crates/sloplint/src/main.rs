//! sloplint CLI.
//!
//! For now the only subcommand is `parse`, a debug aid that dumps the AST and token
//! stream for a Python file — proof that the parser seam works end-to-end. Real
//! `check`/`metrics` commands arrive in later PRs.

use std::fs;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use sloplint_python::{parse, Ranged};

#[derive(Parser)]
#[command(
    name = "sloplint",
    about = "A nitpicking linter that counters AI slop in Python (runs after Ruff)",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse a Python file and dump its AST and token stream (debug aid).
    Parse {
        /// Path to a `.py` file.
        file: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Parse { file } => run_parse(&file),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("sloplint: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run_parse(path: &str) -> anyhow::Result<()> {
    let source = fs::read_to_string(path).map_err(|e| anyhow::anyhow!("reading {path}: {e}"))?;
    let parsed = parse(&source).map_err(|e| anyhow::anyhow!("parsing {path}: {e}"))?;

    println!("=== AST ===");
    println!("{:#?}", parsed.syntax());

    println!("\n=== TOKENS ===");
    for token in parsed.tokens().iter() {
        println!("{:?} {:?}", token.kind(), token.range());
    }

    // No parse-error dump here: `parse` returns `Err` on any syntax error, so a
    // successful `parsed` has none. Syntax errors are reported on the `Err` path in main.
    Ok(())
}
