use crate::discord::AttachmentDownloadId;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use crate::discord::PresenceStatus;
use crate::discord::{
    ChannelState, ChannelUnreadState, GuildFolder, GuildState, MuteDuration, ReactionEmoji,
    ReactionInfo, VoiceParticipantState,
};
use ratatui_image::protocol::Protocol;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelSwitcherItem {
    pub channel_id: Id<ChannelMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub guild_name: Option<String>,
    pub group_label: String,
    pub parent_label: Option<String>,
    pub channel_label: String,
    pub unread: ChannelUnreadState,
    pub unread_message_count: usize,
    pub search_name: String,
    pub depth: usize,
    pub group_order: usize,
    pub original_index: usize,
}

#[cfg(test)]
#[allow(dead_code)]
impl ChannelSwitcherItem {
    pub(crate) fn test(channel_id: Id<ChannelMarker>) -> Self {
        Self {
            channel_id,
            guild_id: None,
            guild_name: None,
            group_label: String::new(),
            parent_label: None,
            channel_label: String::new(),
            unread: ChannelUnreadState::Seen,
            unread_message_count: 0,
            search_name: String::new(),
            depth: 0,
            group_order: 0,
            original_index: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusPane {
    Guilds,
    Channels,
    Messages,
    Members,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachmentDownloadProgressView {
    pub id: AttachmentDownloadId,
    pub filename: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageActionKind {
    CopyContent,
    OpenReactionPicker,
    Reply,
    OpenDeleteConfirmation,
    Edit,
    OpenUrl,
    RemoveEmbeds,
    PlayMedia,
    ViewAttachment,
    ShowProfile,
    OpenPinConfirmation,
    OpenThread,
    ShowReactionUsers,
    OpenPollVotePicker,
    GoToReferencedMessage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionItem<K> {
    pub kind: K,
    pub label: String,
    pub enabled: bool,
}

impl<K> ActionItem<K> {
    pub(crate) fn new(kind: K, label: impl Into<String>, enabled: bool) -> Self {
        Self {
            kind,
            label: label.into(),
            enabled,
        }
    }
}

pub type MessageActionItem = ActionItem<MessageActionKind>;

#[cfg(test)]
#[allow(dead_code)]
impl ActionItem<MessageActionKind> {
    pub(crate) fn test(kind: MessageActionKind) -> Self {
        Self::new(kind, String::new(), true)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageUrlItem {
    pub url: String,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchFieldView {
    pub label: String,
    pub value: String,
    pub placeholder: String,
    pub active: bool,
    pub cursor: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageSearchResultItem {
    pub channel_id: Id<ChannelMarker>,
    pub message_id: Id<MessageMarker>,
    pub channel_label: String,
    pub author: String,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberSearchResultItem {
    pub user_id: Id<UserMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub display_name: String,
    pub username: Option<String>,
    pub status: PresenceStatus,
    pub is_bot: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelSearchSuggestionItem {
    pub channel_id: Id<ChannelMarker>,
    pub channel_label: String,
    pub guild_label: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SearchSuggestionItem {
    Member(MemberSearchResultItem),
    Channel(ChannelSearchSuggestionItem),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SearchResultItem {
    Message(MessageSearchResultItem),
    Member(MemberSearchResultItem),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchPopupMode {
    Message,
    Member,
}

impl SearchPopupMode {
    pub fn title(self) -> &'static str {
        match self {
            Self::Message => "Message Search",
            Self::Member => "Member Search",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchPopupView {
    pub mode: SearchPopupMode,
    pub fields: Vec<SearchFieldView>,
    pub suggestions: Vec<SearchSuggestionItem>,
    pub selected_suggestion: usize,
    pub results: Vec<SearchResultItem>,
    pub selected: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub total_results: Option<usize>,
    pub has_more: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForumPostComposerField {
    Title,
    Body,
    Attachments,
    Tags,
    Submit,
    Cancel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForumPostComposerTagView {
    pub name: String,
    /// Unicode emoji shown inline; `None` for a custom or emoji-less tag.
    pub unicode_emoji: Option<String>,
    /// CDN url of a custom tag emoji, overlaid as an image on a reserved gap.
    pub custom_emoji_url: Option<String>,
    /// Resolved `:name:` text fallback shown until the custom emoji image loads.
    pub custom_emoji_label: Option<String>,
    pub selected: bool,
    pub active: bool,
    /// Whether this tag can still be toggled on. `false` for unselected tags
    /// once the five-tag cap is reached, so the renderer can dim them.
    pub selectable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForumPostComposerAttachmentView {
    pub filename: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForumPostComposerView {
    pub channel_label: String,
    pub active_field: ForumPostComposerField,
    pub editing_field: Option<ForumPostComposerField>,
    pub title: String,
    pub title_cursor: usize,
    pub body: String,
    pub body_cursor: usize,
    pub attachments: Vec<ForumPostComposerAttachmentView>,
    pub tags: Vec<ForumPostComposerTagView>,
    pub requires_tag: bool,
    pub paste_pending: bool,
    pub status: Option<String>,
}

/// Focusable cells in the thread edit popup. A leaner mirror of
/// [`ForumPostComposerField`]: there is no body or attachments, and the
/// slow-mode and auto-archive selectors replace them. The Tags cell only
/// applies to forum posts and is hidden for regular threads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadEditField {
    Title,
    Tags,
    SlowMode,
    AutoArchive,
    Submit,
    Cancel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadEditTagView {
    pub name: String,
    /// Unicode emoji shown inline; `None` for a custom or emoji-less tag.
    pub unicode_emoji: Option<String>,
    /// CDN url of a custom tag emoji, overlaid as an image on a reserved gap.
    pub custom_emoji_url: Option<String>,
    /// Resolved `:name:` text fallback shown until the custom emoji image loads.
    pub custom_emoji_label: Option<String>,
    pub selected: bool,
    pub active: bool,
    /// Whether this tag can still be toggled on. `false` for unselected tags
    /// once the five-tag cap is reached, so the renderer can dim them.
    pub selectable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadEditView {
    pub channel_label: String,
    pub active_field: ThreadEditField,
    pub editing_title: bool,
    pub editing_tags: bool,
    pub title: String,
    pub title_cursor: usize,
    /// Whether the edited thread is a forum post. Only forum posts have tags, so
    /// the renderer omits the Tags row entirely when this is `false`.
    pub is_forum_post: bool,
    pub tags: Vec<ThreadEditTagView>,
    pub requires_tag: bool,
    /// Display label for the current slow-mode option, e.g. "5s" or "Off".
    pub slow_mode_label: String,
    /// Whether the slow-mode selector can be changed (manage-channel permission).
    pub can_set_slow_mode: bool,
    /// Display label for the current auto-archive option, e.g. "1 day".
    pub auto_archive_label: String,
    pub status: Option<String>,
}

pub enum LocalUploadPreviewView<'a> {
    Loading { filename: String },
    Ready { protocol: &'a Protocol },
    Failed { filename: String, message: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachmentViewerItem {
    pub index: usize,
    pub total: usize,
    pub filename: String,
    pub url: Option<String>,
    pub size_bytes: u64,
    pub is_image: bool,
    pub is_video: bool,
}

#[cfg(test)]
#[allow(dead_code)]
impl AttachmentViewerItem {
    pub(crate) fn test() -> Self {
        Self {
            index: 0,
            total: 0,
            filename: String::new(),
            url: None,
            size_bytes: 0,
            is_image: false,
            is_video: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ChannelActionKind {
    JoinVoice,
    LeaveVoice,
    LoadPinnedMessages,
    ShowThreads,
    MarkAsRead,
    ToggleMute,
}

pub type ChannelActionItem = ActionItem<ChannelActionKind>;

/// Actions on a thread (a regular thread or a forum post). Mirrors Discord's
/// thread/forum-post right-click menu. `Pin` only applies to forum posts; the
/// rest apply to every thread (see `selected_thread_action_items`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ThreadActionKind {
    MarkAsRead,
    ToggleFollow,
    Close,
    Lock,
    Edit,
    CopyLink,
    ToggleMute,
    NotificationSettings,
    Pin,
    Delete,
    CopyId,
}

pub type ThreadActionItem = ActionItem<ThreadActionKind>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GuildActionKind {
    NoActionsYet,
    MarkAsRead,
    ToggleMute,
    LeaveServer,
    FolderSettings,
}

pub type GuildActionItem = ActionItem<GuildActionKind>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MuteActionDurationItem {
    pub label: &'static str,
    pub duration: MuteDuration,
}

/// A single row in the thread notification-settings submenu. The label already
/// includes the `[x]`/`[ ]` radio prefix so the renderer needs no extra logic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadNotificationItem {
    pub label: String,
    pub flags: u64,
}

impl ThreadNotificationItem {
    pub(crate) fn new(raw_label: &str, flags: u64, current_flags: u64) -> Self {
        let prefix = if flags == current_flags {
            "[x] "
        } else {
            "[ ] "
        };
        Self {
            label: format!("{prefix}{raw_label}"),
            flags,
        }
    }
}

pub const MUTE_ACTION_DURATIONS: [MuteActionDurationItem; 6] = [
    MuteActionDurationItem {
        label: "15 minutes",
        duration: MuteDuration::Minutes(15),
    },
    MuteActionDurationItem {
        label: "1 hour",
        duration: MuteDuration::Minutes(60),
    },
    MuteActionDurationItem {
        label: "3 hours",
        duration: MuteDuration::Minutes(180),
    },
    MuteActionDurationItem {
        label: "8 hours",
        duration: MuteDuration::Minutes(480),
    },
    MuteActionDurationItem {
        label: "24 hours",
        duration: MuteDuration::Minutes(1_440),
    },
    MuteActionDurationItem {
        label: "Permanently",
        duration: MuteDuration::Permanent,
    },
];

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MemberActionKind {
    ShowProfile,
}

pub type MemberActionItem = ActionItem<MemberActionKind>;

const FORUM_POST_CARD_HEIGHT: usize = 6;

/// A forum tag applied to a post, resolved into display-ready form. At most one
/// emoji field is set: a unicode character, or a custom emoji's CDN image url.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppliedForumTag {
    pub name: String,
    pub unicode_emoji: Option<String>,
    pub custom_emoji_url: Option<String>,
}

#[cfg(test)]
#[allow(dead_code)]
impl AppliedForumTag {
    pub(crate) fn test(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            unicode_emoji: None,
            custom_emoji_url: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelThreadItem {
    pub channel_id: Id<ChannelMarker>,
    pub section_label: Option<String>,
    pub label: String,
    pub archived: bool,
    pub locked: bool,
    pub pinned: bool,
    pub preview_author_id: Option<Id<UserMarker>>,
    pub preview_author: Option<String>,
    pub preview_author_color: Option<u32>,
    pub preview_content: Option<String>,
    pub applied_tags: Vec<AppliedForumTag>,
    pub preview_reactions: Vec<ReactionInfo>,
    pub comment_count: Option<u64>,
    pub new_message_count: usize,
    pub last_activity_message_id: Option<Id<MessageMarker>>,
}

impl ChannelThreadItem {
    pub fn rendered_height(&self) -> usize {
        self.card_height() + usize::from(self.section_label.is_some())
    }

    /// Card body height. The tags row is dropped entirely when the post has no
    /// tags, so an untagged post (and every regular thread) is one row shorter.
    pub fn card_height(&self) -> usize {
        FORUM_POST_CARD_HEIGHT - usize::from(self.applied_tags.is_empty())
    }
}

#[cfg(test)]
#[allow(dead_code)]
impl ChannelThreadItem {
    pub(crate) fn test(channel_id: Id<ChannelMarker>) -> Self {
        Self {
            channel_id,
            section_label: None,
            label: String::new(),
            archived: false,
            locked: false,
            pinned: false,
            preview_author_id: None,
            preview_author: None,
            preview_author_color: None,
            preview_content: None,
            applied_tags: Vec::new(),
            preview_reactions: Vec::new(),
            comment_count: None,
            new_message_count: 0,
            last_activity_message_id: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmojiReactionItem {
    pub emoji: ReactionEmoji,
    pub label: String,
}

impl EmojiReactionItem {
    pub fn custom_image_url(&self) -> Option<String> {
        self.emoji.custom_image_url()
    }
}

#[cfg(test)]
#[allow(dead_code)]
impl EmojiReactionItem {
    pub(crate) fn test(emoji: ReactionEmoji) -> Self {
        Self {
            emoji,
            label: String::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PollVotePickerItem {
    pub answer_id: u8,
    pub label: String,
    pub selected: bool,
}

#[cfg(test)]
#[allow(dead_code)]
impl PollVotePickerItem {
    pub(crate) fn test(answer_id: u8) -> Self {
        Self {
            answer_id,
            label: String::new(),
            selected: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadSummary {
    pub channel_id: Id<ChannelMarker>,
    pub name: String,
    pub message_count: Option<u64>,
    pub total_message_sent: Option<u64>,
    pub archived: Option<bool>,
    pub locked: Option<bool>,
    pub latest_message_id: Option<Id<MessageMarker>>,
    pub latest_message_preview: Option<ThreadMessagePreview>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadMessagePreview {
    pub author: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub enum ChannelPaneEntry<'a> {
    CategoryHeader {
        state: &'a ChannelState,
        collapsed: bool,
    },
    Channel {
        state: &'a ChannelState,
        branch: ChannelBranch,
    },
    Thread {
        state: &'a ChannelState,
        parent_branch: ChannelBranch,
        branch: ChannelBranch,
    },
    VoiceParticipant {
        participant: VoiceParticipantState,
        parent_branch: ChannelBranch,
    },
}

impl ChannelPaneEntry<'_> {
    pub fn channel_state(&self) -> Option<&ChannelState> {
        match self {
            Self::Channel { state, .. } | Self::Thread { state, .. } => Some(state),
            Self::CategoryHeader { .. } | Self::VoiceParticipant { .. } => None,
        }
    }

    pub fn channel_id(&self) -> Option<Id<ChannelMarker>> {
        self.channel_state().map(|state| state.id)
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::CategoryHeader { state, .. }
            | Self::Channel { state, .. }
            | Self::Thread { state, .. } => state.name.as_str(),
            Self::VoiceParticipant { participant, .. } => participant.display_name.as_str(),
        }
    }

    pub(super) fn is_selectable(&self) -> bool {
        matches!(
            self,
            Self::CategoryHeader { .. } | Self::Channel { .. } | Self::Thread { .. }
        )
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ChannelBranch {
    None,
    Middle,
    Last,
}

impl ChannelBranch {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Middle => "├ ",
            Self::Last => "└ ",
        }
    }

    pub fn participant_prefix(self) -> &'static str {
        match self {
            Self::None => "  ",
            Self::Middle => "│ ",
            Self::Last => "  ",
        }
    }

    pub(super) fn is_category_child(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum GuildPaneEntry<'a> {
    DirectMessages,
    FolderHeader {
        folder: &'a GuildFolder,
        collapsed: bool,
    },
    Guild {
        state: &'a GuildState,
        branch: GuildBranch,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GuildBranch {
    None,
    Middle,
    Last,
}

impl GuildBranch {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Middle => "├ ",
            Self::Last => "└ ",
        }
    }

    pub(super) fn is_folder_child(self) -> bool {
        !matches!(self, Self::None)
    }
}

impl GuildPaneEntry<'_> {
    pub fn guild_state(&self) -> Option<&GuildState> {
        match self {
            Self::Guild { state, .. } => Some(state),
            Self::DirectMessages | Self::FolderHeader { .. } => None,
        }
    }

    pub fn guild_id(&self) -> Option<Id<GuildMarker>> {
        self.guild_state().map(|state| state.id)
    }

    pub fn label(&self) -> &str {
        match self {
            Self::DirectMessages => "Direct Messages",
            Self::FolderHeader { folder, .. } => folder.name.as_deref().unwrap_or("Folder"),
            Self::Guild { state, .. } => state.name.as_str(),
        }
    }
}
