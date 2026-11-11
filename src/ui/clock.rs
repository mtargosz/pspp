//! Top-border clock: validates and renders a user-supplied strftime string.

use std::fmt::Write;

use chrono::format::{Item, StrftimeItems};
use chrono::{Local, NaiveDateTime};

/// Renders `format` for `now`; `None` when the format is empty, invalid or fails to render
/// (chrono reports bad specifiers as errors rather than panicking only if we check first).
pub fn format_clock_at(format: &str, now: &NaiveDateTime) -> Option<String> {
    let format = format.trim();
    if format.is_empty() {
        return None;
    }
    if StrftimeItems::new(format).any(|item| matches!(item, Item::Error)) {
        return None;
    }
    let mut out = String::new();
    write!(out, "{}", now.format(format)).ok()?;
    (!out.is_empty()).then_some(out)
}

/// Current local time rendered with `format`.
pub fn format_clock_now(format: &str) -> Option<String> {
    format_clock_at(format, &Local::now().naive_local())
}

/// True when the format shows seconds, so the UI should wake at least once a second.
pub fn needs_second_resolution(format: &str) -> bool {
    !format.trim().is_empty() && format.contains(['S', 'T', 'X', 'c', 's', 'r', '+'])
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn sample() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 11)
            .and_then(|d| d.and_hms_opt(9, 5, 7))
            .expect("valid time")
    }

    #[test]
    fn renders_common_specifiers() {
        assert_eq!(
            format_clock_at("%H:%M:%S", &sample()).as_deref(),
            Some("09:05:07")
        );
        assert_eq!(
            format_clock_at("%Y-%m-%d", &sample()).as_deref(),
            Some("2026-11-11")
        );
        assert_eq!(
            format_clock_at(" %H:%M ", &sample()).as_deref(),
            Some("09:05")
        );
    }

    #[test]
    fn empty_or_invalid_formats_render_nothing() {
        assert_eq!(format_clock_at("", &sample()), None);
        assert_eq!(format_clock_at("   ", &sample()), None);
        assert_eq!(format_clock_at("%Q%", &sample()), None);
        assert_eq!(format_clock_at("%", &sample()), None);
    }

    #[test]
    fn second_resolution_detection() {
        assert!(needs_second_resolution("%H:%M:%S"));
        assert!(needs_second_resolution("%X"));
        assert!(!needs_second_resolution("%H:%M"));
        assert!(!needs_second_resolution(""));
    }
}
