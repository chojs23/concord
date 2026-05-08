use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

mod permissions;

/// Typing indicators stay visible for this long after the latest TYPING_START
/// from a given user — matches Discord's documented 10-second window so the
/// label tracks what other clients show.
const TYPING_INDICATOR_TTL: Duration = Duration::from_secs(10);

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use super::{
    AppEvent, AttachmentInfo, AttachmentUpdate, ChannelInfo, ChannelRecipientInfo, CustomEmojiInfo,
    EmbedInfo, FriendStatus, GuildFolder, InlinePreviewInfo, MemberInfo, MentionInfo, MessageInfo,
    MessageKind, MessageReferenceInfo, MessageSnapshotInfo, PermissionOverwriteInfo, PollInfo,
    PresenceStatus, ReactionInfo, ReplyInfo, RoleInfo, UserProfileInfo,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ChannelReadState {
    last_acked_message_id: Option<Id<MessageMarker>>,
    mention_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelUnreadState {
    Seen,
    Unread,
    Mentioned(u32),
}

const DEFAULT_MAX_MESSAGES_PER_CHANNEL: usize = 200;
const OLDER_HISTORY_EXTRA_WINDOW_MULTIPLIER: usize = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildState {
    pub id: Id<GuildMarker>,
    pub name: String,
    pub member_count: Option<u64>,
    /// Snowflake of the guild owner. Owners short-circuit permission checks
    /// (they always see every channel). `None` until the GUILD_CREATE /
    /// GUILD_UPDATE payload supplies it.
    pub owner_id: Option<Id<UserMarker>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelState {
    pub id: Id<ChannelMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub parent_id: Option<Id<ChannelMarker>>,
    pub position: Option<i32>,
    pub last_message_id: Option<Id<MessageMarker>>,
    pub name: String,
    pub kind: String,
    pub message_count: Option<u64>,
    pub total_message_sent: Option<u64>,
    pub thread_archived: Option<bool>,
    pub thread_locked: Option<bool>,
    pub thread_pinned: Option<bool>,
    pub recipients: Vec<ChannelRecipientState>,
    /// Channel-level permission overrides used by `can_view_channel`. Threads
    /// inherit from their parent channel, so this stays empty for threads
    /// even after a payload arrives.
    pub permission_overwrites: Vec<PermissionOverwriteInfo>,
}

impl ChannelState {
    pub fn is_category(&self) -> bool {
        matches!(self.kind.as_str(), "category" | "GuildCategory")
    }

    pub fn is_thread(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "thread" | "GuildPublicThread" | "GuildPrivateThread" | "GuildNewsThread"
        )
    }

    pub fn is_forum(&self) -> bool {
        matches!(self.kind.as_str(), "forum" | "GuildForum")
    }

    pub fn is_private_thread(&self) -> bool {
        matches!(self.kind.as_str(), "GuildPrivateThread" | "private-thread")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelRecipientState {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    /// Discord login handle. Mirrors `ChannelRecipientInfo::username`; the
    /// @-mention picker matches against this in addition to `display_name`.
    pub username: Option<String>,
    pub is_bot: bool,
    pub avatar_url: Option<String>,
    pub status: PresenceStatus,
}

impl ChannelRecipientState {
    fn from_info(
        recipient: &ChannelRecipientInfo,
        previous_status: Option<PresenceStatus>,
        known_status: Option<PresenceStatus>,
    ) -> Self {
        Self {
            user_id: recipient.user_id,
            display_name: recipient.display_name.clone(),
            username: recipient.username.clone(),
            is_bot: recipient.is_bot,
            avatar_url: recipient.avatar_url.clone(),
            status: recipient
                .status
                .or(previous_status)
                .or(known_status)
                .unwrap_or(PresenceStatus::Unknown),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageState {
    pub id: Id<MessageMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub author_id: Id<UserMarker>,
    pub author: String,
    pub author_avatar_url: Option<String>,
    pub message_kind: MessageKind,
    pub reference: Option<MessageReferenceInfo>,
    pub reply: Option<ReplyInfo>,
    pub poll: Option<PollInfo>,
    pub pinned: bool,
    pub reactions: Vec<ReactionInfo>,
    pub content: Option<String>,
    pub sticker_names: Vec<String>,
    pub mentions: Vec<MentionInfo>,
    pub attachments: Vec<AttachmentInfo>,
    pub embeds: Vec<EmbedInfo>,
    pub forwarded_snapshots: Vec<MessageSnapshotInfo>,
    pub edited_timestamp: Option<String>,
}

impl Default for MessageState {
    fn default() -> Self {
        Self {
            id: Id::new(1),
            guild_id: None,
            channel_id: Id::new(1),
            author_id: Id::new(1),
            author: String::new(),
            author_avatar_url: None,
            message_kind: MessageKind::default(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: None,
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
            edited_timestamp: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MessageCapabilities {
    pub is_reply: bool,
    pub is_forwarded: bool,
    pub has_poll: bool,
    pub has_image: bool,
    pub has_video: bool,
    pub has_file: bool,
}

impl MessageState {
    pub fn attachments_in_display_order(&self) -> impl Iterator<Item = &AttachmentInfo> {
        self.attachments.iter().chain(
            self.forwarded_snapshots
                .iter()
                .flat_map(|snapshot| snapshot.attachments.iter()),
        )
    }

    pub fn first_inline_preview(&self) -> Option<InlinePreviewInfo<'_>> {
        self.attachments_in_display_order()
            .find_map(AttachmentInfo::inline_preview_info)
            .or_else(|| {
                self.embeds
                    .iter()
                    .chain(
                        self.forwarded_snapshots
                            .iter()
                            .flat_map(|snapshot| snapshot.embeds.iter()),
                    )
                    .find_map(EmbedInfo::inline_preview_info)
            })
    }

    pub fn inline_previews(&self) -> Vec<InlinePreviewInfo<'_>> {
        self.attachments_in_display_order()
            .filter_map(AttachmentInfo::inline_preview_info)
            .chain(
                self.embeds
                    .iter()
                    .chain(
                        self.forwarded_snapshots
                            .iter()
                            .flat_map(|snapshot| snapshot.embeds.iter()),
                    )
                    .filter_map(EmbedInfo::inline_preview_info),
            )
            .collect()
    }

    pub fn capabilities(&self) -> MessageCapabilities {
        let mut capabilities = MessageCapabilities {
            is_reply: self.reply.is_some(),
            is_forwarded: !self.forwarded_snapshots.is_empty(),
            ..MessageCapabilities::default()
        };

        // Poll and attachment actions are only valid for regular type 0
        // messages. Non-regular messages can still be replies/forwards, but
        // subtype-like action facets should not leak onto system messages.
        if !self.message_kind.is_regular() {
            return capabilities;
        }

        capabilities.has_poll = self.poll.is_some();
        for attachment in self.attachments_in_display_order() {
            if attachment.is_image() && attachment.preferred_url().is_some() {
                capabilities.has_image = true;
            } else if attachment.is_video() {
                capabilities.has_video = true;
            } else {
                capabilities.has_file = true;
            }
        }
        if self.first_inline_preview().is_some() {
            capabilities.has_image = true;
        }

        capabilities
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildMemberState {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    /// Discord login handle. Mirrors `MemberInfo::username`; the @-mention
    /// picker matches against this in addition to `display_name`.
    pub username: Option<String>,
    pub is_bot: bool,
    pub avatar_url: Option<String>,
    pub role_ids: Vec<Id<RoleMarker>>,
    pub status: PresenceStatus,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct UserProfileCacheKey {
    user_id: Id<UserMarker>,
    guild_id: Option<Id<GuildMarker>>,
}

impl UserProfileCacheKey {
    fn new(user_id: Id<UserMarker>, guild_id: Option<Id<GuildMarker>>) -> Self {
        Self { user_id, guild_id }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleState {
    pub id: Id<RoleMarker>,
    pub name: String,
    pub color: Option<u32>,
    pub position: i64,
    pub hoist: bool,
    /// Discord permission bitfield for the role. Used to compute the
    /// authenticated user's base permissions and detect ADMINISTRATOR.
    pub permissions: u64,
}

type MessageAuthorRoleIds = BTreeMap<(Id<ChannelMarker>, Id<MessageMarker>), Vec<Id<RoleMarker>>>;
type ProfileRoleIds = BTreeMap<(Id<GuildMarker>, Id<UserMarker>), Vec<Id<RoleMarker>>>;

#[derive(Clone, Debug)]
pub struct DiscordState {
    guilds: BTreeMap<Id<GuildMarker>, GuildState>,
    channels: BTreeMap<Id<ChannelMarker>, ChannelState>,
    messages: BTreeMap<Id<ChannelMarker>, VecDeque<MessageState>>,
    pinned_messages: BTreeMap<Id<ChannelMarker>, VecDeque<MessageState>>,
    message_author_role_ids: MessageAuthorRoleIds,
    members: BTreeMap<Id<GuildMarker>, BTreeMap<Id<UserMarker>, GuildMemberState>>,
    roles: BTreeMap<Id<GuildMarker>, BTreeMap<Id<RoleMarker>, RoleState>>,
    profile_role_ids: ProfileRoleIds,
    custom_emojis: BTreeMap<Id<GuildMarker>, Vec<CustomEmojiInfo>>,
    /// User's `guild_folders` setting in display order. Empty until READY
    /// delivers it; the dashboard falls back to a flat guild list.
    guild_folders: Vec<GuildFolder>,
    /// Cached profile lookups so the profile popup can render instantly when
    /// the same user is opened again.
    user_profiles: BTreeMap<UserProfileCacheKey, UserProfileInfo>,
    /// Friend / blocked / pending request state delivered through READY's
    /// `relationships` array. Used to colour the profile popup's friend
    /// indicator and to enrich `UserProfileInfo` on insert.
    relationships: BTreeMap<Id<UserMarker>, FriendStatus>,
    /// Last known presence by user id. This gives DM/profile views a fallback
    /// when the private-channel recipient payload omitted status.
    user_presences: BTreeMap<Id<UserMarker>, PresenceStatus>,
    /// Snowflake of the authenticated user. Captured from the READY payload
    /// and consulted by `can_view_channel` to look up our own roles and
    /// match member-level permission overwrites.
    current_user_id: Option<Id<UserMarker>>,
    current_user: Option<String>,
    /// Most recent TYPING_START arrival per (channel, user). Discord renews
    /// the indicator every ~10 seconds; readers prune stale entries via
    /// `typing_users` so the map stays small.
    typing: BTreeMap<Id<ChannelMarker>, BTreeMap<Id<UserMarker>, Instant>>,
    read_states: BTreeMap<Id<ChannelMarker>, ChannelReadState>,
    max_messages_per_channel: usize,
}

#[derive(Clone, Debug)]
pub struct SnapshotRevision {
    pub revision: u64,
}

#[derive(Clone, Debug)]
pub struct DiscordSnapshot {
    pub revision: u64,
    pub state: DiscordState,
}

struct MessageUpdateFields {
    poll: Option<PollInfo>,
    content: Option<String>,
    sticker_names: Option<Vec<String>>,
    mentions: Option<Vec<MentionInfo>>,
    attachments: AttachmentUpdate,
    embeds: Option<Vec<EmbedInfo>>,
    edited_timestamp: Option<String>,
    pinned: Option<bool>,
    reactions: Option<Vec<ReactionInfo>>,
}

impl Default for DiscordState {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_MESSAGES_PER_CHANNEL)
    }
}

impl DiscordState {
    pub fn new(max_messages_per_channel: usize) -> Self {
        Self {
            guilds: BTreeMap::new(),
            channels: BTreeMap::new(),
            messages: BTreeMap::new(),
            pinned_messages: BTreeMap::new(),
            message_author_role_ids: BTreeMap::new(),
            members: BTreeMap::new(),
            roles: BTreeMap::new(),
            profile_role_ids: BTreeMap::new(),
            custom_emojis: BTreeMap::new(),
            guild_folders: Vec::new(),
            user_profiles: BTreeMap::new(),
            relationships: BTreeMap::new(),
            user_presences: BTreeMap::new(),
            current_user_id: None,
            current_user: None,
            typing: BTreeMap::new(),
            read_states: BTreeMap::new(),
            max_messages_per_channel,
        }
    }

    pub fn apply_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::GuildCreate {
                guild_id,
                name,
                member_count,
                owner_id,
                channels,
                members,
                presences,
                roles,
                emojis,
            } => {
                self.guilds.insert(
                    *guild_id,
                    GuildState {
                        id: *guild_id,
                        name: name.clone(),
                        member_count: *member_count,
                        owner_id: *owner_id,
                    },
                );

                for channel in channels {
                    self.upsert_channel(channel);
                }

                let entry = self.members.entry(*guild_id).or_default();
                for member in members {
                    upsert_member(entry, member, None);
                }
                for (user_id, status) in presences {
                    self.user_presences.insert(*user_id, *status);
                    if let Some(member) = entry.get_mut(user_id) {
                        member.status = *status;
                    }
                }
                self.roles.insert(*guild_id, role_map(roles));
                self.custom_emojis.insert(*guild_id, emojis.clone());
            }
            AppEvent::GuildUpdate {
                guild_id,
                name,
                owner_id,
                roles,
                emojis,
            } => {
                if let Some(guild) = self.guilds.get_mut(guild_id) {
                    guild.name = name.clone();
                    if let Some(owner_id) = owner_id {
                        guild.owner_id = Some(*owner_id);
                    }
                }
                if let Some(roles) = roles {
                    self.roles.insert(*guild_id, role_map(roles));
                }
                if let Some(emojis) = emojis {
                    self.custom_emojis.insert(*guild_id, emojis.clone());
                }
            }
            AppEvent::GuildRolesUpdate { guild_id, roles } => {
                self.roles.insert(*guild_id, role_map(roles));
            }
            AppEvent::GuildEmojisUpdate { guild_id, emojis } => {
                self.custom_emojis.insert(*guild_id, emojis.clone());
            }
            AppEvent::GuildDelete { guild_id } => {
                self.guilds.remove(guild_id);
                self.channels
                    .retain(|_, channel| channel.guild_id != Some(*guild_id));
                self.messages
                    .retain(|channel_id, _| self.channels.contains_key(channel_id));
                self.pinned_messages
                    .retain(|channel_id, _| self.channels.contains_key(channel_id));
                self.message_author_role_ids
                    .retain(|(channel_id, _), _| self.channels.contains_key(channel_id));
                self.members.remove(guild_id);
                self.roles.remove(guild_id);
                self.profile_role_ids
                    .retain(|(profile_guild_id, _), _| profile_guild_id != guild_id);
                self.custom_emojis.remove(guild_id);
            }
            AppEvent::ChannelUpsert(channel) => self.upsert_channel(channel),
            AppEvent::ForumPostsLoaded {
                posts,
                preview_messages,
                ..
            } => {
                for post in posts {
                    self.upsert_channel(post);
                }
                for message in preview_messages {
                    self.merge_message_history(
                        message.channel_id,
                        None,
                        std::slice::from_ref(message),
                    );
                }
            }
            AppEvent::ChannelDelete { channel_id, .. } => {
                self.channels.remove(channel_id);
                self.messages.remove(channel_id);
                self.pinned_messages.remove(channel_id);
                self.message_author_role_ids
                    .retain(|(message_channel_id, _), _| message_channel_id != channel_id);
            }
            AppEvent::MessageCreate {
                guild_id,
                channel_id,
                message_id,
                author_id,
                author,
                author_avatar_url,
                author_role_ids,
                message_kind,
                reference,
                reply,
                poll,
                content,
                sticker_names,
                mentions,
                attachments,
                embeds,
                forwarded_snapshots,
                ..
            } => {
                let guild_id = guild_id.or_else(|| self.channel_guild_id(*channel_id));
                self.record_author_role_ids(*channel_id, *message_id, author_role_ids);
                // Self-authored mentions never bump our own sidebar.
                if let Some(self_id) = self.current_user_id
                    && *author_id != self_id
                    && mentions.iter().any(|mention| mention.user_id == self_id)
                {
                    let entry = self.read_states.entry(*channel_id).or_default();
                    entry.mention_count = entry.mention_count.saturating_add(1);
                }
                self.upsert_message(MessageState {
                    id: *message_id,
                    guild_id,
                    channel_id: *channel_id,
                    author_id: *author_id,
                    author: self.message_author_display_name(guild_id, *author_id, author),
                    author_avatar_url: self.message_author_avatar_url(
                        guild_id,
                        *author_id,
                        author_avatar_url,
                    ),
                    message_kind: *message_kind,
                    reference: reference.clone(),
                    reply: reply.clone(),
                    poll: poll.clone(),
                    pinned: false,
                    reactions: Vec::new(),
                    content: content.clone(),
                    sticker_names: sticker_names.clone(),
                    mentions: mentions.clone(),
                    attachments: attachments.clone(),
                    embeds: embeds.clone(),
                    forwarded_snapshots: forwarded_snapshots.clone(),
                    edited_timestamp: None,
                });
            }
            AppEvent::MessageHistoryLoaded {
                channel_id,
                before,
                messages,
            } => self.merge_message_history(*channel_id, *before, messages),
            AppEvent::ThreadPreviewLoaded {
                channel_id,
                message,
            } => self.merge_message_history(*channel_id, None, std::slice::from_ref(message)),
            AppEvent::MessageHistoryLoadFailed { .. } => {}
            AppEvent::MessageUpdate {
                channel_id,
                message_id,
                poll,
                content,
                mentions,
                sticker_names,
                attachments,
                embeds,
                edited_timestamp,
                ..
            } => self.update_message(
                *channel_id,
                *message_id,
                MessageUpdateFields {
                    poll: poll.clone(),
                    content: content.clone(),
                    sticker_names: sticker_names.clone(),
                    mentions: mentions.clone(),
                    attachments: attachments.clone(),
                    embeds: embeds.clone(),
                    edited_timestamp: edited_timestamp.clone(),
                    pinned: None,
                    reactions: None,
                },
            ),
            AppEvent::CurrentUserReactionAdd {
                channel_id,
                message_id,
                emoji,
            } => self.add_reaction(*channel_id, *message_id, emoji.clone()),
            AppEvent::CurrentUserReactionRemove {
                channel_id,
                message_id,
                emoji,
            } => self.remove_reaction(*channel_id, *message_id, emoji),
            AppEvent::MessageReactionAdd {
                channel_id,
                message_id,
                user_id,
                emoji,
                ..
            } => self.add_gateway_reaction(*channel_id, *message_id, *user_id, emoji.clone()),
            AppEvent::MessageReactionRemove {
                channel_id,
                message_id,
                user_id,
                emoji,
                ..
            } => self.remove_gateway_reaction(*channel_id, *message_id, *user_id, emoji),
            AppEvent::MessageReactionRemoveAll {
                channel_id,
                message_id,
                ..
            } => self.clear_gateway_reactions(*channel_id, *message_id),
            AppEvent::MessageReactionRemoveEmoji {
                channel_id,
                message_id,
                emoji,
                ..
            } => self.clear_gateway_reaction_emoji(*channel_id, *message_id, emoji),
            AppEvent::MessagePinnedUpdate {
                channel_id,
                message_id,
                pinned,
            } => self.set_cached_message_pinned(*channel_id, *message_id, *pinned),
            AppEvent::PinnedMessagesLoaded {
                channel_id,
                messages,
            } => self.replace_pinned_messages(*channel_id, messages),
            AppEvent::PinnedMessagesLoadFailed { .. } => {}
            AppEvent::CurrentUserPollVoteUpdate {
                channel_id,
                message_id,
                answer_ids,
            } => self.update_current_user_poll_vote(*channel_id, *message_id, answer_ids),
            AppEvent::MessageDelete {
                channel_id,
                message_id,
                ..
            } => self.delete_message(*channel_id, *message_id),
            AppEvent::GuildMemberAdd { guild_id, member } => {
                let entry = self.members.entry(*guild_id).or_default();
                let was_known = entry.contains_key(&member.user_id);
                let previous_status = entry.get(&member.user_id).map(|m| m.status);
                upsert_member(entry, member, previous_status);
                if !was_known {
                    self.increment_guild_member_count(*guild_id);
                }
                self.refresh_message_author_display_name(*guild_id, member);
            }
            AppEvent::GuildMemberUpsert { guild_id, member } => {
                let entry = self.members.entry(*guild_id).or_default();
                let previous_status = entry.get(&member.user_id).map(|m| m.status);
                upsert_member(entry, member, previous_status);
                self.refresh_message_author_display_name(*guild_id, member);
            }
            AppEvent::GuildMemberRemove { guild_id, user_id } => {
                if let Some(entry) = self.members.get_mut(guild_id) {
                    entry.remove(user_id);
                }
                self.decrement_guild_member_count(*guild_id);
            }
            AppEvent::PresenceUpdate {
                guild_id,
                user_id,
                status,
            } => {
                self.user_presences.insert(*user_id, *status);
                let entry = self.members.entry(*guild_id).or_default();
                if let Some(member) = entry.get_mut(user_id) {
                    member.status = *status;
                } else {
                    entry.insert(
                        *user_id,
                        GuildMemberState {
                            user_id: *user_id,
                            display_name: format!("user-{}", user_id.get()),
                            username: None,
                            is_bot: false,
                            avatar_url: None,
                            role_ids: Vec::new(),
                            status: *status,
                        },
                    );
                }
                self.update_channel_recipient_presence(*user_id, *status);
            }
            AppEvent::UserPresenceUpdate { user_id, status } => {
                self.user_presences.insert(*user_id, *status);
                self.update_channel_recipient_presence(*user_id, *status);
            }
            AppEvent::TypingStart {
                channel_id,
                user_id,
            } => {
                // Record (or refresh) the typing entry, then sweep this
                // channel's stale entries while we already hold the mutable
                // borrow. Read paths see only fresh entries.
                let now = Instant::now();
                let bucket = self.typing.entry(*channel_id).or_default();
                bucket.insert(*user_id, now);
                bucket.retain(|_, started| now.duration_since(*started) <= TYPING_INDICATOR_TTL);
                if bucket.is_empty() {
                    self.typing.remove(channel_id);
                }
            }
            AppEvent::GuildFoldersUpdate { folders } => {
                self.guild_folders = folders.clone();
            }
            AppEvent::UserProfileLoaded { guild_id, profile } => {
                let mut profile = profile.clone();
                if let Some(guild_id) = guild_id {
                    self.profile_role_ids
                        .insert((*guild_id, profile.user_id), profile.role_ids.clone());
                }
                profile.friend_status = self
                    .relationships
                    .get(&profile.user_id)
                    .copied()
                    .unwrap_or(FriendStatus::None);
                self.user_profiles.insert(
                    UserProfileCacheKey::new(profile.user_id, *guild_id),
                    profile,
                );
            }
            AppEvent::RelationshipsLoaded { relationships } => {
                self.relationships.clear();
                for (user_id, status) in relationships {
                    self.relationships.insert(*user_id, *status);
                    for profile in self
                        .user_profiles
                        .values_mut()
                        .filter(|profile| profile.user_id == *user_id)
                    {
                        profile.friend_status = *status;
                    }
                }
            }
            AppEvent::RelationshipUpsert { user_id, status } => {
                self.relationships.insert(*user_id, *status);
                for profile in self
                    .user_profiles
                    .values_mut()
                    .filter(|profile| profile.user_id == *user_id)
                {
                    profile.friend_status = *status;
                }
            }
            AppEvent::RelationshipRemove { user_id } => {
                self.relationships.remove(user_id);
                for profile in self
                    .user_profiles
                    .values_mut()
                    .filter(|profile| profile.user_id == *user_id)
                {
                    profile.friend_status = FriendStatus::None;
                }
            }
            AppEvent::Ready { user, user_id } => {
                self.current_user = Some(user.clone());
                if let Some(user_id) = user_id {
                    self.current_user_id = Some(*user_id);
                }
            }
            AppEvent::ReadStateInit { entries } => {
                self.read_states.clear();
                for entry in entries {
                    self.read_states.insert(
                        entry.channel_id,
                        ChannelReadState {
                            last_acked_message_id: entry.last_acked_message_id,
                            mention_count: entry.mention_count,
                        },
                    );
                }
            }
            AppEvent::MessageAck {
                channel_id,
                message_id,
                mention_count,
            } => {
                let entry = self.read_states.entry(*channel_id).or_default();
                entry.last_acked_message_id = Some(*message_id);
                entry.mention_count = *mention_count;
            }
            AppEvent::GatewayError { .. }
            | AppEvent::StatusMessage { .. }
            | AppEvent::ReactionUsersLoaded { .. }
            | AppEvent::AttachmentPreviewLoaded { .. }
            | AppEvent::AttachmentPreviewLoadFailed { .. }
            | AppEvent::ThreadPreviewLoadFailed { .. }
            | AppEvent::ForumPostsLoadFailed { .. }
            | AppEvent::UserProfileLoadFailed { .. }
            | AppEvent::ActivateChannel { .. }
            | AppEvent::GatewayClosed => {}
        }
    }

    pub fn channel_unread(&self, channel_id: Id<ChannelMarker>) -> ChannelUnreadState {
        let Some(channel) = self.channels.get(&channel_id) else {
            return ChannelUnreadState::Seen;
        };
        let Some(latest) = channel.last_message_id else {
            return ChannelUnreadState::Seen;
        };
        let read = self
            .read_states
            .get(&channel_id)
            .copied()
            .unwrap_or_default();
        if read.mention_count > 0 {
            return ChannelUnreadState::Mentioned(read.mention_count);
        }
        let acked = read.last_acked_message_id;
        if acked.is_none_or(|acked| acked < latest) {
            ChannelUnreadState::Unread
        } else {
            ChannelUnreadState::Seen
        }
    }

    /// `None` when the channel is already fully read or has no messages.
    pub fn channel_ack_target(&self, channel_id: Id<ChannelMarker>) -> Option<Id<MessageMarker>> {
        let channel = self.channels.get(&channel_id)?;
        let latest = channel.last_message_id?;
        let acked = self
            .read_states
            .get(&channel_id)
            .and_then(|state| state.last_acked_message_id);
        match acked {
            Some(acked) if acked >= latest => None,
            _ => Some(latest),
        }
    }

    /// Snowflake of the most recent message the user has acked in
    /// `channel_id`. `None` when the channel has never been acked or no
    /// read-state has been received yet.
    pub fn channel_last_acked_message_id(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Option<Id<MessageMarker>> {
        self.read_states
            .get(&channel_id)
            .and_then(|state| state.last_acked_message_id)
    }

    pub fn guild_folders(&self) -> &[GuildFolder] {
        &self.guild_folders
    }

    /// Returns the user IDs that have typed in `channel_id` within the TTL
    /// window, sorted by most-recent activity first. Read-only so render
    /// paths can call it without taking a mutable borrow on the whole
    /// `DiscordState`; pruning of stale entries happens lazily on the next
    /// `TYPING_START` for the same channel.
    pub fn typing_users(&self, channel_id: Id<ChannelMarker>) -> Vec<Id<UserMarker>> {
        let now = Instant::now();
        let Some(channel_typers) = self.typing.get(&channel_id) else {
            return Vec::new();
        };
        let mut fresh: Vec<(Id<UserMarker>, Instant)> = channel_typers
            .iter()
            .filter(|(_, started)| now.duration_since(**started) <= TYPING_INDICATOR_TTL)
            .map(|(user_id, started)| (*user_id, *started))
            .collect();
        // Newest typer first so the "X is typing…" label tends to surface the
        // person who just hit a key.
        fresh.sort_by_key(|(_, started)| std::cmp::Reverse(*started));
        fresh.into_iter().map(|(user_id, _)| user_id).collect()
    }

    pub fn user_profile(
        &self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<&UserProfileInfo> {
        self.user_profiles
            .get(&UserProfileCacheKey::new(user_id, guild_id))
    }

    pub fn guild(&self, guild_id: Id<GuildMarker>) -> Option<&GuildState> {
        self.guilds.get(&guild_id)
    }

    pub fn guilds(&self) -> Vec<&GuildState> {
        self.guilds.values().collect()
    }

    pub fn channels_for_guild(&self, guild_id: Option<Id<GuildMarker>>) -> Vec<&ChannelState> {
        self.channels
            .values()
            .filter(|channel| channel.guild_id == guild_id)
            .collect()
    }

    /// Same as `channels_for_guild` but skips channels the authenticated user
    /// cannot see. Use this when populating UI surfaces (sidebar, member-list
    /// subscription targets) so we never present a channel that would 403
    /// when fetched. DMs always pass through unchanged.
    pub fn viewable_channels_for_guild(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Vec<&ChannelState> {
        self.channels
            .values()
            .filter(|channel| channel.guild_id == guild_id)
            .filter(|channel| self.can_view_channel(channel))
            .collect()
    }

    /// Snowflake of the authenticated user, captured during READY. `None`
    /// before the gateway hands us a `READY` payload — callers that depend on
    /// our identity (permission checks, mention detection) should treat the
    /// missing case as "can't compute, fall back to permissive".
    pub fn current_user_id(&self) -> Option<Id<UserMarker>> {
        self.current_user_id
    }

    pub fn current_user(&self) -> Option<&str> {
        self.current_user.as_deref()
    }

    pub fn navigation_snapshot(&self) -> Self {
        let mut snapshot = Self::new(self.max_messages_per_channel);
        snapshot.restore_navigation_snapshot(self);
        snapshot
    }

    pub fn restore_navigation_snapshot(&mut self, snapshot: &Self) {
        self.guilds = snapshot.guilds.clone();
        self.channels = snapshot.channels.clone();
        self.members = snapshot.members.clone();
        self.roles = snapshot.roles.clone();
        self.profile_role_ids = snapshot.profile_role_ids.clone();
        self.custom_emojis = snapshot.custom_emojis.clone();
        self.guild_folders = snapshot.guild_folders.clone();
        self.user_profiles = snapshot.user_profiles.clone();
        self.relationships = snapshot.relationships.clone();
        self.user_presences = snapshot.user_presences.clone();
        self.current_user_id = snapshot.current_user_id;
        self.current_user = snapshot.current_user.clone();
        self.typing = snapshot.typing.clone();
    }

    pub fn user_presence(&self, user_id: Id<UserMarker>) -> Option<PresenceStatus> {
        self.user_presences.get(&user_id).copied()
    }

    /// Visible/hidden channel counts for a guild scope. DM scope reports
    /// `(visible, 0)` since DMs are never hidden. Threads are excluded from
    /// both sides — the debug-panel readout focuses on top-level channels
    /// because those are what the user navigates by.
    pub fn channel_visibility_stats(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> ChannelVisibilityStats {
        let mut visible: usize = 0;
        let mut hidden: usize = 0;
        for channel in self.channels.values() {
            if channel.guild_id != guild_id || channel.is_thread() {
                continue;
            }
            if self.can_view_channel(channel) {
                visible += 1;
            } else {
                hidden += 1;
            }
        }
        ChannelVisibilityStats { visible, hidden }
    }

    pub fn messages_for_channel(&self, channel_id: Id<ChannelMarker>) -> Vec<&MessageState> {
        self.messages
            .get(&channel_id)
            .map(|messages| messages.iter().collect())
            .unwrap_or_default()
    }

    pub fn pinned_messages_for_channel(&self, channel_id: Id<ChannelMarker>) -> Vec<&MessageState> {
        self.pinned_messages
            .get(&channel_id)
            .map(|messages| messages.iter().rev().collect())
            .unwrap_or_default()
    }

    pub fn members_for_guild(&self, guild_id: Id<GuildMarker>) -> Vec<&GuildMemberState> {
        self.members
            .get(&guild_id)
            .map(|map| map.values().collect())
            .unwrap_or_default()
    }

    pub fn roles_for_guild(&self, guild_id: Id<GuildMarker>) -> Vec<&RoleState> {
        self.roles
            .get(&guild_id)
            .map(|map| map.values().collect())
            .unwrap_or_default()
    }

    pub fn member_role_color(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<u32> {
        let member = self.members.get(&guild_id)?.get(&user_id)?;
        let roles = self.roles.get(&guild_id)?;
        selected_member_role_color(member, roles)
    }

    pub fn message_author_role_color(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<u32> {
        let roles = self.roles.get(&guild_id)?;
        if let Some(member) = self
            .members
            .get(&guild_id)
            .and_then(|members| members.get(&user_id))
        {
            return selected_member_role_color(member, roles);
        }

        if let Some(role_ids) = self.profile_role_ids.get(&(guild_id, user_id)) {
            return selected_role_ids_color(role_ids, roles);
        }

        let role_ids = self
            .message_author_role_ids
            .get(&(channel_id, message_id))?;
        selected_role_ids_color(role_ids, roles)
    }

    pub fn custom_emojis_for_guild(&self, guild_id: Id<GuildMarker>) -> &[CustomEmojiInfo] {
        self.custom_emojis
            .get(&guild_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn member_display_name(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<&str> {
        self.members
            .get(&guild_id)
            .and_then(|members| members.get(&user_id))
            .map(|member| member.display_name.as_str())
    }

    pub fn channel(&self, channel_id: Id<ChannelMarker>) -> Option<&ChannelState> {
        self.channels.get(&channel_id)
    }

    fn channel_guild_id(&self, channel_id: Id<ChannelMarker>) -> Option<Id<GuildMarker>> {
        self.channels
            .get(&channel_id)
            .and_then(|channel| channel.guild_id)
    }

    fn message_author_display_name(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        author_id: Id<UserMarker>,
        fallback: &str,
    ) -> String {
        guild_id
            .and_then(|guild_id| self.members.get(&guild_id))
            .and_then(|members| members.get(&author_id))
            .map(|member| member.display_name.clone())
            .unwrap_or_else(|| fallback.to_owned())
    }

    fn message_author_avatar_url(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        author_id: Id<UserMarker>,
        fallback: &Option<String>,
    ) -> Option<String> {
        guild_id
            .and_then(|guild_id| self.members.get(&guild_id))
            .and_then(|members| members.get(&author_id))
            .and_then(|member| member.avatar_url.clone())
            .or_else(|| fallback.clone())
    }

    fn refresh_message_author_display_name(
        &mut self,
        guild_id: Id<GuildMarker>,
        member: &MemberInfo,
    ) {
        for messages in self.messages.values_mut() {
            for message in messages.iter_mut() {
                if message.guild_id == Some(guild_id) && message.author_id == member.user_id {
                    message.author = member.display_name.clone();
                    if member.avatar_url.is_some() || message.author_avatar_url.is_none() {
                        message.author_avatar_url = member.avatar_url.clone();
                    }
                }
            }
        }
        for messages in self.pinned_messages.values_mut() {
            for message in messages.iter_mut() {
                if message.guild_id == Some(guild_id) && message.author_id == member.user_id {
                    message.author = member.display_name.clone();
                    if member.avatar_url.is_some() || message.author_avatar_url.is_none() {
                        message.author_avatar_url = member.avatar_url.clone();
                    }
                }
            }
        }
    }

    fn upsert_channel(&mut self, channel: &ChannelInfo) {
        let existing = self.channels.get(&channel.channel_id);
        let last_message_id = existing
            .and_then(|existing| existing.last_message_id)
            .max(channel.last_message_id);
        let recipients = channel
            .recipients
            .as_ref()
            .map(|recipients| {
                recipients
                    .iter()
                    .map(|recipient| {
                        let previous_status = existing
                            .and_then(|existing| {
                                existing
                                    .recipients
                                    .iter()
                                    .find(|existing| existing.user_id == recipient.user_id)
                            })
                            .map(|recipient| recipient.status);
                        let known_status = self.user_presences.get(&recipient.user_id).copied();
                        ChannelRecipientState::from_info(recipient, previous_status, known_status)
                    })
                    .collect()
            })
            .or_else(|| existing.map(|existing| existing.recipients.clone()))
            .unwrap_or_default();

        // Threads do not own channel-level overwrites — `permitted` is decided
        // by the parent. For everything else we take the newest payload as
        // authoritative, since CHANNEL_UPDATE always carries the full array.
        let permission_overwrites = if permissions::is_thread_kind(&channel.kind) {
            existing
                .map(|existing| existing.permission_overwrites.clone())
                .unwrap_or_default()
        } else {
            channel.permission_overwrites.clone()
        };

        self.channels.insert(
            channel.channel_id,
            ChannelState {
                id: channel.channel_id,
                guild_id: channel.guild_id,
                parent_id: channel.parent_id,
                position: channel.position,
                last_message_id,
                name: channel.name.clone(),
                kind: channel.kind.clone(),
                message_count: channel.message_count,
                total_message_sent: channel.total_message_sent,
                thread_archived: channel.thread_archived,
                thread_locked: channel.thread_locked,
                thread_pinned: channel.thread_pinned,
                recipients,
                permission_overwrites,
            },
        );
    }

    fn update_channel_recipient_presence(
        &mut self,
        user_id: Id<UserMarker>,
        status: PresenceStatus,
    ) {
        for channel in self.channels.values_mut() {
            for recipient in &mut channel.recipients {
                if recipient.user_id == user_id {
                    recipient.status = status;
                }
            }
        }
    }

    fn increment_guild_member_count(&mut self, guild_id: Id<GuildMarker>) {
        if let Some(count) = self
            .guilds
            .get_mut(&guild_id)
            .and_then(|guild| guild.member_count.as_mut())
        {
            *count = count.saturating_add(1);
        }
    }

    fn decrement_guild_member_count(&mut self, guild_id: Id<GuildMarker>) {
        if let Some(count) = self
            .guilds
            .get_mut(&guild_id)
            .and_then(|guild| guild.member_count.as_mut())
        {
            *count = count.saturating_sub(1);
        }
    }

    fn upsert_message(&mut self, mut message: MessageState) {
        let channel_id = message.channel_id;
        let message_id = message.id;
        message.guild_id = message
            .guild_id
            .or_else(|| self.channel_guild_id(channel_id));
        let messages = self.messages.entry(message.channel_id).or_default();
        let inserted = if let Some(existing) =
            messages.iter_mut().find(|item| item.id == message.id)
        {
            existing.guild_id = message.guild_id.or(existing.guild_id);
            existing.channel_id = message.channel_id;
            existing.author_id = message.author_id;
            existing.author = message.author;
            if message.author_avatar_url.is_some() || existing.author_avatar_url.is_none() {
                existing.author_avatar_url = message.author_avatar_url;
            }
            existing.message_kind = message.message_kind;
            if message.reference.is_some() || existing.reference.is_none() {
                existing.reference = message.reference;
            }
            if message.reply.is_some() || existing.reply.is_none() {
                existing.reply = message.reply;
            }
            if message.poll.is_some() || existing.poll.is_none() {
                existing.poll = message.poll;
            }
            existing.pinned = existing.pinned || message.pinned;
            existing.reactions = message.reactions;
            if message.content.is_some() {
                existing.content = message.content;
            }
            if !message.mentions.is_empty() || existing.mentions.is_empty() {
                existing.mentions = merge_message_mentions(&existing.mentions, &message.mentions);
            }
            if !message.attachments.is_empty() || existing.attachments.is_empty() {
                existing.attachments = message.attachments;
            }
            if !message.forwarded_snapshots.is_empty() || existing.forwarded_snapshots.is_empty() {
                existing.forwarded_snapshots = message.forwarded_snapshots;
            }
            false
        } else {
            messages.push_back(message);
            true
        };

        while messages.len() > self.max_messages_per_channel {
            messages.pop_front();
        }
        self.record_channel_message_id(channel_id, message_id);
        if inserted {
            self.increment_thread_message_counts(channel_id);
        }
    }

    fn increment_thread_message_counts(&mut self, channel_id: Id<ChannelMarker>) {
        let Some(channel) = self
            .channels
            .get_mut(&channel_id)
            .filter(|channel| channel.is_thread())
        else {
            return;
        };

        if let Some(count) = channel.message_count.as_mut() {
            *count = count.saturating_add(1);
        }
        if let Some(count) = channel.total_message_sent.as_mut() {
            *count = count.saturating_add(1);
        }
    }

    fn add_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: super::ReactionEmoji,
    ) {
        if let Some(messages) = self.messages.get_mut(&channel_id) {
            add_reaction_in(messages, message_id, emoji.clone());
        }
        if let Some(messages) = self.pinned_messages.get_mut(&channel_id) {
            add_reaction_in(messages, message_id, emoji);
        }
    }

    fn remove_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &super::ReactionEmoji,
    ) {
        if let Some(messages) = self.messages.get_mut(&channel_id) {
            remove_reaction_in(messages, message_id, emoji);
        }
        if let Some(messages) = self.pinned_messages.get_mut(&channel_id) {
            remove_reaction_in(messages, message_id, emoji);
        }
    }

    fn add_gateway_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        user_id: Id<UserMarker>,
        emoji: super::ReactionEmoji,
    ) {
        let is_current_user = self.current_user_id == Some(user_id);
        if let Some(messages) = self.messages.get_mut(&channel_id) {
            add_gateway_reaction_in(messages, message_id, is_current_user, emoji.clone());
        }
        if let Some(messages) = self.pinned_messages.get_mut(&channel_id) {
            add_gateway_reaction_in(messages, message_id, is_current_user, emoji);
        }
    }

    fn remove_gateway_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        user_id: Id<UserMarker>,
        emoji: &super::ReactionEmoji,
    ) {
        let is_current_user = self.current_user_id == Some(user_id);
        if let Some(messages) = self.messages.get_mut(&channel_id) {
            remove_gateway_reaction_in(messages, message_id, is_current_user, emoji);
        }
        if let Some(messages) = self.pinned_messages.get_mut(&channel_id) {
            remove_gateway_reaction_in(messages, message_id, is_current_user, emoji);
        }
    }

    fn clear_gateway_reactions(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        if let Some(messages) = self.messages.get_mut(&channel_id) {
            clear_gateway_reactions_in(messages, message_id);
        }
        if let Some(messages) = self.pinned_messages.get_mut(&channel_id) {
            clear_gateway_reactions_in(messages, message_id);
        }
    }

    fn clear_gateway_reaction_emoji(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &super::ReactionEmoji,
    ) {
        if let Some(messages) = self.messages.get_mut(&channel_id) {
            clear_gateway_reaction_emoji_in(messages, message_id, emoji);
        }
        if let Some(messages) = self.pinned_messages.get_mut(&channel_id) {
            clear_gateway_reaction_emoji_in(messages, message_id, emoji);
        }
    }

    fn update_current_user_poll_vote(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        answer_ids: &[u8],
    ) {
        if let Some(messages) = self.messages.get_mut(&channel_id) {
            update_current_user_poll_vote_in(messages, message_id, answer_ids);
        }
        if let Some(messages) = self.pinned_messages.get_mut(&channel_id) {
            update_current_user_poll_vote_in(messages, message_id, answer_ids);
        }
    }

    fn merge_message_history(
        &mut self,
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        history: &[MessageInfo],
    ) {
        let channel_guild_id = self.channel_guild_id(channel_id);
        let older_history_message_limit = self.older_history_message_limit();
        let incoming_messages = history
            .iter()
            .filter(|message| message.channel_id == channel_id)
            .map(|message| {
                let mut message = self.message_state_from_info(channel_guild_id, message);
                if self.pinned_message_known(channel_id, message.id) {
                    message.pinned = true;
                }
                message
            })
            .collect::<Vec<_>>();
        for message in history
            .iter()
            .filter(|message| message.channel_id == channel_id)
        {
            self.record_message_author_role_ids(message);
        }
        let messages = self.messages.entry(channel_id).or_default();
        let mut by_id: BTreeMap<Id<MessageMarker>, MessageState> = messages
            .drain(..)
            .map(|message| (message.id, message))
            .collect();

        for incoming in incoming_messages {
            by_id
                .entry(incoming.id)
                .and_modify(|existing| merge_message(existing, &incoming))
                .or_insert(incoming);
        }

        *messages = by_id.into_values().collect();
        if before.is_none() {
            while messages.len() > self.max_messages_per_channel {
                messages.pop_front();
            }
        } else {
            while messages.len() > older_history_message_limit {
                messages.pop_back();
            }
        }
        if let Some(last_message_id) = messages.back().map(|message| message.id) {
            self.record_channel_message_id(channel_id, last_message_id);
        }
    }

    fn older_history_message_limit(&self) -> usize {
        self.max_messages_per_channel
            .saturating_mul(OLDER_HISTORY_EXTRA_WINDOW_MULTIPLIER)
    }

    fn replace_pinned_messages(&mut self, channel_id: Id<ChannelMarker>, pins: &[MessageInfo]) {
        let channel_guild_id = self.channel_guild_id(channel_id);
        let mut by_id = BTreeMap::new();
        for pin in pins
            .iter()
            .filter(|message| message.channel_id == channel_id)
        {
            self.record_message_author_role_ids(pin);
            let mut pinned = self.message_state_from_info(channel_guild_id, pin);
            pinned.pinned = true;
            if let Some(existing) = self
                .messages
                .get_mut(&channel_id)
                .and_then(|messages| messages.iter_mut().find(|message| message.id == pinned.id))
            {
                merge_message(existing, &pinned);
            }
            by_id.insert(pinned.id, pinned);
        }

        self.pinned_messages
            .insert(channel_id, by_id.into_values().collect());
    }

    fn message_state_from_info(
        &self,
        channel_guild_id: Option<Id<GuildMarker>>,
        message: &MessageInfo,
    ) -> MessageState {
        let guild_id = message.guild_id.or(channel_guild_id);
        MessageState {
            id: message.message_id,
            guild_id,
            channel_id: message.channel_id,
            author_id: message.author_id,
            author: self.message_author_display_name(guild_id, message.author_id, &message.author),
            author_avatar_url: self.message_author_avatar_url(
                guild_id,
                message.author_id,
                &message.author_avatar_url,
            ),
            message_kind: message.message_kind,
            reference: message.reference.clone(),
            reply: message.reply.clone(),
            poll: message.poll.clone(),
            pinned: message.pinned,
            reactions: message.reactions.clone(),
            content: message.content.clone(),
            sticker_names: message.sticker_names.clone(),
            mentions: message.mentions.clone(),
            attachments: message.attachments.clone(),
            embeds: message.embeds.clone(),
            forwarded_snapshots: message.forwarded_snapshots.clone(),
            edited_timestamp: message.edited_timestamp.clone(),
        }
    }

    fn pinned_message_known(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> bool {
        self.pinned_messages
            .get(&channel_id)
            .is_some_and(|messages| messages.iter().any(|message| message.id == message_id))
    }

    fn record_channel_message_id(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        if let Some(channel) = self.channels.get_mut(&channel_id) {
            channel.last_message_id = channel.last_message_id.max(Some(message_id));
        }
    }

    fn update_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        update: MessageUpdateFields,
    ) {
        if let Some(messages) = self.messages.get_mut(&channel_id) {
            update_message_in(messages, message_id, &update);
        }
        if let Some(messages) = self.pinned_messages.get_mut(&channel_id) {
            update_message_in(messages, message_id, &update);
        }
    }

    fn set_cached_message_pinned(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    ) {
        let normal_message = self.messages.get_mut(&channel_id).and_then(|messages| {
            messages
                .iter_mut()
                .find(|message| message.id == message_id)
                .map(|message| {
                    message.pinned = pinned;
                    message.clone()
                })
        });

        if pinned {
            if let Some(mut message) = normal_message {
                message.pinned = true;
                upsert_sorted_message(self.pinned_messages.entry(channel_id).or_default(), message);
            }
        } else if let Some(messages) = self.pinned_messages.get_mut(&channel_id) {
            messages.retain(|message| message.id != message_id);
        }
    }

    fn delete_message(&mut self, channel_id: Id<ChannelMarker>, message_id: Id<MessageMarker>) {
        if let Some(messages) = self.messages.get_mut(&channel_id) {
            messages.retain(|message| message.id != message_id);
        }
        if let Some(messages) = self.pinned_messages.get_mut(&channel_id) {
            messages.retain(|message| message.id != message_id);
        }
        self.message_author_role_ids
            .remove(&(channel_id, message_id));
    }

    fn record_message_author_role_ids(&mut self, message: &MessageInfo) {
        self.record_author_role_ids(
            message.channel_id,
            message.message_id,
            &message.author_role_ids,
        );
    }

    fn record_author_role_ids(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        author_role_ids: &[Id<RoleMarker>],
    ) {
        let key = (channel_id, message_id);
        if author_role_ids.is_empty() {
            self.message_author_role_ids.remove(&key);
            return;
        }

        self.message_author_role_ids
            .insert(key, author_role_ids.to_vec());
    }
}

fn merge_message(existing: &mut MessageState, incoming: &MessageState) {
    existing.guild_id = incoming.guild_id.or(existing.guild_id);
    existing.channel_id = incoming.channel_id;
    existing.author_id = incoming.author_id;
    existing.author = incoming.author.clone();
    if incoming.author_avatar_url.is_some() || existing.author_avatar_url.is_none() {
        existing.author_avatar_url = incoming.author_avatar_url.clone();
    }
    existing.message_kind = incoming.message_kind;
    if incoming.reply.is_some() || existing.reply.is_none() {
        existing.reply = incoming.reply.clone();
    }
    if incoming.poll.is_some() || existing.poll.is_none() {
        existing.poll = incoming.poll.clone();
    }
    existing.pinned = existing.pinned || incoming.pinned;
    existing.reactions = incoming.reactions.clone();

    if let Some(content) = &incoming.content {
        let existing_is_empty = existing
            .content
            .as_deref()
            .map(str::is_empty)
            .unwrap_or(true);
        if !content.is_empty() || existing_is_empty {
            existing.content = Some(content.clone());
        }
    }
    if !incoming.sticker_names.is_empty() || existing.sticker_names.is_empty() {
        existing.sticker_names = incoming.sticker_names.clone();
    }
    existing.mentions = merge_message_mentions(&existing.mentions, &incoming.mentions);
    if !incoming.attachments.is_empty() || existing.attachments.is_empty() {
        existing.attachments = incoming.attachments.clone();
    }
    if !incoming.embeds.is_empty() || existing.embeds.is_empty() {
        existing.embeds = incoming.embeds.clone();
    }
    if !incoming.forwarded_snapshots.is_empty() || existing.forwarded_snapshots.is_empty() {
        existing.forwarded_snapshots = incoming.forwarded_snapshots.clone();
    }
    if incoming.edited_timestamp.is_some() || existing.edited_timestamp.is_none() {
        existing.edited_timestamp = incoming.edited_timestamp.clone();
    }
}

fn update_message_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    update: &MessageUpdateFields,
) {
    let Some(existing) = messages.iter_mut().find(|item| item.id == message_id) else {
        return;
    };
    if let Some(poll) = &update.poll {
        existing.poll = Some(poll.clone());
    }
    if let Some(pinned) = update.pinned {
        existing.pinned = pinned;
    }
    if let Some(reactions) = &update.reactions {
        existing.reactions = reactions.clone();
    }
    if let Some(content) = &update.content {
        existing.content = Some(content.clone());
    }
    if let Some(sticker_names) = &update.sticker_names {
        existing.sticker_names = sticker_names.clone();
    }
    if let Some(mentions) = &update.mentions {
        existing.mentions = mentions.clone();
    }
    if let Some(embeds) = &update.embeds {
        existing.embeds = embeds.clone();
    }
    if let Some(edited_timestamp) = &update.edited_timestamp {
        existing.edited_timestamp = Some(edited_timestamp.clone());
    }
    match &update.attachments {
        AttachmentUpdate::Replace(attachments) => existing.attachments = attachments.clone(),
        AttachmentUpdate::Unchanged => {}
    }
}

fn add_reaction_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    emoji: super::ReactionEmoji,
) {
    let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
        return;
    };
    if let Some(reaction) = message
        .reactions
        .iter_mut()
        .find(|reaction| reaction.emoji == emoji)
    {
        if !reaction.me {
            reaction.count = reaction.count.saturating_add(1);
        }
        reaction.me = true;
    } else {
        message.reactions.push(ReactionInfo {
            emoji,
            count: 1,
            me: true,
        });
    }
}

fn remove_reaction_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    emoji: &super::ReactionEmoji,
) {
    let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
        return;
    };
    if let Some(reaction) = message
        .reactions
        .iter_mut()
        .find(|reaction| &reaction.emoji == emoji)
    {
        if reaction.me {
            reaction.count = reaction.count.saturating_sub(1);
        }
        reaction.me = false;
    }
    message.reactions.retain(|reaction| reaction.count > 0);
}

fn add_gateway_reaction_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    is_current_user: bool,
    emoji: super::ReactionEmoji,
) {
    let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
        return;
    };
    if let Some(reaction) = message
        .reactions
        .iter_mut()
        .find(|reaction| reaction.emoji == emoji)
    {
        if !(is_current_user && reaction.me) {
            reaction.count = reaction.count.saturating_add(1);
        }
        if is_current_user {
            reaction.me = true;
        }
    } else {
        message.reactions.push(ReactionInfo {
            emoji,
            count: 1,
            me: is_current_user,
        });
    }
}

fn remove_gateway_reaction_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    is_current_user: bool,
    emoji: &super::ReactionEmoji,
) {
    let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
        return;
    };
    if let Some(reaction) = message
        .reactions
        .iter_mut()
        .find(|reaction| &reaction.emoji == emoji)
    {
        if !is_current_user || reaction.me {
            reaction.count = reaction.count.saturating_sub(1);
        }
        if is_current_user {
            reaction.me = false;
        }
    }
    message.reactions.retain(|reaction| reaction.count > 0);
}

fn clear_gateway_reactions_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
) {
    let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
        return;
    };
    message.reactions.clear();
}

fn clear_gateway_reaction_emoji_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    emoji: &super::ReactionEmoji,
) {
    let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
        return;
    };
    message
        .reactions
        .retain(|reaction| &reaction.emoji != emoji);
}

fn update_current_user_poll_vote_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    answer_ids: &[u8],
) {
    let Some(poll) = messages
        .iter_mut()
        .find(|message| message.id == message_id)
        .and_then(|message| message.poll.as_mut())
    else {
        return;
    };

    let mut added_votes = 0u64;
    let mut removed_votes = 0u64;
    for answer in &mut poll.answers {
        let next_me_voted = answer_ids.contains(&answer.answer_id);
        match (answer.me_voted, next_me_voted) {
            (false, true) => {
                answer.vote_count = Some(answer.vote_count.unwrap_or(0).saturating_add(1));
                added_votes = added_votes.saturating_add(1);
            }
            (true, false) => {
                answer.vote_count = Some(answer.vote_count.unwrap_or(0).saturating_sub(1));
                removed_votes = removed_votes.saturating_add(1);
            }
            _ => {}
        }
        answer.me_voted = next_me_voted;
    }
    if let Some(total_votes) = &mut poll.total_votes {
        *total_votes = total_votes
            .saturating_add(added_votes)
            .saturating_sub(removed_votes);
    }
}

fn upsert_sorted_message(messages: &mut VecDeque<MessageState>, message: MessageState) {
    let mut by_id: BTreeMap<Id<MessageMarker>, MessageState> = messages
        .drain(..)
        .map(|message| (message.id, message))
        .collect();
    by_id
        .entry(message.id)
        .and_modify(|existing| merge_message(existing, &message))
        .or_insert(message);
    *messages = by_id.into_values().collect();
}

fn merge_message_mentions(existing: &[MentionInfo], incoming: &[MentionInfo]) -> Vec<MentionInfo> {
    if incoming.is_empty() {
        return Vec::new();
    }

    incoming
        .iter()
        .map(|mention| {
            if mention.guild_nick.is_some() {
                mention.clone()
            } else {
                existing
                    .iter()
                    .find(|existing| existing.user_id == mention.user_id)
                    .cloned()
                    .unwrap_or_else(|| mention.clone())
            }
        })
        .collect()
}

fn upsert_member(
    map: &mut BTreeMap<Id<UserMarker>, GuildMemberState>,
    member: &MemberInfo,
    previous_status: Option<PresenceStatus>,
) {
    let status = previous_status.unwrap_or(PresenceStatus::Unknown);
    map.insert(
        member.user_id,
        GuildMemberState {
            user_id: member.user_id,
            display_name: member.display_name.clone(),
            username: member.username.clone(),
            is_bot: member.is_bot,
            avatar_url: member.avatar_url.clone(),
            role_ids: member.role_ids.clone(),
            status,
        },
    );
}

fn role_map(roles: &[RoleInfo]) -> BTreeMap<Id<RoleMarker>, RoleState> {
    roles
        .iter()
        .map(|role| {
            (
                role.id,
                RoleState {
                    id: role.id,
                    name: role.name.clone(),
                    color: role.color,
                    position: role.position,
                    hoist: role.hoist,
                    permissions: role.permissions,
                },
            )
        })
        .collect()
}

fn selected_member_role_color(
    member: &GuildMemberState,
    roles: &BTreeMap<Id<RoleMarker>, RoleState>,
) -> Option<u32> {
    selected_role_ids_color(&member.role_ids, roles)
}

fn selected_role_ids_color(
    role_ids: &[Id<RoleMarker>],
    roles: &BTreeMap<Id<RoleMarker>, RoleState>,
) -> Option<u32> {
    role_ids
        .iter()
        .filter_map(|role_id| roles.get(role_id))
        .filter(|role| role.color.is_some_and(|color| color != 0))
        .min_by(|left, right| role_display_order(left, right))
        .and_then(|role| role.color)
}

fn role_display_order(left: &RoleState, right: &RoleState) -> std::cmp::Ordering {
    right
        .position
        .cmp(&left.position)
        .then(left.id.get().cmp(&right.id.get()))
}

/// Counts of viewable vs. permission-hidden channels for a single scope.
/// Surfaced in the debug-log popup so the user can confirm whether a
/// channel they expected to see is actually being filtered out.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChannelVisibilityStats {
    pub visible: usize,
    pub hidden: usize,
}

