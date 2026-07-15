use std::fmt;

use super::{
    ChannelState, DiscordPermission, DiscordState, GuildParticipationDataGap,
    GuildParticipationDecision, GuildParticipationRestriction, PermissionDataGap,
    PermissionDecision,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DiscordAction {
    SendMessage,
    CreateForumPost,
    SendTtsMessage,
    ShowTypingIndicator,
    ApplyModeratedForumTag,
    RemoveMessageEmbeds,
    RunApplicationCommand,
    RemoveReaction,
    PinMessage,
    VotePoll,
    ArchiveThread,
    ChangeThreadLock,
    PinForumPost,
    DeleteThread,
    EditThread,
    ChangeThreadMembership,
    ReopenThread,
    ReadMessageHistory,
    EditMessage,
    DeleteMessage,
    AddReaction,
    JoinVoiceChannel,
    TransmitMicrophone,
}

impl DiscordAction {
    pub const fn requires_guild_participation(self) -> bool {
        !matches!(self, Self::ReadMessageHistory)
    }

    const fn base_permission(self) -> Option<DiscordPermission> {
        match self {
            Self::SendMessage | Self::CreateForumPost | Self::ShowTypingIndicator => {
                Some(DiscordPermission::SendMessages)
            }
            Self::SendTtsMessage => Some(DiscordPermission::SendTtsMessages),
            Self::ApplyModeratedForumTag
            | Self::ChangeThreadLock
            | Self::PinForumPost
            | Self::DeleteThread => Some(DiscordPermission::ManageThreads),
            Self::ArchiveThread | Self::EditThread => Some(DiscordPermission::EditOwnThread),
            Self::RunApplicationCommand => Some(DiscordPermission::UseApplicationCommands),
            Self::RemoveReaction => Some(DiscordPermission::ViewChannel),
            Self::PinMessage => Some(DiscordPermission::PinMessages),
            Self::VotePoll | Self::ReadMessageHistory => {
                Some(DiscordPermission::ReadMessageHistory)
            }
            Self::ReopenThread => Some(DiscordPermission::ReopenThread),
            Self::JoinVoiceChannel => Some(DiscordPermission::Connect),
            Self::RemoveMessageEmbeds
            | Self::ChangeThreadMembership
            | Self::EditMessage
            | Self::DeleteMessage
            | Self::AddReaction
            | Self::TransmitMicrophone => None,
        }
    }
}

impl fmt::Display for DiscordAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SendMessage => "send a message",
            Self::CreateForumPost => "create a forum post",
            Self::SendTtsMessage => "send a text-to-speech message",
            Self::ShowTypingIndicator => "show a typing indicator",
            Self::ApplyModeratedForumTag => "apply moderated forum tags",
            Self::RemoveMessageEmbeds => "remove message embeds",
            Self::RunApplicationCommand => "run application commands",
            Self::RemoveReaction => "remove a reaction",
            Self::PinMessage => "pin or unpin messages",
            Self::VotePoll => "vote in a poll",
            Self::ArchiveThread => "archive this thread",
            Self::ChangeThreadLock => "change this thread's lock state",
            Self::PinForumPost => "change this forum post's pin state",
            Self::DeleteThread => "delete this thread",
            Self::EditThread => "edit this thread",
            Self::ChangeThreadMembership => "change thread membership",
            Self::ReopenThread => "reopen this thread",
            Self::ReadMessageHistory => "read messages in this channel",
            Self::EditMessage => "edit this message",
            Self::DeleteMessage => "delete this message",
            Self::AddReaction => "add a reaction",
            Self::JoinVoiceChannel => "join this voice channel",
            Self::TransmitMicrophone => "transmit microphone audio",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ActionBlockReason {
    ChannelDataUnavailable,
    ThreadStateUnavailable,
    ThreadArchived,
    PermissionDenied(DiscordPermission),
    PermissionDataUnavailable(PermissionDataGap),
    ParticipationRestricted(GuildParticipationRestriction),
    ParticipationDataUnavailable(GuildParticipationDataGap),
}

impl fmt::Display for ActionBlockReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChannelDataUnavailable => {
                formatter.write_str("the channel is not loaded, so permissions cannot be verified")
            }
            Self::ThreadStateUnavailable => formatter.write_str("the thread state is not loaded"),
            Self::ThreadArchived => formatter.write_str("the thread is archived"),
            Self::PermissionDenied(permission) => {
                write!(
                    formatter,
                    "Discord permission denied: {permission} is required"
                )
            }
            Self::PermissionDataUnavailable(gap) => {
                write!(formatter, "permissions cannot be verified because {gap}")
            }
            Self::ParticipationRestricted(restriction) => restriction.fmt(formatter),
            Self::ParticipationDataUnavailable(gap) => {
                write!(
                    formatter,
                    "server participation cannot be verified because {gap}"
                )
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ActionDecision {
    Allowed,
    Blocked(ActionBlockReason),
}

impl ActionDecision {
    pub(crate) const fn optimistic_ui_block_reason(self) -> Option<ActionBlockReason> {
        match self {
            Self::Allowed | Self::Blocked(ActionBlockReason::PermissionDataUnavailable(_)) => None,
            Self::Blocked(reason) => Some(reason),
        }
    }
}

impl DiscordState {
    /// Evaluate the state-only part of a Discord action policy.
    ///
    /// Dynamic rules such as message authorship and existing reactions remain
    /// with their focused validators. Both the TUI and the request boundary can
    /// reuse this base decision without removing either check.
    pub(crate) fn channel_action_decision(
        &self,
        channel: &ChannelState,
        action: DiscordAction,
    ) -> ActionDecision {
        if let Some(reason) = self.thread_action_block_reason(channel, action) {
            return ActionDecision::Blocked(reason);
        }
        if action.requires_guild_participation() {
            match self.guild_participation_decision(channel) {
                GuildParticipationDecision::Allowed => {}
                GuildParticipationDecision::Blocked(restriction) => {
                    return ActionDecision::Blocked(ActionBlockReason::ParticipationRestricted(
                        restriction,
                    ));
                }
                GuildParticipationDecision::Unavailable(gap) => {
                    return ActionDecision::Blocked(
                        ActionBlockReason::ParticipationDataUnavailable(gap),
                    );
                }
            }
        }

        let Some(permission) = action.base_permission() else {
            return ActionDecision::Allowed;
        };
        match self.channel_permission_decision(channel, permission) {
            PermissionDecision::Allowed => ActionDecision::Allowed,
            PermissionDecision::Denied(permission) => {
                ActionDecision::Blocked(ActionBlockReason::PermissionDenied(permission))
            }
            PermissionDecision::Unavailable(gap) => {
                ActionDecision::Blocked(ActionBlockReason::PermissionDataUnavailable(gap))
            }
        }
    }

    fn thread_action_block_reason(
        &self,
        channel: &ChannelState,
        action: DiscordAction,
    ) -> Option<ActionBlockReason> {
        if !channel.is_thread() || !thread_state_is_relevant(action) {
            return None;
        }
        let Some(metadata) = channel.thread_metadata.as_ref() else {
            return Some(ActionBlockReason::ThreadStateUnavailable);
        };

        if metadata.archived {
            if action_requires_active_thread(action) {
                return Some(ActionBlockReason::ThreadArchived);
            }
            if metadata.locked && action_automatically_unarchives_thread(action) {
                return match self
                    .channel_permission_decision(channel, DiscordPermission::ReopenThread)
                {
                    PermissionDecision::Allowed => None,
                    PermissionDecision::Denied(permission) => {
                        Some(ActionBlockReason::PermissionDenied(permission))
                    }
                    PermissionDecision::Unavailable(gap) => {
                        Some(ActionBlockReason::PermissionDataUnavailable(gap))
                    }
                };
            }
        }
        None
    }
}

const fn thread_state_is_relevant(action: DiscordAction) -> bool {
    matches!(
        action,
        DiscordAction::SendMessage
            | DiscordAction::SendTtsMessage
            | DiscordAction::ShowTypingIndicator
            | DiscordAction::RemoveMessageEmbeds
            | DiscordAction::RunApplicationCommand
            | DiscordAction::RemoveReaction
            | DiscordAction::PinMessage
            | DiscordAction::VotePoll
            | DiscordAction::ChangeThreadLock
            | DiscordAction::PinForumPost
            | DiscordAction::ChangeThreadMembership
            | DiscordAction::EditThread
            | DiscordAction::ReopenThread
            | DiscordAction::EditMessage
            | DiscordAction::AddReaction
    )
}

const fn action_requires_active_thread(action: DiscordAction) -> bool {
    matches!(
        action,
        DiscordAction::ShowTypingIndicator
            | DiscordAction::RemoveMessageEmbeds
            | DiscordAction::RunApplicationCommand
            | DiscordAction::RemoveReaction
            | DiscordAction::PinMessage
            | DiscordAction::VotePoll
            | DiscordAction::ChangeThreadLock
            | DiscordAction::PinForumPost
            | DiscordAction::ChangeThreadMembership
            | DiscordAction::EditThread
            | DiscordAction::EditMessage
            | DiscordAction::AddReaction
    )
}

const fn action_automatically_unarchives_thread(action: DiscordAction) -> bool {
    matches!(
        action,
        DiscordAction::SendMessage | DiscordAction::SendTtsMessage
    )
}
