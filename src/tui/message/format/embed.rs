use chrono::{DateTime, Local};
use ratatui::style::{Color, Style};
use unicode_width::UnicodeWidthStr;

use crate::{
    discord::EmbedInfo,
    tui::{
        text::{RenderedText, replace_custom_emoji_markup_in_rendered_with_images},
        theme,
    },
};

use super::{
    MessageContentLine, prefix_message_content_line_with_style,
    wrap_rendered_text_lines_with_loaded_custom_emoji_urls,
};

pub(super) fn format_embed_lines(
    embeds: &[EmbedInfo],
    message_content: Option<&str>,
    show_custom_emoji: bool,
    width: usize,
    loaded_custom_emoji_urls: &[String],
) -> Vec<MessageContentLine> {
    embeds
        .iter()
        .flat_map(|embed| {
            format_embed(
                embed,
                message_content,
                show_custom_emoji,
                width,
                loaded_custom_emoji_urls,
            )
        })
        .collect()
}

fn format_embed(
    embed: &EmbedInfo,
    message_content: Option<&str>,
    show_custom_emoji: bool,
    width: usize,
    loaded_custom_emoji_urls: &[String],
) -> Vec<MessageContentLine> {
    const PREFIX: &str = "  ▎ ";
    let inner_width = width.saturating_sub(PREFIX.width()).max(1);
    let mut lines = Vec::new();

    push_embed_text(
        &mut lines,
        embed.provider_name.as_deref(),
        show_custom_emoji,
        inner_width,
        embed_provider_style(),
        loaded_custom_emoji_urls,
    );
    push_embed_text(
        &mut lines,
        embed.author_name.as_deref(),
        show_custom_emoji,
        inner_width,
        embed_author_style(),
        loaded_custom_emoji_urls,
    );
    push_embed_text(
        &mut lines,
        embed.title.as_deref(),
        show_custom_emoji,
        inner_width,
        embed_title_style(),
        loaded_custom_emoji_urls,
    );
    let description = embed.description.as_deref().map(plain_embed_text);
    push_embed_text(
        &mut lines,
        description.as_deref(),
        show_custom_emoji,
        inner_width,
        Style::default(),
        loaded_custom_emoji_urls,
    );
    for field in &embed.fields {
        push_embed_text(
            &mut lines,
            Some(field.name.as_str()),
            show_custom_emoji,
            inner_width,
            embed_field_name_style(),
            loaded_custom_emoji_urls,
        );
        push_embed_text(
            &mut lines,
            Some(field.value.as_str()),
            show_custom_emoji,
            inner_width,
            Style::default(),
            loaded_custom_emoji_urls,
        );
    }
    let footer = format_embed_footer(embed);
    push_embed_text(
        &mut lines,
        footer.as_deref(),
        show_custom_emoji,
        inner_width,
        embed_footer_style(),
        loaded_custom_emoji_urls,
    );
    for url in [&embed.url]
        .into_iter()
        .filter_map(|url| url.as_deref())
        .filter(|url| !message_content.is_some_and(|content| content.contains(url)))
    {
        push_embed_text(
            &mut lines,
            Some(url),
            show_custom_emoji,
            inner_width,
            embed_url_style(),
            loaded_custom_emoji_urls,
        );
    }

    lines
        .into_iter()
        .map(|line| prefix_message_content_line_with_style(PREFIX, embed_line_style(embed), line))
        .collect()
}

fn plain_embed_text(value: &str) -> String {
    let value = value.replace('\u{fe00}', "");
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0usize;
    while let Some(relative_start) = value[cursor..].find('[') {
        let start = cursor.saturating_add(relative_start);
        output.push_str(&plain_embed_fragment(&value[cursor..start]));

        let Some(label_end) = value[start + 1..].find(']').map(|end| start + 1 + end) else {
            output.push_str(&plain_embed_fragment(&value[start..]));
            return output;
        };
        let url_start = label_end.saturating_add(1);
        if !value[url_start..].starts_with('(') {
            output.push('[');
            cursor = start.saturating_add(1);
            continue;
        }
        let Some(url_end) = value[url_start + 1..]
            .find(')')
            .map(|end| url_start + 1 + end)
        else {
            output.push_str(&plain_embed_fragment(&value[start..]));
            return output;
        };

        let label = plain_embed_fragment(&value[start + 1..label_end]);
        let url = unescape_embed_markdown(&value[url_start + 1..url_end]);
        push_plain_embed_link(&mut output, &label, &url);
        cursor = url_end.saturating_add(1);
    }
    output.push_str(&plain_embed_fragment(&value[cursor..]));
    output
}

