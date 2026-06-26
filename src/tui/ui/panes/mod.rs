use std::collections::HashSet;

use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use ratatui_image::Image as RatatuiImage;
use unicode_width::UnicodeWidthStr;

use crate::discord::{
    ActivityInfo, ActivityKind, ChannelUnreadState, MessageState, PresenceStatus,
};

use super::super::{
    format::{
        format_byte_size, sanitize_for_display_width, truncate_display_width,
        truncate_display_width_from,
    },
    message::format::{EMOJI_REACTION_IMAGE_WIDTH, format_attachment_summary, wrap_text_lines},
    state::{
        ChannelPaneEntry, CommandPickerEntry, DashboardState, DmComposerLock, EmojiPickerEntry,
        FocusPane, GuildPaneEntry, LocalUploadPreviewView, MAX_MENTION_PICKER_VISIBLE, MemberEntry,
        MemberGroup, MentionPickerEntry, MentionPickerTarget, discord_color, folder_color,
        presence_color, presence_marker,
    },
};
use super::{
    LOCAL_UPLOAD_PREVIEW_HEIGHT, LOCAL_UPLOAD_PREVIEW_WIDTH, active_text_style,
    activity::{ActivityLeading, ActivityRender, build_activity_render},
    channel_prefix, channel_unread_decoration, dm_presence_dot_span, highlight_style,
    layout::{
        composer_inner_width, composer_rows_before_input, composer_upload_preview_line_count,
        panel_scrollbar_area, prefixed_composer_input, vertical_scrollbar_visible,
    },
    panel_block, panel_block_line, render_vertical_scrollbar, selection_marker, styled_list_item,
    types::{ACCENT, DIM, EmojiImage, MessageAreas},
};

mod channels;
mod composer;
mod guilds;
mod header;
mod members;
mod shared;

pub(super) use channels::render_channels;
pub(super) use composer::{
    active_composer_picker_area, render_composer, render_composer_command_picker,
    render_composer_emoji_picker, render_composer_mention_picker,
};
#[cfg(test)]
pub(super) use composer::{
    composer_cursor_position, composer_lines, composer_lines_with_loaded_custom_emoji_urls,
    composer_text, emoji_picker_lines,
};
pub(super) use guilds::render_guilds;
pub(super) use header::render_header;
pub(super) use members::render_members;
#[cfg(test)]
pub(super) use members::{member_display_label, member_name_style, primary_activity_summary};
use shared::{
    notification_count_badge, render_pane_filter_bar_with_cursor, split_pane_filter_area,
};
