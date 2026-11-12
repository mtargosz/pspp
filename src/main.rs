//! `pspp` command-line entry point: argument parsing and mode selection (TUI vs. one-shot query).
//! All logic lives in the `pspp` library crate (`src/lib.rs`).

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use clap::Parser;
use pspp::model::{AggMode, MemKind, SortBy};
use pspp::query::QueryOpts;
use pspp::{collect, config, query, ui};

#[derive(Debug, Parser)]
#[command(
    name = "pspp",
    version,
    about = "ps++ — process memory grouped by parent (TUI and query CLI)",
    long_about = "pspp ('ps plus plus') groups process memory by parent/child relationships.\n\
                  Mode: query mode (one-shot output) runs when PATTERN, --pid, --min, --top, --json or\n\
                  --query is given, or when stdout is not a terminal; otherwise the interactive TUI runs.\n\
                  --mem, --agg, --sort, -r/--reverse, --base10 and --update-ms apply to whichever mode runs.\n\
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
        conflicts_with = "filter",
        help = "Case-insensitive COMM substring (!regex is unsupported for now)"
    )]
    pattern: Option<String>,

    #[arg(long, value_name = "PID", help = "Limit to a PID subtree")]
    pid: Option<i32>,

    #[arg(
        long,
        value_name = "KIND",
        value_enum,
        help = "Memory metric: rss, vsz, footprint (macOS), pss/uss/swap (Linux)"
    )]
    mem: Option<MemKind>,

    #[arg(
        long,
        value_name = "MODE",
        value_enum,
        help = "Aggregation mode: subtree rollup or direct-children flat sum"
    )]
    agg: Option<AggMode>,

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
    sort: Option<SortBy>,

    #[arg(short = 'r', long = "reverse", help = "Reverse the sort order")]
    reverse: bool,

    #[arg(
        long,
        value_name = "N",
        value_parser = parse_top,
        help = "Limit output to the first N rows after sorting (N >= 1)"
    )]
    top: Option<usize>,

    #[arg(
        long,
        help = "Emit JSON ({ mem, agg, rows, total, total_approximate }) with raw byte counts"
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
    /// Flags that only make sense for one-shot output.
    /// `--mem/--agg/--sort/-r/--reverse/--base10/--update-ms` are deliberately absent: they apply
    /// to the TUI too.
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
            agg_mode: self.agg.unwrap_or(defaults.agg_mode),
            min_bytes: self.min,
            sort_by: self.sort.unwrap_or(defaults.sort_by),
            reverse: self.reverse,
            top: self.top,
            json: self.json,
            base10: self.base10,
        }
    }
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
    let opts = cli.to_query_opts();
    let snapshot = collect::take_snapshot(opts.mem_kind);

    let result = query::run_query(snapshot, &opts);
    match write_stdout_line(&result.output) {
        // `pspp | head` closing the pipe early is not an error.
        Err(err) if err.kind() != io::ErrorKind::BrokenPipe => {
            Err(format!("failed to write stdout: {err}"))
        }
        _ => Ok(result.row_count),
    }
}

