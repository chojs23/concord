use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};
use crate::discord::{
    AppEvent, ApplicationCommandInfo, ChannelUnreadState, DiscordSnapshot, DiscordState,
    MessageInfo, SnapshotAreas, SnapshotRevision,
};

use super::{DashboardState, DesktopNotification, message_notification_body};

#[derive(Debug, Default)]
pub(super) struct DiscordUiState {
    pub(super) cache: DiscordState,
    pub(super) current_user: Option<String>,
    pub(super) current_user_id: Option<Id<UserMarker>>,
    pub(super) application_commands: HashMap<Option<Id<GuildMarker>>, Vec<ApplicationCommandInfo>>,
    pub(super) current_user_has_nitro: Option<bool>,
    pub(super) update_available_version: Option<String>,
}

impl Deref for DiscordUiState {
    type Target = DiscordState;

    fn deref(&self) -> &Self::Target {
        &self.cache
    }
}

impl DerefMut for DiscordUiState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.cache
    }
}

impl DashboardState {
    pub(super) fn current_user_has_nitro(&self) -> bool {
        self.discord.current_user_has_nitro.unwrap_or(false)
    }

    pub fn restore_discord_snapshot(&mut self, discord: DiscordState) {
        self.restore_discord_snapshot_with(SnapshotAreas::all(), |state| {
            *state = discord;
        });
    }

    pub fn restore_discord_snapshot_areas(
        &mut self,
        snapshot: &DiscordSnapshot,
        previous_revision: SnapshotRevision,
    ) {
        let areas = snapshot.revision.changed_areas_since(previous_revision);
        self.restore_discord_snapshot_with(areas, |state| {
            state.restore_snapshot_areas(snapshot, previous_revision);
        });
    }

    fn restore_discord_snapshot_with(
        &mut self,
        areas: SnapshotAreas,
        restore: impl FnOnce(&mut DiscordState),
    ) {
        let was_auto_follow = self.messages.message_auto_follow;
        let was_at_latest = was_auto_follow || self.is_viewport_at_latest_message();
        let was_cursor_on_last = self.cursor_on_last_message();
        let was_following_cursor = was_at_latest && was_cursor_on_last;
        let preserve_selection = !was_following_cursor;
        let preserve_scroll = !(was_at_latest || was_following_cursor);
        let selected_message_id = preserve_selection
            .then(|| {
                self.messages()
                    .get(self.selected_message())
                    .map(|message| message.id)
            })
            .flatten();
        let scroll_message_id = preserve_scroll
            .then(|| {
                self.messages()
                    .get(self.messages.message_scroll)
                    .map(|message| message.id)
            })
            .flatten();
        let channel_cursor_id = self.selected_channel_cursor_id();

        restore(&mut self.discord.cache);
        self.clear_message_row_content_metrics_cache();
        if areas.navigation {
            self.repair_navigation_after_discord_restore(channel_cursor_id);
        }

        let in_message_view = self.message_pane_supports_auto_follow();
        let should_follow = was_following_cursor && in_message_view;
        let should_scroll = should_follow || (was_at_latest && in_message_view);
        if areas.message || areas.navigation {
            self.repair_message_after_discord_restore(
                selected_message_id,
                scroll_message_id,
                should_follow,
                should_scroll,
            );
        }
        self.refresh_search_popup_after_member_cache_update();
    }

    fn repair_navigation_after_discord_restore(
        &mut self,
        channel_cursor_id: Option<Id<ChannelMarker>>,
    ) {
        if let Some(user) = self.discord.current_user() {
            self.discord.current_user = Some(user.to_owned());
        }
        if let Some(user_id) = self.discord.current_user_id() {
            self.discord.current_user_id = Some(user_id);
        }
        self.refresh_composer_emoji_candidates_for_current_query();

        self.clamp_active_selection();
        self.restore_channel_cursor(channel_cursor_id);
        self.navigation.guilds.list.selected = self.selected_guild();
        self.navigation.channels.list.selected = self.selected_channel();
        self.navigation.members.list.selected = self.selected_member();
        self.clamp_guild_viewport();
        self.clamp_channel_viewport();
        self.clamp_member_viewport();
    }

