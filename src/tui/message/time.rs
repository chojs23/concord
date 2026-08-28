use chrono::{DateTime, Local, NaiveDate};

use crate::discord::ids::{Id, marker::MessageMarker};

const DISCORD_EPOCH_MILLIS: u64 = 1_420_070_400_000;
const SNOWFLAKE_TIMESTAMP_SHIFT: u8 = 22;
const TIME_FORMAT_24: &str = "%H:%M";
const TIME_FORMAT_12: &str = "%I:%M %p";

pub(in crate::tui) fn message_unix_millis(message_id: Id<MessageMarker>) -> u64 {
    (message_id.get() >> SNOWFLAKE_TIMESTAMP_SHIFT) + DISCORD_EPOCH_MILLIS
}

pub(in crate::tui) fn message_local_datetime(
    message_id: Id<MessageMarker>,
) -> Option<DateTime<Local>> {
    let unix_millis = i64::try_from(message_unix_millis(message_id)).ok()?;
    DateTime::from_timestamp_millis(unix_millis).map(|dt| dt.with_timezone(&Local))
}

pub(in crate::tui) fn format_message_local_time(
    message_id: Id<MessageMarker>,
    hour_format_24: bool,
) -> String {
    message_local_datetime(message_id)
        .map(|datetime| format_local_time(&datetime, hour_format_24))
        .unwrap_or_else(|| "--:--".to_owned())
}

pub(in crate::tui) fn format_rfc3339_local_time(
    timestamp: &str,
    hour_format_24: bool,
) -> Option<String> {
    DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|datetime| format_local_time(&datetime.with_timezone(&Local), hour_format_24))
}

fn format_local_time(datetime: &DateTime<Local>, hour_format_24: bool) -> String {
    datetime.format(time_format(hour_format_24)).to_string()
}

pub(in crate::tui) fn format_local_date_time(
    datetime: &DateTime<Local>,
    hour_format_24: bool,
) -> String {
    let format = if hour_format_24 {
        "%Y-%m-%d %H:%M"
    } else {
        "%Y-%m-%d %I:%M %p"
    };
    datetime.format(format).to_string()
}

fn time_format(hour_format_24: bool) -> &'static str {
    if hour_format_24 {
        TIME_FORMAT_24
    } else {
        TIME_FORMAT_12
    }
}

pub(in crate::tui) fn message_local_date(message_id: Id<MessageMarker>) -> NaiveDate {
    message_local_datetime(message_id)
        .map(|dt| dt.date_naive())
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(2015, 1, 1).expect("static date is valid"))
}

pub(in crate::tui) fn message_starts_new_day(
    current: Id<MessageMarker>,
    previous: Option<Id<MessageMarker>>,
) -> bool {
    match previous {
        None => true,
        Some(prev) => message_local_date(current) != message_local_date(prev),
    }
}

#[cfg(test)]
pub(in crate::tui) fn discord_epoch_unix_millis() -> u64 {
    DISCORD_EPOCH_MILLIS
}

#[cfg(test)]
pub(in crate::tui) fn test_message_id_for_unix_millis(unix_millis: u64) -> Id<MessageMarker> {
    let since_discord_epoch = unix_millis
        .checked_sub(DISCORD_EPOCH_MILLIS)
        .expect("test timestamp should be after Discord epoch");
    let raw = since_discord_epoch << SNOWFLAKE_TIMESTAMP_SHIFT;
    Id::new(raw.max(1))
}

#[cfg(test)]
pub(in crate::tui) fn format_unix_millis_with_offset(
    unix_millis: u64,
    offset: chrono::FixedOffset,
) -> Option<String> {
    let unix_millis = i64::try_from(unix_millis).ok()?;
    let utc = DateTime::from_timestamp_millis(unix_millis)?;
    Some(utc.with_timezone(&offset).format("%H:%M").to_string())
}