fn cli_and_config_to_ui_settings(cli: &Cli, cfg: &config::Config) -> ui::UiSettings {
    let (sort_by, base_sort_reverse) = match cli.sort {
        Some(sort_by) => (sort_by, sort_by.natural_desc()),
        None => (cfg.sort_key.parse().unwrap_or_default(), cfg.sort_reverse),
    };
    let sort_reverse = if cli.reverse {
        !base_sort_reverse
    } else {
        base_sort_reverse
    };

    ui::UiSettings {
        update_ms: cli.update_ms.unwrap_or(cfg.update_ms),
        mem_kind: cli
            .mem
            .unwrap_or_else(|| MemKind::from_config(&cfg.mem_kind)),
        agg_mode: cli
            .agg
            .unwrap_or_else(|| cfg.agg_mode.parse().unwrap_or_default()),
        graph_symbol: ui::GraphSymbolSetting::from_config(&cfg.graph_symbol),
        base10: if cli.base10 { true } else { cfg.base10 },
        save_on_exit: cfg.save_on_exit,
        sort_by,
        sort_reverse,
        color_theme: cfg.color_theme.clone(),
        theme_background: cfg.theme_background,
        truecolor: ui::theme::TruecolorMode::from_config(&cfg.truecolor),
        rounded_corners: cfg.rounded_corners,
        vim_keys: cfg.vim_keys,
        disable_mouse: cfg.disable_mouse,
        clock_format: cfg.clock_format.clone(),
        background_update: cfg.background_update,
        force_tty: cfg.force_tty,
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
        cfg.mem_kind = current.mem_kind.to_string();
    }
    if current.agg_mode != initial.agg_mode {
        cfg.agg_mode = current.agg_mode.to_string();
    }
    if current.graph_symbol != initial.graph_symbol {
        cfg.graph_symbol = current.graph_symbol.as_config().to_owned();
    }
    if current.base10 != initial.base10 {
        cfg.base10 = current.base10;
    }
    if (current.sort_by, current.sort_reverse) != (initial.sort_by, initial.sort_reverse) {
        cfg.sort_key = current.sort_by.to_string();
        cfg.sort_reverse = current.sort_reverse;
    }
    if current.color_theme != initial.color_theme {
        cfg.color_theme = current.color_theme.clone();
    }
    if current.theme_background != initial.theme_background {
        cfg.theme_background = current.theme_background;
    }
    if current.truecolor != initial.truecolor {
        cfg.truecolor = current.truecolor.as_config().to_owned();
    }
    if current.rounded_corners != initial.rounded_corners {
        cfg.rounded_corners = current.rounded_corners;
    }
    if current.vim_keys != initial.vim_keys {
        cfg.vim_keys = current.vim_keys;
    }
    if current.disable_mouse != initial.disable_mouse {
        cfg.disable_mouse = current.disable_mouse;
    }
    if current.clock_format != initial.clock_format {
        cfg.clock_format = current.clock_format.clone();
    }
    if current.background_update != initial.background_update {
        cfg.background_update = current.background_update;
    }
    if current.force_tty != initial.force_tty {
        cfg.force_tty = current.force_tty;
    }
    cfg.save_on_exit = current.save_on_exit;
}

