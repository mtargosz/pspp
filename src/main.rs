mod collect;
pub mod model;
mod query;

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;
use std::str::FromStr;

use clap::{Parser, ValueEnum};
use model::{AggMode, MemKind, SortBy};
use query::QueryOpts;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliAgg {
    Rollup,
    Flat,
}

impl From<CliAgg> for AggMode {
    fn from(value: CliAgg) -> Self {
        match value {
            CliAgg::Rollup => AggMode::Rollup,
            CliAgg::Flat => AggMode::Flat,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliSort {
    Agg,
    Own,
    Pid,
    Comm,
}

impl From<CliSort> for SortBy {
    fn from(value: CliSort) -> Self {
        match value {
            CliSort::Agg => SortBy::Aggregated,
            CliSort::Own => SortBy::Own,
            CliSort::Pid => SortBy::Pid,
            CliSort::Comm => SortBy::Comm,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "pspp",
    version,
    about = "ps++ memory query mode (TUI milestone pending)",
    long_about = "pspp groups process memory by parent/child relationships.\n\
                  PATTERN is a case-insensitive substring match on COMM.\n\
                  Regex-style !prefix patterns are not yet supported."
)]
struct Cli {
    #[arg(
        value_name = "PATTERN",
        help = "Case-insensitive COMM substring (!regex is unsupported for now)"
    )]
    pattern: Option<String>,

    #[arg(long, value_name = "PID", help = "Limit to a PID subtree")]
    pid: Option<i32>,

    #[arg(long, value_name = "KIND", value_parser = parse_mem_kind)]
    mem: Option<MemKind>,

    #[arg(long, value_name = "MODE", value_enum)]
    agg: Option<CliAgg>,

    #[arg(long, value_name = "SIZE", value_parser = query::parse_size)]
    min: Option<u64>,

    #[arg(long, value_name = "FIELD", value_enum)]
    sort: Option<CliSort>,

    #[arg(long, value_name = "N")]
    top: Option<usize>,

    #[arg(long)]
    json: bool,

    #[arg(
        long,
        help = "Use decimal units (KB/MB/GB) instead of binary (KiB/MiB/GiB)"
    )]
    base10: bool,
}

impl Cli {
    fn is_query_mode_requested(&self) -> bool {
        self.pattern.is_some()
            || self.pid.is_some()
            || self.mem.is_some()
            || self.agg.is_some()
            || self.min.is_some()
            || self.sort.is_some()
            || self.top.is_some()
            || self.json
            || self.base10
    }

    fn to_query_opts(&self) -> QueryOpts {
        QueryOpts {
            pattern: self.pattern.clone(),
            pid: self.pid,
            mem_kind: self.mem.unwrap_or(MemKind::Rss),
            agg_mode: self.agg.unwrap_or(CliAgg::Rollup).into(),
            min_bytes: self.min,
            sort_by: self.sort.unwrap_or(CliSort::Agg).into(),
            top: self.top,
            json: self.json,
            base10: self.base10,
        }
    }
}

fn parse_mem_kind(value: &str) -> Result<MemKind, String> {
    let available = MemKind::available();
    let kind = MemKind::from_str(value).map_err(|_| {
        format!(
            "unknown memory kind '{value}'; available kinds: {}",
            format_available_mem_kinds(&available)
        )
    })?;

    if available.contains(&kind) {
        Ok(kind)
    } else {
        Err(format!(
            "memory kind '{value}' is unavailable on this platform; available kinds: {}",
            format_available_mem_kinds(&available)
        ))
    }
}

fn format_available_mem_kinds(kinds: &[MemKind]) -> String {
    kinds
        .iter()
        .map(|kind| match kind {
            MemKind::Rss => "rss",
            MemKind::Vsz => "vsz",
            MemKind::Footprint => "footprint",
            MemKind::Pss => "pss",
            MemKind::Uss => "uss",
            MemKind::Swap => "swap",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn write_stdout_line(text: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(text.as_bytes())?;
    stdout.write_all(b"\n")
}

fn run_query_mode(cli: &Cli) -> Result<bool, String> {
    let snapshot = collect::take_snapshot();
    let opts = cli.to_query_opts();

    let output = query::run_query(&snapshot, &opts);
    write_stdout_line(&output).map_err(|err| format!("failed to write stdout: {err}"))?;

    Ok(!opts.json && query::count_matches(&snapshot, &opts) == 0)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let query_mode = cli.is_query_mode_requested() || !io::stdout().is_terminal();

    if !query_mode {
        println!("TUI not yet implemented");
        return ExitCode::SUCCESS;
    }

    match run_query_mode(&cli) {
        Ok(no_matches) => {
            if no_matches {
                eprintln!("no matches");
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}
