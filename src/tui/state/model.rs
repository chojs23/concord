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
    PlayMedia,
    ViewAttachment,
    ShowProfile,
    OpenPinConfirmation,
    OpenThread,
    ShowReactionUsers,
    OpenPollVotePicker,
    GoToReferencedMessage,
}

// Message action will be removed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageActionItem {
    pub kind: MessageActionKind,
    pub label: String,
    pub enabled: bool,
}

#[cfg(test)]
#[allow(dead_code)]
impl MessageActionItem {
    pub(crate) fn test(kind: MessageActionKind) -> Self {
        Self {
            kind,
            label: String::new(),
            enabled: true,
        }
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelActionItem {
    pub kind: ChannelActionKind,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GuildActionKind {
    NoActionsYet,
    MarkAsRead,
    ToggleMute,
    LeaveServer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildActionItem {
    pub kind: GuildActionKind,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MuteActionDurationItem {
    pub label: &'static str,
    pub duration: MuteDuration,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberActionItem {
    pub kind: MemberActionKind,
    pub label: String,
    pub enabled: bool,
}

pub const FORUM_POST_CARD_HEIGHT: usize = 5;

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
    pub preview_reactions: Vec<ReactionInfo>,
    pub comment_count: Option<u64>,
    pub new_message_count: usize,
    pub last_activity_message_id: Option<Id<MessageMarker>>,
}

impl ChannelThreadItem {
    pub fn rendered_height(&self) -> usize {
        FORUM_POST_CARD_HEIGHT + usize::from(self.section_label.is_some())
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
