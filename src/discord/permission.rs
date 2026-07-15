use std::fmt;

use chrono::Utc;

use crate::discord::ids::{Id, marker::RoleMarker};
use crate::discord::{PermissionOverwriteKind, ReactionEmoji};

use crate::discord::state::{ChannelState, DiscordState};

/// Discord permission bits we currently care about. Mirrors a subset of
/// Discord's permission bits, kept inline so the state crate
/// does not need to depend on twilight's bitflags.
const PERMISSION_VIEW_CHANNEL: u64 = 0x0000_0000_0000_0400;
const PERMISSION_MANAGE_CHANNELS: u64 = 0x0000_0000_0000_0010;
const PERMISSION_MANAGE_GUILD: u64 = 0x0000_0000_0000_0020;
const PERMISSION_SEND_MESSAGES: u64 = 0x0000_0000_0000_0800;
const PERMISSION_SEND_TTS_MESSAGES: u64 = 0x0000_0000_0000_1000;
const PERMISSION_MANAGE_MESSAGES: u64 = 0x0000_0000_0000_2000;
const PERMISSION_ATTACH_FILES: u64 = 0x0000_0000_0000_8000;
const PERMISSION_READ_MESSAGE_HISTORY: u64 = 0x0000_0000_0001_0000;
const PERMISSION_CONNECT: u64 = 0x0000_0000_0010_0000;
const PERMISSION_SPEAK: u64 = 0x0000_0000_0020_0000;
const PERMISSION_USE_VOICE_ACTIVITY: u64 = 0x0000_0000_0200_0000;
const PERMISSION_ADMINISTRATOR: u64 = 0x0000_0000_0000_0008;
const PERMISSION_ADD_REACTIONS: u64 = 0x0000_0000_0000_0040;
const PERMISSION_USE_EXTERNAL_EMOJIS: u64 = 0x0000_0000_0004_0000;
const PERMISSION_USE_APPLICATION_COMMANDS: u64 = 0x0000_0000_8000_0000;
const PERMISSION_MANAGE_THREADS: u64 = 0x0000_0004_0000_0000;
const PERMISSION_SEND_MESSAGES_IN_THREADS: u64 = 0x0000_0040_0000_0000;
const PERMISSION_PIN_MESSAGES: u64 = 0x0008_0000_0000_0000;
const PERMISSION_BYPASS_SLOWMODE: u64 = 0x0010_0000_0000_0000;

const PERMISSIONS_ALL: u64 = u64::MAX;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DiscordPermission {
    ViewChannel,
    SendMessages,
    SendTtsMessages,
    AttachFiles,
    ManageMessages,
    PinMessages,
    ReadMessageHistory,
    AddReactions,
    UseExternalEmojis,
    UseApplicationCommands,
    Connect,
    Speak,
    UseVoiceActivity,
    ManageThreads,
    EditOwnThread,
    ReopenThread,
    ManageChannelStructure,
}

impl fmt::Display for DiscordPermission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ViewChannel => "View Channel",
            Self::SendMessages => "Send Messages",
            Self::SendTtsMessages => "Send TTS Messages",
            Self::AttachFiles => "Attach Files",
            Self::ManageMessages => "Manage Messages",
            Self::PinMessages => "Pin Messages",
            Self::ReadMessageHistory => "Read Message History",
            Self::AddReactions => "Add Reactions",
            Self::UseExternalEmojis => "Use External Emojis",
            Self::UseApplicationCommands => "Use Application Commands",
            Self::Connect => "Connect",
            Self::Speak => "Speak",
            Self::UseVoiceActivity => "Use Voice Activity",
            Self::ManageThreads => "Manage Threads",
            Self::EditOwnThread => "Thread Creator or Manage Threads",
            Self::ReopenThread => "Send Messages, Thread Creator, or Manage Threads",
            Self::ManageChannelStructure => "Manage Channels or Manage Server",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PermissionDataGap {
    CurrentUser,
    CurrentUserMfa,
    Guild,
    GuildMfaLevel,
    GuildRoles,
    CurrentMember,
    ThreadParent,
}

