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

/// Sentinel returned by `effective_permissions_for_channel` when the data
/// needed to compute the user's permissions is missing (no READY yet, no
/// guild cached, no role cache, no membership entry, etc.). Callers that
/// translate this into a boolean may stay permissive for UI rendering while
/// state hydrates. Request-boundary checks must reject this sentinel before
/// sending a mutation to Discord.
///
/// This must stay distinct from `PERMISSIONS_ALL`, because guild owners and
/// ADMINISTRATOR holders have real full permissions.
const PERMISSIONS_UNKNOWN: u64 = u64::MAX - 1;
const PERMISSIONS_ALL: u64 = u64::MAX;

impl DiscordState {
    pub fn can_view_channel(&self, channel: &ChannelState) -> bool {
        permission_set(
            self.effective_permissions_for_channel(channel),
            PERMISSION_VIEW_CHANNEL,
        )
    }

    /// Whether the user can post messages in `channel`. Returns `true` for
    /// DMs and while guild permission state is still hydrating. Request code
    /// must pair this with `channel_permissions_are_known`.
    pub fn can_send_in_channel(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        can_access_channel_messages(channel, permissions)
            && permission_set(permissions, send_message_permission(channel))
    }

    pub fn can_send_tts_in_channel(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        can_access_channel_messages(channel, permissions)
            && permission_set(permissions, send_message_permission(channel))
            && permission_set(permissions, PERMISSION_SEND_TTS_MESSAGES)
    }

    pub fn can_attach_in_channel(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        can_access_channel_messages(channel, permissions)
            && permission_set(permissions, send_message_permission(channel))
            && permission_set(permissions, PERMISSION_ATTACH_FILES)
    }

    /// Whether the current user has Discord's BYPASS_SLOWMODE permission.
    pub fn bypasses_slow_mode(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        permissions == PERMISSIONS_ALL || permission_set(permissions, PERMISSION_BYPASS_SLOWMODE)
    }

    pub(crate) fn channel_permissions_are_known(&self, channel: &ChannelState) -> bool {
        self.effective_permissions_for_channel(channel) != PERMISSIONS_UNKNOWN
    }

    pub(crate) fn has_full_channel_permissions(&self, channel: &ChannelState) -> bool {
        self.effective_permissions_for_channel(channel) == PERMISSIONS_ALL
    }

    /// Whether the user can delete other users' messages in `channel`.
    /// Deleting your own messages is author-based and should be checked by the
    /// caller before consulting this moderation permission.
    pub fn can_manage_messages_in_channel(&self, channel: &ChannelState) -> bool {
        if channel.guild_id.is_none() {
            return false;
        }
        let permissions = self.effective_permissions_for_channel(channel);
        if permissions == PERMISSIONS_UNKNOWN {
            return self.guild_roles_are_hydrated_but_current_member_is_pending(channel)
                && self.has_required_mfa(channel);
        }
        can_access_channel_messages(channel, permissions)
            && permission_set(permissions, PERMISSION_MANAGE_MESSAGES)
            && self.has_required_mfa(channel)
    }

    pub fn can_pin_messages_in_channel(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        can_access_channel_messages(channel, permissions)
            && permission_set(permissions, PERMISSION_PIN_MESSAGES)
    }

    pub fn can_read_message_history_in_channel(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        can_access_channel_messages(channel, permissions)
            && permission_set(permissions, PERMISSION_READ_MESSAGE_HISTORY)
    }

    /// Whether the user can create a new emoji reaction in `channel`.
    /// Reacting with an emoji that is already present only needs message
    /// history, so callers should combine this with message-local reaction
    /// state.
    pub fn can_add_reactions_in_channel(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        can_access_channel_messages(channel, permissions)
            && permission_set(permissions, PERMISSION_READ_MESSAGE_HISTORY)
            && permission_set(permissions, PERMISSION_ADD_REACTIONS)
    }