/// Updates `cfg` with changes made during the TUI session and returns whether `cfg` should be
/// written to disk.
fn update_config_for_exit(
    cfg: &mut config::Config,
    initial: &ui::UiSettings,
    updated: &ui::UiSettings,
) -> bool {
    if updated.save_on_exit {
        apply_changed_settings_to_config(cfg, initial, updated);
        true
    } else if initial.save_on_exit {
        cfg.save_on_exit = false;
        true
    } else {
        false
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
                if update_config_for_exit(&mut cfg, &initial, &updated_settings) {
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
            &["--mem", "vsz"],
            &["--agg", "flat"],
            &["--sort", "own"],
            &["-r"],
            &["--reverse"],
            &["--sort", "pid", "-r"],
            &["--base10"],
            &["--update-ms", "500"],
            &["-f", "chrome"],
        ] {
            assert!(!parse(args).is_query_mode_requested(), "{args:?}");
        }
    }

    #[test]
    fn pattern_conflicts_with_filter_flag() {
        assert!(Cli::try_parse_from(["pspp", "chrome", "-f", "firefox"]).is_err());
        assert!(Cli::try_parse_from(["pspp", "--filter", "firefox", "chrome"]).is_err());
    }

    #[test]
    fn query_opts_ignore_config_and_use_documented_defaults() {
        let opts = parse(&["--json"]).to_query_opts();
        assert_eq!(opts.mem_kind, MemKind::Rss);
        assert_eq!(opts.agg_mode, AggMode::Rollup);
        assert_eq!(opts.sort_by, SortBy::Aggregated);
        assert!(!opts.reverse);
        assert!(!opts.base10);

        let opts = parse(&["--agg", "flat", "--sort", "pid", "-r", "--base10"]).to_query_opts();
        assert_eq!(opts.agg_mode, AggMode::Flat);
        assert_eq!(opts.sort_by, SortBy::Pid);
        assert!(opts.reverse);
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

    #[test]
    fn save_persists_changed_general_options_only() {
        let mut cfg = config::Config::default();
        let initial = ui::UiSettings::default();
        let current = ui::UiSettings {
            color_theme: "Nord".into(),
            theme_background: false,
            truecolor: ui::theme::TruecolorMode::Off,
            rounded_corners: false,
            vim_keys: false,
            disable_mouse: true,
            clock_format: "%X".into(),
            background_update: false,
            force_tty: true,
            ..initial.clone()
        };
        apply_changed_settings_to_config(&mut cfg, &initial, &current);
        assert_eq!(cfg.color_theme, "Nord");
        assert!(!cfg.theme_background && !cfg.rounded_corners && !cfg.vim_keys);
        assert_eq!(cfg.truecolor, "off");
        assert!(cfg.disable_mouse && cfg.force_tty && !cfg.background_update);
        assert_eq!(cfg.clock_format, "%X");
        // Untouched fields keep the on-disk values.
        assert_eq!(cfg.mem_kind, "rss");

        let settings = cli_and_config_to_ui_settings(&parse(&[]), &cfg);
        assert_eq!(settings.color_theme, "Nord");
        assert_eq!(settings.truecolor, ui::theme::TruecolorMode::Off);
        assert!(!settings.vim_keys && settings.force_tty);
    }

    #[test]
    fn cli_sort_uses_natural_direction_and_reverse_inverts_cli_or_config() {
        let desc_cfg = config::Config {
            sort_key: "agg".into(),
            sort_reverse: true,
            ..config::Config::default()
        };
        let asc_cfg = config::Config {
            sort_key: "pid".into(),
            sort_reverse: false,
            ..config::Config::default()
        };

        // Explicit --sort ignores saved sort_reverse and starts in the key's natural direction.
        for (key, expected_sort, expected_desc) in [
            ("comm", SortBy::Comm, false),
            ("pid", SortBy::Pid, false),
            ("agg", SortBy::Aggregated, true),
            ("own", SortBy::Own, true),
        ] {
            let from_desc = cli_and_config_to_ui_settings(&parse(&["--sort", key]), &desc_cfg);
            assert_eq!(
                (from_desc.sort_by, from_desc.sort_reverse),
                (expected_sort, expected_desc),
                "--sort {key} over desc cfg"
            );
            let from_asc = cli_and_config_to_ui_settings(&parse(&["--sort", key]), &asc_cfg);
            assert_eq!(
                (from_asc.sort_by, from_asc.sort_reverse),
                (expected_sort, expected_desc),
                "--sort {key} over asc cfg"
            );

            // -r inverts the CLI --sort natural direction.
            let reversed = cli_and_config_to_ui_settings(&parse(&["--sort", key, "-r"]), &desc_cfg);
            assert_eq!(
                (reversed.sort_by, reversed.sort_reverse),
                (expected_sort, !expected_desc),
                "--sort {key} -r"
            );
        }

        // Without --sort, -r inverts the config-loaded sort_reverse.
        let inv_desc = cli_and_config_to_ui_settings(&parse(&["-r"]), &desc_cfg);
        assert_eq!(
            (inv_desc.sort_by, inv_desc.sort_reverse),
            (SortBy::Aggregated, false)
        );
        let inv_asc = cli_and_config_to_ui_settings(&parse(&["--reverse"]), &asc_cfg);
        assert_eq!((inv_asc.sort_by, inv_asc.sort_reverse), (SortBy::Pid, true));
    }

    #[test]
    fn save_couples_sort_key_and_sort_reverse_under_cli_override() {
        let base_cfg = config::Config {
            sort_key: "agg".into(),
            sort_reverse: true,
            ..config::Config::default()
        };
        let initial = cli_and_config_to_ui_settings(&parse(&["--sort", "pid"]), &base_cfg);
        assert_eq!(
            (initial.sort_by, initial.sort_reverse),
            (SortBy::Pid, false)
        );

        // Leaving CLI-overridden (Pid, false) untouched writes neither sort_key nor sort_reverse.
        let mut untouched_cfg = base_cfg.clone();
        apply_changed_settings_to_config(&mut untouched_cfg, &initial, &initial);
        assert_eq!(untouched_cfg.sort_key, "agg");
        assert!(untouched_cfg.sort_reverse);

        // Changing in-TUI from (Pid, false) to (Comm, false) writes ("comm", false), not ("comm", true).
        let mut changed_cfg = base_cfg;
        let current = ui::UiSettings {
            sort_by: SortBy::Comm,
            sort_reverse: false,
            ..initial.clone()
        };
        apply_changed_settings_to_config(&mut changed_cfg, &initial, &current);
        assert_eq!(changed_cfg.sort_key, "comm");
        assert!(!changed_cfg.sort_reverse);
    }

    #[test]
    fn update_config_for_exit_persists_turning_save_on_exit_off_without_session_changes() {
        // true -> false: saves save_on_exit = false ONLY, discarding transient session changes.
        let mut cfg = config::Config {
            agg_mode: "rollup".into(),
            color_theme: "Default".into(),
            base10: false,
            save_on_exit: true,
            ..config::Config::default()
        };
        let initial = ui::UiSettings {
            save_on_exit: true,
            ..ui::UiSettings::default()
        };
        let updated = ui::UiSettings {
            save_on_exit: false,
            agg_mode: AggMode::Flat,
            color_theme: "Nord".into(),
            base10: true,
            ..initial.clone()
        };
        assert!(update_config_for_exit(&mut cfg, &initial, &updated));
        assert!(!cfg.save_on_exit);
        assert_eq!(cfg.agg_mode, "rollup");
        assert_eq!(cfg.color_theme, "Default");
        assert!(!cfg.base10);

        // false -> false: skips saving altogether and leaves cfg untouched.
        let mut off_cfg = config::Config {
            save_on_exit: false,
            agg_mode: "rollup".into(),
            ..config::Config::default()
        };
        let initial_off = ui::UiSettings {
            save_on_exit: false,
            ..ui::UiSettings::default()
        };
        let updated_off = ui::UiSettings {
            save_on_exit: false,
            agg_mode: AggMode::Flat,
            ..initial_off.clone()
        };
        assert!(!update_config_for_exit(
            &mut off_cfg,
            &initial_off,
            &updated_off
        ));
        assert!(!off_cfg.save_on_exit);
        assert_eq!(off_cfg.agg_mode, "rollup");

        // true -> true: diffs changed session settings onto cfg and saves.
        let mut on_cfg = config::Config::default();
        let initial_on = ui::UiSettings::default();
        let updated_on = ui::UiSettings {
            agg_mode: AggMode::Flat,
            base10: true,
            ..initial_on.clone()
        };
        assert!(update_config_for_exit(
            &mut on_cfg,
            &initial_on,
            &updated_on
        ));
        assert!(on_cfg.save_on_exit);
        assert_eq!(on_cfg.agg_mode, "flat");
        assert!(on_cfg.base10);

        // false -> true: enables save_on_exit and persists session changes.
        let mut reenabled_cfg = config::Config {
            save_on_exit: false,
            agg_mode: "rollup".into(),
            ..config::Config::default()
        };
        let initial_reenabled = ui::UiSettings {
            save_on_exit: false,
            ..ui::UiSettings::default()
        };
        let updated_reenabled = ui::UiSettings {
            save_on_exit: true,
            agg_mode: AggMode::Flat,
            ..initial_reenabled.clone()
        };
        assert!(update_config_for_exit(
            &mut reenabled_cfg,
            &initial_reenabled,
            &updated_reenabled
        ));
        assert!(reenabled_cfg.save_on_exit);
        assert_eq!(reenabled_cfg.agg_mode, "flat");
    }
}
