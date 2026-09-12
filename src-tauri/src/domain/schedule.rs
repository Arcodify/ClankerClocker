//! Shared schedule rules used to determine daily required work.

use crate::session::BreakConfig;

/// Returns the scheduled span in seconds. Invalid or overnight schedules are
/// treated as having no requirement until the schedule is corrected.
pub fn scheduled_work_seconds(clock_in_time: &str, clock_out_time: &str) -> i64 {
    let parse = |value: &str| chrono::NaiveTime::parse_from_str(value, "%H:%M").ok();
    match (parse(clock_in_time), parse(clock_out_time)) {
        (Some(start), Some(end)) => (end - start).num_seconds().max(0),
        _ => 0,
    }
}

/// Total seconds of scheduled auto-break windows (auto-start → auto-end).
pub fn scheduled_break_seconds(configs: &[BreakConfig]) -> i64 {
    let parse = |value: &str| chrono::NaiveTime::parse_from_str(value, "%H:%M").ok();
    configs
        .iter()
        .filter(|config| config.auto_start_enabled)
        .filter_map(|config| {
            let start = parse(config.auto_start_time.as_deref()?)?;
            let end = parse(config.auto_end_time.as_deref()?)?;
            Some((end - start).num_seconds().max(0))
        })
        .sum()
}

/// Daily work required after scheduled auto-breaks are excluded.
pub fn required_work_seconds(
    clock_in_time: &str,
    clock_out_time: &str,
    breaks: &[BreakConfig],
) -> i64 {
    (scheduled_work_seconds(clock_in_time, clock_out_time) - scheduled_break_seconds(breaks)).max(0)
}
