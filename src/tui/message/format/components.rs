use ratatui::style::Style;
use unicode_width::UnicodeWidthStr;

use crate::{
    discord::{
        AttachmentInfo, AttachmentMediaType, ComponentMediaInfo, ComponentSelectKind, MentionInfo,
        MessageComponentInfo,
        ids::{
            Id,
            marker::{GuildMarker, RoleMarker},
        },
    },
    tui::{
        message::time::render_discord_timestamps, state::DashboardState, text::truncate_text, theme,
    },
};

use super::{
    MessageContentLine, MessageContentPreviewSlot, embed_color,
    prefix_message_content_line_with_style,
    wrap_markdown_message_lines_with_loaded_custom_emoji_urls,
};

const SECTION_THUMBNAIL_MAX_WIDTH: u16 = 18;
const SECTION_THUMBNAIL_MIN_WIDTH: u16 = 8;
const SECTION_THUMBNAIL_HEIGHT: u16 = 6;
const SECTION_THUMBNAIL_GAP: usize = 2;
const SECTION_TEXT_MIN_WIDTH: usize = 16;

pub(super) struct ComponentFormatContext<'a> {
    pub(super) guild_id: Option<Id<GuildMarker>>,
    pub(super) mentions: &'a [MentionInfo],
    pub(super) mention_everyone: bool,
    pub(super) mention_roles: &'a [Id<RoleMarker>],
    pub(super) attachments: &'a [AttachmentInfo],
}

pub(super) fn format_component_lines(
    components: &[MessageComponentInfo],
    context: &ComponentFormatContext<'_>,
    state: &DashboardState,
    width: usize,
    loaded_custom_emoji_urls: &[String],
    next_section_thumbnail_index: &mut usize,
) -> Vec<MessageContentLine> {
    let mut lines = format_components(
        components,
        context,
        state,
        width,
        loaded_custom_emoji_urls,
        next_section_thumbnail_index,
    );
    if lines.is_empty() && !components.is_empty() {
        lines.push(MessageContentLine::styled_text(
            truncate_text("<unsupported message components>", width),
            theme::current().apply(
                theme::HighlightGroup::Muted,
                theme::current().style(theme::HighlightGroup::MessageBody),
            ),
            Vec::new(),
        ));
    }
    lines
}

fn format_components(
    components: &[MessageComponentInfo],
    context: &ComponentFormatContext<'_>,
    state: &DashboardState,
    width: usize,
    loaded_custom_emoji_urls: &[String],
    next_section_thumbnail_index: &mut usize,
) -> Vec<MessageContentLine> {
    let mut lines = Vec::new();
    for component in components {
        lines.extend(format_component(
            component,
            context,
            state,
            width,
            loaded_custom_emoji_urls,
            next_section_thumbnail_index,
        ));
    }
    lines
}

