//! Top-border clock: validates and renders a user-supplied strftime string.

use std::fmt::{Display, Write};

use chrono::format::{Item, StrftimeItems};
use chrono::{DateTime, Local, TimeZone};

/// Renders `format` for `now`; `None` when the format is empty, invalid or fails to render
/// (chrono reports bad specifiers as errors rather than panicking only if we check first).
pub fn format_clock_at<Tz>(format: &str, now: &DateTime<Tz>) -> Option<String>
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
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
    format_clock_at(format, &Local::now())
}

/// True when the format shows seconds, so the UI should wake at least once a second.
pub fn needs_second_resolution(format: &str) -> bool {
    !format.trim().is_empty() && format.contains(['S', 'T', 'X', 'c', 's', 'r', '+'])
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, NaiveDate};

    use super::*;

    fn sample() -> DateTime<FixedOffset> {
        let tz = FixedOffset::east_opt(3600).expect("valid +01:00 offset");
        let naive = NaiveDate::from_ymd_opt(2026, 11, 11)
            .and_then(|d| d.and_hms_opt(9, 5, 7))
            .expect("valid time");
        tz.from_local_datetime(&naive)
            .single()
            .expect("unambiguous fixed-offset time")
    }

    #[test]
    fn renders_common_and_timezone_specifiers() {
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
        assert_eq!(format_clock_at("%z", &sample()).as_deref(), Some("+0100"));
        assert_eq!(format_clock_at("%Z", &sample()).as_deref(), Some("+01:00"));
        assert_eq!(
            format_clock_at("%+", &sample()).as_deref(),
            Some("2026-11-11T09:05:07+01:00")
        );
        assert!(format_clock_now("%+").is_some());
        assert!(format_clock_now("%z").is_some());
        assert!(format_clock_now("%Z").is_some());
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
        assert!(needs_second_resolution("%+"));
        assert!(!needs_second_resolution("%H:%M"));
        assert!(!needs_second_resolution(""));
    }
}
