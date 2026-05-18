use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusPane {
    Guilds,
    Channels,
    Messages,
    Members,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageActionKind {
    Reply,
    Edit,
    Delete,
    OpenThread,
    ViewImage,
    OpenUrl,
    DownloadAttachment(usize),
    AddReaction,
    RemoveReaction(usize),
    ShowReactionUsers,
    ShowProfile,
    SetPinned(bool),
    VotePollAnswer(u8),
    OpenPollVotePicker,
}

// Message action will be removed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageActionItem {
    pub kind: MessageActionKind,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageActionMenuPhase {
    Actions,
    Urls,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageUrlItem {
    pub url: String,
    pub label: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageViewerItem {
    pub index: usize,
    pub total: usize,
    pub filename: String,
    pub url: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::enum_variant_names)]
pub enum VoiceActionKind {
    QuickDeafen,
    QuickMute,
    QuickLeave,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoiceActionItem {
    pub kind: VoiceActionKind,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuildActionKind {
    NoActionsYet,
    MarkAsRead,
    ToggleMute,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    pub last_activity_message_id: Option<Id<MessageMarker>>,
}

impl ChannelThreadItem {
    pub fn rendered_height(&self) -> usize {
        FORUM_POST_CARD_HEIGHT + usize::from(self.section_label.is_some())
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PollVotePickerItem {
    pub answer_id: u8,
    pub label: String,
    pub selected: bool,
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
    VoiceParticipant {
        participant: VoiceParticipantState,
        parent_branch: ChannelBranch,
    },
}

impl ChannelPaneEntry<'_> {
    pub(super) fn is_selectable(&self) -> bool {
        matches!(self, Self::CategoryHeader { .. } | Self::Channel { .. })
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
    pub fn label(&self) -> &str {
        match self {
            Self::DirectMessages => "Direct Messages",
            Self::FolderHeader { folder, .. } => folder.name.as_deref().unwrap_or("Folder"),
            Self::Guild { state, .. } => state.name.as_str(),
        }
    }
}
