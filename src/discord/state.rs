use std::collections::{BTreeMap, VecDeque};

use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use super::{
    AppEvent, AttachmentInfo, AttachmentUpdate, ChannelInfo, GuildFolder, MemberInfo, MessageInfo,
    MessageKind, MessageSnapshotInfo, PresenceStatus, ReplyInfo,
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
}

impl ChannelState {
    pub fn is_category(&self) -> bool {
        matches!(self.kind.as_str(), "category" | "GuildCategory")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageState {
    pub id: Id<MessageMarker>,
    pub channel_id: Id<ChannelMarker>,
    pub author: String,
    pub message_kind: MessageKind,
    pub reply: Option<ReplyInfo>,
    pub content: Option<String>,
    pub attachments: Vec<AttachmentInfo>,
    pub forwarded_snapshots: Vec<MessageSnapshotInfo>,
}

impl MessageState {
    pub fn attachments_in_display_order(&self) -> impl Iterator<Item = &AttachmentInfo> {
        self.attachments.iter().chain(
            self.forwarded_snapshots
                .iter()
                .flat_map(|snapshot| snapshot.attachments.iter()),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildMemberState {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    pub is_bot: bool,
    pub status: PresenceStatus,
}

#[derive(Clone, Debug)]
pub struct DiscordState {
    guilds: BTreeMap<Id<GuildMarker>, GuildState>,
    channels: BTreeMap<Id<ChannelMarker>, ChannelState>,
    messages: BTreeMap<Id<ChannelMarker>, VecDeque<MessageState>>,
    members: BTreeMap<Id<GuildMarker>, BTreeMap<Id<UserMarker>, GuildMemberState>>,
    /// User's `guild_folders` setting in display order. Empty until READY
    /// delivers it; the dashboard falls back to a flat guild list.
    guild_folders: Vec<GuildFolder>,
    max_messages_per_channel: usize,
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
            }
            AppEvent::GuildUpdate { guild_id, name } => {
                if let Some(guild) = self.guilds.get_mut(guild_id) {
                    guild.name = name.clone();
                }
            }
            AppEvent::GuildDelete { guild_id } => {
                self.guilds.remove(guild_id);
                self.channels
                    .retain(|_, channel| channel.guild_id != Some(*guild_id));
                self.messages
                    .retain(|channel_id, _| self.channels.contains_key(channel_id));
                self.members.remove(guild_id);
            }
            AppEvent::ChannelUpsert(channel) => self.upsert_channel(channel),
            AppEvent::ChannelDelete { channel_id, .. } => {
                self.channels.remove(channel_id);
                self.messages.remove(channel_id);
            }
            AppEvent::MessageCreate {
                channel_id,
                message_id,
                author,
                message_kind,
                reply,
                content,
                attachments,
                forwarded_snapshots,
                ..
            } => self.upsert_message(MessageState {
                id: *message_id,
                channel_id: *channel_id,
                author: author.clone(),
                message_kind: *message_kind,
                reply: reply.clone(),
                content: content.clone(),
                attachments: attachments.clone(),
                forwarded_snapshots: forwarded_snapshots.clone(),
            }),
            AppEvent::MessageHistoryLoaded {
                channel_id,
                messages,
            } => self.merge_message_history(*channel_id, messages),
            AppEvent::MessageHistoryLoadFailed { .. } => {}
            AppEvent::MessageUpdate {
                guild_id,
                channel_id,
                message_id,
                content,
                attachments,
            } => self.update_message(
                *guild_id,
                *channel_id,
                *message_id,
                content.clone(),
                attachments.clone(),
            ),
            AppEvent::MessageDelete {
                channel_id,
                message_id,
                ..
            } => self.delete_message(*channel_id, *message_id),
            AppEvent::GuildMemberUpsert { guild_id, member } => {
                let entry = self.members.entry(*guild_id).or_default();
                let previous_status = entry.get(&member.user_id).map(|m| m.status);
                upsert_member(entry, member, previous_status);
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
                            status: *status,
                        },
                    );
                }
            }
            AppEvent::GuildFoldersUpdate { folders } => {
                self.guild_folders = folders.clone();
            }
            AppEvent::Ready { .. }
            | AppEvent::GatewayError { .. }
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

    pub fn channel(&self, channel_id: Id<ChannelMarker>) -> Option<&ChannelState> {
        self.channels.get(&channel_id)
    }

    fn upsert_channel(&mut self, channel: &ChannelInfo) {
        let last_message_id = self
            .channels
            .get(&channel.channel_id)
            .and_then(|existing| existing.last_message_id)
            .max(channel.last_message_id);

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
            },
        );
    }

    fn upsert_message(&mut self, message: MessageState) {
        let channel_id = message.channel_id;
        let message_id = message.id;
        let messages = self.messages.entry(message.channel_id).or_default();
        if let Some(existing) = messages.iter_mut().find(|item| item.id == message.id) {
            existing.channel_id = message.channel_id;
            existing.author = message.author;
            existing.message_kind = message.message_kind;
            if message.reply.is_some() || existing.reply.is_none() {
                existing.reply = message.reply;
            }
            if message.content.is_some() {
                existing.content = message.content;
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

    fn merge_message_history(&mut self, channel_id: Id<ChannelMarker>, history: &[MessageInfo]) {
        let messages = self.messages.entry(channel_id).or_default();
        let mut by_id: BTreeMap<Id<MessageMarker>, MessageState> = messages
            .drain(..)
            .map(|message| (message.id, message))
            .collect();

        for message in history {
            if message.channel_id != channel_id {
                continue;
            }

            let incoming = MessageState {
                id: message.message_id,
                channel_id: message.channel_id,
                author: message.author.clone(),
                message_kind: message.message_kind,
                reply: message.reply.clone(),
                content: message.content.clone(),
                attachments: message.attachments.clone(),
                forwarded_snapshots: message.forwarded_snapshots.clone(),
            };

            by_id
                .entry(incoming.id)
                .and_modify(|existing| merge_message(existing, &incoming))
                .or_insert(incoming);
        }

        *messages = by_id.into_values().collect();
        while messages.len() > self.max_messages_per_channel {
            messages.pop_front();
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
        _guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: Option<String>,
        attachments: AttachmentUpdate,
    ) {
        let messages = self.messages.entry(channel_id).or_default();
        if let Some(existing) = messages.iter_mut().find(|item| item.id == message_id) {
            if let Some(content) = content {
                existing.content = Some(content);
            }
            match attachments {
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
    existing.channel_id = incoming.channel_id;
    existing.author = incoming.author.clone();
    existing.message_kind = incoming.message_kind;
    if incoming.reply.is_some() || existing.reply.is_none() {
        existing.reply = incoming.reply.clone();
    }

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
    let status = previous_status.unwrap_or(PresenceStatus::Offline);
    map.insert(
        member.user_id,
        GuildMemberState {
            user_id: member.user_id,
            display_name: member.display_name.clone(),
            is_bot: member.is_bot,
            status,
        },
    );
}

#[cfg(test)]
mod tests {
    use twilight_model::id::{Id, marker::ChannelMarker};

    use crate::discord::{
        AppEvent, AttachmentUpdate, ChannelInfo, DiscordState, MemberInfo, MessageInfo,
        MessageKind, MessageSnapshotInfo, PresenceStatus, ReplyInfo,
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
            }],
            members: Vec::new(),
            presences: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            content: Some("hello".to_owned()),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(state.guilds().len(), 1);
        assert_eq!(state.channels_for_guild(Some(guild_id)).len(), 1);
        assert_eq!(state.messages_for_channel(channel_id).len(), 1);
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
        }));

        let channel = state.channel(channel_id).unwrap();
        assert_eq!(channel.parent_id, Some(category_id));
        assert_eq!(channel.position, Some(7));
        assert_eq!(channel.last_message_id, Some(Id::new(9)));
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
                message_kind: crate::discord::MessageKind::regular(),
                reply: None,
                content: Some(format!("message {id}")),
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
            message_kind: MessageKind::new(19),
            reply: None,
            content: Some("reply".to_owned()),
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
            message_kind: MessageKind::regular(),
            reply: None,
            content: Some("cached".to_owned()),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            message_kind: MessageKind::new(19),
            reply: None,
            content: None,
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.as_deref(), Some("cached"));
        assert_eq!(messages[0].message_kind, MessageKind::new(19));
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
            message_kind: MessageKind::new(19),
            reply: Some(ReplyInfo {
                author: "Alex".to_owned(),
                content: Some("잘되는군".to_owned()),
            }),
            content: Some("asdf".to_owned()),
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
            message_kind: MessageKind::new(19),
            reply: Some(ReplyInfo {
                author: "Alex".to_owned(),
                content: Some("잘되는군".to_owned()),
            }),
            content: Some("asdf".to_owned()),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            message_kind: MessageKind::new(19),
            reply: None,
            content: None,
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
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            content: Some("hello".to_owned()),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id,
            author_id,
            author: "neo".to_owned(),
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            content: None,
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id,
            content: None,
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
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            content: Some("live".to_owned()),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
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
    fn history_dedupes_and_preserves_known_content() {
        let channel_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DiscordState::default();

        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            content: Some("known".to_owned()),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            messages: vec![MessageInfo {
                content: Some(String::new()),
                ..message_info(channel_id, 20, "")
            }],
        });

        let messages = state.messages_for_channel(channel_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.as_deref(), Some("known"));
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
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            content: Some(String::new()),
            attachments: vec![attachment_info(1, "cat.png", "image/png")],
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
            messages: vec![MessageInfo {
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
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            content: Some(String::new()),
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
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            content: Some(String::new()),
            attachments: Vec::new(),
            forwarded_snapshots: vec![snapshot_info("live snapshot")],
        });
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
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
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            content: Some(String::new()),
            attachments: vec![attachment_info(1, "cat.png", "image/png")],
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            content: None,
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
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            content: Some(String::new()),
            attachments: vec![attachment_info(1, "cat.png", "image/png")],
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageUpdate {
            guild_id: None,
            channel_id,
            message_id: Id::new(20),
            content: None,
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
        }));
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(30),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            content: Some("new".to_owned()),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.apply_event(&AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(10),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            content: Some("old".to_owned()),
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
        }));
        state.apply_event(&AppEvent::MessageHistoryLoaded {
            channel_id,
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
        }));
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "neo renamed".to_owned(),
            kind: "dm".to_owned(),
        }));
        state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: Some(Id::new(20)),
            name: "neo renamed again".to_owned(),
            kind: "dm".to_owned(),
        }));

        let channel = state.channel(channel_id).unwrap();
        assert_eq!(channel.name, "neo renamed again");
        assert_eq!(channel.last_message_id, Some(Id::new(30)));
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
                },
                MemberInfo {
                    user_id: bob,
                    display_name: "bob".to_owned(),
                    is_bot: false,
                },
            ],
            presences: vec![(alice, PresenceStatus::Online)],
        });

        let members = state.members_for_guild(guild_id);
        assert_eq!(members.len(), 2);
        let alice_state = members.iter().find(|m| m.user_id == alice).unwrap();
        assert_eq!(alice_state.status, PresenceStatus::Online);
        let bob_state = members.iter().find(|m| m.user_id == bob).unwrap();
        assert_eq!(bob_state.status, PresenceStatus::Offline);

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

    fn message_info(channel_id: Id<ChannelMarker>, message_id: u64, content: &str) -> MessageInfo {
        MessageInfo {
            guild_id: None,
            channel_id,
            message_id: Id::new(message_id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            message_kind: MessageKind::regular(),
            reply: None,
            content: Some(content.to_owned()),
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

    fn snapshot_info(content: &str) -> MessageSnapshotInfo {
        MessageSnapshotInfo {
            content: Some(content.to_owned()),
            attachments: Vec::new(),
            source_channel_id: None,
            timestamp: None,
        }
    }
}
