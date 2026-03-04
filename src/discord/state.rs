use std::collections::{BTreeMap, VecDeque};

use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};

use super::{
    AppEvent, AttachmentInfo, AttachmentUpdate, ChannelInfo, ChannelRecipientInfo, CustomEmojiInfo,
    GuildFolder, MemberInfo, MentionInfo, MessageInfo, MessageKind, MessageReferenceInfo,
    MessageSnapshotInfo, PollInfo, PresenceStatus, ReactionInfo, ReplyInfo, RoleInfo,
};

const DEFAULT_MAX_MESSAGES_PER_CHANNEL: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildState {
    pub id: Id<GuildMarker>,
    pub name: String,
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
    pub recipients: Vec<ChannelRecipientState>,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelRecipientState {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    pub is_bot: bool,
    pub avatar_url: Option<String>,
    pub status: PresenceStatus,
}

impl ChannelRecipientState {
    fn from_info(
        recipient: &ChannelRecipientInfo,
        previous_status: Option<PresenceStatus>,
    ) -> Self {
        Self {
            user_id: recipient.user_id,
            display_name: recipient.display_name.clone(),
            is_bot: recipient.is_bot,
            avatar_url: recipient.avatar_url.clone(),
            status: recipient
                .status
                .or(previous_status)
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
    pub mentions: Vec<MentionInfo>,
    pub attachments: Vec<AttachmentInfo>,
    pub forwarded_snapshots: Vec<MessageSnapshotInfo>,
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

        capabilities
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildMemberState {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    pub is_bot: bool,
    pub avatar_url: Option<String>,
    pub role_ids: Vec<Id<RoleMarker>>,
    pub status: PresenceStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleState {
    pub id: Id<RoleMarker>,
    pub name: String,
    pub color: Option<u32>,
    pub position: i64,
    pub hoist: bool,
}

#[derive(Clone, Debug)]
pub struct DiscordState {
    guilds: BTreeMap<Id<GuildMarker>, GuildState>,
    channels: BTreeMap<Id<ChannelMarker>, ChannelState>,
    messages: BTreeMap<Id<ChannelMarker>, VecDeque<MessageState>>,
    members: BTreeMap<Id<GuildMarker>, BTreeMap<Id<UserMarker>, GuildMemberState>>,
    roles: BTreeMap<Id<GuildMarker>, BTreeMap<Id<RoleMarker>, RoleState>>,
    custom_emojis: BTreeMap<Id<GuildMarker>, Vec<CustomEmojiInfo>>,
    /// User's `guild_folders` setting in display order. Empty until READY
    /// delivers it; the dashboard falls back to a flat guild list.
    guild_folders: Vec<GuildFolder>,
    max_messages_per_channel: usize,
}

struct MessageUpdateFields {
    poll: Option<PollInfo>,
    content: Option<String>,
    mentions: Option<Vec<MentionInfo>>,
    attachments: AttachmentUpdate,
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
            members: BTreeMap::new(),
            roles: BTreeMap::new(),
            custom_emojis: BTreeMap::new(),
            guild_folders: Vec::new(),
            max_messages_per_channel,
        }
    }

    pub fn apply_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::GuildCreate {
                guild_id,
                name,
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
                roles,
                emojis,
            } => {
                if let Some(guild) = self.guilds.get_mut(guild_id) {
                    guild.name = name.clone();
                }
                if let Some(roles) = roles {
                    self.roles.insert(*guild_id, role_map(roles));
                }
                if let Some(emojis) = emojis {
                    self.custom_emojis.insert(*guild_id, emojis.clone());
                }
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
                self.members.remove(guild_id);
                self.roles.remove(guild_id);
                self.custom_emojis.remove(guild_id);
            }
            AppEvent::ChannelUpsert(channel) => self.upsert_channel(channel),
            AppEvent::ChannelDelete { channel_id, .. } => {
                self.channels.remove(channel_id);
                self.messages.remove(channel_id);
            }
            AppEvent::MessageCreate {
                guild_id,
                channel_id,
                message_id,
                author_id,
                author,
                author_avatar_url,
                message_kind,
                reference,
                reply,
                poll,
                content,
                mentions,
                attachments,
                forwarded_snapshots,
                ..
            } => self.upsert_message(MessageState {
                id: *message_id,
                guild_id: *guild_id,
                channel_id: *channel_id,
                author_id: *author_id,
                author: self.message_author_display_name(*guild_id, *author_id, author),
                author_avatar_url: self.message_author_avatar_url(
                    *guild_id,
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
                mentions: mentions.clone(),
                attachments: attachments.clone(),
                forwarded_snapshots: forwarded_snapshots.clone(),
            }),
            AppEvent::MessageHistoryLoaded {
                channel_id,
                before,
                messages,
            } => self.merge_message_history(*channel_id, *before, messages),
            AppEvent::MessageHistoryLoadFailed { .. } => {}
            AppEvent::MessageUpdate {
                channel_id,
                message_id,
                poll,
                content,
                mentions,
                attachments,
                ..
            } => self.update_message(
                *channel_id,
                *message_id,
                MessageUpdateFields {
                    poll: poll.clone(),
                    content: content.clone(),
                    mentions: mentions.clone(),
                    attachments: attachments.clone(),
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
            AppEvent::MessagePinnedUpdate {
                channel_id,
                message_id,
                pinned,
            } => self.update_message(
                *channel_id,
                *message_id,
                MessageUpdateFields {
                    poll: None,
                    content: None,
                    mentions: None,
                    attachments: AttachmentUpdate::Unchanged,
                    pinned: Some(*pinned),
                    reactions: None,
                },
            ),
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
            }
            AppEvent::PresenceUpdate {
                guild_id,
                user_id,
                status,
            } => {
                let entry = self.members.entry(*guild_id).or_default();
                if let Some(member) = entry.get_mut(user_id) {
                    member.status = *status;
                } else {
                    entry.insert(
                        *user_id,
                        GuildMemberState {
                            user_id: *user_id,
                            display_name: format!("user-{}", user_id.get()),
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
                self.update_channel_recipient_presence(*user_id, *status);
            }
            AppEvent::GuildFoldersUpdate { folders } => {
                self.guild_folders = folders.clone();
            }
            AppEvent::Ready { .. }
            | AppEvent::GatewayError { .. }
            | AppEvent::StatusMessage { .. }
            | AppEvent::ReactionUsersLoaded { .. }
            | AppEvent::AttachmentPreviewLoaded { .. }
            | AppEvent::AttachmentPreviewLoadFailed { .. }
            | AppEvent::GatewayClosed => {}
        }
    }

    pub fn guild_folders(&self) -> &[GuildFolder] {
        &self.guild_folders
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

    pub fn messages_for_channel(&self, channel_id: Id<ChannelMarker>) -> Vec<&MessageState> {
        self.messages
            .get(&channel_id)
            .map(|messages| messages.iter().collect())
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
                        ChannelRecipientState::from_info(recipient, previous_status)
                    })
                    .collect()
            })
            .or_else(|| existing.map(|existing| existing.recipients.clone()))
            .unwrap_or_default();

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
                recipients,
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

    fn upsert_message(&mut self, message: MessageState) {
        let channel_id = message.channel_id;
        let message_id = message.id;
        let messages = self.messages.entry(message.channel_id).or_default();
        if let Some(existing) = messages.iter_mut().find(|item| item.id == message.id) {
            existing.guild_id = message.guild_id;
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
            existing.pinned = message.pinned;
            existing.reactions = message.reactions;
            if message.content.is_some() {
                existing.content = message.content;
            }
            if !message.mentions.is_empty() || existing.mentions.is_empty() {
                existing.mentions = message.mentions;
            }
            if !message.attachments.is_empty() || existing.attachments.is_empty() {
                existing.attachments = message.attachments;
            }
            if !message.forwarded_snapshots.is_empty() || existing.forwarded_snapshots.is_empty() {
                existing.forwarded_snapshots = message.forwarded_snapshots;
            }
        } else {
            messages.push_back(message);
        }

        while messages.len() > self.max_messages_per_channel {
            messages.pop_front();
        }
        self.record_channel_message_id(channel_id, message_id);
    }

    fn add_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: super::ReactionEmoji,
    ) {
        let Some(message) = self
            .messages
            .get_mut(&channel_id)
            .and_then(|messages| messages.iter_mut().find(|message| message.id == message_id))
        else {
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

    fn remove_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &super::ReactionEmoji,
    ) {
        let Some(message) = self
            .messages
            .get_mut(&channel_id)
            .and_then(|messages| messages.iter_mut().find(|message| message.id == message_id))
        else {
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

    fn update_current_user_poll_vote(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        answer_ids: &[u8],
    ) {
        let Some(poll) = self
            .messages
            .get_mut(&channel_id)
            .and_then(|messages| messages.iter_mut().find(|message| message.id == message_id))
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

    fn merge_message_history(
        &mut self,
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        history: &[MessageInfo],
    ) {
        let incoming_messages = history
            .iter()
            .filter(|message| message.channel_id == channel_id)
            .map(|message| MessageState {
                id: message.message_id,
                guild_id: message.guild_id,
                channel_id: message.channel_id,
                author_id: message.author_id,
                author: self.message_author_display_name(
                    message.guild_id,
                    message.author_id,
                    &message.author,
                ),
                author_avatar_url: self.message_author_avatar_url(
                    message.guild_id,
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
                mentions: message.mentions.clone(),
                attachments: message.attachments.clone(),
                forwarded_snapshots: message.forwarded_snapshots.clone(),
            })
            .collect::<Vec<_>>();
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
        }
        if let Some(last_message_id) = messages.back().map(|message| message.id) {
            self.record_channel_message_id(channel_id, last_message_id);
        }
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
        let messages = self.messages.entry(channel_id).or_default();
        if let Some(existing) = messages.iter_mut().find(|item| item.id == message_id) {
            if let Some(poll) = update.poll {
                existing.poll = Some(poll);
            }
            if let Some(pinned) = update.pinned {
                existing.pinned = pinned;
            }
            if let Some(reactions) = update.reactions {
                existing.reactions = reactions;
            }
            if let Some(content) = update.content {
                existing.content = Some(content);
            }
            if let Some(mentions) = update.mentions {
                existing.mentions = mentions;
            }
            match update.attachments {
                AttachmentUpdate::Replace(attachments) => existing.attachments = attachments,
                AttachmentUpdate::Unchanged => {}
            }
        }
    }

    fn delete_message(&mut self, channel_id: Id<ChannelMarker>, message_id: Id<MessageMarker>) {
        if let Some(messages) = self.messages.get_mut(&channel_id) {
            messages.retain(|message| message.id != message_id);
        }
    }
}

fn merge_message(existing: &mut MessageState, incoming: &MessageState) {
    existing.guild_id = incoming.guild_id;
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
    existing.pinned = incoming.pinned;
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
    existing.mentions = incoming.mentions.clone();
    if !incoming.attachments.is_empty() || existing.attachments.is_empty() {
        existing.attachments = incoming.attachments.clone();
    }
    if !incoming.forwarded_snapshots.is_empty() || existing.forwarded_snapshots.is_empty() {
        existing.forwarded_snapshots = incoming.forwarded_snapshots.clone();
    }
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
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use twilight_model::id::{Id, marker::ChannelMarker};

    use crate::discord::{
        AppEvent, AttachmentUpdate, ChannelInfo, ChannelRecipientInfo, CustomEmojiInfo,
        DiscordState, MemberInfo, MentionInfo, MessageInfo, MessageKind, MessageReferenceInfo,
        MessageSnapshotInfo, MessageState, PollAnswerInfo, PollInfo, PresenceStatus, ReactionEmoji,
        ReactionInfo, ReplyInfo, RoleInfo,
    };

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
                recipients: None,
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(state.guilds().len(), 1);
        assert_eq!(state.channels_for_guild(Some(guild_id)).len(), 1);
        assert_eq!(state.messages_for_channel(channel_id).len(), 1);
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
                recipients: None,
            }],
            members: vec![MemberInfo {
                user_id: author_id,
                display_name: "server alias".to_owned(),
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            }],
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(3),
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::GuildMemberUpsert {
            guild_id,
            member: MemberInfo {
                user_id: author_id,
                display_name: "server alias".to_owned(),
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
            recipients: None,
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
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                is_bot: false,
                avatar_url: Some("https://cdn.discordapp.com/avatar.png".to_owned()),
                status: Some(PresenceStatus::Online),
            }]),
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
            recipients: None,
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
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                is_bot: false,
                avatar_url: None,
                status: Some(PresenceStatus::Online),
            }]),
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
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice renamed".to_owned(),
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
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
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
        }));

        let channel = state.channel(channel_id).expect("channel should be stored");
        assert_eq!(channel.recipients[0].status, PresenceStatus::Unknown);
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
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
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
            recipients: Some(vec![ChannelRecipientInfo {
                user_id: Id::new(20),
                display_name: "alice".to_owned(),
                is_bot: false,
                avatar_url: None,
                status: None,
            }]),
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
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some(format!("message {id}")),
                mentions: Vec::new(),
                attachments: Vec::new(),
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
            message_kind: MessageKind::new(19),
            reference: None,
            reply: None,
            poll: None,
            content: Some("reply".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("cached".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::new(19),
            reference: None,
            reply: None,
            poll: None,
            content: None,
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            mentions: vec![mention_info(10, "alice")],
            attachments: Vec::new(),
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
            message_kind: MessageKind::new(19),
            reference: None,
            reply: Some(ReplyInfo {
                author: "Alex".to_owned(),
                content: Some("잘되는군".to_owned()),
                mentions: Vec::new(),
            }),
            poll: None,
            content: Some("asdf".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: MessageKind::new(19),
            reference: None,
            reply: Some(ReplyInfo {
                author: "Alex".to_owned(),
                content: Some("잘되는군".to_owned()),
                mentions: Vec::new(),
            }),
            poll: None,
            content: Some("asdf".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::new(19),
            reference: None,
            reply: None,
            poll: None,
            content: None,
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info()),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info()),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: None,
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info()),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            mentions: None,
            attachments: AttachmentUpdate::Unchanged,
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
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info()),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            mentions: Some(vec![mention_info(10, "alice")]),
            attachments: AttachmentUpdate::Unchanged,
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
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            mentions: vec![mention_info(10, "alice")],
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            poll: Some(poll_info()),
            content: None,
            mentions: None,
            attachments: AttachmentUpdate::Unchanged,
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
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            mentions: vec![mention_info(10, "alice")],
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            poll: None,
            content: Some("hello".to_owned()),
            mentions: Some(Vec::new()),
            attachments: AttachmentUpdate::Unchanged,
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
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: None,
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            poll: None,
            content: None,
            mentions: None,
            attachments: AttachmentUpdate::Unchanged,
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
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("live".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("known".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello <@10>".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: vec![attachment_info(1, "cat.png", "image/png")],
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
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: vec![attachment_info(1, "cat.png", "image/png")],
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            poll: None,
            content: None,
            mentions: None,
            attachments: AttachmentUpdate::Unchanged,
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
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: vec![attachment_info(1, "cat.png", "image/png")],
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            poll: None,
            content: None,
            mentions: None,
            attachments: AttachmentUpdate::Replace(Vec::new()),
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
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("newest".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            recipients: None,
        }));
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(30),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("new".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(10),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("old".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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
            recipients: None,
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
            recipients: None,
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
            recipients: None,
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
            recipients: None,
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
            recipients: None,
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
            channels: Vec::new(),
            members: vec![
                MemberInfo {
                    user_id: alice,
                    display_name: "alice".to_owned(),
                    is_bot: false,
                    avatar_url: None,
                    role_ids: Vec::new(),
                },
                MemberInfo {
                    user_id: bob,
                    display_name: "bob".to_owned(),
                    is_bot: false,
                    avatar_url: None,
                    role_ids: Vec::new(),
                },
            ],
            presences: vec![(alice, PresenceStatus::Online)],
            roles: Vec::new(),
            emojis: Vec::new(),
        });

        let members = state.members_for_guild(guild_id);
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
    fn guild_create_caches_roles_and_member_role_ids() {
        let guild_id = Id::new(1);
        let role_id = Id::new(90);
        let user_id = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: Vec::new(),
            members: vec![MemberInfo {
                user_id,
                display_name: "alice".to_owned(),
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
            }],
            emojis: Vec::new(),
        });

        let roles = state.roles_for_guild(guild_id);
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].name, "Admin");
        let members = state.members_for_guild(guild_id);
        assert_eq!(members[0].role_ids, vec![role_id]);
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
        });
        state.apply_event(&AppEvent::GuildUpdate {
            guild_id,
            name: "guild renamed".to_owned(),
            roles: None,
            emojis: None,
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
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("hello".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
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

    fn message_info(channel_id: Id<ChannelMarker>, message_id: u64, content: &str) -> MessageInfo {
        MessageInfo {
            guild_id: None,
            channel_id,
            message_id: Id::new(message_id),
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
            forwarded_snapshots: Vec::new(),
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
            forwarded_snapshots: Vec::new(),
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
            mentions: Vec::new(),
            attachments: Vec::new(),
            source_channel_id: None,
            timestamp: None,
        }
    }
}
