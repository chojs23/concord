use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker, UserMarker},
};

use crate::discord::{ChannelState, GuildFolder, GuildState, ReactionEmoji, ReactionInfo};

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
    OpenThread,
    DownloadImage,
    AddReaction,
    RemoveReaction(usize),
    ShowReactionUsers,
    ShowProfile,
    LoadPinnedMessages,
    SetPinned(bool),
    VotePollAnswer(u8),
    OpenPollVotePicker,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageActionItem {
    pub kind: MessageActionKind,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelActionKind {
    ShowThreads,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelActionItem {
    pub kind: ChannelActionKind,
    pub label: String,
    pub enabled: bool,
}

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

#[derive(Debug, Clone, Copy)]
pub enum ChannelPaneEntry<'a> {
    CategoryHeader {
        state: &'a ChannelState,
        collapsed: bool,
    },
    Channel {
        state: &'a ChannelState,
        branch: ChannelBranch,
    },
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
