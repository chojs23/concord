use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};
use crate::discord::{
    AppEvent, ChannelNotificationOverrideInfo, GuildNotificationSettingsInfo, MentionInfo,
    NotificationLevel,
};

use crate::discord::{MessageState, state::DiscordState};

const SUPPRESS_NOTIFICATIONS_FLAG: u64 = 1 << 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelUnreadState {
    Seen,
    Unread,
    Mentioned(u32),
    Notified(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::discord) enum MessageNotificationKind {
    None,
    Mention,
    Notify,
}

pub(in crate::discord) struct MessageNotificationInput<'a> {
    pub(in crate::discord) guild_id: Option<Id<GuildMarker>>,
    pub(in crate::discord) channel_id: Id<ChannelMarker>,
    pub(in crate::discord) message_id: Id<MessageMarker>,
    pub(in crate::discord) author_id: Id<UserMarker>,
    pub(in crate::discord) mentions: &'a [MentionInfo],
    pub(in crate::discord) mention_everyone: bool,
    pub(in crate::discord) mention_roles: &'a [Id<RoleMarker>],
    pub(in crate::discord) flags: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChannelNotificationSettingsState {
    message_notifications: Option<NotificationLevel>,
    muted: bool,
    mute_end_time: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::discord) struct GuildNotificationSettingsState {
    message_notifications: Option<NotificationLevel>,
    muted: bool,
    mute_end_time: Option<String>,
    suppress_everyone: bool,
    suppress_roles: bool,
    channel_overrides: BTreeMap<Id<ChannelMarker>, ChannelNotificationSettingsState>,
}

impl DiscordState {
    pub fn channel_sidebar_unread(&self, channel_id: Id<ChannelMarker>) -> ChannelUnreadState {
        if self
            .navigation
            .channels
            .get(&channel_id)
            .is_some_and(|channel| channel.is_thread() && !channel.current_user_joined_thread)
        {
            return ChannelUnreadState::Seen;
        }
        if self.channel_notification_muted(channel_id) {
            ChannelUnreadState::Seen
        } else if self
            .navigation
            .channels
            .get(&channel_id)
            .is_some_and(|channel| channel.is_forum())
        {
            aggregate_unread_states(
                std::iter::once(self.channel_unread(channel_id)).chain(
                    self.navigation
                        .channels
                        .values()
                        .filter(|channel| {
                            channel.is_thread()
                                && channel.parent_id == Some(channel_id)
                                && channel.current_user_joined_thread
                                && self.can_view_channel(channel)
                        })
                        .map(|channel| self.channel_sidebar_unread(channel.id)),
                ),
            )
        } else {
            self.channel_unread(channel_id)
        }
    }

    pub fn guild_sidebar_unread(&self, guild_id: Id<GuildMarker>) -> ChannelUnreadState {
        if self.guild_notification_muted(guild_id) {
            ChannelUnreadState::Seen
        } else {
            aggregate_unread_states(
                self.viewable_channels_for_guild(Some(guild_id))
                    .into_iter()
                    .filter(|channel| !self.channel_is_forum_child_thread(channel.id))
                    .map(|channel| self.channel_sidebar_unread(channel.id)),
            )
        }
    }

    fn channel_is_forum_child_thread(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.navigation
            .channels
            .get(&channel_id)
            .filter(|channel| channel.is_thread())
            .and_then(|channel| channel.parent_id)
            .and_then(|parent_id| self.navigation.channels.get(&parent_id))
            .is_some_and(|parent| parent.is_forum())
    }

    pub fn guild_notification_muted(&self, guild_id: Id<GuildMarker>) -> bool {
        self.notifications
            .notification_settings
            .get(&guild_id)
            .is_some_and(|settings| {
                notification_setting_muted(settings.muted, settings.mute_end_time.as_deref())
            })
    }

    pub fn channel_notification_muted(&self, channel_id: Id<ChannelMarker>) -> bool {
        match self.channel_guild_id(channel_id) {
            Some(guild_id) => self
                .notifications
                .notification_settings
                .get(&guild_id)
                .is_some_and(|settings| {
                    self.channel_notification_muted_in_settings(settings, channel_id)
                }),
            None => self
                .notifications
                .private_notification_settings
                .as_ref()
                .is_some_and(|settings| {
                    self.channel_notification_muted_in_settings(settings, channel_id)
                }),
        }
    }

    pub fn guild_notification_settings_info(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> GuildNotificationSettingsInfo {
        let settings = match guild_id {
            Some(guild_id) => self.notifications.notification_settings.get(&guild_id),
            None => self.notifications.private_notification_settings.as_ref(),
        };
        GuildNotificationSettingsInfo {
            guild_id,
            message_notifications: settings.and_then(|settings| settings.message_notifications),
            muted: settings.is_some_and(|settings| settings.muted),
            mute_end_time: settings.and_then(|settings| settings.mute_end_time.clone()),
            suppress_everyone: settings.is_some_and(|settings| settings.suppress_everyone),
            suppress_roles: settings.is_some_and(|settings| settings.suppress_roles),
            channel_overrides: settings.map(channel_override_infos).unwrap_or_default(),
        }
    }

    pub fn channel_unread(&self, channel_id: Id<ChannelMarker>) -> ChannelUnreadState {
        let latest = self
            .navigation
            .channels
            .get(&channel_id)
            .and_then(|channel| channel.last_message_id);
        let Some(latest) = latest else {
            return ChannelUnreadState::Seen;
        };
        let read = self
            .notifications
            .read_states
            .get(&channel_id)
            .copied()
            .unwrap_or_default();
        if read.mention_count > 0 {
            return ChannelUnreadState::Mentioned(read.mention_count);
        }
        if read.notification_count > 0 {
            return ChannelUnreadState::Notified(read.notification_count);
        }

        let (loaded_mentions, loaded_notifications) =
            self.loaded_unread_notification_counts(channel_id);
        if loaded_mentions > 0 {
            return ChannelUnreadState::Mentioned(saturating_u32_count(loaded_mentions));
        }
        if loaded_notifications > 0 {
            return ChannelUnreadState::Notified(saturating_u32_count(loaded_notifications));
        }

        if read
            .last_acked_message_id
            .is_none_or(|acked| acked < latest)
        {
            return ChannelUnreadState::Unread;
        }

        ChannelUnreadState::Seen
    }

    pub fn guild_unread(&self, guild_id: Id<GuildMarker>) -> ChannelUnreadState {
        aggregate_unread_states(
            self.viewable_channels_for_guild(Some(guild_id))
                .into_iter()
                .map(|channel| self.channel_unread(channel.id)),
        )
    }

    pub fn direct_message_unread_count(&self) -> usize {
        self.channels_for_guild(None)
            .into_iter()
            .filter(|channel| self.channel_sidebar_unread(channel.id) != ChannelUnreadState::Seen)
            .count()
    }

    pub fn channel_unread_message_count(&self, channel_id: Id<ChannelMarker>) -> usize {
        let (mentions, notifications) = self.loaded_unread_notification_counts(channel_id);
        mentions.saturating_add(notifications)
    }

    pub fn message_event_triggers_notification(&self, event: &AppEvent) -> bool {
        let AppEvent::MessageCreate { message } = event else {
            return false;
        };

        let guild_id = message
            .guild_id
            .or_else(|| self.channel_guild_id(message.channel_id));
        self.message_create_notification_kind(MessageNotificationInput {
            guild_id,
            channel_id: message.channel_id,
            message_id: message.message_id,
            author_id: message.author_id,
            mentions: &message.mentions,
            mention_everyone: message.mention_everyone,
            mention_roles: &message.mention_roles,
            flags: message.flags,
        }) != MessageNotificationKind::None
    }

    pub(in crate::discord) fn upsert_notification_settings(
        &mut self,
        settings: &GuildNotificationSettingsInfo,
    ) {
        let state = GuildNotificationSettingsState {
            message_notifications: settings.message_notifications,
            muted: settings.muted,
            mute_end_time: settings.mute_end_time.clone(),
            suppress_everyone: settings.suppress_everyone,
            suppress_roles: settings.suppress_roles,
            channel_overrides: notification_override_map(&settings.channel_overrides),
        };
        if let Some(guild_id) = settings.guild_id {
            self.notifications
                .notification_settings
                .insert(guild_id, state);
        } else {
            self.notifications.private_notification_settings = Some(state);
        }
    }

    fn message_state_notification_kind(&self, message: &MessageState) -> MessageNotificationKind {
        self.message_create_notification_kind(MessageNotificationInput {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
            author_id: message.author_id,
            mentions: &message.mentions,
            mention_everyone: message.mention_everyone,
            mention_roles: &message.mention_roles,
            flags: message.flags,
        })
    }

    pub(in crate::discord) fn message_create_notification_kind(
        &self,
        message: MessageNotificationInput<'_>,
    ) -> MessageNotificationKind {
        if self.session.current_user_id == Some(message.author_id) {
            return MessageNotificationKind::None;
        }
        if message.flags & SUPPRESS_NOTIFICATIONS_FLAG != 0 {
            return MessageNotificationKind::None;
        }
        if self
            .notifications
            .read_states
            .get(&message.channel_id)
            .and_then(|state| state.last_acked_message_id)
            .is_some_and(|acked| acked >= message.message_id)
        {
            return MessageNotificationKind::None;
        }
        let Some(guild_id) = message.guild_id else {
            return self.private_message_notification_kind(message.channel_id, message.mentions);
        };
        let mentions_current_user = |settings: &GuildNotificationSettingsState| {
            self.message_mentions_current_user(
                guild_id,
                message.mentions,
                message.mention_everyone,
                message.mention_roles,
                settings.suppress_everyone,
                settings.suppress_roles,
            )
        };
        let Some(settings) = self.notifications.notification_settings.get(&guild_id) else {
            return if self.message_mentions_current_user(
                guild_id,
                message.mentions,
                message.mention_everyone,
                message.mention_roles,
                false,
                false,
            ) {
                MessageNotificationKind::Mention
            } else {
                MessageNotificationKind::None
            };
        };
        if notification_setting_muted(settings.muted, settings.mute_end_time.as_deref())
            || self.channel_notification_muted_in_settings(settings, message.channel_id)
        {
            return MessageNotificationKind::None;
        }

        match self.channel_notification_level(settings, message.channel_id) {
            NotificationLevel::AllMessages if mentions_current_user(settings) => {
                MessageNotificationKind::Mention
            }
            NotificationLevel::AllMessages => MessageNotificationKind::Notify,
            NotificationLevel::OnlyMentions | NotificationLevel::ParentDefault => {
                if mentions_current_user(settings) {
                    MessageNotificationKind::Mention
                } else {
                    MessageNotificationKind::None
                }
            }
            NotificationLevel::NoMessages => MessageNotificationKind::None,
        }
    }

    fn private_message_notification_kind(
        &self,
        channel_id: Id<ChannelMarker>,
        mentions: &[MentionInfo],
    ) -> MessageNotificationKind {
        let Some(settings) = self.notifications.private_notification_settings.as_ref() else {
            return MessageNotificationKind::Notify;
        };
        if notification_setting_muted(settings.muted, settings.mute_end_time.as_deref())
            || self.channel_notification_muted_in_settings(settings, channel_id)
        {
            return MessageNotificationKind::None;
        }
        let mentions_current_user = self
            .session
            .current_user_id
            .is_some_and(|self_id| mentions.iter().any(|mention| mention.user_id == self_id));
        match self.channel_notification_level(settings, channel_id) {
            NotificationLevel::AllMessages if mentions_current_user => {
                MessageNotificationKind::Mention
            }
            NotificationLevel::AllMessages => MessageNotificationKind::Notify,
            NotificationLevel::OnlyMentions | NotificationLevel::ParentDefault => {
                if mentions_current_user {
                    MessageNotificationKind::Mention
                } else {
                    MessageNotificationKind::None
                }
            }
            NotificationLevel::NoMessages => MessageNotificationKind::None,
        }
    }

    fn loaded_unread_notification_counts(&self, channel_id: Id<ChannelMarker>) -> (usize, usize) {
        let Some(messages) = self.message_cache.messages.get(&channel_id) else {
            return (0, 0);
        };
        let last_acked = self
            .notifications
            .read_states
            .get(&channel_id)
            .and_then(|state| state.last_acked_message_id);
        let mut mentions = 0usize;
        let mut notifications = 0usize;
        for message in messages
            .iter()
            .filter(|message| last_acked.is_none_or(|last_acked| message.id > last_acked))
        {
            match self.message_state_notification_kind(message) {
                MessageNotificationKind::Mention => mentions = mentions.saturating_add(1),
                MessageNotificationKind::Notify => notifications = notifications.saturating_add(1),
                MessageNotificationKind::None => {}
            }
        }
        (mentions, notifications)
    }

    fn channel_notification_level(
        &self,
        settings: &GuildNotificationSettingsState,
        channel_id: Id<ChannelMarker>,
    ) -> NotificationLevel {
        if let Some(level) = settings
            .channel_overrides
            .get(&channel_id)
            .and_then(|setting| setting.message_notifications)
            .filter(|level| *level != NotificationLevel::ParentDefault)
        {
            return level;
        }
        if let Some(parent_id) = self
            .navigation
            .channels
            .get(&channel_id)
            .and_then(|channel| channel.parent_id)
            && let Some(level) = settings
                .channel_overrides
                .get(&parent_id)
                .and_then(|setting| setting.message_notifications)
                .filter(|level| *level != NotificationLevel::ParentDefault)
        {
            return level;
        }

        settings
            .message_notifications
            .filter(|level| *level != NotificationLevel::ParentDefault)
            .unwrap_or(NotificationLevel::OnlyMentions)
    }

    fn channel_notification_muted_in_settings(
        &self,
        settings: &GuildNotificationSettingsState,
        channel_id: Id<ChannelMarker>,
    ) -> bool {
        if let Some(setting) = settings.channel_overrides.get(&channel_id) {
            return notification_setting_muted(setting.muted, setting.mute_end_time.as_deref());
        }
        self.navigation
            .channels
            .get(&channel_id)
            .and_then(|channel| channel.parent_id)
            .and_then(|parent_id| settings.channel_overrides.get(&parent_id))
            .is_some_and(|setting| {
                notification_setting_muted(setting.muted, setting.mute_end_time.as_deref())
            })
    }

    fn message_mentions_current_user(
        &self,
        guild_id: Id<GuildMarker>,
        mentions: &[MentionInfo],
        mention_everyone: bool,
        mention_roles: &[Id<RoleMarker>],
        suppress_everyone: bool,
        suppress_roles: bool,
    ) -> bool {
        let Some(self_id) = self.session.current_user_id else {
            return false;
        };
        if mentions.iter().any(|mention| mention.user_id == self_id) {
            return true;
        }
        if !suppress_everyone && mention_everyone {
            return true;
        }
        if suppress_roles {
            return false;
        }
        self.current_user_role_ids_for_guild(guild_id)
            .is_some_and(|role_ids| {
                role_ids
                    .iter()
                    .any(|role_id| mention_roles.contains(role_id))
            })
    }
}

fn channel_override_infos(
    settings: &GuildNotificationSettingsState,
) -> Vec<ChannelNotificationOverrideInfo> {
    settings
        .channel_overrides
        .iter()
        .map(|(channel_id, setting)| ChannelNotificationOverrideInfo {
            channel_id: *channel_id,
            message_notifications: setting.message_notifications,
            muted: setting.muted,
            mute_end_time: setting.mute_end_time.clone(),
        })
        .collect()
}

fn notification_override_map(
    overrides: &[ChannelNotificationOverrideInfo],
) -> BTreeMap<Id<ChannelMarker>, ChannelNotificationSettingsState> {
    overrides
        .iter()
        .map(|override_info| {
            (
                override_info.channel_id,
                ChannelNotificationSettingsState {
                    message_notifications: override_info.message_notifications,
                    muted: override_info.muted,
                    mute_end_time: override_info.mute_end_time.clone(),
                },
            )
        })
        .collect()
}

fn notification_setting_muted(muted: bool, end_time: Option<&str>) -> bool {
    if !muted {
        return false;
    }
    let Some(end_time) = end_time else {
        return true;
    };
    DateTime::parse_from_rfc3339(end_time)
        .map(|end_time| end_time.with_timezone(&Utc) > Utc::now())
        .unwrap_or(true)
}

fn aggregate_unread_states(
    states: impl IntoIterator<Item = ChannelUnreadState>,
) -> ChannelUnreadState {
    let mut mention_count = 0u32;
    let mut notification_count = 0u32;
    let mut has_unread = false;
    for state in states {
        match state {
            ChannelUnreadState::Mentioned(count) => {
                mention_count = mention_count.saturating_add(count);
            }
            ChannelUnreadState::Notified(count) => {
                notification_count = notification_count.saturating_add(count);
            }
            ChannelUnreadState::Unread => has_unread = true,
            ChannelUnreadState::Seen => {}
        }
    }

    if mention_count > 0 {
        ChannelUnreadState::Mentioned(mention_count)
    } else if notification_count > 0 {
        ChannelUnreadState::Notified(notification_count)
    } else if has_unread {
        ChannelUnreadState::Unread
    } else {
        ChannelUnreadState::Seen
    }
}

fn saturating_u32_count(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}
