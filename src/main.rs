//! `pspp` ("ps++") — interactive btop-style TUI and scriptable query CLI for
//! viewing process memory grouped by parent/child (`pid <- ppid`) relationships.

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
    about = "ps++ — process memory grouped by parent (TUI and query CLI)",
    long_about = "pspp ('ps plus plus') groups process memory by parent/child relationships.\n\
                  Mode: query mode (one-shot output) runs when PATTERN, --pid, --min, --top, --json or\n\
                  --query is given, or when stdout is not a terminal; otherwise the interactive TUI runs.\n\
                  --mem, --agg, --sort, --base10 and --update-ms apply to whichever mode runs.\n\
                  Query mode is deterministic: it never reads pspp.conf (defaults: rss, rollup, sort agg,\n\
                  binary units). The TUI loads pspp.conf; CLI flags override it for that run only and are\n\
                  never saved.\n\n\
                  PATTERN is a case-insensitive substring match on COMM (!regex is not yet supported).\n\
                  TOTAL counts every process covered by the listed rows exactly once.\n\
                  Exit status (query mode): 0 rows printed, 1 no matching rows, 2 usage or runtime error.",
    after_long_help = "Examples:\n  \
                       pspp                              Launch interactive TUI\n  \
                       pspp --update-ms 1000             Launch TUI with 1s refresh\n  \
                       pspp --mem footprint -f chrome    TUI showing footprint, filter preset\n  \
                       pspp --query --top 20             One-shot table even on a terminal\n  \
                       pspp --top 10                     Top 10 process subtrees by RSS\n  \
                       pspp chrome --min 100M            Matching processes >= 100 MiB\n  \
                       pspp --agg flat --sort own        Direct-children sum sorted by own memory\n  \
                       pspp --json --top 5 | jq .total   JSON output for scripting"
)]
struct Cli {
    #[arg(
        value_name = "PATTERN",
        help = "Case-insensitive COMM substring (!regex is unsupported for now)"
    )]
    pattern: Option<String>,

    #[arg(long, value_name = "PID", help = "Limit to a PID subtree")]
    pid: Option<i32>,

    #[arg(
        long,
        value_name = "KIND",
        value_parser = parse_mem_kind,
        help = "Memory metric: rss, vsz, footprint (macOS), pss/uss/swap (Linux)"
    )]
    mem: Option<MemKind>,

    #[arg(
        long,
        value_name = "MODE",
        value_enum,
        help = "Aggregation mode: subtree rollup or direct-children flat sum"
    )]
    agg: Option<CliAgg>,

    #[arg(
        long,
        value_name = "SIZE",
        value_parser = query::parse_size,
        help = "Minimum aggregated memory threshold (e.g. 512K, 100M, 1.5G)"
    )]
    min: Option<u64>,

    #[arg(
        long,
        value_name = "FIELD",
        value_enum,
        help = "Sort rows by aggregated sum, own memory, PID, or COMM"
    )]
    sort: Option<CliSort>,

    #[arg(
        long,
        value_name = "N",
        value_parser = parse_top,
        help = "Limit output to the first N rows after sorting (N >= 1)"
    )]
    top: Option<usize>,

    #[arg(
        long,
        help = "Emit JSON ({ mem, agg, rows, total }) with raw byte counts"
    )]
    json: bool,

    #[arg(
        long,
        help = "Force one-shot query mode even when stdout is a terminal"
    )]
    query: bool,

    #[arg(
        short = 'f',
        long = "filter",
        value_name = "TEXT",
        help = "Pre-set the TUI process filter (like btop -f); a PATTERN fallback in query mode"
    )]
    filter: Option<String>,

    #[arg(
        long,
        help = "Use decimal units (KB/MB/GB) instead of binary (KiB/MiB/GiB)"
    )]
    base10: bool,

    #[arg(
        long,
        value_name = "MS",
        help = "Refresh interval in milliseconds for TUI mode (100 to 86400000)"
    )]
    update_ms: Option<u64>,
}

impl Cli {
    /// Flags that only make sense for one-shot output. `--mem/--agg/--sort/--base10/--update-ms`
    /// are deliberately absent: they apply to the TUI too.
    fn is_query_mode_requested(&self) -> bool {
        self.pattern.is_some()
            || self.pid.is_some()
            || self.min.is_some()
            || self.top.is_some()
            || self.json
            || self.query
    }