    pub fn can_use_external_emojis_in_channel(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        can_access_channel_messages(channel, permissions)
            && permission_set(permissions, PERMISSION_USE_EXTERNAL_EMOJIS)
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
        let ReactionEmoji::Custom { id, .. } = emoji else {
            return true;
        };
        let Some(guild_id) = channel.guild_id else {
            return true;
        };
        self.custom_emojis_for_guild(guild_id)
            .iter()
            .any(|candidate| candidate.id == *id)
            || self.can_use_external_emojis_in_channel(channel)
    }

    pub fn can_use_application_commands_in_channel(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        can_access_channel_messages(channel, permissions)
            && permission_set(permissions, PERMISSION_USE_APPLICATION_COMMANDS)
    }

    /// Whether the user can connect to a guild voice channel. Unknown
    /// permissions stay optimistic while state hydrates, but an explicit
    /// missing `CONNECT` bit disables the join affordance.
    pub fn can_connect_voice_channel(&self, channel: &ChannelState) -> bool {
        if !channel.is_voice() {
            return false;
        }
        let permissions = self.effective_permissions_for_channel(channel);
        permission_set(permissions, PERMISSION_VIEW_CHANNEL)
            && permission_set(permissions, PERMISSION_CONNECT)
    }

    pub fn can_speak_in_voice_channel(&self, channel: &ChannelState) -> bool {
        if !channel.is_voice() {
            return false;
        }
        let permissions = self.effective_permissions_for_channel(channel);
        permission_set(permissions, PERMISSION_VIEW_CHANNEL)
            && permission_set(permissions, PERMISSION_CONNECT)
            && permission_set(permissions, PERMISSION_SPEAK)
    }

    pub fn can_use_voice_activity_in_channel(&self, channel: &ChannelState) -> bool {
        if !channel.is_voice() {
            return false;
        }
        let permissions = self.effective_permissions_for_channel(channel);
        permission_set(permissions, PERMISSION_VIEW_CHANNEL)
            && permission_set(permissions, PERMISSION_CONNECT)
            && permission_set(permissions, PERMISSION_USE_VOICE_ACTIVITY)
    }

    pub fn can_transmit_microphone_in_voice_channel(&self, channel: &ChannelState) -> bool {
        self.can_speak_in_voice_channel(channel) && self.can_use_voice_activity_in_channel(channel)
    }

    pub fn can_manage_threads_in_channel(&self, channel: &ChannelState) -> bool {
        if channel.guild_id.is_none() {
            return false;
        }
        let permissions = self.effective_permissions_for_channel(channel);
        permissions != PERMISSIONS_UNKNOWN
            && permission_set(permissions, PERMISSION_VIEW_CHANNEL)
            && permission_set(permissions, PERMISSION_MANAGE_THREADS)
            && self.has_required_mfa(channel)
    }

    /// Discord allows an unlocked thread to be reopened with `SEND_MESSAGES`.
    /// Every other thread mutation requires `MANAGE_THREADS`.
    pub fn can_reopen_thread(&self, channel: &ChannelState) -> bool {
        if !channel.is_thread() || channel.thread_locked().unwrap_or(false) {
            return self.can_manage_threads_in_channel(channel);
        }
        let permissions = self.effective_permissions_for_channel(channel);
        permissions != PERMISSIONS_UNKNOWN
            && permission_set(permissions, PERMISSION_VIEW_CHANNEL)
            && (permission_set(permissions, PERMISSION_MANAGE_THREADS)
                || permission_set(permissions, PERMISSION_SEND_MESSAGES))
    }

    /// Whether the user can manage guild/channel structure around `channel`.
    /// Empty categories are only useful to users who can configure the server
    /// or channel tree, so this check is intentionally pessimistic while
    /// permission state is still hydrating.
    pub fn can_manage_channel_structure_in_channel(&self, channel: &ChannelState) -> bool {
        if channel.guild_id.is_none() {
            return false;
        }
        let permissions = self.effective_permissions_for_channel(channel);
        if permissions == PERMISSIONS_UNKNOWN {
            return false;
        }
        permission_set(permissions, PERMISSION_MANAGE_CHANNELS)
            || permission_set(permissions, PERMISSION_MANAGE_GUILD)
    }