impl fmt::Display for PermissionDataGap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CurrentUser => "the current user is not loaded",
            Self::CurrentUserMfa => "the current user's MFA status is not loaded",
            Self::Guild => "the server is not loaded",
            Self::GuildMfaLevel => "the server MFA requirement is not loaded",
            Self::GuildRoles => "the server roles are not loaded",
            Self::CurrentMember => "the current server member is not loaded",
            Self::ThreadParent => "the thread parent is not loaded",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ChannelPermissions {
    bits: u64,
}

impl ChannelPermissions {
    const ALL: Self = Self {
        bits: PERMISSIONS_ALL,
    };

    const NONE: Self = Self { bits: 0 };

    const fn new(bits: u64) -> Self {
        Self { bits }
    }

    const fn contains(self, permission: u64) -> bool {
        self.bits & permission == permission
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PermissionResolution {
    Known(ChannelPermissions),
    Unavailable(PermissionDataGap),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PermissionDecision {
    Allowed,
    Denied(DiscordPermission),
    Unavailable(PermissionDataGap),
}

impl PermissionDecision {
    pub const fn is_allowed(self) -> bool {
        matches!(self, Self::Allowed)
    }

    pub const fn allows_optimistic_ui(self) -> bool {
        !matches!(self, Self::Denied(_))
    }
}

impl DiscordState {
    pub fn can_view_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::ViewChannel)
            .allows_optimistic_ui()
    }

    /// Whether the user can post messages in `channel`. The UI stays optimistic
    /// while guild permission state hydrates. Request code must use
    /// `channel_permission_decision` and reject unavailable data.
    pub fn can_send_in_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::SendMessages)
            .allows_optimistic_ui()
    }

    pub fn can_send_tts_in_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::SendTtsMessages)
            .allows_optimistic_ui()
    }

    pub fn can_attach_in_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::AttachFiles)
            .allows_optimistic_ui()
    }

    /// Whether the current user has Discord's BYPASS_SLOWMODE permission.
    pub fn bypasses_slow_mode(&self, channel: &ChannelState) -> bool {
        match self.effective_permissions_for_channel(channel) {
            PermissionResolution::Known(permissions) => {
                permissions.bits == PERMISSIONS_ALL
                    || permissions.contains(PERMISSION_BYPASS_SLOWMODE)
            }
            PermissionResolution::Unavailable(_) => true,
        }
    }

    pub(crate) fn has_full_channel_permissions(&self, channel: &ChannelState) -> bool {
        matches!(
            self.effective_permissions_for_channel(channel),
            PermissionResolution::Known(ChannelPermissions {
                bits: PERMISSIONS_ALL
            })
        )
    }

    /// Whether the user can delete other users' messages in `channel`.
    /// Deleting your own messages is author-based and should be checked by the
    /// caller before consulting this moderation permission.
    pub fn can_manage_messages_in_channel(&self, channel: &ChannelState) -> bool {
        match self.channel_permission_decision(channel, DiscordPermission::ManageMessages) {
            PermissionDecision::Allowed => true,
            PermissionDecision::Denied(_) => false,
            PermissionDecision::Unavailable(_) => {
                self.guild_roles_are_hydrated_but_current_member_is_pending(channel)
                    && matches!(
                        self.permission_mfa_decision(channel, DiscordPermission::ManageMessages),
                        PermissionDecision::Allowed
                    )
            }
        }
    }

    pub fn can_pin_messages_in_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::PinMessages)
            .allows_optimistic_ui()
    }

    pub fn can_read_message_history_in_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::ReadMessageHistory)
            .allows_optimistic_ui()
    }

    /// Whether the user can create a new emoji reaction in `channel`.
    /// Reacting with an emoji that is already present only needs message
    /// history, so callers should combine this with message-local reaction
    /// state.
    pub fn can_add_reactions_in_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::AddReactions)
            .allows_optimistic_ui()
    }

    pub fn can_use_external_emojis_in_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::UseExternalEmojis)
            .allows_optimistic_ui()
    }

    /// Whether `emoji` can be used in this channel without Discord rejecting
    /// a cross-server custom emoji. DMs have no guild permission overwrite,
    /// while custom emoji from the channel's own guild do not need the external
    /// emoji permission.
    pub fn can_use_reaction_emoji_in_channel(
        &self,
        channel: &ChannelState,
        emoji: &ReactionEmoji,
    ) -> bool {
        !self.reaction_emoji_requires_external_permission(channel, emoji)
            || self.can_use_external_emojis_in_channel(channel)
    }

    pub(crate) fn reaction_emoji_requires_external_permission(
        &self,
        channel: &ChannelState,
        emoji: &ReactionEmoji,
    ) -> bool {
        let ReactionEmoji::Custom { id, .. } = emoji else {
            return false;
        };
        let Some(guild_id) = channel.guild_id else {
            return false;
        };
        !self
            .custom_emojis_for_guild(guild_id)
            .iter()
            .any(|candidate| candidate.id == *id)
    }

    pub fn can_use_application_commands_in_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::UseApplicationCommands)
            .allows_optimistic_ui()
    }

    /// Whether the user can connect to a guild voice channel. Unknown
    /// permissions stay optimistic while state hydrates, but an explicit
    /// missing `CONNECT` bit disables the join affordance.
    pub fn can_connect_voice_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::Connect)
            .allows_optimistic_ui()
    }

    pub fn can_speak_in_voice_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::Speak)
            .allows_optimistic_ui()
    }

    pub fn can_use_voice_activity_in_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::UseVoiceActivity)
            .allows_optimistic_ui()
    }

    pub fn can_transmit_microphone_in_voice_channel(&self, channel: &ChannelState) -> bool {
        self.can_speak_in_voice_channel(channel) && self.can_use_voice_activity_in_channel(channel)
    }

    pub fn can_manage_threads_in_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::ManageThreads)
            .is_allowed()
    }

    /// Discord allows an unlocked thread to be reopened with `SEND_MESSAGES`.
    /// Reopening a locked thread requires its creator or `MANAGE_THREADS`.
    pub fn can_reopen_thread(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::ReopenThread)
            .is_allowed()
    }

    /// Whether the user can manage guild/channel structure around `channel`.
    /// Empty categories are only useful to users who can configure the server
    /// or channel tree, so this check is intentionally pessimistic while
    /// permission state is still hydrating.
    pub fn can_manage_channel_structure_in_channel(&self, channel: &ChannelState) -> bool {
        self.channel_permission_decision(channel, DiscordPermission::ManageChannelStructure)
            .is_allowed()
    }

    pub(crate) fn channel_permission_decision(
        &self,
        channel: &ChannelState,
        permission: DiscordPermission,
    ) -> PermissionDecision {
        let current_user_is_thread_creator = self
            .session
            .current_user_id
            .is_some_and(|user_id| channel.owner_id == Some(user_id));
        if channel.is_thread()
            && current_user_is_thread_creator
            && (matches!(permission, DiscordPermission::EditOwnThread)
                || (matches!(permission, DiscordPermission::ReopenThread)
                    && channel.thread_locked() == Some(true)))
        {
            return PermissionDecision::Allowed;
        }

        let permissions = match self.effective_permissions_for_channel(channel) {
            PermissionResolution::Known(permissions) => permissions,
            PermissionResolution::Unavailable(gap) => {
                return PermissionDecision::Unavailable(gap);
            }
        };

        if !self.permission_is_allowed(channel, permissions, permission) {
            return PermissionDecision::Denied(permission);
        }
        self.permission_mfa_decision(channel, permission)
    }

    fn permission_is_allowed(
        &self,
        channel: &ChannelState,
        permissions: ChannelPermissions,
        permission: DiscordPermission,
    ) -> bool {
        let can_access_messages = can_access_channel_messages(channel, permissions);
        match permission {
            DiscordPermission::ViewChannel => permissions.contains(PERMISSION_VIEW_CHANNEL),
            DiscordPermission::SendMessages => {
                can_access_messages && permissions.contains(send_message_permission(channel))
            }
            DiscordPermission::SendTtsMessages => {
                can_access_messages
                    && permissions.contains(send_message_permission(channel))
                    && permissions.contains(PERMISSION_SEND_TTS_MESSAGES)
            }
            DiscordPermission::AttachFiles => {
                can_access_messages
                    && permissions.contains(send_message_permission(channel))
                    && permissions.contains(PERMISSION_ATTACH_FILES)
            }
            DiscordPermission::ManageMessages => {
                channel.guild_id.is_some()
                    && can_access_messages
                    && permissions.contains(PERMISSION_MANAGE_MESSAGES)
            }
            DiscordPermission::PinMessages => {
                can_access_messages && permissions.contains(PERMISSION_PIN_MESSAGES)
            }
            DiscordPermission::ReadMessageHistory => {
                can_access_messages && permissions.contains(PERMISSION_READ_MESSAGE_HISTORY)
            }
            DiscordPermission::AddReactions => {
                can_access_messages
                    && permissions.contains(PERMISSION_READ_MESSAGE_HISTORY)
                    && permissions.contains(PERMISSION_ADD_REACTIONS)
            }
            DiscordPermission::UseExternalEmojis => {
                can_access_messages && permissions.contains(PERMISSION_USE_EXTERNAL_EMOJIS)
            }
            DiscordPermission::UseApplicationCommands => {
                can_access_messages && permissions.contains(PERMISSION_USE_APPLICATION_COMMANDS)
            }
            DiscordPermission::Connect => {
                channel.is_voice()
                    && permissions.contains(PERMISSION_VIEW_CHANNEL)
                    && permissions.contains(PERMISSION_CONNECT)
            }
            DiscordPermission::Speak => {
                channel.is_voice()
                    && permissions.contains(PERMISSION_VIEW_CHANNEL)
                    && permissions.contains(PERMISSION_CONNECT)
                    && permissions.contains(PERMISSION_SPEAK)
            }
            DiscordPermission::UseVoiceActivity => {
                channel.is_voice()
                    && permissions.contains(PERMISSION_VIEW_CHANNEL)
                    && permissions.contains(PERMISSION_CONNECT)
                    && permissions.contains(PERMISSION_USE_VOICE_ACTIVITY)
            }
            DiscordPermission::ManageThreads => {
                channel.guild_id.is_some()
                    && permissions.contains(PERMISSION_VIEW_CHANNEL)
                    && permissions.contains(PERMISSION_MANAGE_THREADS)
            }
            DiscordPermission::EditOwnThread => {
                channel.is_thread()
                    && permissions.contains(PERMISSION_VIEW_CHANNEL)
                    && (channel.owner_id.is_some()
                        && channel.owner_id == self.session.current_user_id
                        || permissions.contains(PERMISSION_MANAGE_THREADS))
            }
            DiscordPermission::ReopenThread => {
                if !channel.is_thread() {
                    return false;
                }
                if channel.thread_locked().unwrap_or(false) {
                    return self.permission_is_allowed(
                        channel,
                        permissions,
                        DiscordPermission::EditOwnThread,
                    );
                }
                permissions.contains(PERMISSION_VIEW_CHANNEL)
                    && (permissions.contains(PERMISSION_MANAGE_THREADS)
                        || permissions.contains(PERMISSION_SEND_MESSAGES))
            }
            DiscordPermission::ManageChannelStructure => {
                channel.guild_id.is_some()
                    && (permissions.contains(PERMISSION_MANAGE_CHANNELS)
                        || permissions.contains(PERMISSION_MANAGE_GUILD))
            }
        }
    }

    /// Compute the effective Discord permission bitfield for the
    /// authenticated user in `channel`.
    ///
    /// 1. DMs and group DMs grant every permission because Discord does not apply
    ///    guild-style overwrites to them.
    /// 2. Threads inherit from their parent. A missing parent returns an
    ///    unavailable resolution so each caller chooses its UI or request policy.
    /// 3. Owners and ADMINISTRATOR holders get the full bitfield.
    /// 4. Otherwise: base permissions ← OR of `@everyone` and every role the
    ///    member holds, then `@everyone` overwrite, then accumulated role
    ///    overwrites (deny then allow), then member overwrite (deny then
    ///    allow).
    ///
    fn effective_permissions_for_channel(&self, channel: &ChannelState) -> PermissionResolution {
        let Some(guild_id) = channel.guild_id else {
            return PermissionResolution::Known(ChannelPermissions::ALL);
        };
        if channel.is_thread() {
            let Some(parent_id) = channel.parent_id else {
                return if channel.is_private_thread() && !channel.current_user_joined_thread {
                    PermissionResolution::Known(ChannelPermissions::NONE)
                } else {
                    PermissionResolution::Unavailable(PermissionDataGap::ThreadParent)
                };
            };
            let Some(parent) = self.navigation.channels.get(&parent_id) else {
                return if channel.is_private_thread() && !channel.current_user_joined_thread {
                    PermissionResolution::Known(ChannelPermissions::NONE)
                } else {
                    PermissionResolution::Unavailable(PermissionDataGap::ThreadParent)
                };
            };
            let parent_permissions = self.effective_permissions_for_channel(parent);
            if channel.is_private_thread()
                && !channel.current_user_joined_thread
                && !matches!(
                    parent_permissions,
                    PermissionResolution::Known(permissions)
                        if permissions.contains(PERMISSION_MANAGE_THREADS)
                )
            {
                return PermissionResolution::Known(ChannelPermissions::NONE);
            }
            return parent_permissions;
        }

        let Some(my_id) = self.session.current_user_id else {
            return PermissionResolution::Unavailable(PermissionDataGap::CurrentUser);
        };
        let Some(guild) = self.navigation.guilds.get(&guild_id) else {
            return PermissionResolution::Unavailable(PermissionDataGap::Guild);
        };
        if guild.owner_id == Some(my_id) {
            return PermissionResolution::Known(ChannelPermissions::ALL);
        }
        let Some(roles) = self.guild_details.roles.get(&guild_id) else {
            return PermissionResolution::Unavailable(PermissionDataGap::GuildRoles);
        };
        let Some(member_role_ids) = self.current_user_role_ids_for_guild(guild_id) else {
            return PermissionResolution::Unavailable(PermissionDataGap::CurrentMember);
        };

        let everyone_role_id: Id<RoleMarker> = Id::new(guild_id.get());
        let Some(everyone_role) = roles.get(&everyone_role_id) else {
            return PermissionResolution::Unavailable(PermissionDataGap::GuildRoles);
        };
        let mut base_permissions = everyone_role.permissions;
        for role_id in member_role_ids {
            if let Some(role) = roles.get(role_id) {
                base_permissions |= role.permissions;
            }
        }
        if base_permissions & PERMISSION_ADMINISTRATOR == PERMISSION_ADMINISTRATOR {
            return PermissionResolution::Known(ChannelPermissions::ALL);
        }

        let overwrites = &channel.permission_overwrites;
        let guild_id_raw = guild_id.get();
        let my_id_raw = my_id.get();

        let mut perms = base_permissions;
        if let Some(overwrite) = overwrites
            .iter()
            .find(|o| matches!(o.kind, PermissionOverwriteKind::Role) && o.id == guild_id_raw)
        {
            perms &= !overwrite.deny;
            perms |= overwrite.allow;
        }

        let mut role_allow: u64 = 0;
        let mut role_deny: u64 = 0;
        let member_role_ids: Vec<u64> = member_role_ids.iter().map(|id| id.get()).collect();
        for overwrite in overwrites {
            if matches!(overwrite.kind, PermissionOverwriteKind::Role)
                && overwrite.id != guild_id_raw
                && member_role_ids.contains(&overwrite.id)
            {
                role_allow |= overwrite.allow;
                role_deny |= overwrite.deny;
            }
        }
        perms &= !role_deny;
        perms |= role_allow;

        if let Some(overwrite) = overwrites
            .iter()
            .find(|o| matches!(o.kind, PermissionOverwriteKind::Member) && o.id == my_id_raw)
        {
            perms &= !overwrite.deny;
            perms |= overwrite.allow;
        }

        let current_user_is_timed_out = self
            .guild_details
            .members
            .get(&guild_id)
            .and_then(|members| members.get(&my_id))
            .and_then(|member| member.communication_disabled_until)
            .is_some_and(|until| until > Utc::now());
        if current_user_is_timed_out {
            perms &= PERMISSION_VIEW_CHANNEL | PERMISSION_READ_MESSAGE_HISTORY;
        }

        PermissionResolution::Known(ChannelPermissions::new(perms))
    }

    fn guild_roles_are_hydrated_but_current_member_is_pending(
        &self,
        channel: &ChannelState,
    ) -> bool {
        let Some(guild_id) = channel.guild_id else {
            return false;
        };
        if channel.is_thread() {
            let Some(parent_id) = channel.parent_id else {
                return false;
            };
            let Some(parent) = self.navigation.channels.get(&parent_id) else {
                return false;
            };
            return self.guild_roles_are_hydrated_but_current_member_is_pending(parent);
        }
        let Some(my_id) = self.session.current_user_id else {
            return false;
        };
        if !self.navigation.guilds.contains_key(&guild_id) {
            return false;
        }
        let Some(roles) = self.guild_details.roles.get(&guild_id) else {
            return false;
        };
        !roles.is_empty()
            && !self
                .guild_details
                .members
                .get(&guild_id)
                .is_some_and(|members| members.contains_key(&my_id))
    }

    fn permission_mfa_decision(
        &self,
        channel: &ChannelState,
        permission: DiscordPermission,
    ) -> PermissionDecision {
        let current_user_is_creator = self
            .session
            .current_user_id
            .is_some_and(|user_id| channel.owner_id == Some(user_id));
        let requires_mfa = matches!(
            permission,
            DiscordPermission::ManageMessages | DiscordPermission::ManageThreads
        ) || (matches!(permission, DiscordPermission::ReopenThread)
            && channel.thread_locked().unwrap_or(false)
            && !current_user_is_creator)
            || (matches!(permission, DiscordPermission::EditOwnThread) && !current_user_is_creator);
        if !requires_mfa {
            return PermissionDecision::Allowed;
        }
        let Some(guild_id) = channel.guild_id else {
            return PermissionDecision::Allowed;
        };
        let Some(guild) = self.guild(guild_id) else {
            return PermissionDecision::Unavailable(PermissionDataGap::Guild);
        };
        let Some(mfa_level) = guild.mfa_level else {
            return PermissionDecision::Unavailable(PermissionDataGap::GuildMfaLevel);
        };
        if mfa_level == 0 {
            return PermissionDecision::Allowed;
        }
        match self.session.current_user_mfa_enabled {
            Some(true) => PermissionDecision::Allowed,
            Some(false) => PermissionDecision::Denied(permission),
            None => PermissionDecision::Unavailable(PermissionDataGap::CurrentUserMfa),
        }
    }
}

fn send_message_permission(channel: &ChannelState) -> u64 {
    if channel.is_thread() {
        PERMISSION_SEND_MESSAGES_IN_THREADS
    } else {
        PERMISSION_SEND_MESSAGES
    }
}

fn can_access_channel_messages(channel: &ChannelState, permissions: ChannelPermissions) -> bool {
    permissions.contains(PERMISSION_VIEW_CHANNEL)
        && (!channel.is_voice() || permissions.contains(PERMISSION_CONNECT))
}