fn format_component(
    component: &MessageComponentInfo,
    context: &ComponentFormatContext<'_>,
    state: &DashboardState,
    width: usize,
    loaded_custom_emoji_urls: &[String],
    next_section_thumbnail_index: &mut usize,
) -> Vec<MessageContentLine> {
    match component {
        MessageComponentInfo::ActionRow { components } => format_components(
            components,
            context,
            state,
            width,
            loaded_custom_emoji_urls,
            next_section_thumbnail_index,
        ),
        MessageComponentInfo::Button {
            label,
            emoji,
            url,
            disabled,
        } => {
            let label = component_button_label(label.as_deref(), emoji.as_deref());
            let disabled = if *disabled { " (disabled)" } else { "" };
            let text = match url.as_deref() {
                Some(url) => format!("[{label}]{disabled} {url}"),
                None => format!("[{label}]{disabled}"),
            };
            format_text(
                &text,
                context,
                state,
                width,
                theme::current().style(theme::HighlightGroup::MessageBody),
                loaded_custom_emoji_urls,
            )
        }
        MessageComponentInfo::Select {
            kind,
            placeholder,
            options,
            disabled,
        } => {
            let label = placeholder
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    let options = options
                        .iter()
                        .take(3)
                        .map(|option| option.label.as_str())
                        .collect::<Vec<_>>();
                    (!options.is_empty()).then(|| options.join(", "))
                })
                .unwrap_or_else(|| component_select_kind_label(*kind).to_owned());
            let disabled = if *disabled { " (disabled)" } else { "" };
            format_text(
                &format!("[Select: {label}]{disabled}"),
                context,
                state,
                width,
                theme::current().style(theme::HighlightGroup::MessageBody),
                loaded_custom_emoji_urls,
            )
        }
        MessageComponentInfo::Section {
            components,
            accessory,
        } => {
            let section_thumbnail = accessory.as_deref().and_then(|accessory| match accessory {
                MessageComponentInfo::Thumbnail { media, .. } => {
                    let index = *next_section_thumbnail_index;
                    *next_section_thumbnail_index =
                        (*next_section_thumbnail_index).saturating_add(1);
                    Some((index, media))
                }
                _ => None,
            });
            if let Some((index, media)) = section_thumbnail
                && let Some((text_width, slot)) =
                    section_thumbnail_slot(index, media, context, state, width)
            {
                let mut lines = format_components(
                    components,
                    context,
                    state,
                    text_width,
                    loaded_custom_emoji_urls,
                    next_section_thumbnail_index,
                );
                if lines.is_empty() {
                    lines.push(MessageContentLine::plain(String::new()));
                }
                while lines.len() < usize::from(slot.height) {
                    lines.push(MessageContentLine::plain(String::new()));
                }
                lines[0].preview_slots.push(slot);
                return lines;
            }

            let mut lines = format_components(
                components,
                context,
                state,
                width,
                loaded_custom_emoji_urls,
                next_section_thumbnail_index,
            );
            if let Some(accessory) = accessory {
                lines.extend(format_component(
                    accessory,
                    context,
                    state,
                    width,
                    loaded_custom_emoji_urls,
                    next_section_thumbnail_index,
                ));
            }
            lines
        }
        MessageComponentInfo::TextDisplay { content } => {
            let content = render_discord_timestamps(content, state.hour_format_24());
            format_text(
                &content,
                context,
                state,
                width,
                theme::current().style(theme::HighlightGroup::MessageBody),
                loaded_custom_emoji_urls,
            )
        }
        MessageComponentInfo::Thumbnail {
            media, description, ..
        } => vec![format_component_media_line(
            media,
            description.as_deref(),
            context,
            width,
        )],
        MessageComponentInfo::MediaGallery { items } => items
            .iter()
            .map(|item| {
                format_component_media_line(
                    &item.media,
                    item.description.as_deref(),
                    context,
                    width,
                )
            })
            .collect(),
        MessageComponentInfo::File {
            file, name, size, ..
        } => vec![format_component_file_line(
            file,
            name.as_deref(),
            *size,
            context,
            width,
        )],
        MessageComponentInfo::Separator { divider, spacing } => {
            let mut lines = Vec::new();
            if *divider {
                lines.push(MessageContentLine::dim("─".repeat(width.max(1))));
            }
            if *spacing >= 2 {
                lines.push(MessageContentLine::plain(String::new()));
            }
            lines
        }
        MessageComponentInfo::Container {
            components,
            accent_color,
            ..
        } => {
            const PREFIX: &str = "  ▎ ";
            let inner_width = width.saturating_sub(PREFIX.width()).max(1);
            let gutter_style = accent_color.map_or_else(
                || theme::current().style(theme::HighlightGroup::EmbedGutter),
                |color| {
                    theme::current()
                        .style(theme::HighlightGroup::EmbedGutter)
                        .fg(embed_color(color))
                },
            );
            format_components(
                components,
                context,
                state,
                inner_width,
                loaded_custom_emoji_urls,
                next_section_thumbnail_index,
            )
            .into_iter()
            .map(|line| prefix_message_content_line_with_style(PREFIX, gutter_style, line))
            .collect()
        }
        MessageComponentInfo::Unknown { .. } => Vec::new(),
    }
}

fn section_thumbnail_slot(
    section_thumbnail_index: usize,
    media: &ComponentMediaInfo,
    context: &ComponentFormatContext<'_>,
    state: &DashboardState,
    width: usize,
) -> Option<(usize, MessageContentPreviewSlot)> {
    if !state.show_images() {
        return None;
    }
    let attachment = media.attachment_filename().and_then(|filename| {
        context
            .attachments
            .iter()
            .find(|attachment| attachment.filename == filename)
    });
    let media_type = attachment
        .and_then(AttachmentInfo::media_type)
        .or_else(|| media.media_type());
    if media_type != Some(AttachmentMediaType::Image) {
        return None;
    }

    let available = width.saturating_sub(SECTION_TEXT_MIN_WIDTH + SECTION_THUMBNAIL_GAP);
    let slot_width = u16::try_from(available)
        .unwrap_or(u16::MAX)
        .min(SECTION_THUMBNAIL_MAX_WIDTH);
    if slot_width < SECTION_THUMBNAIL_MIN_WIDTH {
        return None;
    }
    let text_width = width
        .saturating_sub(usize::from(slot_width) + SECTION_THUMBNAIL_GAP)
        .max(1);
    let col = u16::try_from(text_width + SECTION_THUMBNAIL_GAP).unwrap_or(u16::MAX);
    Some((
        text_width,
        MessageContentPreviewSlot {
            section_thumbnail_index,
            col,
            width: slot_width,
            height: SECTION_THUMBNAIL_HEIGHT,
        },
    ))
}

