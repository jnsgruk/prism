use time::OffsetDateTime;

pub fn timestamp(ts: Option<&prost_types::Timestamp>) -> String {
    let Some(ts) = ts else {
        return "—".to_string();
    };
    let Ok(dt) = OffsetDateTime::from_unix_timestamp(ts.seconds) else {
        return "invalid".to_string();
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        dt.year(),
        dt.month() as u8,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
    )
}

pub fn duration_between(
    start: Option<&prost_types::Timestamp>,
    end: Option<&prost_types::Timestamp>,
) -> String {
    let (Some(start), Some(end)) = (start, end) else {
        return "—".to_string();
    };
    let secs = end.seconds - start.seconds;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// Truncate a string to `max` characters (not bytes), appending an ellipsis
/// if truncated. Safe for multi-byte UTF-8.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let end = s
            .char_indices()
            .nth(max.saturating_sub(1))
            .map_or(0, |(i, _)| i);
        format!("{}\u{2026}", &s[..end])
    }
}

pub fn source_state(state: i32) -> &'static str {
    match ps_proto::prism::v1::SourceState::try_from(state) {
        Ok(ps_proto::prism::v1::SourceState::Idle) => "idle",
        Ok(ps_proto::prism::v1::SourceState::Collecting) => "collecting",
        Ok(ps_proto::prism::v1::SourceState::Waiting) => "waiting",
        Ok(ps_proto::prism::v1::SourceState::Error) => "error",
        _ => "unknown",
    }
}
