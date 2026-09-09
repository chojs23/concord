use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, NaiveDate};

use crate::{
    discord::ids::{Id, marker::MessageMarker},
    tui::text::{RenderedText, TextHighlight, TextHighlightKind, TextReplacement},
};

const DISCORD_EPOCH_MILLIS: u64 = 1_420_070_400_000;
const SNOWFLAKE_TIMESTAMP_SHIFT: u8 = 22;
const TIME_FORMAT_24: &str = "%H:%M";
const TIME_FORMAT_12: &str = "%I:%M %p";

/// Renders Discord timestamp markup while preserving markup shown as inline or
/// fenced code. Absolute timestamps use the local timezone and Concord's clock
/// format preference.
pub(in crate::tui) fn render_discord_timestamps(
    rendered: impl Into<RenderedText>,
    hour_format_24: bool,
) -> RenderedText {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or_default();
    render_discord_timestamps_at(rendered.into(), hour_format_24, now)
}

fn render_discord_timestamps_at(
    mut rendered: RenderedText,
    hour_format_24: bool,
    now: i64,
) -> RenderedText {
    let matches = discord_timestamp_matches(&rendered.text, hour_format_24, now);
    if matches.is_empty() {
        return rendered;
    }

    let input = std::mem::take(&mut rendered.text);
    let mut output = String::with_capacity(input.len());
    let mut replacements = Vec::with_capacity(matches.len());
    let mut timestamp_highlights = Vec::with_capacity(matches.len());
    let mut cursor = 0usize;
    for matched in matches {
        output.push_str(&input[cursor..matched.input_start]);
        let output_start = output.len();
        output.push_str(&matched.formatted);
        let output_end = output.len();
        replacements.push(TextReplacement {
            input_start: matched.input_start,
            input_end: matched.input_end,
            output_start,
            output_len: output_end.saturating_sub(output_start),
        });
        timestamp_highlights.push(TextHighlight {
            start: output_start,
            end: output_end,
            kind: TextHighlightKind::Timestamp,
        });
        cursor = matched.input_end;
    }
    output.push_str(&input[cursor..]);

    rendered.remap_metadata(&replacements);
    rendered.highlights.extend(timestamp_highlights);
    rendered
        .highlights
        .sort_by_key(|highlight| (highlight.start, highlight.end));
    rendered.text = output;
    rendered
}

struct DiscordTimestampMatch {
    input_start: usize,
    input_end: usize,
    formatted: String,
}