fn format_text(
    content: &str,
    context: &ComponentFormatContext<'_>,
    state: &DashboardState,
    width: usize,
    style: Style,
    loaded_custom_emoji_urls: &[String],
) -> Vec<MessageContentLine> {
    if content.is_empty() {
        return Vec::new();
    }
    let rendered = state.render_user_mentions_with_highlights(
        context.guild_id,
        context.mentions,
        context.mention_everyone,
        context.mention_roles,
        content,
    );
    wrap_markdown_message_lines_with_loaded_custom_emoji_urls(
        state,
        rendered,
        width.max(1),
        style,
        loaded_custom_emoji_urls,
    )
}

fn format_component_media_line(
    media: &ComponentMediaInfo,
    description: Option<&str>,
    context: &ComponentFormatContext<'_>,
    width: usize,
) -> MessageContentLine {
    let attachment = media.attachment_filename().and_then(|filename| {
        context
            .attachments
            .iter()
            .find(|attachment| attachment.filename == filename)
    });
    let media_type = attachment
        .and_then(AttachmentInfo::media_type)
        .or_else(|| media.media_type());
    let kind = component_media_kind(media_type);
    let label = description
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            attachment.map_or_else(
                || media.display_filename(),
                |attachment| &attachment.filename,
            )
        });
    let dimensions = attachment
        .map(|attachment| (attachment.width, attachment.height))
        .unwrap_or((media.width, media.height));
    let dimensions = match dimensions {
        (Some(width), Some(height)) => format!(" {width}x{height}"),
        _ => String::new(),
    };
    MessageContentLine::attachment(truncate_text(
        &format!("[{kind}: {label}]{dimensions}"),
        width,
    ))
}

fn format_component_file_line(
    media: &ComponentMediaInfo,
    name: Option<&str>,
    size: Option<u64>,
    context: &ComponentFormatContext<'_>,
    width: usize,
) -> MessageContentLine {
    let attachment = media.attachment_filename().and_then(|filename| {
        context
            .attachments
            .iter()
            .find(|attachment| attachment.filename == filename)
    });
    let label = name
        .filter(|value| !value.trim().is_empty())
        .or_else(|| attachment.map(|attachment| attachment.filename.as_str()))
        .unwrap_or_else(|| media.display_filename());
    let size = size
        .or_else(|| attachment.map(|attachment| attachment.size))
        .filter(|size| *size > 0)
        .map(|size| format!(" · {size} bytes"))
        .unwrap_or_default();
    MessageContentLine::attachment(truncate_text(&format!("[file: {label}]{size}"), width))
}

fn component_button_label(label: Option<&str>, emoji: Option<&str>) -> String {
    match (
        emoji.filter(|value| !value.trim().is_empty()),
        label.filter(|value| !value.trim().is_empty()),
    ) {
        (Some(emoji), Some(label)) => format!("{emoji} {label}"),
        (Some(emoji), None) => emoji.to_owned(),
        (None, Some(label)) => label.to_owned(),
        (None, None) => "Button".to_owned(),
    }
}

fn component_select_kind_label(kind: ComponentSelectKind) -> &'static str {
    match kind {
        ComponentSelectKind::String => "options",
        ComponentSelectKind::User => "user",
        ComponentSelectKind::Role => "role",
        ComponentSelectKind::Mentionable => "mentionable",
        ComponentSelectKind::Channel => "channel",
    }
}

fn component_media_kind(media_type: Option<AttachmentMediaType>) -> &'static str {
    match media_type {
        Some(AttachmentMediaType::Image) => "image",
        Some(AttachmentMediaType::Video) => "video",
        Some(AttachmentMediaType::Audio) => "audio",
        None => "media",
    }
}
