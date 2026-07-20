use crate::discord::{
    AppCommand, ChannelState, ChannelUnreadState, MessageInfo,
    ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
    },
};
use crate::tui::keybindings::SelectionAction;

use super::super::{ActiveGuildScope, DashboardState, FocusPane};
use crate::tui::state::popups::{ActiveModalPopupKind, ModalPopup, SelectablePopupState};

const MAX_INBOX_MESSAGES_PER_CHANNEL: usize = 3;
const INITIAL_UNREAD_CHANNELS: usize = 4;
const INBOX_PAGE_REQUEST_LOOKAHEAD: usize = 3;
const UNREAD_REQUEST_LOOKAHEAD: usize = 2;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NotificationInboxTab {
    #[default]
    Unreads,
    Mentions,
}

impl NotificationInboxTab {
    fn step(self) -> Self {
        match self {
            Self::Unreads => Self::Mentions,
            Self::Mentions => Self::Unreads,
        }
    }
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationInboxMessage {
    pub author_id: Id<UserMarker>,
    pub author: String,
    pub author_role_ids: Vec<Id<RoleMarker>>,
    pub author_role_color: Option<u32>,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationInboxUnreadItem {
    pub channel_id: Id<ChannelMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub ack_target: Option<Id<MessageMarker>>,
    pub title: String,
    pub context: Option<String>,
    pub unread: ChannelUnreadState,
    pub messages: Vec<NotificationInboxMessage>,
    pub load: NotificationInboxChannelLoad,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationInboxMentionItem {
    pub channel_id: Id<ChannelMarker>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub message_id: Id<MessageMarker>,
    pub title: String,
    pub context: Option<String>,
    pub author_id: Id<UserMarker>,
    pub author: String,
    pub author_role_ids: Vec<Id<RoleMarker>>,
    pub author_role_color: Option<u32>,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NotificationInboxItem {
    Unread(NotificationInboxUnreadItem),
    Mention(NotificationInboxMentionItem),
}

#[derive(Debug)]
pub(in crate::tui::state) struct NotificationInboxState {
    request_id: u64,
    tab: NotificationInboxTab,
    unreads: Vec<NotificationInboxUnreadItem>,
    mentions: Vec<NotificationInboxMentionItem>,
    unreads_selection: SelectablePopupState,
    mentions_selection: SelectablePopupState,
    mentions_status: NotificationInboxLoad,
    mentions_has_more: bool,
    mentions_loading_more: bool,
    mentions_next_before: Option<Id<MessageMarker>>,
    confirming_mark_all: bool,
}

impl NotificationInboxState {
    fn new(request_id: u64, unreads: Vec<NotificationInboxUnreadItem>) -> Self {
        Self {
            request_id,
            tab: NotificationInboxTab::default(),
            unreads,
            mentions: Vec::new(),
            unreads_selection: SelectablePopupState::default(),
            mentions_selection: SelectablePopupState::default(),
            mentions_status: NotificationInboxLoad::Loading,
            mentions_has_more: false,
            mentions_loading_more: false,
            mentions_next_before: None,
            confirming_mark_all: false,
        }
    }

    fn active_len(&self) -> usize {
        match self.tab {
            NotificationInboxTab::Unreads => self.unreads.len(),
            NotificationInboxTab::Mentions => self.mentions.len(),
        }
    }

    fn active_items(&self) -> Vec<NotificationInboxItem> {
        match self.tab {
            NotificationInboxTab::Unreads => self
                .unreads
                .iter()
                .cloned()
                .map(NotificationInboxItem::Unread)
                .collect(),
            NotificationInboxTab::Mentions => self
                .mentions
                .iter()
                .cloned()
                .map(NotificationInboxItem::Mention)
                .collect(),
        }
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
        self.selection(self.tab).selected_for_len(self.active_len())
    }

    fn has_markable_items(&self) -> bool {
        self.tab == NotificationInboxTab::Unreads && !self.unreads.is_empty()
    }
}

impl DashboardState {
    pub fn open_notification_inbox(&mut self) {
        let request_id = self.next_inbox_request_id();
        let unreads = self.build_unread_inbox_items();
        self.popups.modal = Some(ModalPopup::NotificationInbox(NotificationInboxState::new(
            request_id, unreads,
        )));
        self.enqueue_pending_command(AppCommand::LoadInboxMentions {
            request_id,
            before: None,
        });
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
        let mut items = self
            .popups
            .notification_inbox()
            .map(NotificationInboxState::active_items)
            .unwrap_or_default();
        // Inbox snapshots can arrive before the member request that supplies
        // role IDs, so resolve author colors against the live cache on read.
        for item in &mut items {
            self.refresh_notification_inbox_item_role_colors(item);
        }
        items
    }

    pub fn notification_inbox_unread_count(&self) -> usize {
        self.popups
            .notification_inbox()
            .map(|inbox| inbox.unreads.len())
            .unwrap_or_default()
    }

    pub fn notification_inbox_mention_count(&self) -> usize {
        self.popups
            .notification_inbox()
            .map(|inbox| inbox.mentions.len())
            .unwrap_or_default()
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
        let (tab, len) = (inbox.tab, inbox.active_len());
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.selection_mut(tab).move_down(len);
        }
        self.ensure_notification_inbox_requests();
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
        let (tab, len) = (inbox.tab, inbox.active_len());
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.selection_mut(tab).page(len, action);
        }
        self.ensure_notification_inbox_requests();
    }

    pub fn switch_notification_inbox_tab(&mut self, _action: SelectionAction) {
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.tab = inbox.tab.step();
        }
        self.ensure_notification_inbox_requests();
    }

    pub fn activate_selected_notification_inbox_item(&mut self) -> Option<AppCommand> {
        let (tab, index) = {
            let inbox = self.popups.notification_inbox()?;
            (inbox.tab, inbox.selected_index())
        };
        match tab {
            NotificationInboxTab::Unreads => {
                let item = self
                    .popups
                    .notification_inbox()?
                    .unreads
                    .get(index)?
                    .clone();
                self.close_notification_inbox();
                self.navigate_to_inbox_channel(item.channel_id)
            }
            NotificationInboxTab::Mentions => {
                let item = self
                    .popups
                    .notification_inbox()?
                    .mentions
                    .get(index)?
                    .clone();
                self.close_notification_inbox();
                self.navigate_to_inbox_message(item.channel_id, item.message_id, item.guild_id)
            }
        }
    }

    pub fn mark_selected_notification_inbox_item_read(&mut self) -> Option<AppCommand> {
        let (tab, index) = {
            let inbox = self.popups.notification_inbox()?;
            (inbox.tab, inbox.selected_index())
        };
        match tab {
            NotificationInboxTab::Unreads => {
                let item = self
                    .popups
                    .notification_inbox()?
                    .unreads
                    .get(index)?
                    .clone();
                if let Some(inbox) = self.popups.notification_inbox_mut() {
                    inbox.unreads.remove(index);
                }
                let message_id = item.ack_target?;
                self.queue_ack_channel_command(item.channel_id, message_id);
                None
            }
            NotificationInboxTab::Mentions => {
                let message_id = self
                    .popups
                    .notification_inbox()?
                    .mentions
                    .get(index)?
                    .message_id;
                Some(AppCommand::DeleteInboxMention { message_id })
            }
        }
    }

    pub fn notification_inbox_is_confirming_mark_all(&self) -> bool {
        self.popups
            .notification_inbox()
            .is_some_and(|inbox| inbox.confirming_mark_all)
    }

    pub fn begin_mark_all_notification_inbox_read(&mut self) {
        let can_confirm = self
            .popups
            .notification_inbox()
            .is_some_and(NotificationInboxState::has_markable_items);
        if can_confirm {
            self.popups.confirmation_button = super::ConfirmationButton::default();
        }
        if let Some(inbox) = self.popups.notification_inbox_mut()
            && can_confirm
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
        let tab = self.notification_inbox_tab()?;
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.confirming_mark_all = false;
        }
        if tab != NotificationInboxTab::Unreads {
            return None;
        }

        let targets = {
            let inbox = self.popups.notification_inbox()?;
            inbox
                .unreads
                .iter()
                .filter_map(|item| {
                    item.ack_target
                        .map(|message_id| (item.channel_id, message_id))
                })
                .collect::<Vec<_>>()
        };
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.unreads.clear();
        }
        if !targets.is_empty() {
            self.queue_ack_channels_command(targets);
        }
        None
    }

    pub(in crate::tui) fn apply_inbox_mentions_loaded(
        &mut self,
        request_id: u64,
        before: Option<Id<MessageMarker>>,
        messages: &[MessageInfo],
        has_more: bool,
    ) {
        if !self.inbox_request_matches(request_id) {
            return;
        }
        let items = messages
            .iter()
            .map(|message| self.inbox_mention_item(message))
            .collect::<Vec<_>>();
        let next_before = messages.last().map(|message| message.message_id);
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            if before.is_none() {
                inbox.mentions = items;
            } else {
                for item in items {
                    if !inbox
                        .mentions
                        .iter()
                        .any(|existing| existing.message_id == item.message_id)
                    {
                        inbox.mentions.push(item);
                    }
                }
            }
            inbox.mentions_status = NotificationInboxLoad::Loaded;
            inbox.mentions_has_more = has_more;
            inbox.mentions_loading_more = false;
            inbox.mentions_next_before = if has_more { next_before } else { None };
        }
    }

    pub(in crate::tui) fn apply_inbox_mentions_load_failed(
        &mut self,
        request_id: u64,
        before: Option<Id<MessageMarker>>,
    ) {
        if !self.inbox_request_matches(request_id) {
            return;
        }
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            if before.is_none() {
                inbox.mentions_status = NotificationInboxLoad::Failed;
            }
            inbox.mentions_loading_more = false;
        }
    }

    pub(in crate::tui) fn apply_inbox_recent_mention_deleted(
        &mut self,
        message_id: Id<MessageMarker>,
    ) {
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.mentions.retain(|item| item.message_id != message_id);
        }
        self.ensure_mentions_inbox_request();
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
        let refs = messages.iter().collect::<Vec<_>>();
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
            item.load = NotificationInboxChannelLoad::Loaded;
        }
    }

    fn inbox_request_matches(&self, request_id: u64) -> bool {
        self.popups
            .notification_inbox()
            .is_some_and(|inbox| inbox.request_id == request_id)
    }

    fn ensure_notification_inbox_requests(&mut self) {
        let Some(inbox) = self.popups.notification_inbox() else {
            return;
        };
        match inbox.tab {
            NotificationInboxTab::Unreads => self.ensure_unread_inbox_requests(),
            NotificationInboxTab::Mentions => self.ensure_mentions_inbox_request(),
        }
    }

    fn ensure_unread_inbox_requests(&mut self) {
        let upto = {
            let Some(inbox) = self.popups.notification_inbox() else {
                return;
            };
            (inbox.selected_index() + 1 + UNREAD_REQUEST_LOOKAHEAD).min(inbox.unreads.len())
        };
        self.request_unread_inbox_history(upto);
    }

    fn ensure_mentions_inbox_request(&mut self) {
        let request = {
            let Some(inbox) = self.popups.notification_inbox() else {
                return;
            };
            let near_end =
                inbox.selected_index() + INBOX_PAGE_REQUEST_LOOKAHEAD >= inbox.mentions.len();
            if !near_end || !inbox.mentions_has_more || inbox.mentions_loading_more {
                return;
            }
            inbox
                .mentions_next_before
                .map(|before| (inbox.request_id, before))
        };
        let Some((request_id, before)) = request else {
            return;
        };
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.mentions_loading_more = true;
        }
        self.enqueue_pending_command(AppCommand::LoadInboxMentions {
            request_id,
            before: Some(before),
        });
    }

    fn request_unread_inbox_history(&mut self, upto: usize) {
        let (request_id, to_request) = {
            let Some(inbox) = self.popups.notification_inbox() else {
                return;
            };
            let channels = inbox
                .unreads
                .iter()
                .take(upto)
                .filter(|item| item.load == NotificationInboxChannelLoad::Pending)
                .map(|item| item.channel_id)
                .collect::<Vec<_>>();
            (inbox.request_id, channels)
        };
        if to_request.is_empty() {
            return;
        }
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            for item in &mut inbox.unreads {
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

    fn navigate_to_inbox_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<AppCommand> {
        let guild_id = guild_id.or_else(|| {
            self.discord
                .cache
                .channel(channel_id)
                .and_then(|channel| channel.guild_id)
        });
        match guild_id {
            Some(guild_id) => self.activate_guild(ActiveGuildScope::Guild(guild_id)),
            None => self.activate_guild(ActiveGuildScope::DirectMessages),
        }
        self.restore_channel_cursor(Some(channel_id));
        self.activate_channel(channel_id);
        self.focus_pane(FocusPane::Messages);
        Some(AppCommand::LoadMessageHistoryAround {
            channel_id,
            message_id,
        })
    }

    fn build_unread_inbox_items(&self) -> Vec<NotificationInboxUnreadItem> {
        let mut channels: Vec<&ChannelState> = self.discord.cache.channels_for_guild(None);
        for guild in self.discord.cache.guilds() {
            channels.extend(
                self.discord
                    .cache
                    .viewable_channels_for_guild(Some(guild.id)),
            );
        }

        channels
            .into_iter()
            .filter(|channel| !channel.is_category())
            .filter(|channel| !channel.is_thread() || channel.current_user_joined_thread)
            .filter(|channel| self.channel_unread(channel.id) != ChannelUnreadState::Seen)
            .filter(|channel| !self.channel_notification_muted(channel.id))
            .map(|channel| self.inbox_channel_meta_from(channel))
            .collect()
    }

    fn inbox_channel_meta_from(&self, channel: &ChannelState) -> NotificationInboxUnreadItem {
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
        NotificationInboxUnreadItem {
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

    fn inbox_mention_item(&self, message: &MessageInfo) -> NotificationInboxMentionItem {
        let channel = self.discord.cache.channel(message.channel_id);
        let title = channel
            .map(|channel| self.channel_label(channel.id))
            .unwrap_or_else(|| format!("#{}", message.channel_id.get()));
        let context = message.guild_id.map(|guild_id| {
            self.guild_name(guild_id)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("guild-{}", guild_id.get()))
        });
        let preview = self.inbox_message_preview(message);
        NotificationInboxMentionItem {
            channel_id: message.channel_id,
            guild_id: message.guild_id,
            message_id: message.message_id,
            title,
            context,
            author_id: preview.author_id,
            author: preview.author,
            author_role_ids: preview.author_role_ids,
            author_role_color: preview.author_role_color,
            content: preview.content,
        }
    }

    fn inbox_channel_previews(&self, messages: &[&MessageInfo]) -> Vec<NotificationInboxMessage> {
        let current_user = self.current_user_id();
        let mut ordered = messages
            .iter()
            .filter(|message| Some(message.author_id) != current_user)
            .collect::<Vec<_>>();
        ordered.sort_by_key(|message| message.message_id);
        let start = ordered.len().saturating_sub(MAX_INBOX_MESSAGES_PER_CHANNEL);
        ordered[start..]
            .iter()
            .map(|message| self.inbox_message_preview(message))
            .collect()
    }

    fn inbox_message_preview(&self, message: &MessageInfo) -> NotificationInboxMessage {
        let author_role_color = self.inbox_author_role_color(
            message.guild_id,
            message.channel_id,
            message.author_id,
            &message.author_role_ids,
        );
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
            author_id: message.author_id,
            author: message.author.clone(),
            author_role_ids: message.author_role_ids.clone(),
            author_role_color,
            content,
        }
    }

    fn refresh_notification_inbox_item_role_colors(&self, item: &mut NotificationInboxItem) {
        match item {
            NotificationInboxItem::Unread(item) => {
                for message in &mut item.messages {
                    message.author_role_color = self.inbox_author_role_color(
                        item.guild_id,
                        item.channel_id,
                        message.author_id,
                        &message.author_role_ids,
                    );
                }
            }
            NotificationInboxItem::Mention(item) => {
                item.author_role_color = self.inbox_author_role_color(
                    item.guild_id,
                    item.channel_id,
                    item.author_id,
                    &item.author_role_ids,
                );
            }
        }
    }

    fn inbox_author_role_color(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        author_id: Id<UserMarker>,
        author_role_ids: &[Id<RoleMarker>],
    ) -> Option<u32> {
        let guild_id = guild_id.or_else(|| {
            self.discord
                .cache
                .channel(channel_id)
                .and_then(|channel| channel.guild_id)
        })?;
        self.discord
            .cache
            .user_role_color(guild_id, author_id)
            .or_else(|| {
                self.discord
                    .cache
                    .role_color_for_ids(guild_id, author_role_ids)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_tabs_toggle() {
        assert_eq!(
            NotificationInboxTab::Unreads.step(),
            NotificationInboxTab::Mentions
        );
        assert_eq!(
            NotificationInboxTab::Mentions.step(),
            NotificationInboxTab::Unreads
        );
    }
}