    /// Compute the effective Discord permission bitfield for the
    /// authenticated user in `channel`.
    ///
    /// 1. DMs and group DMs grant every permission because Discord does not apply
    ///    guild-style overwrites to them.
    /// 2. Threads inherit from their parent. A missing parent returns
    ///    `PERMISSIONS_UNKNOWN` so callers default to "permissive".
    /// 3. Owners and ADMINISTRATOR holders get the full bitfield.
    /// 4. Otherwise: base permissions ← OR of `@everyone` and every role the
    ///    member holds, then `@everyone` overwrite, then accumulated role
    ///    overwrites (deny then allow), then member overwrite (deny then
    ///    allow).
    ///
    /// When required data is missing the function returns
    /// `PERMISSIONS_UNKNOWN` so callers can choose whether to render the
    /// affordance optimistically (composer enabled) or pessimistically.
    fn effective_permissions_for_channel(&self, channel: &ChannelState) -> u64 {
        let Some(guild_id) = channel.guild_id else {
            return PERMISSIONS_ALL;
        };
        if channel.is_thread() {
            let Some(parent_id) = channel.parent_id else {
                return if channel.is_private_thread() && !channel.current_user_joined_thread {
                    0
                } else {
                    PERMISSIONS_UNKNOWN
                };
            };
            let Some(parent) = self.navigation.channels.get(&parent_id) else {
                return if channel.is_private_thread() && !channel.current_user_joined_thread {
                    0
                } else {
                    PERMISSIONS_UNKNOWN
                };
            };
            let parent_permissions = self.effective_permissions_for_channel(parent);
            if channel.is_private_thread()
                && !channel.current_user_joined_thread
                && (parent_permissions == PERMISSIONS_UNKNOWN
                    || !permission_set(parent_permissions, PERMISSION_MANAGE_THREADS))
            {
                return 0;
            }
            return parent_permissions;
        }

        let Some(my_id) = self.session.current_user_id else {
            return PERMISSIONS_UNKNOWN;
        };
        let Some(guild) = self.navigation.guilds.get(&guild_id) else {
            return PERMISSIONS_UNKNOWN;
        };
        if guild.owner_id == Some(my_id) {
            return PERMISSIONS_ALL;
        }
        let Some(roles) = self.guild_details.roles.get(&guild_id) else {
            return PERMISSIONS_UNKNOWN;
        };
        let Some(member_role_ids) = self.current_user_role_ids_for_guild(guild_id) else {
            return PERMISSIONS_UNKNOWN;
        };

        let everyone_role_id: Id<RoleMarker> = Id::new(guild_id.get());
        let mut base_permissions: u64 = roles
            .get(&everyone_role_id)
            .map(|role| role.permissions)
            .unwrap_or(0);
        for role_id in member_role_ids {
            if let Some(role) = roles.get(role_id) {
                base_permissions |= role.permissions;
            }
        }
        if base_permissions & PERMISSION_ADMINISTRATOR == PERMISSION_ADMINISTRATOR {
            return PERMISSIONS_ALL;
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

        perms
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

    fn has_required_mfa(&self, channel: &ChannelState) -> bool {
        let Some(guild_id) = channel.guild_id else {
            return true;
        };
        self.guild(guild_id).is_some_and(|guild| {
            guild.mfa_level == 0 || self.session.current_user_mfa_enabled == Some(true)
        })
    }
}

fn permission_set(bitfield: u64, bit: u64) -> bool {
    bitfield & bit == bit
}

fn send_message_permission(channel: &ChannelState) -> u64 {
    if channel.is_thread() {
        PERMISSION_SEND_MESSAGES_IN_THREADS
    } else {
        PERMISSION_SEND_MESSAGES
    }
}

fn can_access_channel_messages(channel: &ChannelState, permissions: u64) -> bool {
    permission_set(permissions, PERMISSION_VIEW_CHANNEL)
        && (!channel.is_voice() || permission_set(permissions, PERMISSION_CONNECT))
}