#[cfg(test)]
mod tests {
    use crate::discord::ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker},
    };

    use crate::discord::{
        AppEvent, AttachmentUpdate, ChannelInfo, ChannelRecipientInfo, ChannelUnreadState,
        ChannelVisibilityStats, CustomEmojiInfo, DiscordState, FriendStatus, MemberInfo,
        MentionInfo, MessageInfo, MessageKind, MessageReferenceInfo, MessageSnapshotInfo,
        MessageState, MutualGuildInfo, PermissionOverwriteInfo, PermissionOverwriteKind,
        PollAnswerInfo, PollInfo, PresenceStatus, ReactionEmoji, ReactionInfo, ReadStateInfo,
        ReplyInfo, RoleInfo, UserProfileInfo,
    };

    fn profile_info(user_id: u64, guild_nick: Option<&str>) -> UserProfileInfo {
        UserProfileInfo {
            user_id: Id::new(user_id),
            username: format!("user-{user_id}"),
            global_name: None,
            guild_nick: guild_nick.map(str::to_owned),
            role_ids: Vec::new(),
            avatar_url: None,
            bio: None,
            pronouns: None,
            mutual_guilds: Vec::<MutualGuildInfo>::new(),
            mutual_friends_count: 0,
            friend_status: FriendStatus::None,
            note: None,
        }
    }

    #[test]
    fn applies_guild_channels_and_messages() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let message_id = Id::new(3);
        let author_id = Id::new(4);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(state.guilds().len(), 1);
        assert_eq!(state.channels_for_guild(Some(guild_id)).len(), 1);
        assert_eq!(state.messages_for_channel(channel_id).len(), 1);
    }

    #[test]
    fn navigation_snapshot_keeps_sidebar_data_without_message_cache() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let message_id = Id::new(3);
        let author_id = Id::new(4);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(author_id),
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let snapshot = state.navigation_snapshot();

        assert_eq!(snapshot.current_user(), Some("neo"));
        assert_eq!(snapshot.current_user_id(), Some(author_id));
        assert_eq!(snapshot.guilds().len(), 1);
        assert_eq!(snapshot.channels_for_guild(Some(guild_id)).len(), 1);
        assert_eq!(snapshot.messages_for_channel(channel_id).len(), 0);
    }

    #[test]
    fn user_profile_cache_is_scoped_by_guild() {
        let user_id = Id::new(10);
        let guild_a = Id::new(1);
        let guild_b = Id::new(2);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::UserProfileLoaded {
            guild_id: Some(guild_a),
            profile: profile_info(user_id.get(), Some("guild a nick")),
        });
        state.apply_event(&AppEvent::UserProfileLoaded {
            guild_id: Some(guild_b),
            profile: profile_info(user_id.get(), Some("guild b nick")),
        });

        assert_eq!(
            state
                .user_profile(user_id, Some(guild_a))
                .and_then(|profile| profile.guild_nick.as_deref()),
            Some("guild a nick")
        );
        assert_eq!(
            state
                .user_profile(user_id, Some(guild_b))
                .and_then(|profile| profile.guild_nick.as_deref()),
            Some("guild b nick")
        );
        assert!(state.user_profile(user_id, None).is_none());
    }

    #[test]
    fn message_author_uses_cached_member_display_name() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let author_id = Id::new(4);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: Vec::new(),
            }],
            members: vec![MemberInfo {
                user_id: author_id,
                display_name: "server alias".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            }],
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(3),
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].author, "server alias");
    }

    #[test]
    fn member_update_refreshes_existing_message_author() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let author_id = Id::new(4);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(3),
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id,
            member: MemberInfo {
                user_id: author_id,
                display_name: "server alias".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].author, "server alias");
    }

    #[test]
    fn stores_channel_parent_and_position() {
        let guild_id = Id::new(1);
        let category_id = Id::new(2);
        let channel_id = Id::new(3);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: Some(category_id),
            position: Some(7),
            last_message_id: Some(Id::new(9)),
            name: "general".to_owned(),
            kind: "text".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));

        let channel = state.channel(channel_id).unwrap();
        assert_eq!(channel.parent_id, Some(category_id));
        assert_eq!(channel.position, Some(7));
        assert_eq!(channel.last_message_id, Some(Id::new(9)));
    }

    #[test]
    fn channel_upsert_stores_and_preserves_recipients() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "project chat".to_owned(),
            kind: "group-dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: Some("https://cdn.discordapp.com/avatar.png".to_owned()),
                status: Some(PresenceStatus::Online),
            }]),
            permission_overwrites: Vec::new(),
        }));

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(30)),
            name: "renamed project chat".to_owned(),
            kind: "group-dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.name, "renamed project chat");
        assert_eq!(channel.recipients.len(), 1);
        assert_eq!(channel.recipients[0].user_id, Id::new(20));
        assert_eq!(channel.recipients[0].display_name, "alice");
        assert_eq!(
            channel.recipients[0].avatar_url.as_deref(),
            Some("https://cdn.discordapp.com/avatar.png")
        );
        assert_eq!(channel.recipients[0].status, PresenceStatus::Online);
    }

    #[test]
    fn channel_upsert_preserves_recipient_status_when_omitted() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "project chat".to_owned(),
            kind: "group-dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: Some(PresenceStatus::Online),
            }]),
            permission_overwrites: Vec::new(),
        }));

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(30)),
            name: "renamed project chat".to_owned(),
            kind: "group-dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice renamed".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
            permission_overwrites: Vec::new(),
        }));

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.recipients[0].display_name, "alice renamed");
        assert_eq!(channel.recipients[0].status, PresenceStatus::Online);
    }

    #[test]
    fn channel_upsert_defaults_missing_recipient_status_to_unknown() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "alice".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
            permission_overwrites: Vec::new(),
        }));

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.recipients[0].status, PresenceStatus::Unknown);
    }

    #[test]
    fn channel_upsert_uses_cached_user_presence_when_status_is_omitted() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let user_id: Id<UserMarker> = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::UserPresenceUpdate {
            user_id,
            status: PresenceStatus::Idle,
        });
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "test-user".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id,
                display_name: "test-user".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
            permission_overwrites: Vec::new(),
        }));

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.recipients[0].status, PresenceStatus::Idle);
        assert_eq!(state.user_presence(user_id), Some(PresenceStatus::Idle));
    }

    #[test]
    fn user_presence_update_updates_channel_recipients() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "project chat".to_owned(),
            kind: "group-dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
            permission_overwrites: Vec::new(),
        }));

        state.apply_event(&AppEvent::UserPresenceUpdate {
            user_id: Id::new(20),
            status: PresenceStatus::DoNotDisturb,
        });

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.recipients[0].status, PresenceStatus::DoNotDisturb);
    }

    #[test]
    fn guild_presence_update_updates_matching_channel_recipients() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "alice".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
            permission_overwrites: Vec::new(),
        }));

        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id: Id::new(1),
            user_id: Id::new(20),
            status: PresenceStatus::Idle,
        });

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.recipients[0].status, PresenceStatus::Idle);
    }

    #[test]
    fn bounds_messages_per_channel() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::new(1);

        for id in [1, 2] {
            state.apply_event(&AppEvent::MessageCreate {
                guild_id: None,
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                author_role_ids: Vec::new(),
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some(format!("message {id}")),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
                attachments: Vec::new(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id.get(), 2);
    }

    #[test]
    fn stores_message_kind_from_message_create() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::new(19),
            reference: None,
            reply: None,
            poll: None,
            content: Some("reply".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].message_kind, MessageKind::new(19));
    }

    #[test]
    fn duplicate_message_create_refreshes_message_kind() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("cached".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::new(19),
            reference: None,
            reply: None,
            poll: None,
            content: None,
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.as_deref(), Some("cached"));
        assert_eq!(messages[0].message_kind, MessageKind::new(19));
    }

    #[test]
    fn duplicate_message_create_adds_missing_mentions() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            sticker_names: Vec::new(),
            mentions: vec![mention_info(10, "alice")],
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].mentions, vec![mention_info(10, "alice")]);
    }

    #[test]
    fn stores_reply_preview_from_message_create() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::new(19),
            reference: None,
            reply: Some(ReplyInfo {
                author: "Alex".to_owned(),
                content: Some("잘되는군".to_owned()),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
            }),
            poll: None,
            content: Some("asdf".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages[0]
                .reply
                .as_ref()
                .map(|reply| reply.author.as_str()),
            Some("Alex")
        );
        assert_eq!(
            messages[0]
                .reply
                .as_ref()
                .and_then(|reply| reply.content.as_deref()),
            Some("잘되는군")
        );
    }

    #[test]
    fn duplicate_message_create_preserves_cached_reply_preview() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::new(19),
            reference: None,
            reply: Some(ReplyInfo {
                author: "Alex".to_owned(),
                content: Some("잘되는군".to_owned()),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
            }),
            poll: None,
            content: Some("asdf".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::new(19),
            reference: None,
            reply: None,
            poll: None,
            content: None,
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0]
                .reply
                .as_ref()
                .and_then(|reply| reply.content.as_deref()),
            Some("잘되는군")
        );
    }

    #[test]
    fn stores_poll_payload_from_message_create() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info()),
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages[0].poll.as_ref().map(|poll| poll.question.as_str()),
            Some("오늘 뭐 먹지?")
        );
    }

    #[test]
    fn duplicate_message_create_preserves_cached_poll_payload() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info()),
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: None,
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].poll.as_ref().map(|poll| poll.answers.len()),
            Some(2)
        );
    }

    #[test]
    fn message_update_refreshes_cached_poll_results() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info()),
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        let mut updated_poll = poll_info();
        updated_poll.results_finalized = Some(true);
        updated_poll.answers[0].vote_count = Some(5);
        updated_poll.answers[1].vote_count = Some(3);
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            poll: Some(updated_poll),
            content: None,
            sticker_names: None,
            mentions: None,
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            edited_timestamp: None,
        });

        let messages = state.messages_for_channel(channel_id);
        let poll = messages[0].poll.as_ref().expect("poll should stay cached");
        assert_eq!(poll.results_finalized, Some(true));
        assert_eq!(poll.answers[0].vote_count, Some(5));
        assert_eq!(poll.answers[1].vote_count, Some(3));
    }

    #[test]
    fn current_user_poll_vote_update_refreshes_cached_poll_counts() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info()),
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        state.apply_event(&AppEvent::CurrentUserPollVoteUpdate {
            channel_id,
            message_id,
            answer_ids: vec![2],
        });
        let poll = state.messages_for_channel(channel_id)[0]
            .poll
            .as_ref()
            .expect("poll should be cached");
        assert_eq!(poll.answers[0].vote_count, Some(1));
        assert!(!poll.answers[0].me_voted);
        assert_eq!(poll.answers[1].vote_count, Some(2));
        assert!(poll.answers[1].me_voted);
        assert_eq!(poll.total_votes, Some(3));

        state.apply_event(&AppEvent::CurrentUserPollVoteUpdate {
            channel_id,
            message_id,
            answer_ids: Vec::new(),
        });
        let poll = state.messages_for_channel(channel_id)[0]
            .poll
            .as_ref()
            .expect("poll should be cached");
        assert_eq!(poll.answers[0].vote_count, Some(1));
        assert!(!poll.answers[0].me_voted);
        assert_eq!(poll.answers[1].vote_count, Some(1));
        assert!(!poll.answers[1].me_voted);
        assert_eq!(poll.total_votes, Some(2));
    }

    #[test]
    fn current_user_poll_vote_update_handles_missing_answer_counts() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();
        let mut poll = poll_info();
        poll.answers[1].vote_count = None;

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll),
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        state.apply_event(&AppEvent::CurrentUserPollVoteUpdate {
            channel_id,
            message_id,
            answer_ids: vec![2],
        });

        let poll = state.messages_for_channel(channel_id)[0]
            .poll
            .as_ref()
            .expect("poll should be cached");
        assert_eq!(poll.answers[0].vote_count, Some(1));
        assert!(!poll.answers[0].me_voted);
        assert_eq!(poll.answers[1].vote_count, Some(1));
        assert!(poll.answers[1].me_voted);
        assert_eq!(poll.total_votes, Some(3));
    }

    #[test]
    fn message_update_refreshes_mentions_when_present() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            sticker_names: None,
            mentions: Some(vec![mention_info(10, "alice")]),
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            edited_timestamp: None,
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].mentions, vec![mention_info(10, "alice")]);
    }

    #[test]
    fn message_update_preserves_mentions_when_absent() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            sticker_names: Vec::new(),
            mentions: vec![mention_info(10, "alice")],
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            poll: Some(poll_info()),
            content: None,
            sticker_names: None,
            mentions: None,
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            edited_timestamp: None,
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].mentions, vec![mention_info(10, "alice")]);
    }

    #[test]
    fn message_update_clears_mentions_when_present_and_empty() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(99);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            sticker_names: Vec::new(),
            mentions: vec![mention_info(10, "alice")],
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: None,
            mentions: Some(Vec::new()),
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            edited_timestamp: None,
        });

        let messages = state.messages_for_channel(channel_id);
        assert!(messages[0].mentions.is_empty());
    }

    #[test]
    fn message_capabilities_preserve_overlapping_traits() {
        let mut message = message_state("hello");
        assert_eq!(message.capabilities(), Default::default());

        message.attachments = vec![attachment_info(1, "cat.png", "image/png")];
        let capabilities = message.capabilities();
        assert!(capabilities.has_image);
        assert!(!capabilities.has_poll);

        message.poll = Some(poll_info());
        let capabilities = message.capabilities();
        assert!(capabilities.has_image);
        assert!(capabilities.has_poll);
    }

    #[test]
    fn message_capabilities_only_expose_action_facets_for_regular_messages() {
        let mut message = message_state("system body");
        message.message_kind = MessageKind::new(19);
        message.attachments = vec![attachment_info(1, "cat.png", "image/png")];
        message.poll = Some(poll_info());

        let capabilities = message.capabilities();
        assert!(!capabilities.has_poll);
        assert!(!capabilities.has_image);
    }

    #[test]
    fn message_capabilities_track_reply_and_forwarded_traits() {
        let mut message = message_state("reply body");
        message.reply = Some(ReplyInfo {
            author: "neo".to_owned(),
            content: Some("original".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
        });
        message.forwarded_snapshots = vec![snapshot_info("forwarded")];

        let capabilities = message.capabilities();

        assert!(capabilities.is_reply);
        assert!(capabilities.is_forwarded);
    }

    #[test]
    fn keeps_known_content_when_gateway_echo_has_no_content() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let message_id = Id::new(20);
        let author_id = Id::new(30);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: None,
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            poll: None,
            content: None,
            sticker_names: None,
            mentions: None,
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            edited_timestamp: None,
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.as_deref(), Some("hello"));
    }

    #[test]
    fn merges_history_in_chronological_order() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(30),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("live".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![
                message_info(channel_id, 20, "history 20"),
                message_info(channel_id, 10, "history 10"),
            ],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn history_merge_preserves_message_reference() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();
        let reference = MessageReferenceInfo {
            guild_id: Some(Id::new(1)),
            channel_id: Some(Id::new(20)),
            message_id: Some(Id::new(30)),
        };

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                reference: Some(reference.clone()),
                ..message_info(channel_id, 20, "history")
            }],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].reference, Some(reference));
    }

    #[test]
    fn history_dedupes_and_preserves_known_content() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("known".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                pinned: false,
                reactions: Vec::new(),
                content: Some(String::new()),
                ..message_info(channel_id, 20, "")
            }],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.as_deref(), Some("known"));
    }

    #[test]
    fn pinned_messages_loaded_stay_out_of_normal_history() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message_info(channel_id, 20, "latest")],
        });
        state.apply_event(&AppEvent::PinnedMessagesLoaded {
            channel_id,
            messages: vec![message_info(channel_id, 5, "old pin")],
        });

        assert_eq!(
            state
                .messages_for_channel(channel_id)
                .into_iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![20]
        );
        assert_eq!(
            state
                .pinned_messages_for_channel(channel_id)
                .into_iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![5]
        );
    }

    #[test]
    fn pinned_messages_loaded_mark_overlapping_normal_messages() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message_info(channel_id, 20, "normal")],
        });
        state.apply_event(&AppEvent::PinnedMessagesLoaded {
            channel_id,
            messages: vec![message_info(channel_id, 20, "normal")],
        });

        assert!(state.messages_for_channel(channel_id)[0].pinned);
        assert_eq!(state.pinned_messages_for_channel(channel_id).len(), 1);
    }

    #[test]
    fn later_history_preserves_pin_state_from_pinned_cache() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::PinnedMessagesLoaded {
            channel_id,
            messages: vec![message_info(channel_id, 20, "pin")],
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message_info(channel_id, 20, "pin")],
        });

        assert!(state.messages_for_channel(channel_id)[0].pinned);
    }

    #[test]
    fn message_pinned_update_updates_pinned_cache() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message_info(channel_id, 20, "normal")],
        });
        state.apply_event(&AppEvent::MessagePinnedUpdate {
            channel_id,
            message_id: Id::new(20),
            pinned: true,
        });
        assert!(state.messages_for_channel(channel_id)[0].pinned);
        assert_eq!(state.pinned_messages_for_channel(channel_id).len(), 1);

        state.apply_event(&AppEvent::MessagePinnedUpdate {
            channel_id,
            message_id: Id::new(20),
            pinned: false,
        });
        assert!(!state.messages_for_channel(channel_id)[0].pinned);
        assert!(state.pinned_messages_for_channel(channel_id).is_empty());
    }

    #[test]
    fn reaction_events_update_pinned_cache() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();
        let emoji = ReactionEmoji::Unicode("👍".to_owned());

        state.apply_event(&AppEvent::PinnedMessagesLoaded {
            channel_id,
            messages: vec![message_info(channel_id, 20, "pin")],
        });
        state.apply_event(&AppEvent::MessageReactionAdd {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            user_id: Id::new(50),
            emoji: emoji.clone(),
        });

        let pinned = state.pinned_messages_for_channel(channel_id)[0];
        assert_eq!(pinned.reactions.len(), 1);
        assert_eq!(pinned.reactions[0].emoji, emoji);
        assert_eq!(pinned.reactions[0].count, 1);

        state.apply_event(&AppEvent::MessageReactionRemoveAll {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
        });
        assert!(
            state.pinned_messages_for_channel(channel_id)[0]
                .reactions
                .is_empty()
        );
    }

    #[test]
    fn poll_vote_updates_update_pinned_cache() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();
        let mut message = message_info(channel_id, 20, "poll");
        message.poll = Some(poll_info());

        state.apply_event(&AppEvent::PinnedMessagesLoaded {
            channel_id,
            messages: vec![message],
        });
        state.apply_event(&AppEvent::CurrentUserPollVoteUpdate {
            channel_id,
            message_id: Id::new(20),
            answer_ids: vec![2],
        });

        let poll = state.pinned_messages_for_channel(channel_id)[0]
            .poll
            .as_ref()
            .expect("pinned poll should stay cached");
        assert!(!poll.answers[0].me_voted);
        assert_eq!(poll.answers[0].vote_count, Some(1));
        assert!(poll.answers[1].me_voted);
        assert_eq!(poll.answers[1].vote_count, Some(2));
        assert_eq!(poll.total_votes, Some(3));
    }

    #[test]
    fn history_merge_replaces_mentions_from_authoritative_history() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                mentions: vec![mention_info(10, "alice")],
                ..message_info(channel_id, 20, "hello <@10>")
            }],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].mentions, vec![mention_info(10, "alice")]);

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message_info(channel_id, 20, "hello")],
        });

        let messages = state.messages_for_channel(channel_id);
        assert!(messages[0].mentions.is_empty());
    }

    #[test]
    fn history_merge_preserves_richer_gateway_mention_display_name() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            sticker_names: Vec::new(),
            mentions: vec![mention_info(10, "global alias")],
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                mentions: vec![mention_info(10, "username")],
                ..message_info(channel_id, 20, "hello <@10>")
            }],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].mentions, vec![mention_info(10, "global alias")]);
    }

    #[test]
    fn history_merge_clears_reactions_from_authoritative_history() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                reactions: vec![ReactionInfo {
                    emoji: ReactionEmoji::Unicode("👍".to_owned()),
                    count: 2,
                    me: true,
                }],
                ..message_info(channel_id, 20, "hello")
            }],
        });
        assert_eq!(state.messages_for_channel(channel_id)[0].reactions.len(), 1);

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                reactions: Vec::new(),
                ..message_info(channel_id, 20, "hello")
            }],
        });

        assert!(
            state.messages_for_channel(channel_id)[0]
                .reactions
                .is_empty()
        );
    }

    #[test]
    fn stores_and_merges_message_attachments() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: vec![attachment_info(1, "cat.png", "image/png")],
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                pinned: false,
                reactions: Vec::new(),
                content: Some(String::new()),
                attachments: Vec::new(),
                ..message_info(channel_id, 20, "")
            }],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].attachments.len(), 1);
        assert_eq!(messages[0].attachments[0].filename, "cat.png");
    }

    #[test]
    fn stores_forwarded_snapshots_from_message_create() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: vec![snapshot_info("forwarded text")],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].forwarded_snapshots.len(), 1);
        assert_eq!(
            messages[0].forwarded_snapshots[0].content.as_deref(),
            Some("forwarded text")
        );
    }

    #[test]
    fn history_merge_preserves_existing_forwarded_snapshots() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: vec![snapshot_info("live snapshot")],
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message_info(channel_id, 20, "")],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages[0].forwarded_snapshots[0].content.as_deref(),
            Some("live snapshot")
        );
    }

    #[test]
    fn message_update_without_attachments_payload_keeps_cached_attachments() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: vec![attachment_info(1, "cat.png", "image/png")],
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            poll: None,
            content: None,
            sticker_names: None,
            mentions: None,
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            edited_timestamp: None,
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages[0].attachments.len(), 1);
        assert_eq!(messages[0].attachments[0].filename, "cat.png");
    }

    #[test]
    fn message_update_can_clear_attachments_when_payload_includes_them() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: vec![attachment_info(1, "cat.png", "image/png")],
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            poll: None,
            content: None,
            sticker_names: None,
            mentions: None,
            attachments: AttachmentUpdate::Replace(Vec::new()),
            embeds: None,
            edited_timestamp: None,
        });

        let messages = state.messages_for_channel(channel_id);
        assert!(messages[0].attachments.is_empty());
    }

    #[test]
    fn history_respects_message_limit_after_merge() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::new(2);

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![
                message_info(channel_id, 10, "old"),
                message_info(channel_id, 20, "middle"),
                message_info(channel_id, 30, "new"),
            ],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![20, 30]
        );
    }

    #[test]
    fn older_history_preserves_existing_messages_when_message_limit_is_reached() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::new(3);

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![
                message_info(channel_id, 10, "old"),
                message_info(channel_id, 11, "middle"),
                message_info(channel_id, 12, "new"),
            ],
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: Some(Id::new(10)),
            messages: vec![message_info(channel_id, 5, "older")],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![5, 10, 11, 12]
        );
    }

    #[test]
    fn older_history_is_bounded_by_extra_window() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::new(3);

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![
                message_info(channel_id, 10, "old"),
                message_info(channel_id, 11, "middle"),
                message_info(channel_id, 12, "new"),
            ],
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: Some(Id::new(10)),
            messages: vec![
                message_info(channel_id, 1, "older 1"),
                message_info(channel_id, 2, "older 2"),
                message_info(channel_id, 3, "older 3"),
                message_info(channel_id, 4, "older 4"),
                message_info(channel_id, 5, "older 5"),
            ],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 6);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 10]
        );
    }

    #[test]
    fn live_message_after_older_history_keeps_newer_window() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::new(4);

        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![
                message_info(channel_id, 10, "old"),
                message_info(channel_id, 11, "middle"),
                message_info(channel_id, 12, "new"),
            ],
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: Some(Id::new(10)),
            messages: vec![message_info(channel_id, 5, "older")],
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(13),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("newest".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.id.get())
                .collect::<Vec<_>>(),
            vec![10, 11, 12, 13]
        );
    }

    #[test]
    fn live_messages_update_channel_last_message_id() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(20)),
            name: "neo".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(30),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("new".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(10),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("old".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(
            state
                .channel(channel_id)
                .and_then(|channel| channel.last_message_id),
            Some(Id::new(30))
        );
    }

    #[test]
    fn live_thread_messages_increment_cached_counts_once() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id,
            parent_id: Some(Id::new(2)),
            position: None,
            last_message_id: None,
            name: "release notes".to_owned(),
            kind: "thread".to_owned(),
            message_count: Some(12),
            total_message_sent: Some(14),
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        for _ in 0..2 {
            state.apply_event(&AppEvent::MessageCreate {
                guild_id: Some(Id::new(1)),
                channel_id,
                message_id: Id::new(30),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                author_role_ids: Vec::new(),
                message_kind: MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some("new".to_owned()),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
                attachments: Vec::new(),
                embeds: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: Some(Id::new(30)),
            messages: vec![message_info(channel_id, 20, "old")],
        });

        let channel = state
            .channel(channel_id)
            .expect("thread should stay cached");
        assert_eq!(channel.message_count, Some(13));
        assert_eq!(channel.total_message_sent, Some(15));
    }

    #[test]
    fn history_updates_channel_last_message_id() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(20)),
            name: "neo".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![
                message_info(channel_id, 10, "old"),
                message_info(channel_id, 40, "new"),
            ],
        });

        assert_eq!(
            state
                .channel(channel_id)
                .and_then(|channel| channel.last_message_id),
            Some(Id::new(40))
        );
    }

    #[test]
    fn channel_upsert_does_not_regress_last_message_id() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(30)),
            name: "neo".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "neo renamed".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(20)),
            name: "neo renamed again".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));

        let channel = state.channel(channel_id).unwrap();
        assert_eq!(channel.name, "neo renamed again");
        assert_eq!(channel.last_message_id, Some(Id::new(30)));
    }

    #[test]
    fn channel_delete_removes_cached_thread() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(guild_id),
            channel_id,
            parent_id: Some(Id::new(2)),
            position: None,
            last_message_id: None,
            name: "release notes".to_owned(),
            kind: "thread".to_owned(),
            message_count: Some(12),
            total_message_sent: Some(14),
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        state.apply_event(&AppEvent::ChannelDelete {
            guild_id: Some(guild_id),
            channel_id,
        });

        assert_eq!(state.channel(channel_id), None);
    }

    #[test]
    fn tracks_members_and_presences() {
        let guild_id = Id::new(1);
        let alice = Id::new(10);
        let bob = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: Some(100),
            channels: Vec::new(),
            members: vec![
                MemberInfo {
                    user_id: alice,
                    display_name: "alice".to_owned(),
                    username: None,
                    is_bot: false,
                    avatar_url: None,
                    role_ids: Vec::new(),
                },
                MemberInfo {
                    user_id: bob,
                    display_name: "bob".to_owned(),
                    username: None,
                    is_bot: false,
                    avatar_url: None,
                    role_ids: Vec::new(),
                },
            ],
            presences: vec![(alice, PresenceStatus::Online)],
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });

        let members = state.members_for_guild(guild_id);
        assert_eq!(state.guild(guild_id).unwrap().member_count, Some(100));
        assert_eq!(members.len(), 2);
        let alice_state = members.iter().find(|m| m.user_id == alice).unwrap();
        assert_eq!(alice_state.status, PresenceStatus::Online);
        let bob_state = members.iter().find(|m| m.user_id == bob).unwrap();
        assert_eq!(bob_state.status, PresenceStatus::Unknown);

        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id,
            user_id: bob,
            status: PresenceStatus::Idle,
        });
        assert_eq!(
            state
                .members_for_guild(guild_id)
                .iter()
                .find(|m| m.user_id == bob)
                .unwrap()
                .status,
            PresenceStatus::Idle,
        );
    }

    #[test]
    fn real_member_add_and_remove_update_known_member_count() {
        let guild_id = Id::new(1);
        let alice = Id::new(10);
        let bob = Id::new(20);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: Some(1),
            channels: Vec::new(),
            members: vec![MemberInfo {
                user_id: alice,
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            }],
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });

        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id,
            member: MemberInfo {
                user_id: bob,
                display_name: "bob".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });
        assert_eq!(state.guild(guild_id).unwrap().member_count, Some(1));

        state.apply_event(&AppEvent::GuildMemberAdd {
            guild_id,
            member: MemberInfo {
                user_id: bob,
                display_name: "bob".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });
        assert_eq!(state.guild(guild_id).unwrap().member_count, Some(1));

        state.apply_event(&AppEvent::GuildMemberAdd {
            guild_id,
            member: MemberInfo {
                user_id: Id::new(30),
                display_name: "carol".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });
        assert_eq!(state.guild(guild_id).unwrap().member_count, Some(2));

        state.apply_event(&AppEvent::GuildMemberRemove {
            guild_id,
            user_id: Id::new(30),
        });
        assert_eq!(state.guild(guild_id).unwrap().member_count, Some(1));
    }

    #[test]
    fn guild_member_remove_decrements_known_count_for_unloaded_member() {
        let guild_id = Id::new(1);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: Some(3),
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });

        state.apply_event(&AppEvent::GuildMemberRemove {
            guild_id,
            user_id: Id::new(99),
        });

        assert_eq!(state.guild(guild_id).unwrap().member_count, Some(2));
        assert!(state.members_for_guild(guild_id).is_empty());
    }

    #[test]
    fn guild_create_caches_roles_and_member_role_ids() {
        let guild_id = Id::new(1);
        let role_id = Id::new(90);
        let user_id = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: vec![MemberInfo {
                user_id,
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: vec![role_id],
            }],
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: role_id,
                name: "Admin".to_owned(),
                color: Some(0xFFAA00),
                position: 10,
                hoist: true,
                permissions: 0,
            }],
            emojis: Vec::new(),
            owner_id: None,
        });

        let roles = state.roles_for_guild(guild_id);
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].name, "Admin");
        let members = state.members_for_guild(guild_id);
        assert_eq!(members[0].role_ids, vec![role_id]);
    }

    #[test]
    fn message_author_role_color_uses_history_author_roles_when_member_is_missing() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let message_id = Id::new(3);
        let role_id = Id::new(90);
        let user_id = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: role_id,
                name: "Red".to_owned(),
                color: Some(0xCC0000),
                position: 10,
                hoist: true,
                permissions: 0,
            }],
            emojis: Vec::new(),
            owner_id: None,
        });
        let mut message = message_info(channel_id, message_id.get(), "hello");
        message.guild_id = Some(guild_id);
        message.author_id = user_id;
        message.author_role_ids = vec![role_id];
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message],
        });

        assert_eq!(
            state.message_author_role_color(guild_id, channel_id, message_id, user_id),
            Some(0xCC0000)
        );
    }

    #[test]
    fn message_author_role_color_uses_live_author_roles_when_member_is_missing() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let message_id = Id::new(3);
        let role_id = Id::new(90);
        let user_id = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: role_id,
                name: "Red".to_owned(),
                color: Some(0xCC0000),
                position: 10,
                hoist: true,
                permissions: 0,
            }],
            emojis: Vec::new(),
            owner_id: None,
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id,
            author_id: user_id,
            author: "test-user".to_owned(),
            author_avatar_url: None,
            author_role_ids: vec![role_id],
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(
            state.message_author_role_color(guild_id, channel_id, message_id, user_id),
            Some(0xCC0000)
        );
    }

    #[test]
    fn message_author_role_color_uses_profile_roles_when_message_roles_are_missing() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let message_id = Id::new(3);
        let role_id = Id::new(90);
        let user_id = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: role_id,
                name: "Red".to_owned(),
                color: Some(0xCC0000),
                position: 10,
                hoist: true,
                permissions: 0,
            }],
            emojis: Vec::new(),
            owner_id: None,
        });
        let mut message = message_info(channel_id, message_id.get(), "hello");
        message.guild_id = Some(guild_id);
        message.author_id = user_id;
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message],
        });
        let mut profile = profile_info(user_id.get(), Some("test-user"));
        profile.role_ids = vec![role_id];
        state.apply_event(&AppEvent::UserProfileLoaded {
            guild_id: Some(guild_id),
            profile,
        });

        assert_eq!(
            state.message_author_role_color(guild_id, channel_id, message_id, user_id),
            Some(0xCC0000)
        );
    }

    #[test]
    fn message_author_role_color_does_not_use_message_roles_when_member_is_cached() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let message_id = Id::new(3);
        let stale_role_id = Id::new(90);
        let user_id = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: vec![MemberInfo {
                user_id,
                display_name: "test-user".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            }],
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: stale_role_id,
                name: "Old Red".to_owned(),
                color: Some(0xCC0000),
                position: 10,
                hoist: true,
                permissions: 0,
            }],
            emojis: Vec::new(),
            owner_id: None,
        });
        let mut message = message_info(channel_id, message_id.get(), "hello");
        message.guild_id = Some(guild_id);
        message.author_id = user_id;
        message.author_role_ids = vec![stale_role_id];
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message],
        });

        assert_eq!(
            state.message_author_role_color(guild_id, channel_id, message_id, user_id),
            None
        );
    }

    #[test]
    fn chunk_style_member_upserts_populate_member_list() {
        let guild_id = Id::new(1);
        let alice = Id::new(10);
        let bob = Id::new(20);
        let mut state = DiscordState::default();

        for (user_id, display_name) in [(alice, "alice"), (bob, "bob")] {
            state.apply_event(&AppEvent::GuildMemberUpsert {
                guild_id,
                member: MemberInfo {
                    user_id,
                    display_name: display_name.to_owned(),
                    username: None,
                    is_bot: false,
                    avatar_url: None,
                    role_ids: Vec::new(),
                },
            });
        }
        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id,
            user_id: alice,
            status: PresenceStatus::Online,
        });

        let members = state.members_for_guild(guild_id);
        assert_eq!(members.len(), 2);
        assert_eq!(
            members
                .iter()
                .find(|member| member.user_id == alice)
                .map(|member| member.status),
            Some(PresenceStatus::Online)
        );
        assert_eq!(
            members
                .iter()
                .find(|member| member.user_id == bob)
                .map(|member| member.status),
            Some(PresenceStatus::Unknown)
        );
    }

    #[test]
    fn stores_and_clears_custom_guild_emojis() {
        let guild_id = Id::new(1);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: vec![CustomEmojiInfo {
                id: Id::new(50),
                name: "party".to_owned(),
                animated: true,
                available: true,
            }],
            owner_id: None,
        });

        assert_eq!(state.custom_emojis_for_guild(guild_id).len(), 1);
        assert_eq!(state.custom_emojis_for_guild(guild_id)[0].name, "party");

        state.apply_event(&AppEvent::GuildDelete { guild_id });

        assert!(state.custom_emojis_for_guild(guild_id).is_empty());
    }

    #[test]
    fn guild_emojis_update_replaces_cached_custom_emojis() {
        let guild_id = Id::new(1);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: vec![CustomEmojiInfo {
                id: Id::new(50),
                name: "party".to_owned(),
                animated: false,
                available: true,
            }],
            owner_id: None,
        });
        state.apply_event(&AppEvent::GuildEmojisUpdate {
            guild_id,
            emojis: vec![CustomEmojiInfo {
                id: Id::new(60),
                name: "wave".to_owned(),
                animated: true,
                available: true,
            }],
        });

        let emojis = state.custom_emojis_for_guild(guild_id);
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0].id, Id::new(60));
        assert_eq!(emojis[0].name, "wave");
        assert!(emojis[0].animated);
    }

    #[test]
    fn guild_update_replaces_custom_emojis_when_field_is_present() {
        let guild_id = Id::new(1);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: vec![CustomEmojiInfo {
                id: Id::new(50),
                name: "party".to_owned(),
                animated: false,
                available: true,
            }],
            owner_id: None,
        });
        state.apply_event(&AppEvent::GuildUpdate {
            guild_id,
            name: "guild renamed".to_owned(),
            roles: None,
            emojis: Some(vec![CustomEmojiInfo {
                id: Id::new(70),
                name: "dance".to_owned(),
                animated: true,
                available: true,
            }]),
            owner_id: None,
        });

        let emojis = state.custom_emojis_for_guild(guild_id);
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0].id, Id::new(70));
        assert_eq!(emojis[0].name, "dance");
    }

    #[test]
    fn guild_update_without_emoji_field_keeps_cached_custom_emojis() {
        let guild_id = Id::new(1);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: vec![CustomEmojiInfo {
                id: Id::new(50),
                name: "party".to_owned(),
                animated: false,
                available: true,
            }],
            owner_id: None,
        });
        state.apply_event(&AppEvent::GuildUpdate {
            guild_id,
            name: "guild renamed".to_owned(),
            roles: None,
            emojis: None,
            owner_id: None,
        });

        let emojis = state.custom_emojis_for_guild(guild_id);
        assert_eq!(emojis.len(), 1);
        assert_eq!(emojis[0].name, "party");
    }

    #[test]
    fn member_upsert_preserves_existing_status() {
        let guild_id = Id::new(1);
        let user = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id,
            member: MemberInfo {
                user_id: user,
                display_name: "alice".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });
        state.apply_event(&AppEvent::PresenceUpdate {
            guild_id,
            user_id: user,
            status: PresenceStatus::Online,
        });
        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id,
            member: MemberInfo {
                user_id: user,
                display_name: "alice-renamed".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });

        let member = state
            .members_for_guild(guild_id)
            .into_iter()
            .find(|m| m.user_id == user)
            .unwrap();
        assert_eq!(member.display_name, "alice-renamed");
        assert_eq!(member.status, PresenceStatus::Online);
    }

    #[test]
    fn current_user_reaction_events_update_cached_reaction_summary() {
        let mut state = DiscordState::default();
        let channel_id = Id::new(2);
        let message_id = Id::new(1);
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        state.apply_event(&AppEvent::CurrentUserReactionAdd {
            channel_id,
            message_id,
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
        });
        let message = state.messages_for_channel(channel_id)[0];
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(message.reactions[0].count, 1);
        assert!(message.reactions[0].me);

        state.apply_event(&AppEvent::CurrentUserReactionRemove {
            channel_id,
            message_id,
            emoji: ReactionEmoji::Unicode("👍".to_owned()),
        });
        assert!(
            state.messages_for_channel(channel_id)[0]
                .reactions
                .is_empty()
        );
    }

    #[test]
    fn gateway_reaction_events_update_cached_reaction_summary() {
        let mut state = DiscordState::default();
        let channel_id = Id::new(2);
        let message_id = Id::new(1);
        let emoji = ReactionEmoji::Unicode("👍".to_owned());
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        state.apply_event(&AppEvent::MessageReactionAdd {
            guild_id: None,
            channel_id,
            message_id,
            user_id: Id::new(50),
            emoji: emoji.clone(),
        });
        state.apply_event(&AppEvent::MessageReactionAdd {
            guild_id: None,
            channel_id,
            message_id,
            user_id: Id::new(51),
            emoji: emoji.clone(),
        });

        let message = state.messages_for_channel(channel_id)[0];
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(message.reactions[0].count, 2);
        assert!(!message.reactions[0].me);

        state.apply_event(&AppEvent::MessageReactionRemove {
            guild_id: None,
            channel_id,
            message_id,
            user_id: Id::new(50),
            emoji,
        });

        let message = state.messages_for_channel(channel_id)[0];
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(message.reactions[0].count, 1);
        assert!(!message.reactions[0].me);
    }

    #[test]
    fn current_user_gateway_reaction_events_reconcile_optimistic_updates() {
        let mut state = DiscordState::default();
        let channel_id = Id::new(2);
        let message_id = Id::new(1);
        let current_user_id = Id::new(7);
        let emoji = ReactionEmoji::Unicode("👍".to_owned());
        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(current_user_id),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        state.apply_event(&AppEvent::CurrentUserReactionAdd {
            channel_id,
            message_id,
            emoji: emoji.clone(),
        });
        state.apply_event(&AppEvent::MessageReactionAdd {
            guild_id: None,
            channel_id,
            message_id,
            user_id: current_user_id,
            emoji: emoji.clone(),
        });
        let message = state.messages_for_channel(channel_id)[0];
        assert_eq!(message.reactions[0].count, 1);
        assert!(message.reactions[0].me);

        state.apply_event(&AppEvent::MessageReactionAdd {
            guild_id: None,
            channel_id,
            message_id,
            user_id: Id::new(50),
            emoji: emoji.clone(),
        });
        state.apply_event(&AppEvent::CurrentUserReactionRemove {
            channel_id,
            message_id,
            emoji: emoji.clone(),
        });
        state.apply_event(&AppEvent::MessageReactionRemove {
            guild_id: None,
            channel_id,
            message_id,
            user_id: current_user_id,
            emoji,
        });

        let message = state.messages_for_channel(channel_id)[0];
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(message.reactions[0].count, 1);
        assert!(!message.reactions[0].me);
    }

    #[test]
    fn gateway_reaction_clear_events_update_cached_reaction_summary() {
        let mut state = DiscordState::default();
        let channel_id = Id::new(2);
        let message_id = Id::new(1);
        let thumbs_up = ReactionEmoji::Unicode("👍".to_owned());
        let party = ReactionEmoji::Unicode("🎉".to_owned());
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![MessageInfo {
                reactions: vec![
                    ReactionInfo {
                        emoji: thumbs_up.clone(),
                        count: 2,
                        me: true,
                    },
                    ReactionInfo {
                        emoji: party,
                        count: 1,
                        me: false,
                    },
                ],
                ..message_info(channel_id, message_id.get(), "hello")
            }],
        });

        state.apply_event(&AppEvent::MessageReactionRemoveEmoji {
            guild_id: None,
            channel_id,
            message_id,
            emoji: thumbs_up,
        });

        let message = state.messages_for_channel(channel_id)[0];
        assert_eq!(message.reactions.len(), 1);
        assert_eq!(
            message.reactions[0].emoji,
            ReactionEmoji::Unicode("🎉".to_owned())
        );

        state.apply_event(&AppEvent::MessageReactionRemoveAll {
            guild_id: None,
            channel_id,
            message_id,
        });

        assert!(
            state.messages_for_channel(channel_id)[0]
                .reactions
                .is_empty()
        );
    }

    const VIEW_CHANNEL: u64 = 0x0000_0000_0000_0400;
    const SEND_MESSAGES: u64 = 0x0000_0000_0000_0800;
    const ATTACH_FILES: u64 = 0x0000_0000_0000_8000;
    const ADMINISTRATOR: u64 = 0x0000_0000_0000_0008;

    fn perm_role(id: u64, allow: u64, deny: u64) -> PermissionOverwriteInfo {
        PermissionOverwriteInfo {
            id,
            kind: PermissionOverwriteKind::Role,
            allow,
            deny,
        }
    }

    fn perm_member(id: u64, allow: u64, deny: u64) -> PermissionOverwriteInfo {
        PermissionOverwriteInfo {
            id,
            kind: PermissionOverwriteKind::Member,
            allow,
            deny,
        }
    }

    /// Build a single-guild state with one text channel, one member, and the
    /// given role permissions / channel overwrites. The current user is set
    /// from `READY` so permission lookups have an identity to consult.
    fn guild_with_permissions(
        owner_id: Id<UserMarker>,
        my_id: Id<UserMarker>,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        my_role_ids: Vec<Id<RoleMarker>>,
        roles: Vec<RoleInfo>,
        overwrites: Vec<PermissionOverwriteInfo>,
    ) -> DiscordState {
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(my_id),
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: Some(1),
            owner_id: Some(owner_id),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: Some(0),
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: overwrites,
            }],
            members: vec![MemberInfo {
                user_id: my_id,
                display_name: "me".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: my_role_ids,
            }],
            presences: Vec::new(),
            roles,
            emojis: Vec::new(),
        });
        state
    }

    #[test]
    fn dm_channels_are_always_viewable() {
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id: Id::new(99),
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "alice".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        let channels = state.viewable_channels_for_guild(None);
        assert_eq!(channels.len(), 1);
    }

    #[test]
    fn guild_owner_sees_everything_even_when_everyone_denies() {
        let me = Id::new(10);
        let guild = Id::new(1);
        let channel = Id::new(2);
        // @everyone (role id == guild id) explicitly denies VIEW_CHANNEL — but
        // the owner short-circuit must still grant access.
        let state = guild_with_permissions(
            me,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: 0,
            }],
            vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_view_channel(ch));
    }

    #[test]
    fn administrator_role_bypasses_channel_overwrites() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let admin_role = Id::new(50);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![admin_role],
            vec![
                RoleInfo {
                    id: Id::new(guild.get()),
                    name: "@everyone".to_owned(),
                    color: None,
                    position: 0,
                    hoist: false,
                    permissions: 0,
                },
                RoleInfo {
                    id: admin_role,
                    name: "Admin".to_owned(),
                    color: None,
                    position: 1,
                    hoist: false,
                    permissions: ADMINISTRATOR,
                },
            ],
            // Channel-level deny is irrelevant for ADMINISTRATOR holders.
            vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_view_channel(ch));
    }

    #[test]
    fn everyone_deny_hides_channel_for_plain_member() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        // @everyone has VIEW_CHANNEL by default, but the channel-level
        // overwrite revokes it — non-admin, non-owner user cannot see it.
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL,
            }],
            vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(!state.can_view_channel(ch));
        assert!(state.viewable_channels_for_guild(Some(guild)).is_empty());
    }

    #[test]
    fn role_allow_overrides_everyone_deny() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let staff_role = Id::new(50);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![staff_role],
            vec![
                RoleInfo {
                    id: Id::new(guild.get()),
                    name: "@everyone".to_owned(),
                    color: None,
                    position: 0,
                    hoist: false,
                    permissions: VIEW_CHANNEL,
                },
                RoleInfo {
                    id: staff_role,
                    name: "Staff".to_owned(),
                    color: None,
                    position: 1,
                    hoist: false,
                    permissions: 0,
                },
            ],
            vec![
                perm_role(guild.get(), 0, VIEW_CHANNEL),
                perm_role(staff_role.get(), VIEW_CHANNEL, 0),
            ],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_view_channel(ch));
    }

    #[test]
    fn member_overwrite_has_the_final_word() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let staff_role = Id::new(50);
        // Role-level grants VIEW, but the member-specific deny removes it.
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![staff_role],
            vec![
                RoleInfo {
                    id: Id::new(guild.get()),
                    name: "@everyone".to_owned(),
                    color: None,
                    position: 0,
                    hoist: false,
                    permissions: 0,
                },
                RoleInfo {
                    id: staff_role,
                    name: "Staff".to_owned(),
                    color: None,
                    position: 1,
                    hoist: false,
                    permissions: VIEW_CHANNEL,
                },
            ],
            vec![perm_member(me.get(), 0, VIEW_CHANNEL)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(!state.can_view_channel(ch));
    }

    #[test]
    fn threads_inherit_parent_permission() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let parent = Id::new(2);
        let thread = Id::new(3);
        // Parent denies VIEW_CHANNEL — the thread (which carries no overwrites
        // of its own) must inherit the same answer.
        let mut state = guild_with_permissions(
            owner,
            me,
            guild,
            parent,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL,
            }],
            vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
        );
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(guild),
            channel_id: thread,
            parent_id: Some(parent),
            position: None,
            last_message_id: None,
            name: "design-discussion".to_owned(),
            kind: "GuildPublicThread".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        let thread_state = state.channel(thread).expect("thread");
        assert!(!state.can_view_channel(thread_state));
    }

    #[test]
    fn message_create_for_hidden_channel_does_not_promote_it() {
        // Regression guard: a MESSAGE_CREATE for a permission-hidden channel
        // must not flip the channel into the visible bucket. The message
        // itself is still tracked (it's a real Discord message), but the
        // sidebar must keep filtering the channel out and the visibility
        // stats must continue to count it as hidden.
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let mut state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL,
            }],
            vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
        );

        // Sanity check: starts hidden.
        assert_eq!(
            state.channel_visibility_stats(Some(guild)),
            ChannelVisibilityStats {
                visible: 0,
                hidden: 1,
            }
        );
        assert!(state.viewable_channels_for_guild(Some(guild)).is_empty());

        // A message arrives for the hidden channel — same author as a
        // legitimate Discord push.
        let message_id = Id::new(900);
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild),
            channel_id: channel,
            message_id,
            author_id: owner,
            author: "owner".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::default(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hidden chatter".to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        // Channel must remain hidden — no permission promotion happened.
        assert!(state.viewable_channels_for_guild(Some(guild)).is_empty());
        assert_eq!(
            state.channel_visibility_stats(Some(guild)),
            ChannelVisibilityStats {
                visible: 0,
                hidden: 1,
            }
        );
        // The underlying channel record still exists and the message was
        // stored — gating is a sidebar concern, not a data-purge concern.
        assert!(state.channel(channel).is_some());
        assert_eq!(state.messages_for_channel(channel).len(), 1);
    }

    #[test]
    fn cannot_send_when_role_overwrite_denies_send_messages() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                // VIEW + SEND globally, but channel overwrite revokes SEND.
                permissions: VIEW_CHANNEL | SEND_MESSAGES,
            }],
            vec![perm_role(guild.get(), 0, SEND_MESSAGES)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_view_channel(ch));
        assert!(!state.can_send_in_channel(ch));
    }

    #[test]
    fn cannot_send_when_view_channel_is_denied() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL | SEND_MESSAGES,
            }],
            vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(!state.can_view_channel(ch));
        assert!(!state.can_send_in_channel(ch));
    }

    #[test]
    fn cannot_attach_when_role_overwrite_denies_attach_files() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                // VIEW + SEND + ATTACH globally, channel revokes only ATTACH.
                permissions: VIEW_CHANNEL | SEND_MESSAGES | ATTACH_FILES,
            }],
            vec![perm_role(guild.get(), 0, ATTACH_FILES)],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_send_in_channel(ch));
        assert!(!state.can_attach_in_channel(ch));
    }

    #[test]
    fn cannot_attach_when_send_messages_is_missing() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let state = guild_with_permissions(
            owner,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL | ATTACH_FILES,
            }],
            Vec::new(),
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_view_channel(ch));
        assert!(!state.can_send_in_channel(ch));
        assert!(!state.can_attach_in_channel(ch));
    }

    #[test]
    fn owner_can_send_and_attach_unconditionally() {
        let me = Id::new(10);
        let guild = Id::new(1);
        let channel = Id::new(2);
        let state = guild_with_permissions(
            me,
            me,
            guild,
            channel,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: 0,
            }],
            vec![perm_role(
                guild.get(),
                0,
                VIEW_CHANNEL | SEND_MESSAGES | ATTACH_FILES,
            )],
        );
        let ch = state.channel(channel).expect("channel");
        assert!(state.can_send_in_channel(ch));
        assert!(state.can_attach_in_channel(ch));
    }

    #[test]
    fn private_threads_are_hidden_without_membership_state() {
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let parent = Id::new(2);
        let thread = Id::new(3);
        let mut state = guild_with_permissions(
            owner,
            me,
            guild,
            parent,
            vec![],
            vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL | SEND_MESSAGES,
            }],
            Vec::new(),
        );
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(guild),
            channel_id: thread,
            parent_id: Some(parent),
            position: None,
            last_message_id: None,
            name: "private planning".to_owned(),
            kind: "GuildPrivateThread".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));
        let thread_state = state.channel(thread).expect("thread");
        assert!(!state.can_view_channel(thread_state));
        assert!(!state.can_send_in_channel(thread_state));
    }

    #[test]
    fn private_threads_are_hidden_while_permission_state_is_missing() {
        let guild = Id::new(1);
        let thread = Id::new(3);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(guild),
            channel_id: thread,
            parent_id: Some(Id::new(2)),
            position: None,
            last_message_id: None,
            name: "private planning".to_owned(),
            kind: "GuildPrivateThread".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: Some(false),
            thread_locked: Some(false),
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }));

        let thread_state = state.channel(thread).expect("thread");
        assert!(!state.can_view_channel(thread_state));
    }

    #[test]
    fn channel_visibility_stats_count_only_top_level() {
        // Threads should not skew the stats — the user navigates by channel,
        // and a thread under a hidden parent already inherits the parent's
        // visibility.
        let me = Id::new(10);
        let owner = Id::new(11);
        let guild = Id::new(1);
        let visible_channel = Id::new(2);
        let hidden_channel = Id::new(3);
        let visible_thread = Id::new(20);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::Ready {
            user: "me".to_owned(),
            user_id: Some(me),
        });
        state.apply_event(&AppEvent::GuildCreate {
            guild_id: guild,
            name: "guild".to_owned(),
            member_count: Some(1),
            owner_id: Some(owner),
            channels: vec![
                ChannelInfo {
                    guild_id: Some(guild),
                    channel_id: visible_channel,
                    parent_id: None,
                    position: Some(0),
                    last_message_id: None,
                    name: "general".to_owned(),
                    kind: "GuildText".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    thread_pinned: None,
                    recipients: None,
                    permission_overwrites: Vec::new(),
                },
                ChannelInfo {
                    guild_id: Some(guild),
                    channel_id: hidden_channel,
                    parent_id: None,
                    position: Some(1),
                    last_message_id: None,
                    name: "secret".to_owned(),
                    kind: "GuildText".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    thread_pinned: None,
                    recipients: None,
                    permission_overwrites: vec![perm_role(guild.get(), 0, VIEW_CHANNEL)],
                },
                ChannelInfo {
                    guild_id: Some(guild),
                    channel_id: visible_thread,
                    parent_id: Some(visible_channel),
                    position: None,
                    last_message_id: None,
                    name: "design".to_owned(),
                    kind: "GuildPublicThread".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: Some(false),
                    thread_locked: Some(false),
                    thread_pinned: None,
                    recipients: None,
                    permission_overwrites: Vec::new(),
                },
            ],
            members: vec![MemberInfo {
                user_id: me,
                display_name: "me".to_owned(),
                username: None,
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            }],
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: Id::new(guild.get()),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL,
            }],
            emojis: Vec::new(),
        });

        let stats = state.channel_visibility_stats(Some(guild));
        assert_eq!(
            stats,
            ChannelVisibilityStats {
                visible: 1,
                hidden: 1,
            },
            "expected the thread to be excluded from both buckets"
        );
    }

    #[test]
    fn missing_current_user_id_falls_back_to_visible() {
        // Until READY arrives we cannot decide — be permissive so the sidebar
        // is not empty during the brief window between connect and READY.
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::GuildCreate {
            guild_id: Id::new(1),
            name: "guild".to_owned(),
            member_count: None,
            owner_id: Some(Id::new(99)),
            channels: vec![ChannelInfo {
                guild_id: Some(Id::new(1)),
                channel_id: Id::new(2),
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                thread_pinned: None,
                recipients: None,
                permission_overwrites: vec![perm_role(1, 0, VIEW_CHANNEL)],
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: vec![RoleInfo {
                id: Id::new(1),
                name: "@everyone".to_owned(),
                color: None,
                position: 0,
                hoist: false,
                permissions: VIEW_CHANNEL,
            }],
            emojis: Vec::new(),
        });
        let ch = state.channel(Id::new(2)).expect("channel");
        assert!(state.can_view_channel(ch));
    }

    fn message_info(channel_id: Id<ChannelMarker>, message_id: u64, content: &str) -> MessageInfo {
        MessageInfo {
            guild_id: None,
            channel_id,
            message_id: Id::new(message_id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            author_role_ids: Vec::new(),
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some(content.to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
            ..MessageInfo::default()
        }
    }

    fn message_state(content: &str) -> MessageState {
        MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some(content.to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
            ..MessageState::default()
        }
    }

    fn attachment_info(
        id: u64,
        filename: &str,
        content_type: &str,
    ) -> crate::discord::AttachmentInfo {
        crate::discord::AttachmentInfo {
            id: Id::new(id),
            filename: filename.to_owned(),
            url: format!("https://cdn.discordapp.com/{filename}"),
            proxy_url: format!("https://media.discordapp.net/{filename}"),
            content_type: Some(content_type.to_owned()),
            size: 1000,
            width: Some(100),
            height: Some(100),
            description: None,
        }
    }

    fn mention_info(user_id: u64, display_name: &str) -> MentionInfo {
        MentionInfo {
            user_id: Id::new(user_id),
            guild_nick: None,
            display_name: display_name.to_owned(),
        }
    }

    fn poll_info() -> PollInfo {
        PollInfo {
            question: "오늘 뭐 먹지?".to_owned(),
            answers: vec![
                PollAnswerInfo {
                    answer_id: 1,
                    text: "김치찌개".to_owned(),
                    vote_count: Some(2),
                    me_voted: true,
                },
                PollAnswerInfo {
                    answer_id: 2,
                    text: "라멘".to_owned(),
                    vote_count: Some(1),
                    me_voted: false,
                },
            ],
            allow_multiselect: false,
            results_finalized: Some(false),
            total_votes: Some(3),
        }
    }

    fn snapshot_info(content: &str) -> MessageSnapshotInfo {
        MessageSnapshotInfo {
            content: Some(content.to_owned()),
            sticker_names: Vec::new(),
            mentions: Vec::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            source_channel_id: None,
            timestamp: None,
        }
    }

    fn channel_with_last_message(
        channel_id: Id<ChannelMarker>,
        last_message_id: u64,
    ) -> ChannelInfo {
        ChannelInfo {
            guild_id: Some(Id::new(1)),
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(last_message_id)),
            name: "general".to_owned(),
            kind: "text".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            thread_pinned: None,
            recipients: None,
            permission_overwrites: Vec::new(),
        }
    }

    #[test]
    fn channel_with_no_ack_pointer_is_unread_when_messages_exist() {
        let channel_id = Id::new(7);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            channel_id, 100,
        )));

        // No ReadStateInit at all — Discord never told us about this channel,
        // so any non-empty channel should light up as unread.
        assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Unread);
    }

    #[test]
    fn channel_with_ack_pointer_below_latest_is_unread() {
        let channel_id = Id::new(7);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            channel_id, 200,
        )));
        state.apply_event(&AppEvent::ReadStateInit {
            entries: vec![ReadStateInfo {
                channel_id,
                last_acked_message_id: Some(Id::new(150)),
                mention_count: 0,
            }],
        });

        assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Unread);
    }

    #[test]
    fn channel_with_ack_at_latest_is_seen() {
        let channel_id = Id::new(7);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            channel_id, 200,
        )));
        state.apply_event(&AppEvent::ReadStateInit {
            entries: vec![ReadStateInfo {
                channel_id,
                last_acked_message_id: Some(Id::new(200)),
                mention_count: 0,
            }],
        });

        assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Seen);
    }

    #[test]
    fn channel_with_pending_mentions_reports_mention_count() {
        let channel_id = Id::new(7);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            channel_id, 200,
        )));
        state.apply_event(&AppEvent::ReadStateInit {
            entries: vec![ReadStateInfo {
                channel_id,
                last_acked_message_id: Some(Id::new(200)),
                mention_count: 3,
            }],
        });

        assert_eq!(
            state.channel_unread(channel_id),
            ChannelUnreadState::Mentioned(3)
        );
    }

    #[test]
    fn message_ack_clears_outstanding_mentions_and_advances_pointer() {
        let channel_id = Id::new(7);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            channel_id, 500,
        )));
        state.apply_event(&AppEvent::ReadStateInit {
            entries: vec![ReadStateInfo {
                channel_id,
                last_acked_message_id: Some(Id::new(100)),
                mention_count: 5,
            }],
        });
        assert_eq!(
            state.channel_unread(channel_id),
            ChannelUnreadState::Mentioned(5)
        );

        state.apply_event(&AppEvent::MessageAck {
            channel_id,
            message_id: Id::new(500),
            mention_count: 0,
        });

        assert_eq!(state.channel_unread(channel_id), ChannelUnreadState::Seen);
        assert_eq!(
            state.channel_ack_target(channel_id),
            None,
            "fully-acked channels need no further ack"
        );
    }

    #[test]
    fn channel_ack_target_returns_latest_when_unread() {
        let channel_id = Id::new(7);
        let mut state = DiscordState::default();
        state.apply_event(&AppEvent::ChannelUpsert(channel_with_last_message(
            channel_id, 500,
        )));

        // No ack pointer at all -> ack target is the channel's last message.
        assert_eq!(state.channel_ack_target(channel_id), Some(Id::new(500)));
    }
}