    /// Query mode is deterministic: it never reads `pspp.conf`.
    fn to_query_opts(&self) -> QueryOpts {
        let defaults = QueryOpts::default();
        QueryOpts {
            pattern: self.pattern.clone().or_else(|| self.filter.clone()),
            pid: self.pid,
            mem_kind: self.mem.unwrap_or(defaults.mem_kind),
            agg_mode: self.agg.map(Into::into).unwrap_or(defaults.agg_mode),
            min_bytes: self.min,
            sort_by: self.sort.map(Into::into).unwrap_or(defaults.sort_by),
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

fn parse_top(value: &str) -> Result<usize, String> {
    match value.trim().parse::<usize>() {
        Ok(0) => Err("must be at least 1".to_owned()),
        Ok(top) => Ok(top),
        Err(err) => Err(err.to_string()),
    }
}

/// Returns the number of rows printed.
fn run_query_mode(cli: &Cli) -> Result<usize, String> {
    let snapshot = collect::take_snapshot();
    let opts = cli.to_query_opts();

    let result = query::run_query(&snapshot, &opts);
    match write_stdout_line(&result.output) {
        // `pspp | head` closing the pipe early is not an error.
        Err(err) if err.kind() != io::ErrorKind::BrokenPipe => {
            Err(format!("failed to write stdout: {err}"))
        }
        _ => Ok(result.row_count),
    }
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

/// Copies into `cfg` only the settings the user changed during the session (`initial` is what the
/// TUI started with, i.e. config plus CLI overrides), so CLI overrides are never persisted.
fn apply_changed_settings_to_config(
    cfg: &mut config::Config,
    initial: &ui::UiSettings,
    current: &ui::UiSettings,
) {
    if current.update_ms != initial.update_ms {
        cfg.update_ms = current.update_ms;
    }
    if current.mem_kind != initial.mem_kind {
        cfg.mem_kind = mem_kind_to_config(current.mem_kind).to_owned();
    }
    if current.agg_mode != initial.agg_mode {
        cfg.agg_mode = match current.agg_mode {
            AggMode::Rollup => "rollup".to_owned(),
            AggMode::Flat => "flat".to_owned(),
        };
    }
    if current.graph_symbol != initial.graph_symbol {
        cfg.graph_symbol = current.graph_symbol.as_config().to_owned();
    }
    if current.base10 != initial.base10 {
        cfg.base10 = current.base10;
    }
    if current.sort_by != initial.sort_by {
        cfg.sort_key = ui::sort_to_config(current.sort_by).to_owned();
    }
    if current.sort_reverse != initial.sort_reverse {
        cfg.sort_reverse = current.sort_reverse;
    }
    cfg.save_on_exit = current.save_on_exit;
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
    let query_mode = cli.is_query_mode_requested() || !io::stdout().is_terminal();

    if !query_mode {
        let mut cfg = config::Config::load();
        let settings = cli_and_config_to_ui_settings(&cli, &cfg);
        let initial = settings.clone();
        return match ui::run_tui(settings, cli.filter.clone()) {
            Ok(updated_settings) => {
                if updated_settings.save_on_exit {
                    apply_changed_settings_to_config(&mut cfg, &initial, &updated_settings);
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

    // grep/pgrep-style status: 0 = rows printed, 1 = no matches, 2 = error.
    match run_query_mode(&cli) {
        Ok(0) => {
            if !cli.json {
                eprintln!("no matches");
            }
            ExitCode::from(1)
        }
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("pspp").chain(args.iter().copied()))
            .expect("args should parse")
    }

    #[test]
    fn query_mode_flags_select_query_mode() {
        for args in [
            &["chrome"][..],
            &["--pid", "1"],
            &["--min", "1M"],
            &["--top", "3"],
            &["--json"],
            &["--query"],
        ] {
            assert!(parse(args).is_query_mode_requested(), "{args:?}");
        }
    }

    #[test]
    fn display_flags_do_not_force_query_mode() {
        for args in [
            &[][..],
            &["--mem", "footprint"],
            &["--agg", "flat"],
            &["--sort", "own"],
            &["--base10"],
            &["--update-ms", "500"],
            &["-f", "chrome"],
        ] {
            assert!(!parse(args).is_query_mode_requested(), "{args:?}");
        }
    }

    #[test]
    fn query_opts_ignore_config_and_use_documented_defaults() {
        let opts = parse(&["--json"]).to_query_opts();
        assert_eq!(opts.mem_kind, MemKind::Rss);
        assert_eq!(opts.agg_mode, AggMode::Rollup);
        assert_eq!(opts.sort_by, SortBy::Aggregated);
        assert!(!opts.base10);

        let opts = parse(&["--agg", "flat", "--sort", "pid", "--base10"]).to_query_opts();
        assert_eq!(opts.agg_mode, AggMode::Flat);
        assert_eq!(opts.sort_by, SortBy::Pid);
        assert!(opts.base10);
    }

    #[test]
    fn tui_settings_apply_cli_overrides_over_config() {
        let cfg = config::Config {
            agg_mode: "flat".into(),
            ..config::Config::default()
        };
        let settings = cli_and_config_to_ui_settings(&parse(&["--agg", "rollup"]), &cfg);
        assert_eq!(settings.agg_mode, AggMode::Rollup);
        let settings = cli_and_config_to_ui_settings(&parse(&[]), &cfg);
        assert_eq!(settings.agg_mode, AggMode::Flat);
    }

    #[test]
    fn save_persists_only_user_changes_not_cli_overrides() {
        let mut cfg = config::Config::default();
        let initial = ui::UiSettings {
            mem_kind: MemKind::Vsz, // CLI override of the saved rss
            update_ms: 500,         // CLI override of the saved 2000
            ..ui::UiSettings::default()
        };
        let mut current = initial.clone();
        current.agg_mode = AggMode::Flat; // changed in the Options editor / `e`

        apply_changed_settings_to_config(&mut cfg, &initial, &current);

        assert_eq!(cfg.agg_mode, "flat");
        assert_eq!(cfg.mem_kind, "rss", "CLI --mem must not be saved");
        assert_eq!(cfg.update_ms, 2_000, "CLI --update-ms must not be saved");
    }
}
