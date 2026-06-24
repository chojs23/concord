use std::collections::HashSet;

use crate::discord::AppCommand;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker},
};
use crate::discord::{ChannelState, ChannelUnreadState, MessageInfo};
use crate::tui::keybindings::SelectionAction;

use super::super::model::GuildPaneEntry;
use super::super::{ActiveGuildScope, DashboardState};
use crate::tui::state::popups::{ActiveModalPopupKind, ModalPopup, SelectablePopupState};

const MAX_INBOX_MESSAGES_PER_CHANNEL: usize = 3;
const INITIAL_UNREAD_CHANNELS: usize = 4;
/// How far past the selection we prefetch history so scrolling reveals
/// already-loading channels.
const UNREAD_REQUEST_LOOKAHEAD: usize = 2;

/// "For You" is omitted: there is no local data source for that feed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NotificationInboxTab {
    #[default]
    Unreads,
    Mentions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationInboxLoad {
    Loading,
    Loaded,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationInboxChannelLoad {
    Pending,
    Loading,
    Loaded,
}

/// Captured from the REST response; never updated by live gateway messages.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationInboxMessage {
    pub author: String,
    pub content: String,
}

/// `ack_target` is snapshotted at open so "mark read" works after scrolling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationInboxItem {
    pub channel_id: Id<ChannelMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub ack_target: Option<Id<MessageMarker>>,
    pub title: String,
    pub context: Option<String>,
    pub unread: ChannelUnreadState,
    pub messages: Vec<NotificationInboxMessage>,
    pub load: NotificationInboxChannelLoad,
}

#[derive(Debug)]
pub(in crate::tui::state) struct NotificationInboxState {
    /// Identifies this open so a previous open's late responses are ignored.
    request_id: u64,
    tab: NotificationInboxTab,
    unreads: Vec<NotificationInboxItem>,
    mentions: Vec<NotificationInboxItem>,
    unreads_selection: SelectablePopupState,
    mentions_selection: SelectablePopupState,
    mentions_status: NotificationInboxLoad,
    confirming_mark_all: bool,
}

impl NotificationInboxState {
    fn new(request_id: u64, unreads: Vec<NotificationInboxItem>) -> Self {
        Self {
            request_id,
            tab: NotificationInboxTab::default(),
            unreads,
            mentions: Vec::new(),
            unreads_selection: SelectablePopupState::default(),
            mentions_selection: SelectablePopupState::default(),
            mentions_status: NotificationInboxLoad::Loading,
            confirming_mark_all: false,
        }
    }

    fn items(&self, tab: NotificationInboxTab) -> &[NotificationInboxItem] {
        match tab {
            NotificationInboxTab::Unreads => &self.unreads,
            NotificationInboxTab::Mentions => &self.mentions,
        }
    }

    fn active_items(&self) -> &[NotificationInboxItem] {
        self.items(self.tab)
    }

    fn selection(&self, tab: NotificationInboxTab) -> &SelectablePopupState {
        match tab {
            NotificationInboxTab::Unreads => &self.unreads_selection,
            NotificationInboxTab::Mentions => &self.mentions_selection,
        }
    }

    fn selection_mut(&mut self, tab: NotificationInboxTab) -> &mut SelectablePopupState {
        match tab {
            NotificationInboxTab::Unreads => &mut self.unreads_selection,
            NotificationInboxTab::Mentions => &mut self.mentions_selection,
        }
    }

    fn selected_index(&self) -> usize {
        self.selection(self.tab)
            .selected_for_len(self.active_items().len())
    }
}

impl DashboardState {
    pub fn open_notification_inbox(&mut self) {
        let request_id = self.next_inbox_request_id();
        let unreads = self.build_unread_inbox_items();
        self.popups.modal = Some(ModalPopup::NotificationInbox(NotificationInboxState::new(
            request_id, unreads,
        )));
        self.enqueue_pending_command(AppCommand::LoadInboxMentions { request_id });
        self.request_unread_inbox_history(INITIAL_UNREAD_CHANNELS);
    }

