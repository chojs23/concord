use crate::discord::PermissionOverwriteKind;
use crate::discord::ids::{
    Id,
    marker::{GuildMarker, RoleMarker},
};

use super::{ChannelState, DiscordState};

/// Discord permission bits we currently care about. Mirrors a subset of
/// Discord's permission bits, kept inline so the state crate
/// does not need to depend on twilight's bitflags.
const PERMISSION_VIEW_CHANNEL: u64 = 0x0000_0000_0000_0400;
const PERMISSION_SEND_MESSAGES: u64 = 0x0000_0000_0000_0800;
const PERMISSION_MANAGE_MESSAGES: u64 = 0x0000_0000_0000_2000;
const PERMISSION_ATTACH_FILES: u64 = 0x0000_0000_0000_8000;
const PERMISSION_READ_MESSAGE_HISTORY: u64 = 0x0000_0000_0001_0000;
const PERMISSION_ADMINISTRATOR: u64 = 0x0000_0000_0000_0008;
const PERMISSION_ADD_REACTIONS: u64 = 0x0000_0000_0000_0040;
const PERMISSION_PIN_MESSAGES: u64 = 0x0008_0000_0000_0000;

/// Sentinel returned by `effective_permissions_for_channel` when the data
/// needed to compute the user's permissions is missing (no READY yet, no
/// guild cached, no role cache, no membership entry, etc.). Callers that
/// translate this into a boolean should default to "permissive" so the UI is
/// not silently disabled while we are still hydrating state.
const PERMISSIONS_UNKNOWN: u64 = u64::MAX;

impl DiscordState {
    /// Whether the authenticated user has `VIEW_CHANNEL` for `channel`.
    /// Thin wrapper over `effective_permissions_for_channel`. See that
    /// function for the algorithm.
    pub fn can_view_channel(&self, channel: &ChannelState) -> bool {
        permission_set(
            self.effective_permissions_for_channel(channel),
            PERMISSION_VIEW_CHANNEL,
        )
    }

    /// Whether the user can post messages in `channel`. Returns `true` for
    /// DMs (no guild-style perms apply) and when the underlying permission
    /// computation is "unknown" (state still hydrating).
    pub fn can_send_in_channel(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        permission_set(permissions, PERMISSION_VIEW_CHANNEL)
            && permission_set(permissions, PERMISSION_SEND_MESSAGES)
    }

    /// Whether the user can upload attachments in `channel`. Same fall-back
    /// behavior as `can_send_in_channel`.
    pub fn can_attach_in_channel(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        permission_set(permissions, PERMISSION_VIEW_CHANNEL)
            && permission_set(permissions, PERMISSION_SEND_MESSAGES)
            && permission_set(permissions, PERMISSION_ATTACH_FILES)
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
            return self.guild_roles_are_hydrated_but_current_member_is_pending(channel);
        }
        permission_set(permissions, PERMISSION_VIEW_CHANNEL)
            && permission_set(permissions, PERMISSION_MANAGE_MESSAGES)
    }

    /// Whether the user can pin or unpin messages in `channel`.
    pub fn can_pin_messages_in_channel(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        permission_set(permissions, PERMISSION_VIEW_CHANNEL)
            && permission_set(permissions, PERMISSION_PIN_MESSAGES)
    }

    /// Whether the user can read already-sent messages in `channel`.
    pub fn can_read_message_history_in_channel(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        permission_set(permissions, PERMISSION_VIEW_CHANNEL)
            && permission_set(permissions, PERMISSION_READ_MESSAGE_HISTORY)
    }

    /// Whether the user can create a new emoji reaction in `channel`.
    /// Reacting with an emoji that is already present only needs message
    /// history, so callers should combine this with message-local reaction
    /// state.
    pub fn can_add_reactions_in_channel(&self, channel: &ChannelState) -> bool {
        let permissions = self.effective_permissions_for_channel(channel);
        permission_set(permissions, PERMISSION_VIEW_CHANNEL)
            && permission_set(permissions, PERMISSION_READ_MESSAGE_HISTORY)
            && permission_set(permissions, PERMISSION_ADD_REACTIONS)
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
            return u64::MAX;
        };
        if channel.is_private_thread() {
            return self.private_thread_permissions_for_channel(guild_id);
        }
        if channel.is_thread() {
            let Some(parent_id) = channel.parent_id else {
                return PERMISSIONS_UNKNOWN;
            };
            let Some(parent) = self.navigation.channels.get(&parent_id) else {
                return PERMISSIONS_UNKNOWN;
            };
            return self.effective_permissions_for_channel(parent);
        }

        let Some(my_id) = self.session.current_user_id else {
            return PERMISSIONS_UNKNOWN;
        };
        let Some(guild) = self.navigation.guilds.get(&guild_id) else {
            return PERMISSIONS_UNKNOWN;
        };
        if guild.owner_id == Some(my_id) {
            return u64::MAX;
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
            return u64::MAX;
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

        perms
    }

    fn private_thread_permissions_for_channel(&self, guild_id: Id<GuildMarker>) -> u64 {
        let Some(my_id) = self.session.current_user_id else {
            return 0;
        };
        let Some(guild) = self.navigation.guilds.get(&guild_id) else {
            return 0;
        };
        if guild.owner_id == Some(my_id) {
            return u64::MAX;
        }
        let Some(roles) = self.guild_details.roles.get(&guild_id) else {
            return 0;
        };
        let Some(member_role_ids) = self.current_user_role_ids_for_guild(guild_id) else {
            return 0;
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
            return u64::MAX;
        }

        0
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
}

/// Whether a Discord channel kind string represents a thread. Mirrors
/// `ChannelState::is_thread` so that bare `ChannelInfo` inputs can be
/// classified before they become a `ChannelState`.
pub(super) fn is_thread_kind(kind: &str) -> bool {
    matches!(
        kind,
        "thread"
            | "GuildPublicThread"
            | "GuildPrivateThread"
            | "GuildNewsThread"
            | "private-thread"
    )
}

/// Test whether a permission bit is set in `bitfield`. Encapsulated so the
/// permission-check call sites stay readable.
fn permission_set(bitfield: u64, bit: u64) -> bool {
    bitfield & bit == bit
}