    fn repair_message_after_discord_restore(
        &mut self,
        selected_message_id: Option<Id<MessageMarker>>,
        scroll_message_id: Option<Id<MessageMarker>>,
        should_follow: bool,
        should_scroll: bool,
    ) {
        self.messages.selected_message = self.selected_message();
        if should_follow {
            self.follow_latest_message();
        } else {
            self.restore_message_position(selected_message_id, scroll_message_id);
        }
        if should_scroll {
            self.messages.message_auto_follow = true;
        }
        self.clamp_message_viewport();
        if !should_scroll {
            self.refresh_message_auto_follow();
        }
    }
}

impl DashboardState {
    pub fn channel_unread(&self, channel_id: Id<ChannelMarker>) -> ChannelUnreadState {
        self.discord.cache.channel_unread(channel_id)
    }

    pub fn sidebar_channel_unread(&self, channel_id: Id<ChannelMarker>) -> ChannelUnreadState {
        self.discord.cache.channel_sidebar_unread(channel_id)
    }

    pub fn sidebar_guild_unread(&self, guild_id: Id<GuildMarker>) -> ChannelUnreadState {
        self.discord.cache.guild_sidebar_unread(guild_id)
    }

    pub fn channel_notification_muted(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.discord.cache.channel_notification_muted(channel_id)
    }

    pub fn guild_notification_muted(&self, guild_id: Id<GuildMarker>) -> bool {
        self.discord.cache.guild_notification_muted(guild_id)
    }

    pub fn direct_message_unread_count(&self) -> usize {
        self.discord.cache.direct_message_unread_count()
    }

    pub fn total_mention_count(&self) -> usize {
        self.discord.cache.total_mention_count() as usize
    }

    pub fn channel_unread_message_count(&self, channel_id: Id<ChannelMarker>) -> usize {
        self.discord.cache.channel_unread_message_count(channel_id)
    }

    /// Whether `channel_id` is a thread whose parent is a forum channel.
    pub fn is_forum_post_thread(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.discord
            .cache
            .channel(channel_id)
            .filter(|channel| channel.is_thread())
            .and_then(|channel| channel.parent_id)
            .and_then(|parent_id| self.discord.cache.channel(parent_id))
            .is_some_and(|parent| parent.is_forum())
    }
}

impl DashboardState {
    pub(crate) fn desktop_notification_for_event(
        &self,
        event: &AppEvent,
    ) -> Option<DesktopNotification> {
        let AppEvent::MessageCreate { message } = event else {
            return None;
        };
        if !self.desktop_notifications_enabled() || self.message_notification_suppressed(message) {
            return None;
        }
        if !self
            .discord
            .cache
            .message_event_triggers_notification(event)
        {
            return None;
        }

        let channel = self.discord.cache.channel(message.channel_id);
        let guild_id = message
            .guild_id
            .or_else(|| channel.and_then(|channel| channel.guild_id));
        let title = match guild_id.and_then(|guild_id| self.discord.cache.guild(guild_id)) {
            Some(guild) => {
                let channel_name = channel
                    .map(|channel| channel.name.as_str())
                    .unwrap_or("unknown-channel");
                format!("{} in {} #{channel_name}", message.author, guild.name)
            }
            None => message.author.clone(),
        };
        let body = message_notification_body(
            message.content.as_deref(),
            message.sticker_names.len(),
            message.attachments.len(),
            message.embeds.len(),
        );
        Some(DesktopNotification { title, body })
    }

    pub(crate) fn notification_sound_for_event(&self, event: &AppEvent) -> bool {
        let AppEvent::MessageCreate { message } = event else {
            return false;
        };
        self.desktop_notifications_enabled()
            && !self.message_notification_suppressed(message)
            && self
                .discord
                .cache
                .message_event_triggers_notification(event)
    }

    fn message_notification_suppressed(&self, message: &MessageInfo) -> bool {
        self.terminal_focused()
            && self.navigation.channels.active_channel_id == Some(message.channel_id)
    }
}

impl DashboardState {
    pub fn current_user(&self) -> Option<&str> {
        self.discord.current_user.as_deref()
    }

    pub fn current_user_id(&self) -> Option<Id<UserMarker>> {
        self.discord.current_user_id
    }
}