fn discord_timestamp_matches(
    value: &str,
    hour_format_24: bool,
    now: i64,
) -> Vec<DiscordTimestampMatch> {
    if !value.contains("<t:") {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut in_code_block = false;
    let mut line_start = 0usize;
    for line in value.split_inclusive('\n') {
        let (content, newline) = line
            .strip_suffix('\n')
            .map_or((line, ""), |content| (content, "\n"));

        if in_code_block {
            if closes_markdown_code_block(content) {
                in_code_block = false;
            }
        } else if content.trim_start().starts_with("```") {
            in_code_block = true;
        } else {
            collect_discord_timestamps_outside_inline_code(
                &mut matches,
                content,
                line_start,
                hour_format_24,
                now,
            );
        }
        line_start = line_start
            .saturating_add(content.len())
            .saturating_add(newline.len());
    }
    matches
}

fn closes_markdown_code_block(value: &str) -> bool {
    if value.trim() == "```" {
        return true;
    }

    let trimmed = value.trim_end();
    trimmed
        .strip_suffix("```")
        .is_some_and(|before_fence| !before_fence.trim().is_empty())
}

fn collect_discord_timestamps_outside_inline_code(
    matches: &mut Vec<DiscordTimestampMatch>,
    value: &str,
    source_start: usize,
    hour_format_24: bool,
    now: i64,
) {
    let mut cursor = 0usize;
    while let Some(relative_open) = value[cursor..].find('`') {
        let open = cursor.saturating_add(relative_open);
        collect_discord_timestamp_markup(
            matches,
            &value[cursor..open],
            source_start.saturating_add(cursor),
            hour_format_24,
            now,
        );

        let content_start = open.saturating_add(1);
        let close = value[content_start..]
            .find('`')
            .filter(|relative_close| *relative_close > 0)
            .map(|relative_close| content_start.saturating_add(relative_close));
        let Some(close) = close else {
            collect_discord_timestamp_markup(
                matches,
                &value[open..],
                source_start.saturating_add(open),
                hour_format_24,
                now,
            );
            return;
        };

        cursor = close.saturating_add(1);
    }
    collect_discord_timestamp_markup(
        matches,
        &value[cursor..],
        source_start.saturating_add(cursor),
        hour_format_24,
        now,
    );
}

fn collect_discord_timestamp_markup(
    matches: &mut Vec<DiscordTimestampMatch>,
    value: &str,
    source_start: usize,
    hour_format_24: bool,
    now: i64,
) {
    let mut cursor = 0usize;
    while let Some(relative_start) = value[cursor..].find("<t:") {
        let start = cursor.saturating_add(relative_start);
        match parse_discord_timestamp(value, start, hour_format_24, now) {
            Some((end, formatted)) => {
                matches.push(DiscordTimestampMatch {
                    input_start: source_start.saturating_add(start),
                    input_end: source_start.saturating_add(end),
                    formatted,
                });
                cursor = end;
            }
            None => {
                cursor = start.saturating_add(1);
            }
        }
    }
}

fn parse_discord_timestamp(
    value: &str,
    start: usize,
    hour_format_24: bool,
    now: i64,
) -> Option<(usize, String)> {
    let bytes = value.as_bytes();
    if bytes.get(start..start.saturating_add(3)) != Some(b"<t:") {
        return None;
    }

    let mut index = start.saturating_add(3);
    let digits_start = index;
    while matches!(bytes.get(index), Some(byte) if byte.is_ascii_digit()) {
        index = index.saturating_add(1);
    }
    if index == digits_start {
        return None;
    }

    let unix_seconds: i64 = value[digits_start..index].parse().ok()?;
    let style = match bytes.get(index).copied() {
        Some(b'>') => 'f',
        Some(b':') => {
            index = index.saturating_add(1);
            let style = bytes.get(index).copied()?;
            if !matches!(
                style,
                b't' | b'T' | b'd' | b'D' | b'f' | b'F' | b's' | b'S' | b'R'
            ) {
                return None;
            }
            index = index.saturating_add(1);
            char::from(style)
        }
        _ => return None,
    };

    if bytes.get(index) != Some(&b'>') {
        return None;
    }
    let end = index.saturating_add(1);
    Some((
        end,
        format_discord_timestamp(unix_seconds, style, hour_format_24, now),
    ))
}

fn format_discord_timestamp(
    unix_seconds: i64,
    style: char,
    hour_format_24: bool,
    now: i64,
) -> String {
    if style == 'R' {
        return format_discord_relative(unix_seconds, now);
    }

    let Some(local) =
        DateTime::from_timestamp(unix_seconds, 0).map(|date| date.with_timezone(&Local))
    else {
        return "<invalid time>".to_owned();
    };
    let format = match (style, hour_format_24) {
        ('t', true) => "%H:%M",
        ('t', false) => "%I:%M %p",
        ('T', true) => "%H:%M:%S",
        ('T', false) => "%I:%M:%S %p",
        ('d', _) => "%m/%d/%Y",
        ('D', _) => "%B %d, %Y",
        ('s', true) => "%m/%d/%Y, %H:%M",
        ('s', false) => "%m/%d/%Y, %I:%M %p",
        ('S', true) => "%m/%d/%Y, %H:%M:%S",
        ('S', false) => "%m/%d/%Y, %I:%M:%S %p",
        ('F', true) => "%A, %B %d, %Y %H:%M",
        ('F', false) => "%A, %B %d, %Y %I:%M %p",
        (_, true) => "%B %d, %Y %H:%M",
        (_, false) => "%B %d, %Y %I:%M %p",
    };
    local.format(format).to_string()
}

fn format_discord_relative(unix_seconds: i64, now: i64) -> String {
    let difference = unix_seconds.saturating_sub(now);
    if difference > 0 {
        format_relative_time_future(difference.unsigned_abs())
    } else {
        format_relative_time_past(difference.unsigned_abs())
    }
}

pub(in crate::tui::message) fn format_relative_time_past(seconds: u64) -> String {
    if seconds < 60 {
        return "just now".to_owned();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{} ago", relative_quantity(minutes, "minute"));
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{} ago", relative_quantity(hours, "hour"));
    }
    let days = hours / 24;
    if days < 30 {
        return format!("{} ago", relative_quantity(days, "day"));
    }
    let months = days / 30;
    if months < 12 {
        return format!("{} ago", relative_quantity(months, "month"));
    }
    format!("{} ago", relative_quantity((days / 365).max(1), "year"))
}

fn format_relative_time_future(seconds: u64) -> String {
    if seconds < 60 {
        return "in less than a minute".to_owned();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("in {}", relative_quantity(minutes, "minute"));
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("in {}", relative_quantity(hours, "hour"));
    }
    let days = hours / 24;
    if days < 30 {
        return format!("in {}", relative_quantity(days, "day"));
    }
    let months = days / 30;
    if months < 12 {
        return format!("in {}", relative_quantity(months, "month"));
    }
    format!("in {}", relative_quantity((days / 365).max(1), "year"))
}

fn relative_quantity(value: u64, unit: &str) -> String {
    let suffix = if value == 1 { "" } else { "s" };
    format!("{value} {unit}{suffix}")
}

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
        .map(|datetime| format_time(&datetime, hour_format_24))
        .unwrap_or_else(|| "--:--".to_owned())
}

