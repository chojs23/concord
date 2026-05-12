use ratatui::{layout::Rect, style::Color, text::Line};
use ratatui_image::protocol::{Protocol, StatefulProtocol};

use super::super::state::FocusPane;

pub(super) const ACCENT: Color = Color::Cyan;
pub(super) const DIM: Color = Color::DarkGray;
pub(super) const SCROLLBAR_THUMB: Color = Color::Rgb(170, 170, 170);
pub(super) const MIN_MESSAGE_INPUT_HEIGHT: u16 = 3;
pub(super) const IMAGE_PREVIEW_HEIGHT: u16 = 10;
pub(super) const IMAGE_PREVIEW_WIDTH: u16 = 72;
pub(super) const MESSAGE_AVATAR_PLACEHOLDER: &str = "oo";
pub(super) const MESSAGE_AVATAR_OFFSET: u16 = 3;
pub(crate) const MESSAGE_ROW_GAP: usize = 1;
pub(super) const EMBED_PREVIEW_GUTTER_PREFIX: &str = "  ▎ ";
pub(super) const DISCORD_EPOCH_MILLIS: u64 = 1_420_070_400_000;
pub(super) const SNOWFLAKE_TIMESTAMP_SHIFT: u8 = 22;
pub(super) const MAX_REACTION_USERS_VISIBLE_LINES: usize = 14;
pub(super) const IMAGE_VIEWER_POPUP_WIDTH: u16 = 78;
pub(super) const IMAGE_VIEWER_POPUP_HEIGHT: u16 = 16;
pub(super) const SELECTED_FORUM_POST_BORDER: Color = Color::Green;
pub(super) const SELECTED_MESSAGE_BORDER: Color = Color::Green;
pub(super) const SELECTED_MESSAGE_CONTENT_OFFSET: u16 = 2;

pub struct ImagePreview<'a> {
    pub viewer: bool,
    pub message_index: usize,
    pub preview_x_offset_columns: u16,
    pub preview_y_offset_rows: usize,
    pub preview_width: u16,
    pub preview_height: u16,
    pub preview_overflow_count: usize,
    pub accent_color: Option<u32>,
    pub state: ImagePreviewState<'a>,
}

pub struct AvatarImage {
    pub row: isize,
    pub visible_height: u16,
    pub protocol: Protocol,
}

pub struct EmojiImage<'a> {
    pub url: String,
    pub protocol: &'a Protocol,
}

#[derive(Clone, Copy)]
pub struct ImagePreviewLayout {
    pub list_height: usize,
    pub content_width: usize,
    pub preview_width: u16,
    pub max_preview_height: u16,
    pub viewer_preview_width: u16,
    pub viewer_max_preview_height: u16,
}

#[derive(Clone, Copy)]
pub(super) struct MessageViewportLayout {
    pub(super) content_width: usize,
    pub(super) list_width: usize,
    pub(super) selected_card_width: usize,
    pub(super) preview_width: u16,
    pub(super) max_preview_height: u16,
}

pub enum ImagePreviewState<'a> {
    Loading { filename: String },
    Failed { filename: String, message: String },
    Ready { protocol: &'a mut StatefulProtocol },
}

#[derive(Clone, Copy)]
pub(super) struct DashboardAreas {
    pub(super) header: Rect,
    pub(super) guilds: Rect,
    pub(super) channels: Rect,
    pub(super) messages: Rect,
    pub(super) members: Rect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MessageAreas {
    /// One-row strip pinned above the message list while the active channel
    /// has unread messages. Height is zero (and the list reclaims the row)
    /// when no banner needs to be shown.
    pub(super) unread_banner: Rect,
    pub(super) list: Rect,
    /// One-row strip rendered between the message list and the composer when
    /// somebody else is typing in the selected channel. Width is zero when
    /// nobody is typing so the message list reclaims the row.
    pub(super) typing: Rect,
    pub(super) composer: Rect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MouseTarget {
    Pane(FocusPane),
    PaneRow { pane: FocusPane, row: usize },
    Composer,
    ActionRow { menu: ActionMenuTarget, row: usize },
    ChannelSwitcherRow { row: usize },
    ModalBackdrop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActionMenuTarget {
    Message,
    Guild,
    Channel,
    Member,
}

pub(super) struct UserProfilePopupText {
    pub(super) lines: Vec<Line<'static>>,
    pub(super) emoji_overlays: Vec<(usize, String)>,
}
