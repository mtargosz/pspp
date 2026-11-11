//! Test-only helper that keeps generated blocks in README.md / doc/pspp.1 in sync with the code.
//!
//! A block sits between two marker lines. Tests compare the generated text with what is on disk
//! and fail with a hint; running them with `PSPP_UPDATE_DOCS=1` rewrites the block instead.

use std::fs;
use std::path::PathBuf;

/// Marker lines for a Markdown block named `name`.
pub(crate) fn markdown_markers(name: &str) -> (String, String) {
    (
        format!("<!-- BEGIN GENERATED: {name} -->"),
        format!("<!-- END GENERATED: {name} -->"),
    )
}

/// Marker lines for a roff block named `name`.
pub(crate) fn roff_markers(name: &str) -> (String, String) {
    (
        format!(".\\\" BEGIN GENERATED: {name}"),
        format!(".\\\" END GENERATED: {name}"),
    )
}

/// Ensures the text between `markers` in `relative_path` equals `body` (no trailing newline).
pub(crate) fn sync_block(relative_path: &str, markers: (String, String), body: &str) {
    let (begin, end) = markers;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));

    let start = text
        .find(&begin)
        .unwrap_or_else(|| panic!("{relative_path}: missing marker `{begin}`"))
        + begin.len();
    let stop = text[start..]
        .find(&end)
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("{relative_path}: missing marker `{end}`"));

    let wanted = format!("\n{}\n", body.trim_end_matches('\n'));
    if text[start..stop] == wanted {
        return;
    }
    if std::env::var_os("PSPP_UPDATE_DOCS").is_some() {
        let updated = format!("{}{}{}", &text[..start], wanted, &text[stop..]);
        fs::write(&path, updated).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        return;
    }
    panic!(
        "{relative_path}: generated block `{begin}` is out of date.\n\
         Regenerate with: PSPP_UPDATE_DOCS=1 cargo test --lib"
    );
}
