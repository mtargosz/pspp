---
name: "pspp Rust & Cross-Platform Rules"
description: "Use when writing or reviewing Rust source, collectors, tree/query logic, configuration, or integration tests in pspp."
applyTo: "src/**/*.rs, tests/**/*.rs, Cargo.toml"
---

# Rust, Collector & Cross-Platform Conventions

- **Panic-free production paths**: Never use `unwrap()`, `expect()`, or unchecked slice indexing outside `#[cfg(test)]` and `tests/`. Parsing of `/proc`, `sysctl`, `proc_pidinfo`, `proc_pid_rusage`, TOML config (`pspp.conf`), and CLI size arguments must degrade cleanly on malformed, truncated, or permission-denied inputs.
- **Byte math & formatting**: Represent memory quantities internally as `u64` bytes (`Option<u64>` when a metric is unreadable or skipped). Use `saturating_add`, `saturating_sub`, and `saturating_mul` for sums and page conversions. Format human-readable units (`KiB`/`MiB`/`GiB` vs `KB`/`MB`/`GB`) only at render time.
- **Graceful metric degradation**: Never drop a live process when privileged counters fail (`EPERM` / unreadable `smaps_rollup` or `PROC_PIDTASKINFO`). Represent unreadable counters as `None`, which aggregates as `(0, approximate = true)` and renders with a trailing `~` on the row and its ancestors.
- **Platform isolation**:
  - Gate OS-specific collectors behind `#[cfg(target_os = "macos")]` and `#[cfg(target_os = "linux")]`; keep `model`, `query`, `config`, and `ui` platform-independent.
  - Gate pure Linux parsers (`parse_linux_stat_line`, `parse_smaps_rollup`, `parse_cmdline_name`, `parse_meminfo`) under `#[cfg(any(target_os = "linux", test))]` so unit tests execute on macOS.
  - Always verify Linux compilation from macOS via `cargo check --target x86_64-unknown-linux-gnu --all-targets` (install target via `rustup target add x86_64-unknown-linux-gnu` if missing).
- **Deterministic tests**:
  - Drive tree/query/UI tests with synthetic `Snapshot`s (`App::with_snapshot` and JSON fixtures in `tests/fixtures/`).
  - Live-host tests may only assert universal invariants (non-empty snapshot, own `ppid == libc::getppid()`, `launchd` / PID 1 presence)—never exact live process counts or memory totals.
  - Isolate config tests with `PSPP_CONFIG_DIR` pointing to a unique temp directory rather than touching user config paths.
