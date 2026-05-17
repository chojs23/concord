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

impl MessageActionKind {
    fn preferred_shortcut(&self) -> Option<char> {
        match self {
            MessageActionKind::Reply => Some('R'),
            MessageActionKind::Edit => Some('e'),
            MessageActionKind::Delete => Some('d'),
            MessageActionKind::OpenThread => Some('t'),
            MessageActionKind::ViewImage => Some('v'),
            MessageActionKind::DownloadAttachment(_) => Some('f'),
            MessageActionKind::AddReaction => Some('r'),
            MessageActionKind::RemoveReaction(_) => Some('x'),
            MessageActionKind::ShowReactionUsers => Some('u'),
            MessageActionKind::ShowProfile => Some('p'),
            MessageActionKind::SetPinned(_) => Some('P'),
            MessageActionKind::VotePollAnswer(_) => None,
            MessageActionKind::OpenPollVotePicker => Some('c'),
        }
    }
}

pub fn message_action_shortcut(actions: &[MessageActionItem], index: usize) -> Option<char> {
    let action = actions.get(index)?;
    unique_preferred_shortcut(
        action.kind.preferred_shortcut(),
        actions.iter().map(|item| item.kind.preferred_shortcut()),
    )
    .or_else(|| indexed_shortcut(index))
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

impl ChannelActionKind {
    fn preferred_shortcut(&self) -> char {
        match self {
            ChannelActionKind::JoinVoice => 'j',
            ChannelActionKind::LeaveVoice => 'l',
            ChannelActionKind::LoadPinnedMessages => 'p',
            ChannelActionKind::ShowThreads => 't',
            ChannelActionKind::MarkAsRead => 'm',
            ChannelActionKind::ToggleMute => 'u',
        }
    }
}

pub fn channel_action_shortcut(actions: &[ChannelActionItem], index: usize) -> Option<char> {
    let action = actions.get(index)?;
    unique_preferred_shortcut(
        Some(action.kind.preferred_shortcut()),
        actions
            .iter()
            .map(|item| Some(item.kind.preferred_shortcut())),
    )
    .or_else(|| indexed_shortcut(index))
}

pub fn voice_action_shortcut(actions: &[VoiceActionItem], index: usize) -> Option<char> {
    let action = actions.get(index)?;
    let preferred = match action.kind {
        VoiceActionKind::QuickDeafen => 'd',
        VoiceActionKind::QuickMute => 'm',
        VoiceActionKind::QuickLeave => 'l',
    };
    unique_preferred_shortcut(
        Some(preferred),
        actions.iter().map(|item| match item.kind {
            VoiceActionKind::QuickDeafen => Some('d'),
            VoiceActionKind::QuickMute => Some('m'),
            VoiceActionKind::QuickLeave => Some('l'),
        }),
    )
    .or_else(|| indexed_shortcut(index))
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

pub fn guild_action_shortcut(actions: &[GuildActionItem], index: usize) -> Option<char> {
    let action = actions.get(index)?;
    let preferred = match action.kind {
        GuildActionKind::MarkAsRead => Some('m'),
        GuildActionKind::ToggleMute => Some('u'),
        GuildActionKind::NoActionsYet => None,
    }?;
    unique_preferred_shortcut(
        Some(preferred),
        actions.iter().map(|item| match item.kind {
            GuildActionKind::MarkAsRead => Some('m'),
            GuildActionKind::ToggleMute => Some('u'),
            GuildActionKind::NoActionsYet => None,
        }),
    )
    .or_else(|| indexed_shortcut(index))
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

impl MemberActionKind {
    fn preferred_shortcut(&self) -> char {
        match self {
            MemberActionKind::ShowProfile => 'p',
        }
    }
}

pub fn member_action_shortcut(actions: &[MemberActionItem], index: usize) -> Option<char> {
    let action = actions.get(index)?;
    unique_preferred_shortcut(
        Some(action.kind.preferred_shortcut()),
        actions
            .iter()
            .map(|item| Some(item.kind.preferred_shortcut())),
    )
    .or_else(|| indexed_shortcut(index))
}

pub fn indexed_shortcut(index: usize) -> Option<char> {
    match index {
        0..=8 => char::from_digit(u32::try_from(index + 1).ok()?, 10),
        9 => Some('0'),
        _ => None,
    }
}

pub fn emoji_reaction_shortcut(
    reactions: &[EmojiReactionItem],
    existing_reactions: &[ReactionEmoji],
    index: usize,
) -> Option<char> {
    let reaction = reactions.get(index)?;
    if let Some(existing_index) = existing_reactions
        .iter()
        .position(|existing| existing == &reaction.emoji)
    {
        return qwerty_shortcut(existing_index);
    }

    let regular_index = reactions[..index]
        .iter()
        .filter(|item| !existing_reactions.contains(&item.emoji))
        .count();
    indexed_shortcut(regular_index)
}

fn qwerty_shortcut(index: usize) -> Option<char> {
    const SHORTCUTS: &[u8] = b"qwertyuiop";
    SHORTCUTS.get(index).map(|shortcut| char::from(*shortcut))
}

fn unique_preferred_shortcut(
    preferred: Option<char>,
    shortcuts: impl IntoIterator<Item = Option<char>>,
) -> Option<char> {
    let preferred = preferred?;
    let matches = shortcuts
        .into_iter()
        .filter(|shortcut| shortcut.is_some_and(|shortcut| shortcut == preferred))
        .count();
    (matches == 1).then_some(preferred)
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