pub(in crate::tui) fn format_rfc3339_local_time(
    timestamp: &str,
    hour_format_24: bool,
) -> Option<String> {
    DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|datetime| format_time(&datetime.with_timezone(&Local), hour_format_24))
}

fn format_time<Tz>(datetime: &DateTime<Tz>, hour_format_24: bool) -> String
where
    Tz: chrono::TimeZone,
    Tz::Offset: std::fmt::Display,
{
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
mod tests {
    use super::*;
    use crate::tui::text::InlineEmojiSlot;

    const TIMESTAMP: i64 = 1_735_689_600;

    fn rendered(value: impl Into<String>) -> RenderedText {
        RenderedText {
            text: value.into(),
            ..RenderedText::default()
        }
    }

    fn render_at(value: &str, hour_format_24: bool, now: i64) -> RenderedText {
        render_discord_timestamps_at(rendered(value), hour_format_24, now)
    }

    #[test]
    fn format_message_sent_time_with_injected_offset_uses_production_formatter() {
        let kst = chrono::FixedOffset::east_opt(9 * 60 * 60).expect("KST offset is valid");
        let unix_millis =
            i64::try_from(DISCORD_EPOCH_MILLIS).expect("Discord epoch fits an i64 timestamp");
        let utc = DateTime::from_timestamp_millis(unix_millis)
            .expect("Discord epoch is a valid timestamp");

        assert_eq!(format_time(&utc.with_timezone(&kst), true), "09:00");
    }

    #[test]
    fn discord_timestamp_markup_renders_supported_absolute_styles() {
        let local = DateTime::from_timestamp(TIMESTAMP, 0)
            .expect("static timestamp is valid")
            .with_timezone(&Local);

        for (hour_format_24, style, format) in [
            (true, "", "%B %d, %Y %H:%M"),
            (true, "t", "%H:%M"),
            (true, "T", "%H:%M:%S"),
            (true, "d", "%m/%d/%Y"),
            (true, "D", "%B %d, %Y"),
            (true, "f", "%B %d, %Y %H:%M"),
            (true, "F", "%A, %B %d, %Y %H:%M"),
            (true, "s", "%m/%d/%Y, %H:%M"),
            (true, "S", "%m/%d/%Y, %H:%M:%S"),
            (false, "t", "%I:%M %p"),
            (false, "T", "%I:%M:%S %p"),
            (false, "f", "%B %d, %Y %I:%M %p"),
            (false, "F", "%A, %B %d, %Y %I:%M %p"),
            (false, "s", "%m/%d/%Y, %I:%M %p"),
            (false, "S", "%m/%d/%Y, %I:%M:%S %p"),
        ] {
            let markup = if style.is_empty() {
                format!("<t:{TIMESTAMP}>")
            } else {
                format!("<t:{TIMESTAMP}:{style}>")
            };

            let expected = local.format(format).to_string();
            let rendered = render_at(&markup, hour_format_24, TIMESTAMP);

            assert_eq!(rendered.text, expected, "{markup}");
            assert_eq!(
                rendered.highlights,
                vec![TextHighlight {
                    start: 0,
                    end: rendered.text.len(),
                    kind: TextHighlightKind::Timestamp,
                }],
                "{markup}"
            );
        }
    }

    #[test]
    fn discord_relative_timestamp_markup_uses_expected_boundaries() {
        let now = 10_000;
        for (timestamp, expected) in [
            (now - 59, "just now"),
            (now, "just now"),
            (now + 59, "in less than a minute"),
            (now - 60, "1 minute ago"),
            (now + 60, "in 1 minute"),
            (now - 2 * 60 * 60, "2 hours ago"),
            (now + 2 * 24 * 60 * 60, "in 2 days"),
        ] {
            let markup = format!("<t:{timestamp}:R>");
            let rendered = render_at(&markup, true, now);
            assert_eq!(rendered.text, expected, "{markup}");
            assert_eq!(rendered.highlights.len(), 1, "{markup}");
            assert_eq!(rendered.highlights[0].kind, TextHighlightKind::Timestamp);
        }
    }

    #[test]
    fn discord_timestamp_markup_preserves_code_and_malformed_input() {
        let input = concat!(
            "plain <t:9940:R>\n",
            "inline `<t:9940:R>`\n",
            "```text\n",
            "<t:9940:R>\n",
            "```\n",
            "bad <t:> <t:9940:x> <t:9940:R <t:999999999999999999999:R>"
        );

        let rendered = render_at(input, true, 10_000);

        assert_eq!(
            rendered.text,
            concat!(
                "plain 1 minute ago\n",
                "inline `<t:9940:R>`\n",
                "```text\n",
                "<t:9940:R>\n",
                "```\n",
                "bad <t:> <t:9940:x> <t:9940:R <t:999999999999999999999:R>"
            )
        );
        assert_eq!(
            rendered.highlights,
            vec![TextHighlight {
                start: "plain ".len(),
                end: "plain 1 minute ago".len(),
                kind: TextHighlightKind::Timestamp,
            }]
        );
    }

    #[test]
    fn discord_timestamp_rendering_remaps_existing_metadata() {
        let mut input = rendered("<t:9940:R> @alice :wave:");
        input.highlights.push(TextHighlight {
            start: 11,
            end: 17,
            kind: TextHighlightKind::OtherMention,
        });
        input.emoji_slots.push(InlineEmojiSlot {
            byte_start: 18,
            byte_len: 6,
            display_width: 6,
            url: "emoji".to_owned(),
        });

        let rendered = render_discord_timestamps_at(input, true, 10_000);

        assert_eq!(rendered.text, "1 minute ago @alice :wave:");
        assert_eq!(
            rendered.highlights,
            vec![
                TextHighlight {
                    start: 0,
                    end: 12,
                    kind: TextHighlightKind::Timestamp,
                },
                TextHighlight {
                    start: 13,
                    end: 19,
                    kind: TextHighlightKind::OtherMention,
                },
            ]
        );
        assert_eq!(rendered.emoji_slots[0].byte_start, 20);
    }
}