    pub fn close_notification_inbox(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::NotificationInbox) {
            self.popups.clear_modal();
        }
    }

    fn next_inbox_request_id(&mut self) -> u64 {
        self.popups.inbox_request_generation = self.popups.inbox_request_generation.wrapping_add(1);
        self.popups.inbox_request_generation
    }

    pub fn notification_inbox_tab(&self) -> Option<NotificationInboxTab> {
        self.popups.notification_inbox().map(|inbox| inbox.tab)
    }

    pub fn notification_inbox_items(&self) -> Vec<NotificationInboxItem> {
        self.popups
            .notification_inbox()
            .map(|inbox| inbox.active_items().to_vec())
            .unwrap_or_default()
    }

    pub fn notification_inbox_unread_count(&self) -> usize {
        self.popups
            .notification_inbox()
            .map(|inbox| inbox.unreads.len())
            .unwrap_or_default()
    }

    /// Unconfirmed mention count from the server read state (not the number of
    /// recent mentions fetched), so it reads 0 once everything is acked.
    pub fn notification_inbox_mention_count(&self) -> usize {
        self.total_mention_count()
    }

    pub fn notification_inbox_mentions_status(&self) -> Option<NotificationInboxLoad> {
        self.popups
            .notification_inbox()
            .map(|inbox| inbox.mentions_status)
    }

    pub fn selected_notification_inbox_index(&self) -> Option<usize> {
        self.popups
            .notification_inbox()
            .map(NotificationInboxState::selected_index)
    }

    pub fn move_notification_inbox_down(&mut self) {
        let Some(inbox) = self.popups.notification_inbox() else {
            return;
        };
        let (tab, len) = (inbox.tab, inbox.active_items().len());
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.selection_mut(tab).move_down(len);
        }
        self.ensure_unread_inbox_requests();
    }

    pub fn move_notification_inbox_up(&mut self) {
        let Some(tab) = self.notification_inbox_tab() else {
            return;
        };
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.selection_mut(tab).move_up();
        }
    }

    pub(super) fn page_notification_inbox_selection(&mut self, action: SelectionAction) {
        let Some(inbox) = self.popups.notification_inbox() else {
            return;
        };
        let (tab, len) = (inbox.tab, inbox.active_items().len());
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.selection_mut(tab).page(len, action);
        }
        self.ensure_unread_inbox_requests();
    }

    pub fn switch_notification_inbox_tab(&mut self, action: SelectionAction) {
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.tab = match (inbox.tab, action) {
                (NotificationInboxTab::Unreads, _) => NotificationInboxTab::Mentions,
                (NotificationInboxTab::Mentions, _) => NotificationInboxTab::Unreads,
            };
        }
        self.ensure_unread_inbox_requests();
    }

    pub fn activate_selected_notification_inbox_item(&mut self) -> Option<AppCommand> {
        let item = {
            let inbox = self.popups.notification_inbox()?;
            inbox.active_items().get(inbox.selected_index())?.clone()
        };
        self.close_notification_inbox();
        self.navigate_to_inbox_channel(item.channel_id)
    }

    pub fn mark_selected_notification_inbox_item_read(&mut self) -> Option<AppCommand> {
        let (tab, index, item) = {
            let inbox = self.popups.notification_inbox()?;
            let index = inbox.selected_index();
            let item = inbox.active_items().get(index)?.clone();
            (inbox.tab, index, item)
        };
        self.remove_notification_inbox_item(tab, index);
        let message_id = item.ack_target?;
        self.queue_ack_channel_command(item.channel_id, message_id);
        None
    }

    pub fn notification_inbox_is_confirming_mark_all(&self) -> bool {
        self.popups
            .notification_inbox()
            .is_some_and(|inbox| inbox.confirming_mark_all)
    }

    pub fn begin_mark_all_notification_inbox_read(&mut self) {
        if let Some(inbox) = self.popups.notification_inbox_mut()
            && !inbox.active_items().is_empty()
        {
            inbox.confirming_mark_all = true;
        }
    }

    pub fn cancel_mark_all_notification_inbox_read(&mut self) {
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.confirming_mark_all = false;
        }
    }

    pub fn confirm_mark_all_notification_inbox_read(&mut self) -> Option<AppCommand> {
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.confirming_mark_all = false;
        }
        let (tab, targets) = {
            let inbox = self.popups.notification_inbox()?;
            let targets: Vec<(Id<ChannelMarker>, Id<MessageMarker>)> = inbox
                .active_items()
                .iter()
                .filter_map(|item| {
                    item.ack_target
                        .map(|message_id| (item.channel_id, message_id))
                })
                .collect();
            (inbox.tab, targets)
        };
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            match tab {
                NotificationInboxTab::Unreads => inbox.unreads.clear(),
                NotificationInboxTab::Mentions => inbox.mentions.clear(),
            }
        }
        if targets.is_empty() {
            return None;
        }
        self.queue_ack_channels_command(targets);
        None
    }

    pub(in crate::tui) fn apply_inbox_mentions_loaded(
        &mut self,
        request_id: u64,
        messages: &[MessageInfo],
    ) {
        if !self.inbox_request_matches(request_id) {
            return;
        }
        let mut order: Vec<Id<ChannelMarker>> = Vec::new();
        let mut seen = HashSet::new();
        for message in messages {
            if seen.insert(message.channel_id) {
                order.push(message.channel_id);
            }
        }
        let items: Vec<NotificationInboxItem> = order
            .into_iter()
            .filter_map(|channel_id| {
                let mut item = self.inbox_channel_meta(channel_id)?;
                let channel_messages: Vec<&MessageInfo> = messages
                    .iter()
                    .filter(|message| message.channel_id == channel_id)
                    .collect();
                item.messages = self.inbox_channel_previews(&channel_messages);
                item.load = NotificationInboxChannelLoad::Loaded;
                Some(item)
            })
            .collect();
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.mentions = items;
            inbox.mentions_status = NotificationInboxLoad::Loaded;
        }
    }

    pub(in crate::tui) fn apply_inbox_mentions_load_failed(&mut self, request_id: u64) {
        if !self.inbox_request_matches(request_id) {
            return;
        }
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.mentions_status = NotificationInboxLoad::Failed;
        }
    }

    pub(in crate::tui) fn apply_inbox_channel_messages_loaded(
        &mut self,
        request_id: u64,
        channel_id: Id<ChannelMarker>,
        messages: &[MessageInfo],
    ) {
        if !self.inbox_request_matches(request_id) {
            return;
        }
        let refs: Vec<&MessageInfo> = messages.iter().collect();
        let previews = self.inbox_channel_previews(&refs);
        if let Some(inbox) = self.popups.notification_inbox_mut()
            && let Some(item) = inbox
                .unreads
                .iter_mut()
                .find(|item| item.channel_id == channel_id)
        {
            item.messages = previews;
            item.load = NotificationInboxChannelLoad::Loaded;
        }
    }

    pub(in crate::tui) fn apply_inbox_channel_messages_load_failed(
        &mut self,
        request_id: u64,
        channel_id: Id<ChannelMarker>,
    ) {
        if !self.inbox_request_matches(request_id) {
            return;
        }
        if let Some(inbox) = self.popups.notification_inbox_mut()
            && let Some(item) = inbox
                .unreads
                .iter_mut()
                .find(|item| item.channel_id == channel_id)
        {
            // Stop the spinner; the row falls back to its unread-count line.
            item.load = NotificationInboxChannelLoad::Loaded;
        }
    }

    fn inbox_request_matches(&self, request_id: u64) -> bool {
        self.popups
            .notification_inbox()
            .is_some_and(|inbox| inbox.request_id == request_id)
    }

    fn remove_notification_inbox_item(&mut self, tab: NotificationInboxTab, index: usize) {
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            let items = match tab {
                NotificationInboxTab::Unreads => &mut inbox.unreads,
                NotificationInboxTab::Mentions => &mut inbox.mentions,
            };
            if index < items.len() {
                items.remove(index);
            }
        }
    }

    fn ensure_unread_inbox_requests(&mut self) {
        let upto = {
            let Some(inbox) = self.popups.notification_inbox() else {
                return;
            };
            if inbox.tab != NotificationInboxTab::Unreads {
                return;
            }
            (inbox.selected_index() + 1 + UNREAD_REQUEST_LOOKAHEAD).min(inbox.unreads.len())
        };
        self.request_unread_inbox_history(upto);
    }

    fn request_unread_inbox_history(&mut self, upto: usize) {
        let (request_id, to_request): (u64, Vec<Id<ChannelMarker>>) = {
            let Some(inbox) = self.popups.notification_inbox() else {
                return;
            };
            let channels = inbox
                .unreads
                .iter()
                .take(upto)
                .filter(|item| item.load == NotificationInboxChannelLoad::Pending)
                .map(|item| item.channel_id)
                .collect();
            (inbox.request_id, channels)
        };
        if to_request.is_empty() {
            return;
        }
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            for item in inbox.unreads.iter_mut() {
                if to_request.contains(&item.channel_id) {
                    item.load = NotificationInboxChannelLoad::Loading;
                }
            }
        }
        for channel_id in to_request {
            self.enqueue_pending_command(AppCommand::LoadInboxChannelHistory {
                channel_id,
                request_id,
            });
        }
    }

    fn navigate_to_inbox_channel(&mut self, channel_id: Id<ChannelMarker>) -> Option<AppCommand> {
        let channel = self.discord.cache.channel(channel_id)?;
        let guild_id = channel.guild_id;
        let parent_id = channel.parent_id;
        match guild_id {
            Some(guild_id) => {
                self.activate_guild(ActiveGuildScope::Guild(guild_id));
                if let Some(parent_id) = parent_id {
                    self.navigation
                        .channels
                        .collapsed_channel_categories
                        .remove(&parent_id);
                }
                self.restore_channel_cursor(Some(channel_id));
                self.activate_channel(channel_id);
                Some(AppCommand::SubscribeGuildChannel {
                    guild_id,
                    channel_id,
                })
            }
            None => {
                self.activate_guild(ActiveGuildScope::DirectMessages);
                self.restore_channel_cursor(Some(channel_id));
                self.activate_channel(channel_id);
                Some(AppCommand::SubscribeDirectMessage { channel_id })
            }
        }
    }

    /// Per-channel (not aggregated) so a forum and its threads don't double-count.
    fn build_unread_inbox_items(&self) -> Vec<NotificationInboxItem> {
        let mut channels: Vec<&ChannelState> = self.discord.cache.channels_for_guild(None);
        let mut seen_guilds = HashSet::new();
        for entry in self.guild_pane_entries() {
            let GuildPaneEntry::Guild { state: guild, .. } = entry else {
                continue;
            };
            if seen_guilds.insert(guild.id) {
                channels.extend(
                    self.discord
                        .cache
                        .viewable_channels_for_guild(Some(guild.id)),
                );
            }
        }

        channels
            .into_iter()
            .filter(|channel| !channel.is_category())
            // Unjoined threads never surface unread, mirroring the sidebar.
            .filter(|channel| !channel.is_thread() || channel.current_user_joined_thread)
            .filter(|channel| self.channel_unread(channel.id) != ChannelUnreadState::Seen)
            .filter(|channel| !self.channel_notification_muted(channel.id))
            .map(|channel| self.inbox_channel_meta_from(channel))
            .collect()
    }

    fn inbox_channel_meta(&self, channel_id: Id<ChannelMarker>) -> Option<NotificationInboxItem> {
        let channel = self.discord.cache.channel(channel_id)?;
        Some(self.inbox_channel_meta_from(channel))
    }

    fn inbox_channel_meta_from(&self, channel: &ChannelState) -> NotificationInboxItem {
        let context = channel.guild_id.map(|guild_id| {
            let guild = self
                .guild_name(guild_id)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("guild-{}", guild_id.get()));
            match channel
                .parent_id
                .and_then(|parent_id| self.discord.cache.channel(parent_id))
                .filter(|parent| !parent.is_category())
            {
                Some(parent) => format!("{guild} › #{}", parent.name),
                None => guild,
            }
        });
        NotificationInboxItem {
            channel_id: channel.id,
            guild_id: channel.guild_id,
            ack_target: self.discord.cache.channel_ack_target(channel.id),
            title: self.channel_label(channel.id),
            context,
            unread: self.channel_unread(channel.id),
            messages: Vec::new(),
            load: NotificationInboxChannelLoad::Pending,
        }
    }

    fn inbox_channel_previews(&self, messages: &[&MessageInfo]) -> Vec<NotificationInboxMessage> {
        let current_user = self.current_user_id();
        let mut ordered: Vec<&&MessageInfo> = messages
            .iter()
            .filter(|message| Some(message.author_id) != current_user)
            .collect();
        ordered.sort_by_key(|message| message.message_id);
        let start = ordered.len().saturating_sub(MAX_INBOX_MESSAGES_PER_CHANNEL);
        ordered[start..]
            .iter()
            .map(|message| self.inbox_message_preview(message))
            .collect()
    }

    fn inbox_message_preview(&self, message: &MessageInfo) -> NotificationInboxMessage {
        let content = match message
            .content
            .as_deref()
            .map(str::trim)
            .filter(|content| !content.is_empty())
        {
            Some(text) => self
                .render_user_mentions(message.guild_id, &message.mentions, text)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            None if !message.attachments.is_empty() => "[attachment]".to_owned(),
            None if !message.sticker_names.is_empty() => {
                format!("[sticker] {}", message.sticker_names.join(", "))
            }
            None if !message.embeds.is_empty() => "[embed]".to_owned(),
            None => "<empty message>".to_owned(),
        };
        NotificationInboxMessage {
            author: message.author.clone(),
            content,
        }
    }
}
