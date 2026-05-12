use std::collections::{BTreeMap, VecDeque};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};
use crate::discord::{
    AttachmentInfo, AttachmentUpdate, EmbedInfo, InlinePreviewInfo, MemberInfo, MentionInfo,
    MessageInfo, MessageKind, MessageReferenceInfo, MessageSnapshotInfo, PollInfo, ReactionEmoji,
    ReactionInfo, ReplyInfo,
};

use super::{
    DiscordState, OLDER_HISTORY_EXTRA_WINDOW_MULTIPLIER,
    members::{selected_member_role_color, selected_role_ids_color},
    profiles::UserProfileCacheKey,
};

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

        // Poll and attachment actions are valid for chat messages, including
        // replies. Other non-regular messages can still be rendered as
        // replies/forwards, but subtype-like action facets should not leak
        // onto system messages.
        if !self.message_kind.is_regular_or_reply() {
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

pub(super) type MessageAuthorRoleIds =
    BTreeMap<(Id<ChannelMarker>, Id<MessageMarker>), Vec<Id<RoleMarker>>>;

pub(super) struct MessageUpdateFields {
    pub(super) poll: Option<PollInfo>,
    pub(super) content: Option<String>,
    pub(super) sticker_names: Option<Vec<String>>,
    pub(super) mentions: Option<Vec<MentionInfo>>,
    pub(super) attachments: AttachmentUpdate,
    pub(super) embeds: Option<Vec<EmbedInfo>>,
    pub(super) edited_timestamp: Option<String>,
    pub(super) pinned: Option<bool>,
    pub(super) reactions: Option<Vec<ReactionInfo>>,
}

impl DiscordState {
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

    pub(super) fn message_author_display_name(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        author_id: Id<UserMarker>,
        fallback: &str,
    ) -> String {
        if let Some(member) = guild_id
            .and_then(|guild_id| self.members.get(&guild_id))
            .and_then(|members| members.get(&author_id))
        {
            // Only trust the member entry if it has real name data. When
            // username is None and display_name is "unknown" the entry is a
            // bare fallback — fall through to the profile cache instead.
            if member.username.is_some() || member.display_name != "unknown" {
                return member.display_name.clone();
            }
        }
        self.user_profiles
            .get(&UserProfileCacheKey::new(author_id, guild_id))
            .map(|profile| profile.display_name().to_owned())
            .unwrap_or_else(|| fallback.to_owned())
    }

    pub(super) fn message_author_avatar_url(
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

    fn for_each_cached_message_mut(&mut self, mut update: impl FnMut(&mut MessageState)) {
        for messages in self
            .messages
            .values_mut()
            .chain(self.pinned_messages.values_mut())
        {
            for message in messages {
                update(message);
            }
        }
    }

    fn update_cached_messages_in_channel(
        &mut self,
        channel_id: Id<ChannelMarker>,
        mut update: impl FnMut(&mut VecDeque<MessageState>),
    ) {
        if let Some(messages) = self.messages.get_mut(&channel_id) {
            update(messages);
        }
        if let Some(messages) = self.pinned_messages.get_mut(&channel_id) {
            update(messages);
        }
    }

    pub(super) fn refresh_message_author_display_name(
        &mut self,
        guild_id: Id<GuildMarker>,
        member: &MemberInfo,
    ) {
        // If this member payload is a fallback ("unknown", no username), avoid
        // clobbering messages that already have a real name. Try the profile
        // cache for a better name; if nothing is available, skip the refresh.
        let display_name = if member.username.is_none() && member.display_name == "unknown" {
            match self
                .user_profiles
                .get(&UserProfileCacheKey::new(member.user_id, Some(guild_id)))
            {
                Some(profile) => profile.display_name().to_owned(),
                None => return,
            }
        } else {
            member.display_name.clone()
        };
        let avatar_url = member.avatar_url.clone();

        for messages in self
            .messages
            .values_mut()
            .chain(self.pinned_messages.values_mut())
        {
            for message in messages.iter_mut() {
                if message.guild_id == Some(guild_id) {
                    if message.author_id == member.user_id {
                        message.author = display_name.clone();
                        if avatar_url.is_some() || message.author_avatar_url.is_none() {
                            message.author_avatar_url = avatar_url.clone();
                        }
                    }
                    if let Some(reply) = &mut message.reply {
                        if reply.author_id == Some(member.user_id) {
                            reply.author = display_name.clone();
                        }
                    }
                }
            }
        }
    }

    pub(super) fn refresh_message_author_from_profile(
        &mut self,
        guild_id: Option<Id<GuildMarker>>,
        user_id: Id<UserMarker>,
        display_name: &str,
        avatar_url: Option<&str>,
    ) {
        self.for_each_cached_message_mut(|message| {
            if message.guild_id == guild_id {
                if message.author_id == user_id {
                    message.author = display_name.to_owned();
                    if avatar_url.is_some() || message.author_avatar_url.is_none() {
                        message.author_avatar_url = avatar_url.map(str::to_owned);
                    }
                }
                if let Some(reply) = &mut message.reply {
                    if reply.author_id == Some(user_id) {
                        reply.author = display_name.to_owned();
                    }
                }
            }
        });
    }

    pub(super) fn upsert_message(&mut self, mut message: MessageState) {
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

    pub(super) fn add_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    ) {
        self.update_cached_messages_in_channel(channel_id, |messages| {
            add_reaction_in(messages, message_id, emoji.clone());
        });
    }

    pub(super) fn remove_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) {
        self.update_cached_messages_in_channel(channel_id, |messages| {
            remove_reaction_in(messages, message_id, emoji);
        });
    }

    pub(super) fn add_gateway_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        user_id: Id<UserMarker>,
        emoji: ReactionEmoji,
    ) {
        let is_current_user = self.current_user_id == Some(user_id);
        self.update_cached_messages_in_channel(channel_id, |messages| {
            add_gateway_reaction_in(messages, message_id, is_current_user, emoji.clone());
        });
    }

    pub(super) fn remove_gateway_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        user_id: Id<UserMarker>,
        emoji: &ReactionEmoji,
    ) {
        let is_current_user = self.current_user_id == Some(user_id);
        self.update_cached_messages_in_channel(channel_id, |messages| {
            remove_gateway_reaction_in(messages, message_id, is_current_user, emoji);
        });
    }

    pub(super) fn clear_gateway_reactions(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        self.update_cached_messages_in_channel(channel_id, |messages| {
            clear_gateway_reactions_in(messages, message_id);
        });
    }

    pub(super) fn clear_gateway_reaction_emoji(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) {
        self.update_cached_messages_in_channel(channel_id, |messages| {
            clear_gateway_reaction_emoji_in(messages, message_id, emoji);
        });
    }

    pub(super) fn update_current_user_poll_vote(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        answer_ids: &[u8],
    ) {
        self.update_cached_messages_in_channel(channel_id, |messages| {
            update_current_user_poll_vote_in(messages, message_id, answer_ids);
        });
    }

    pub(super) fn merge_message_history(
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

    pub(super) fn replace_pinned_messages(
        &mut self,
        channel_id: Id<ChannelMarker>,
        pins: &[MessageInfo],
    ) {
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

    pub(super) fn update_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        update: MessageUpdateFields,
    ) {
        self.update_cached_messages_in_channel(channel_id, |messages| {
            update_message_in(messages, message_id, &update);
        });
    }

    pub(super) fn set_cached_message_pinned(
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

    pub(super) fn delete_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        self.update_cached_messages_in_channel(channel_id, |messages| {
            messages.retain(|message| message.id != message_id);
        });
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

    pub(super) fn record_author_role_ids(
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
    emoji: ReactionEmoji,
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
    emoji: &ReactionEmoji,
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
    emoji: ReactionEmoji,
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
    emoji: &ReactionEmoji,
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
    emoji: &ReactionEmoji,
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