fn plain_embed_fragment(value: &str) -> String {
    unescape_embed_markdown(&strip_embed_markdown_emphasis(value))
}

fn push_plain_embed_link(output: &mut String, label: &str, url: &str) {
    if is_low_value_embed_link_url(url) {
        output.push_str(label);
        return;
    }

    if label.is_empty() {
        output.push_str(url);
    } else if label == url || url.is_empty() {
        output.push_str(label);
    } else {
        output.push_str(label);
        output.push_str(" (");
        output.push_str(url);
        output.push(')');
    }
}

fn is_low_value_embed_link_url(url: &str) -> bool {
    let url = url.to_ascii_lowercase();
    url.starts_with("https://x.com/intent/") || url.starts_with("https://twitter.com/intent/")
}

fn unescape_embed_markdown(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\'
            && chars.peek().is_some_and(|next| {
                matches!(
                    next,
                    '\\' | '*' | '_' | '`' | '~' | '|' | '[' | ']' | '(' | ')' | '.' | '!' | '#'
                )
            })
        {
            if let Some(next) = chars.next() {
                output.push(next);
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn strip_embed_markdown_emphasis(value: &str) -> String {
    value.replace("**", "")
}

fn format_embed_footer(embed: &EmbedInfo) -> Option<String> {
    match (
        embed.footer_text.as_deref(),
        embed.timestamp.as_deref().and_then(format_embed_timestamp),
    ) {
        (Some(text), Some(timestamp)) => Some(format!("{text} · {timestamp}")),
        (Some(text), None) => Some(text.to_owned()),
        (None, Some(timestamp)) => Some(timestamp),
        (None, None) => None,
    }
}

fn format_embed_timestamp(timestamp: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|datetime| datetime.with_timezone(&Local).format("%H:%M").to_string())
}

fn push_embed_text(
    lines: &mut Vec<MessageContentLine>,
    value: Option<&str>,
    show_custom_emoji: bool,
    width: usize,
    style: Style,
    loaded_custom_emoji_urls: &[String],
) {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return;
    };
    // Skip the mention pass. Embeds never carry user mentions but custom
    // emojis in title/fields/footer must still produce slots.
    let rendered = replace_custom_emoji_markup_in_rendered_with_images(
        RenderedText {
            text: value.to_owned(),
            highlights: Vec::new(),
            emoji_slots: Vec::new(),
        },
        show_custom_emoji,
    );
    lines.extend(wrap_rendered_text_lines_with_loaded_custom_emoji_urls(
        rendered,
        width,
        style,
        loaded_custom_emoji_urls,
    ));
}

fn embed_provider_style() -> Style {
    theme::current().style(theme::HighlightGroup::EmbedFooter)
}

fn embed_author_style() -> Style {
    theme::current().style(theme::HighlightGroup::EmbedAuthor)
}

fn embed_title_style() -> Style {
    theme::current().style(theme::HighlightGroup::EmbedTitle)
}

fn embed_field_name_style() -> Style {
    theme::current().style(theme::HighlightGroup::EmbedFieldName)
}

fn embed_footer_style() -> Style {
    theme::current().style(theme::HighlightGroup::EmbedFooter)
}

fn embed_url_style() -> Style {
    theme::current().style(theme::HighlightGroup::EmbedLink)
}

fn embed_line_style(embed: &EmbedInfo) -> Style {
    let style = theme::current().style(theme::HighlightGroup::EmbedGutter);
    embed
        .color
        .map_or(style, |color| style.fg(embed_color(color)))
}

pub(in crate::tui) fn embed_color(color: u32) -> Color {
    Color::Rgb(
        ((color >> 16) & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        (color & 0xff) as u8,
    )
}
