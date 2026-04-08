use std::collections::HashSet;

use twilight_model::id::{Id, marker::MessageMarker};

use super::super::{
    selection,
    state::DashboardState,
    ui::{self, ImagePreviewLayout},
};
use super::AVATAR_PREVIEW_HEIGHT;

const IMAGE_PREVIEW_SOURCE_PIXELS_PER_COLUMN: u64 = 10;

pub(in crate::tui) struct ImagePreviewTarget {
    pub(super) message_index: usize,
    pub(super) preview_width: u16,
    pub(super) preview_height: u16,
    pub(super) visible_preview_height: u16,
    pub(super) top_clip_rows: u16,
    pub(super) accent_color: Option<u32>,
    pub(super) message_id: Id<MessageMarker>,
    pub(super) url: String,
    pub(super) filename: String,
}

#[derive(Clone)]
pub(in crate::tui) struct AvatarTarget {
    pub(super) row: isize,
    pub(super) visible_height: u16,
    pub(super) top_clip_rows: u16,
    pub(super) url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct EmojiImageTarget {
    pub(super) url: String,
}

pub(in crate::tui) fn visible_image_preview_targets(
    state: &DashboardState,
    layout: ImagePreviewLayout,
) -> Vec<ImagePreviewTarget> {
    let mut rendered_rows = 0usize;
    let mut targets = Vec::new();

    for (message_index, message) in state.visible_messages().into_iter().enumerate() {
        if rendered_rows >= layout.list_height {
            break;
        }

        let line_offset = usize::from(message_index == 0) * state.message_line_scroll();
        let base_rows = state.message_base_line_count_for_width(message, layout.content_width);

        let Some(preview) = message.first_inline_preview() else {
            rendered_rows = rendered_rows.saturating_add(
                base_rows
                    .saturating_add(ui::MESSAGE_ROW_GAP)
                    .saturating_sub(line_offset),
            );
            continue;
        };

        let preview_height = image_preview_height_for_dimensions(
            layout.preview_width,
            layout.max_preview_height,
            preview.width,
            preview.height,
        );
        let preview_top = rendered_rows as isize + base_rows as isize - line_offset as isize;
        let preview_bottom = preview_top.saturating_add(preview_height as isize);
        let visible_top = preview_top.max(0);
        let visible_bottom = preview_bottom.min(layout.list_height as isize);
        if preview_height > 0 && visible_top < visible_bottom {
            targets.push(ImagePreviewTarget {
                message_index,
                preview_width: layout.preview_width,
                preview_height,
                visible_preview_height: u16::try_from(visible_bottom - visible_top)
                    .unwrap_or(u16::MAX),
                top_clip_rows: u16::try_from(visible_top - preview_top).unwrap_or(u16::MAX),
                accent_color: preview.accent_color,
                message_id: message.id,
                url: preview.url.to_owned(),
                filename: preview.filename.to_owned(),
            });
        }

        rendered_rows = rendered_rows.saturating_add(
            base_rows
                .saturating_add(preview_height as usize)
                .saturating_add(ui::MESSAGE_ROW_GAP)
                .saturating_sub(line_offset),
        );
    }

    targets
}

pub(in crate::tui) fn visible_avatar_targets(
    state: &DashboardState,
    layout: ImagePreviewLayout,
) -> Vec<AvatarTarget> {
    let mut rendered_rows = 0usize;
    let mut targets = Vec::new();

    for (local_index, message) in state.visible_messages().into_iter().enumerate() {
        if rendered_rows >= layout.list_height {
            break;
        }

        let line_offset = usize::from(rendered_rows == 0) * state.message_line_scroll();
        let global_index = state.message_scroll().saturating_add(local_index);
        let separator_lines = state.message_extra_top_lines(global_index);
        let body_base_rows = state.message_base_line_count_for_width(message, layout.content_width);
        let block_rows = body_base_rows + separator_lines;
        let message_block_top = rendered_rows as isize - line_offset as isize;
        let body_top = message_block_top + separator_lines as isize;
        let avatar_bottom = body_top.saturating_add(AVATAR_PREVIEW_HEIGHT as isize);
        let visible_top = body_top.max(0);
        let visible_bottom = avatar_bottom.min(layout.list_height as isize);
        if let Some(url) = message.author_avatar_url.as_ref()
            && visible_top < visible_bottom
        {
            targets.push(AvatarTarget {
                row: visible_top,
                visible_height: u16::try_from(visible_bottom - visible_top).unwrap_or(u16::MAX),
                top_clip_rows: u16::try_from(visible_top - body_top).unwrap_or(u16::MAX),
                url: url.clone(),
            });
        }

        let preview_height = message
            .first_inline_preview()
            .map(|preview| {
                image_preview_height_for_dimensions(
                    layout.preview_width,
                    layout.max_preview_height,
                    preview.width,
                    preview.height,
                )
            })
            .unwrap_or(0);
        rendered_rows = rendered_rows.saturating_add(
            block_rows
                .saturating_add(preview_height as usize)
                .saturating_add(ui::MESSAGE_ROW_GAP)
                .saturating_sub(line_offset),
        );
    }

    targets
}

pub(in crate::tui) fn visible_emoji_image_targets(state: &DashboardState) -> Vec<EmojiImageTarget> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut targets: Vec<EmojiImageTarget> = Vec::new();

    // Picker emojis (existing behaviour).
    if state.is_emoji_reaction_picker_open() {
        let reactions = state.emoji_reaction_items();
        if !reactions.is_empty() {
            let selected = state
                .selected_emoji_reaction_index()
                .unwrap_or(0)
                .min(reactions.len().saturating_sub(1));
            let visible_items = reactions
                .len()
                .clamp(1, selection::MAX_EMOJI_REACTION_VISIBLE_ITEMS);
            let visible_range =
                selection::visible_item_range(reactions.len(), selected, visible_items);
            for reaction in &reactions[visible_range] {
                if let Some(url) = reaction.custom_image_url()
                    && seen.insert(url.clone())
                {
                    targets.push(EmojiImageTarget { url });
                }
            }
        }
    }

    // Custom-emoji reactions on currently visible messages so they can be
    // overlaid as inline images below each message body.
    for message in state.visible_messages() {
        for reaction in &message.reactions {
            if reaction.count == 0 {
                continue;
            }
            if let Some(url) = reaction.emoji.custom_image_url()
                && seen.insert(url.clone())
            {
                targets.push(EmojiImageTarget { url });
            }
        }
    }

    targets
}

pub(in crate::tui) fn image_preview_height_for_dimensions(
    preview_width: u16,
    max_preview_height: u16,
    image_width: Option<u64>,
    image_height: Option<u64>,
) -> u16 {
    if preview_width == 0 || max_preview_height == 0 {
        return 0;
    }

    let (Some(image_width), Some(image_height)) = (image_width, image_height) else {
        return max_preview_height;
    };
    if image_width == 0 || image_height == 0 {
        return max_preview_height;
    }

    let source_width_columns = image_width.div_ceil(IMAGE_PREVIEW_SOURCE_PIXELS_PER_COLUMN);
    let preview_width = preview_width.min(u16::try_from(source_width_columns).unwrap_or(u16::MAX));

    let rows = (u128::from(preview_width) * u128::from(image_height))
        .div_ceil(u128::from(image_width) * 3);
    let rows = u16::try_from(rows).unwrap_or(u16::MAX);

    rows.clamp(3.min(max_preview_height), max_preview_height)
}
