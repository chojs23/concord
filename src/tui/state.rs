use std::collections::HashSet;

use crate::discord::ids::marker::{GuildMarker, UserMarker};

use crate::discord::{
    AppCommand, AppEvent, ForumPostArchiveState, MentionInfo, MessageSnapshotInfo,
};
mod channel_tree;
mod channels;
mod composer;
mod dashboard;
mod diagnostics;
mod discord_ui;
mod emoji;
mod events;
mod guilds;
mod layout_cache;
mod local_upload_preview;
mod member_grouping;
mod message_history_refresh;
mod message_layout;
mod message_render;
mod message_viewport;
mod model;
mod navigation;
mod options;
mod pane_filter;
mod popups;
mod presentation;
mod request_tracking;
mod runtime_state;
mod scroll;
mod subscriptions;
mod text_completion;
mod toast;
mod user;
mod voice_actions;

use composer::ComposerUiState;
use discord_ui::DiscordUiState;
use layout_cache::{LayoutCacheState, MessageRowContentMetrics, MessageRowContentMetricsCacheKey};
use message_history_refresh::MessageHistoryRefreshState;
use message_render::{add_literal_mention_highlights, normalize_text_highlights};
use message_viewport::{MessageViewportState, ThreadReturnTarget};
use navigation::{ActiveGuildScope, FolderKey, FolderSettingsState, NavigationState};
use options::SettingsState;
use pane_filter::PaneFilterState;
use popups::{ModalPopup, PopupUiState};
use request_tracking::RequestTrackingState;
use runtime_state::{
    MediaPlaybackPreparingUiState, RuntimeUiState, ToastMessage, VoiceConnectionUiState,
};
use scroll::clamp_selected_index;

pub use composer::{
    CommandPickerEntry, DmComposerLock, EmojiPickerEntry, MAX_MENTION_PICKER_VISIBLE,
    MentionPickerEntry, MentionPickerTarget,
};
pub use dashboard::DashboardState;
pub use member_grouping::{MemberEntry, MemberGroup};
pub use message_viewport::MessagePaneSource;
pub use model::{
    AppliedForumTag, AttachmentDownloadProgressView, AttachmentViewerItem, ChannelActionItem,
    ChannelPaneEntry, ChannelSearchSuggestionItem, ChannelSwitcherItem, ChannelThreadItem,
    EmojiReactionItem, FocusPane, ForumPostComposerAttachmentView, ForumPostComposerField,
    ForumPostComposerTagView, ForumPostComposerView, GuildActionItem, GuildPaneEntry,
    LocalUploadPreviewView, MemberActionItem, MemberSearchResultItem, MessageActionItem,
    MessageActionKind, MessageSearchResultItem, MuteActionDurationItem, PollVotePickerItem,
    SearchFieldView, SearchPopupMode, SearchPopupView, SearchResultItem, SearchSuggestionItem,
    ThreadActionItem, ThreadEditField, ThreadEditTagView, ThreadEditView, ThreadMessagePreview,
    ThreadNotificationItem, ThreadSummary,
};
pub use model::{
    ChannelActionKind, GuildActionKind, MemberActionKind, MessageUrlItem, ThreadActionKind,
};
pub use options::DisplayOptionItem;
pub(in crate::tui) use popups::{ActiveModalPopupKind, MessageConfirmationKind};
pub use popups::{
    AttachmentViewerZoom, EmojiReactionPickerState, MessageActionMenuState, MessageUrlPickerState,
    NotificationInboxChannelLoad, NotificationInboxItem, NotificationInboxLoad,
    NotificationInboxMessage, NotificationInboxTab, PollVotePickerState, ReactionUsersPopupState,
    UserProfileSettingsField, UserProfileSettingsTab,
};
pub use presentation::{discord_color, folder_color, presence_color, presence_marker};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToastView<'a> {
    pub text: &'a str,
    pub kind: ToastKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopNotification {
    pub title: String,
    pub body: String,
}

fn message_notification_body(
    content: Option<&str>,
    sticker_count: usize,
    attachment_count: usize,
    embed_count: usize,
) -> String {
    let content = content.unwrap_or_default().trim();
    if !content.is_empty() {
        let single_line = content.split_whitespace().collect::<Vec<_>>().join(" ");
        return truncate_notification_text(&single_line, 200);
    }
    if attachment_count > 0 {
        return format!("sent {attachment_count} attachment(s)");
    }
    if sticker_count > 0 {
        return format!("sent {sticker_count} sticker(s)");
    }
    if embed_count > 0 {
        return format!("sent {embed_count} embed(s)");
    }
    "sent a message".to_owned()
}

fn truncate_notification_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

impl DashboardState {
    #[cfg(test)]
    pub fn push_event(&mut self, event: AppEvent) {
        self.push_event_inner(event, true);
    }

    pub fn push_effect(&mut self, event: AppEvent) {
        if let AppEvent::ChannelUpsert(channel) = &event {
            self.record_thread_channel_upserted(channel);
            return;
        }
        self.push_event_inner(event, false);
    }
}

#[cfg(test)]
mod tests;
