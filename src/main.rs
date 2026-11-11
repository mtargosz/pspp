mod collect;
mod config;
pub mod model;
mod query;
mod ui;

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
    about = "ps++ memory viewer and query tool",
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

    #[arg(
        long,
        value_name = "MS",
        help = "Refresh interval in milliseconds for TUI mode (minimum 100)"
    )]
    update_ms: Option<u64>,
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

    fn to_query_opts(&self, defaults: &config::Config) -> QueryOpts {
        QueryOpts {
            pattern: self.pattern.clone(),
            pid: self.pid,
            mem_kind: self
                .mem
                .unwrap_or_else(|| ui::parse_mem_kind_config(&defaults.mem_kind)),
            agg_mode: self
                .agg
                .map(Into::into)
                .unwrap_or_else(|| ui::parse_agg_mode_config(&defaults.agg_mode)),
            min_bytes: self.min,
            sort_by: self
                .sort
                .map(Into::into)
                .unwrap_or_else(|| ui::parse_sort_config(&defaults.sort_key)),
            top: self.top,
            json: self.json,
            base10: if self.base10 { true } else { defaults.base10 },
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

fn run_query_mode(cli: &Cli, defaults: &config::Config) -> Result<bool, String> {
    let snapshot = collect::take_snapshot();
    let opts = cli.to_query_opts(defaults);

    let output = query::run_query(&snapshot, &opts);
    write_stdout_line(&output).map_err(|err| format!("failed to write stdout: {err}"))?;

    Ok(!opts.json && query::count_matches(&snapshot, &opts) == 0)
}

fn cli_and_config_to_ui_settings(cli: &Cli, cfg: &config::Config) -> ui::UiSettings {
    ui::UiSettings {
        update_ms: cli.update_ms.unwrap_or(cfg.update_ms),
        mem_kind: cli
            .mem
            .unwrap_or_else(|| ui::parse_mem_kind_config(&cfg.mem_kind)),
        agg_mode: cli
            .agg
            .map(Into::into)
            .unwrap_or_else(|| ui::parse_agg_mode_config(&cfg.agg_mode)),
        graph_symbol: ui::GraphSymbolSetting::from_config(&cfg.graph_symbol),
        base10: if cli.base10 { true } else { cfg.base10 },
        save_on_exit: cfg.save_on_exit,
        sort_by: cli
            .sort
            .map(Into::into)
            .unwrap_or_else(|| ui::parse_sort_config(&cfg.sort_key)),
        sort_reverse: cfg.sort_reverse,
    }
}

fn apply_ui_settings_to_config(cfg: &mut config::Config, settings: &ui::UiSettings) {
    cfg.update_ms = settings.update_ms;
    cfg.mem_kind = mem_kind_to_config(settings.mem_kind).to_owned();
    cfg.agg_mode = match settings.agg_mode {
        AggMode::Rollup => "rollup".to_owned(),
        AggMode::Flat => "flat".to_owned(),
    };
    cfg.graph_symbol = settings.graph_symbol.as_config().to_owned();
    cfg.base10 = settings.base10;
    cfg.save_on_exit = settings.save_on_exit;
    cfg.sort_key = ui::sort_to_config(settings.sort_by).to_owned();
    cfg.sort_reverse = settings.sort_reverse;
}

fn mem_kind_to_config(mem_kind: MemKind) -> &'static str {
    match mem_kind {
        MemKind::Rss => "rss",
        MemKind::Vsz => "vsz",
        MemKind::Footprint => "footprint",
        MemKind::Pss => "pss",
        MemKind::Uss => "uss",
        MemKind::Swap => "swap",
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let mut cfg = config::Config::load();
    let query_mode = cli.is_query_mode_requested() || !io::stdout().is_terminal();

    if !query_mode {
        let settings = cli_and_config_to_ui_settings(&cli, &cfg);
        return match ui::run_tui(settings) {
            Ok(updated_settings) => {
                if updated_settings.save_on_exit {
                    apply_ui_settings_to_config(&mut cfg, &updated_settings);
                    if let Err(err) = cfg.save() {
                        eprintln!("failed to save config: {err}");
                    }
                }
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("failed to run TUI: {err}");
                ExitCode::from(1)
            }
        };
    }

    match run_query_mode(&cli, &cfg) {
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
