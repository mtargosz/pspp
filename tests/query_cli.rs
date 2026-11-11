use std::process::Command;

use serde_json::Value;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_pspp"))
        .args(args)
        .output()
        .expect("pspp binary should run")
}

fn count_data_rows(stdout: &str) -> usize {
    stdout
        .lines()
        .filter(|line| {
            line.chars()
                .find(|ch| !ch.is_whitespace())
                .is_some_and(|ch| ch.is_ascii_digit())
        })
        .count()
}

#[test]
fn query_json_output_is_parseable_and_non_empty() {
    let output = run(&["--json", "--top", "8"]);
    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let parsed: Value =
        serde_json::from_slice(&output.stdout).expect("json output should be valid json");
    let rows = parsed["rows"]
        .as_array()
        .expect("json output should contain rows array");
    let total = parsed["total"]
        .as_u64()
        .expect("json output should contain total number");

    assert!(!rows.is_empty(), "rows should not be empty");
    assert!(total > 0, "total should be positive");
}

#[test]
fn top_limits_plain_rows() {
    let output = run(&["--mem", "vsz", "--top", "3"]);
    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let data_rows = count_data_rows(&stdout);
    assert!(
        data_rows <= 3,
        "expected at most 3 data rows, got {data_rows}\n{stdout}"
    );
}

#[test]
fn high_min_reports_no_matches() {
    let output = run(&["--min", "100T"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 for no matches, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_ascii_lowercase().contains("no matches"),
        "expected no matches message in stderr, got: {stderr}"
    );
}

#[test]
fn invalid_mem_lists_available_kinds() {
    let output = run(&["--mem", "not-a-kind"]);
    assert!(
        !output.status.success(),
        "expected non-zero exit code for invalid --mem"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("available kinds"),
        "expected helpful mem error with available kinds, got: {stderr}"
    );
}

#[test]
fn top_zero_is_rejected_and_unmatched_json_exits_one() {
    let output = run(&["--top", "0"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "--top 0 should be a usage error"
    );

    let output = run(&["--json", "--pid", "2147483647"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "unmatched --pid should exit 1"
    );
    let parsed: Value =
        serde_json::from_slice(&output.stdout).expect("json should still be emitted");
    assert_eq!(parsed["rows"].as_array().map(Vec::len), Some(0));
}

#[test]
fn json_names_mem_and_agg_with_rollup_default() {
    let output = run(&["--json", "--top", "3"]);
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    assert_eq!(parsed["mem"], "rss");
    assert_eq!(parsed["agg"], "rollup");

    let output = run(&["--json", "--top", "3", "--agg", "flat", "--mem", "vsz"]);
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    assert_eq!(parsed["mem"], "vsz");
    assert_eq!(parsed["agg"], "flat");
}

#[test]
fn plain_header_names_the_aggregation_mode() {
    let stdout = String::from_utf8_lossy(&run(&["--top", "2"]).stdout).into_owned();
    assert!(stdout
        .lines()
        .next()
        .is_some_and(|h| h.contains("RSS Σ (rollup)")));

    let stdout =
        String::from_utf8_lossy(&run(&["--top", "2", "--agg", "flat"]).stdout).into_owned();
    assert!(stdout
        .lines()
        .next()
        .is_some_and(|h| h.contains("RSS Σ (flat)")));
}

#[test]
fn query_mode_ignores_a_saved_config() {
    let dir = std::env::temp_dir().join(format!("pspp-query-cfg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(
        dir.join("pspp.conf"),
        "agg_mode = \"flat\"\nmem_kind = \"vsz\"\nsort_key = \"pid\"\nbase10 = true\n",
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_pspp"))
        .env("PSPP_CONFIG_DIR", &dir)
        .args(["--json", "--top", "3"])
        .output()
        .expect("pspp binary should run");
    let _ = std::fs::remove_dir_all(&dir);

    let parsed: Value = serde_json::from_slice(&output.stdout).expect("valid json");
    assert_eq!(parsed["agg"], "rollup");
    assert_eq!(parsed["mem"], "rss");
}
