use std::{
    cmp::Reverse,
    collections::{BTreeMap, VecDeque},
};

use crate::discord::{
    AppCommand, ChannelState, ChannelUnreadState, MessageInfo, StickerInfo,
    ids::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
    },
};
use crate::tui::keybindings::SelectionAction;

use super::super::{
    ActiveGuildScope, DashboardState, FocusPane, channel_tree,
    presentation::sort_direct_message_channels,
};
use crate::tui::state::popups::{
    ActiveModalPopupKind, ModalPopup, SelectablePopupState, SelectablePopupTarget,
};

const MAX_INBOX_MESSAGES_PER_CHANNEL: usize = 3;
const UNREAD_PAGE_SIZE: usize = 4;
const INBOX_PAGE_REQUEST_LOOKAHEAD: usize = 3;

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
    pub channel_id: Id<ChannelMarker>,
    pub message_id: Id<MessageMarker>,
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
    pub fallback: Option<String>,
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
    queued_unreads: VecDeque<NotificationInboxUnreadItem>,
    loading_unread_page: Option<Vec<NotificationInboxUnreadItem>>,
    unread_history_in_flight: Option<Id<ChannelMarker>>,
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
        let mut queued_unreads = VecDeque::from(unreads);
        let first_page_len = UNREAD_PAGE_SIZE.min(queued_unreads.len());
        let unreads = queued_unreads.drain(..first_page_len).collect();

        Self {
            request_id,
            tab: NotificationInboxTab::default(),
            unreads,
            queued_unreads,
            loading_unread_page: None,
            unread_history_in_flight: None,
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

    pub(super) fn active_len(&self) -> usize {
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

    pub(super) fn selection(&self, tab: NotificationInboxTab) -> &SelectablePopupState {
        match tab {
            NotificationInboxTab::Unreads => &self.unreads_selection,
            NotificationInboxTab::Mentions => &self.mentions_selection,
        }
    }

    pub(super) fn selection_mut(&mut self, tab: NotificationInboxTab) -> &mut SelectablePopupState {
        match tab {
            NotificationInboxTab::Unreads => &mut self.unreads_selection,
            NotificationInboxTab::Mentions => &mut self.mentions_selection,
        }
    }

    fn selected_index(&self) -> usize {
        self.selection(self.tab).selected_for_len(self.active_len())
    }

    fn has_markable_items(&self) -> bool {
        self.tab == NotificationInboxTab::Unreads && self.unread_count() > 0
    }

    fn unread_count(&self) -> usize {
        self.unreads.len()
            + self.queued_unreads.len()
            + self
                .loading_unread_page
                .as_ref()
                .map(Vec::len)
                .unwrap_or_default()
    }

    fn unread_ack_targets(&self) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        self.unreads
            .iter()
            .chain(self.loading_unread_page.iter().flatten())
            .chain(self.queued_unreads.iter())
            .filter_map(|item| {
                item.ack_target
                    .map(|message_id| (item.channel_id, message_id))
            })
            .collect()
    }

    fn clear_unreads(&mut self) {
        self.unreads.clear();
        self.queued_unreads.clear();
        self.loading_unread_page = None;
    }

    fn unread_item_mut(
        &mut self,
        channel_id: Id<ChannelMarker>,
    ) -> Option<&mut NotificationInboxUnreadItem> {
        if let Some(item) = self
            .unreads
            .iter_mut()
            .find(|item| item.channel_id == channel_id)
        {
            return Some(item);
        }
        self.loading_unread_page
            .as_mut()?
            .iter_mut()
            .find(|item| item.channel_id == channel_id)
    }

    fn next_unread_history_channel(&mut self) -> Option<Id<ChannelMarker>> {
        self.append_settled_unread_page();
        if self.unread_history_in_flight.is_some() {
            return None;
        }

        if let Some(item) = self
            .unreads
            .iter_mut()
            .find(|item| item.load == NotificationInboxChannelLoad::Pending)
        {
            item.load = NotificationInboxChannelLoad::Loading;
            let channel_id = item.channel_id;
            self.unread_history_in_flight = Some(channel_id);
            return Some(channel_id);
        }

        if self.loading_unread_page.is_none() && self.should_start_next_unread_page() {
            let page_len = UNREAD_PAGE_SIZE.min(self.queued_unreads.len());
            self.loading_unread_page = Some(self.queued_unreads.drain(..page_len).collect());
        }

        let item = self
            .loading_unread_page
            .as_mut()?
            .iter_mut()
            .find(|item| item.load == NotificationInboxChannelLoad::Pending)?;
        item.load = NotificationInboxChannelLoad::Loading;
        let channel_id = item.channel_id;
        self.unread_history_in_flight = Some(channel_id);
        Some(channel_id)
    }

    fn finish_unread_history_request(&mut self, channel_id: Id<ChannelMarker>) {
        if self.unread_history_in_flight == Some(channel_id) {
            self.unread_history_in_flight = None;
        }
    }

    fn should_start_next_unread_page(&self) -> bool {
        if self.queued_unreads.is_empty() {
            return false;
        }
        self.unreads.is_empty()
            || self.unreads_selection.selected_for_len(self.unreads.len()) + 1 == self.unreads.len()
    }

    fn append_settled_unread_page(&mut self) {
        let settled = self.loading_unread_page.as_ref().is_some_and(|page| {
            page.iter()
                .all(|item| item.load == NotificationInboxChannelLoad::Loaded)
        });
        if settled {
            self.unreads.extend(
                self.loading_unread_page
                    .take()
                    .expect("settled unread page exists"),
            );
        }
    }
}

impl DashboardState {
    pub fn open_notification_inbox(&mut self) {
        let request_id = self.next_inbox_request_id();
        let unreads = self.build_unread_inbox_items();
        self.popups
            .set_modal(ModalPopup::NotificationInbox(NotificationInboxState::new(
                request_id, unreads,
            )));
        self.enqueue_pending_command(AppCommand::LoadInboxMentions {
            request_id,
            before: None,
        });
        self.ensure_unread_inbox_requests();
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
        // identity and roles, so resolve both against the live cache on read.
        for item in &mut items {
            self.refresh_notification_inbox_item_authors(item);
        }
        items
    }

    pub fn notification_inbox_unread_count(&self) -> usize {
        self.popups
            .notification_inbox()
            .map(NotificationInboxState::unread_count)
            .unwrap_or_default()
    }

    pub(in crate::tui) fn notification_inbox_is_loading_more_unreads(&self) -> bool {
        self.popups
            .notification_inbox()
            .is_some_and(|inbox| inbox.loading_unread_page.is_some())
    }

    pub(in crate::tui) fn notification_inbox_unread_mention_count(&self) -> u32 {
        self.discord.total_mention_count()
    }

    pub fn notification_inbox_mentions_status(&self) -> Option<NotificationInboxLoad> {
        self.popups
            .notification_inbox()
            .map(|inbox| inbox.mentions_status)
    }

    pub(in crate::tui) fn notification_inbox_is_loading_more_mentions(&self) -> bool {
        self.popups
            .notification_inbox()
            .is_some_and(|inbox| inbox.mentions_loading_more)
    }

    pub(in crate::tui) fn notification_inbox_has_visible_loading_indicator(&self) -> bool {
        self.popups
            .notification_inbox()
            .is_some_and(|inbox| match inbox.tab {
                NotificationInboxTab::Unreads => inbox.loading_unread_page.is_some(),
                NotificationInboxTab::Mentions => {
                    inbox.mentions_status == NotificationInboxLoad::Loading
                        || inbox.mentions_loading_more
                }
            })
    }

    pub fn selected_notification_inbox_index(&self) -> Option<usize> {
        self.popups
            .notification_inbox()
            .map(NotificationInboxState::selected_index)
    }

    pub fn move_notification_inbox_down(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::NotificationInbox,
            SelectionAction::Next,
        );
    }

    pub fn move_notification_inbox_up(&mut self) {
        self.move_selectable_popup(
            SelectablePopupTarget::NotificationInbox,
            SelectionAction::Previous,
        );
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
                if let Some(message_id) = item.ack_target {
                    self.queue_ack_channel_command(item.channel_id, message_id);
                }
                self.ensure_unread_inbox_requests();
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
            inbox.unread_ack_targets()
        };
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.clear_unreads();
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
            .filter(|message| {
                self.discord
                    .cache
                    .channel_notification_eligible(message.channel_id)
            })
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
        let mut previews = self.inbox_channel_previews(&refs);
        if previews.is_empty() {
            previews = self.inbox_cached_channel_previews(channel_id);
        }
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.finish_unread_history_request(channel_id);
            if let Some(item) = inbox.unread_item_mut(channel_id) {
                item.messages = previews;
                item.load = NotificationInboxChannelLoad::Loaded;
            }
        }
        self.ensure_unread_inbox_requests();
    }

    pub(in crate::tui) fn apply_inbox_channel_messages_load_failed(
        &mut self,
        request_id: u64,
        channel_id: Id<ChannelMarker>,
    ) {
        if !self.inbox_request_matches(request_id) {
            return;
        }
        let previews = self.inbox_cached_channel_previews(channel_id);
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.finish_unread_history_request(channel_id);
            if let Some(item) = inbox.unread_item_mut(channel_id) {
                item.messages = previews;
                item.load = NotificationInboxChannelLoad::Loaded;
            }
        }
        self.ensure_unread_inbox_requests();
    }

    pub(in crate::tui) fn apply_inbox_forum_post_data_loaded(
        &mut self,
        channel_id: Id<ChannelMarker>,
    ) {
        let is_requested_forum = self
            .popups
            .notification_inbox()
            .is_some_and(|inbox| inbox.unread_history_in_flight == Some(channel_id));
        if !is_requested_forum {
            return;
        }

        let missing_thread_ids = self
            .discord
            .cache
            .active_thread_ids_for_parent(channel_id)
            .into_iter()
            .filter(|thread_id| !self.discord.cache.thread_post_data_loaded(*thread_id))
            .take(10)
            .collect::<Vec<_>>();
        if !missing_thread_ids.is_empty() {
            if let Some(guild_id) = self
                .discord
                .cache
                .channel(channel_id)
                .and_then(|channel| channel.guild_id)
            {
                self.enqueue_pending_command(AppCommand::LoadForumPostData {
                    guild_id,
                    channel_id,
                    thread_ids: missing_thread_ids,
                });
            }
            return;
        }

        let previews = self.inbox_forum_post_previews(channel_id);
        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.finish_unread_history_request(channel_id);
            if let Some(item) = inbox.unread_item_mut(channel_id) {
                item.messages = previews;
                item.fallback = item
                    .messages
                    .is_empty()
                    .then(|| "No recent posts".to_owned());
                item.load = NotificationInboxChannelLoad::Loaded;
            }
        }
        self.ensure_unread_inbox_requests();
    }

    pub(in crate::tui) fn apply_inbox_forum_post_data_load_failed(
        &mut self,
        channel_id: Id<ChannelMarker>,
    ) {
        let is_requested_forum = self
            .popups
            .notification_inbox()
            .is_some_and(|inbox| inbox.unread_history_in_flight == Some(channel_id));
        if !is_requested_forum {
            return;
        }

        if let Some(inbox) = self.popups.notification_inbox_mut() {
            inbox.finish_unread_history_request(channel_id);
            if let Some(item) = inbox.unread_item_mut(channel_id) {
                item.fallback = Some("Could not load recent posts".to_owned());
                item.load = NotificationInboxChannelLoad::Loaded;
            }
        }
        self.ensure_unread_inbox_requests();
    }

    fn inbox_request_matches(&self, request_id: u64) -> bool {
        self.popups
            .notification_inbox()
            .is_some_and(|inbox| inbox.request_id == request_id)
    }

    pub(super) fn ensure_notification_inbox_requests(&mut self) {
        let Some(inbox) = self.popups.notification_inbox() else {
            return;
        };
        match inbox.tab {
            NotificationInboxTab::Unreads => self.ensure_unread_inbox_requests(),
            NotificationInboxTab::Mentions => self.ensure_mentions_inbox_request(),
        }
    }

    fn ensure_unread_inbox_requests(&mut self) {
        let request = self.popups.notification_inbox_mut().and_then(|inbox| {
            inbox
                .next_unread_history_channel()
                .map(|channel_id| (inbox.request_id, channel_id))
        });
        let Some((request_id, channel_id)) = request else {
            return;
        };
        let forum_guild_id = self
            .discord
            .cache
            .channel(channel_id)
            .filter(|channel| channel.is_forum())
            .and_then(|channel| channel.guild_id);
        let Some(guild_id) = forum_guild_id else {
            self.enqueue_pending_command(AppCommand::LoadInboxChannelHistory {
                channel_id,
                request_id,
            });
            return;
        };
        let missing_thread_ids = self
            .discord
            .cache
            .active_thread_ids_for_parent(channel_id)
            .into_iter()
            .filter(|thread_id| !self.discord.cache.thread_post_data_loaded(*thread_id))
            .take(10)
            .collect::<Vec<_>>();
        if missing_thread_ids.is_empty() {
            self.apply_inbox_forum_post_data_loaded(channel_id);
        } else {
            self.enqueue_pending_command(AppCommand::LoadForumPostData {
                guild_id,
                channel_id,
                thread_ids: missing_thread_ids,
            });
        }
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
        let scope = match guild_id {
            Some(guild_id) => ActiveGuildScope::Guild(guild_id),
            None => ActiveGuildScope::DirectMessages,
        };
        self.activate_message_history_channel(channel_id, Some(scope));
        self.focus_pane(FocusPane::Messages);
        Some(AppCommand::LoadMessageHistoryAround {
            channel_id,
            message_id,
        })
    }

    fn build_unread_inbox_items(&self) -> Vec<NotificationInboxUnreadItem> {
        let channels = self.inbox_channels_in_display_order();

        channels
            .into_iter()
            // Discord tracks chat unread state for voice-like channels but does
            // not surface those channel chats in the Unreads Inbox.
            .filter(|channel| {
                !matches!(
                    channel.kind.as_str(),
                    "voice" | "GuildVoice" | "stage" | "GuildStageVoice"
                )
            })
            .filter_map(|channel| {
                let unread = self.discord.cache.channel_inbox_unread(channel.id);
                (unread != ChannelUnreadState::Seen)
                    .then(|| self.inbox_channel_meta_from(channel, unread))
            })
            .collect()
    }

    fn inbox_channels_in_display_order(&self) -> Vec<&ChannelState> {
        let mut ordered = self.discord.cache.channels_for_guild(None);
        ordered.retain(|channel| {
            !channel.is_thread() || self.discord.cache.thread_is_sidebar_active(channel.id)
        });
        sort_direct_message_channels(&mut ordered);

        for guild in self.guilds_in_display_order() {
            let channels = self
                .discord
                .cache
                .viewable_channels_for_guild(Some(guild.id));
            self.extend_inbox_channels_in_tree_order(&mut ordered, &channels);
        }

        ordered
    }

    fn extend_inbox_channels_in_tree_order<'a>(
        &self,
        ordered: &mut Vec<&'a ChannelState>,
        channels: &[&'a ChannelState],
    ) {
        let mut joined_threads_by_parent: BTreeMap<_, Vec<&ChannelState>> = BTreeMap::new();
        for channel in channels {
            if self.discord.cache.thread_is_sidebar_active(channel.id)
                && let Some(parent_id) = channel.parent_id
            {
                joined_threads_by_parent
                    .entry(parent_id)
                    .or_default()
                    .push(*channel);
            }
        }
        for threads in joined_threads_by_parent.values_mut() {
            channel_tree::sort_thread_channels(threads);
        }

        let mut push_channel = |channel: &'a ChannelState| {
            ordered.push(channel);
            if let Some(threads) = joined_threads_by_parent.get(&channel.id) {
                ordered.extend(threads.iter().copied());
            }
        };
        for root in channel_tree::sorted_channel_tree_roots(channels) {
            if root.is_category() {
                for child in channel_tree::sorted_category_children(channels, root.id) {
                    push_channel(child);
                }
            } else {
                push_channel(root);
            }
        }
    }

    fn inbox_channel_meta_from(
        &self,
        channel: &ChannelState,
        unread: ChannelUnreadState,
    ) -> NotificationInboxUnreadItem {
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
            unread,
            messages: Vec::new(),
            fallback: channel
                .is_thread()
                .then(|| format!("Post: {}", channel.name)),
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

    fn inbox_cached_channel_previews(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> Vec<NotificationInboxMessage> {
        let current_user = self.current_user_id();
        let messages = self.discord.cache.messages_for_channel(channel_id);
        let eligible = messages
            .into_iter()
            .filter(|message| Some(message.author_id) != current_user)
            .filter(|message| {
                message.content.is_some()
                    || !message.attachments.is_empty()
                    || !message.stickers.is_empty()
                    || !message.embeds.is_empty()
                    || !message.components.is_empty()
            })
            .collect::<Vec<_>>();
        let start = eligible
            .len()
            .saturating_sub(MAX_INBOX_MESSAGES_PER_CHANNEL);
        eligible[start..]
            .iter()
            .map(|message| NotificationInboxMessage {
                channel_id: message.channel_id,
                message_id: message.id,
                author_id: message.author_id,
                author: message.author.clone(),
                author_role_ids: Vec::new(),
                author_role_color: self.message_author_role_color(message),
                content: self.inbox_preview_content(
                    message.guild_id,
                    &message.mentions,
                    message.summary_text(),
                    !message.attachments.is_empty(),
                    &message.stickers,
                    !message.embeds.is_empty(),
                ),
            })
            .collect()
    }

    fn inbox_forum_post_previews(
        &self,
        forum_id: Id<ChannelMarker>,
    ) -> Vec<NotificationInboxMessage> {
        let mut ordered = self
            .discord
            .cache
            .active_thread_ids_for_parent(forum_id)
            .into_iter()
            .filter_map(|thread_id| self.discord.cache.channel(thread_id))
            .collect::<Vec<_>>();
        ordered.sort_by_key(|thread| {
            Reverse(
                thread
                    .last_message_id
                    .map(Id::get)
                    .unwrap_or_else(|| thread.id.get()),
            )
        });

        ordered
            .into_iter()
            .take(MAX_INBOX_MESSAGES_PER_CHANNEL)
            .filter_map(|thread| {
                let post = self.thread_card_item(thread, None, false);
                Some(NotificationInboxMessage {
                    channel_id: thread.id,
                    message_id: Id::new(thread.id.get()),
                    author_id: post.preview_author_id?,
                    author: post.preview_author?,
                    author_role_ids: Vec::new(),
                    author_role_color: post.preview_author_color,
                    content: post.label,
                })
            })
            .collect()
    }

    fn inbox_message_preview(&self, message: &MessageInfo) -> NotificationInboxMessage {
        let author_role_color = self.inbox_author_role_color(
            message.guild_id,
            message.channel_id,
            message.author_id,
            &message.author_role_ids,
        );
        let content = self.inbox_preview_content(
            message.guild_id,
            &message.mentions,
            message.summary_text(),
            !message.attachments.is_empty(),
            &message.stickers,
            !message.embeds.is_empty(),
        );
        NotificationInboxMessage {
            channel_id: message.channel_id,
            message_id: message.message_id,
            author_id: message.author_id,
            author: message.author.clone(),
            author_role_ids: message.author_role_ids.clone(),
            author_role_color,
            content,
        }
    }

    fn inbox_preview_content(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        mentions: &[crate::discord::MentionInfo],
        content: Option<&str>,
        has_attachments: bool,
        stickers: &[StickerInfo],
        has_embeds: bool,
    ) -> String {
        match content.map(str::trim).filter(|content| !content.is_empty()) {
            Some(text) => self
                .render_user_mentions(guild_id, mentions, text)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            None if has_attachments => "[attachment]".to_owned(),
            None if !stickers.is_empty() => {
                format!("[sticker] {}", StickerInfo::names(stickers).join(", "))
            }
            None if has_embeds => "[embed]".to_owned(),
            None => "<empty message>".to_owned(),
        }
    }

    fn refresh_notification_inbox_item_authors(&self, item: &mut NotificationInboxItem) {
        match item {
            NotificationInboxItem::Unread(item) => {
                for message in &mut item.messages {
                    message.author = self.channel_user_display_name(
                        message.channel_id,
                        message.author_id,
                        &message.author,
                    );
                    message.author_role_color =
                        self.inbox_message_author_role_color(item.guild_id, message);
                }
            }
            NotificationInboxItem::Mention(item) => {
                item.author =
                    self.channel_user_display_name(item.channel_id, item.author_id, &item.author);
                item.author_role_color = self.inbox_author_role_color(
                    item.guild_id,
                    item.channel_id,
                    item.author_id,
                    &item.author_role_ids,
                );
            }
        }
    }

    fn inbox_message_author_role_color(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        message: &NotificationInboxMessage,
    ) -> Option<u32> {
        let guild_id = guild_id.or_else(|| {
            self.discord
                .cache
                .channel(message.channel_id)
                .and_then(|channel| channel.guild_id)
        })?;
        self.discord
            .cache
            .message_author_role_color(
                guild_id,
                message.channel_id,
                message.message_id,
                message.author_id,
            )
            .or_else(|| {
                self.discord
                    .cache
                    .role_color_for_ids(guild_id, &message.author_role_ids)
            })
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

    fn unread_item(channel_id: u64) -> NotificationInboxUnreadItem {
        NotificationInboxUnreadItem {
            channel_id: Id::new(channel_id),
            guild_id: None,
            ack_target: Some(Id::new(channel_id + 100)),
            title: format!("channel-{channel_id}"),
            context: None,
            unread: ChannelUnreadState::Unread,
            messages: Vec::new(),
            fallback: None,
            load: NotificationInboxChannelLoad::Pending,
        }
    }

    fn take_unread_history_request(state: &mut DashboardState) -> (u64, Id<ChannelMarker>) {
        let commands = state.drain_pending_commands();
        assert_eq!(commands.len(), 1, "expected one unread history request");
        match commands.into_iter().next().expect("history request exists") {
            AppCommand::LoadInboxChannelHistory {
                channel_id,
                request_id,
            } => (request_id, channel_id),
            command => panic!("expected unread history request, got {command:?}"),
        }
    }

    #[test]
    fn unread_inbox_appends_the_next_page_after_its_previews_settle() {
        let request_id = 7;
        let mut state = DashboardState::new();
        state
            .popups
            .set_modal(ModalPopup::NotificationInbox(NotificationInboxState::new(
                request_id,
                (1..=6).map(unread_item).collect(),
            )));
        state.ensure_unread_inbox_requests();

        assert_eq!(state.notification_inbox_unread_count(), 6);
        assert_eq!(state.notification_inbox_items().len(), UNREAD_PAGE_SIZE);

        for expected_channel_id in 1..=UNREAD_PAGE_SIZE as u64 {
            let (command_request_id, channel_id) = take_unread_history_request(&mut state);
            assert_eq!(command_request_id, request_id);
            assert_eq!(channel_id, Id::new(expected_channel_id));
            state.apply_inbox_channel_messages_loaded(request_id, channel_id, &[]);
        }
        assert!(state.drain_pending_commands().is_empty());

        for _ in 1..UNREAD_PAGE_SIZE {
            state.move_notification_inbox_down();
        }
        for _ in 0..3 {
            state.move_notification_inbox_down();
        }
        assert!(state.notification_inbox_is_loading_more_unreads());
        assert_eq!(state.notification_inbox_items().len(), UNREAD_PAGE_SIZE);

        let (_, fifth_channel) = take_unread_history_request(&mut state);
        assert_eq!(fifth_channel, Id::new(5));
        state.apply_inbox_channel_messages_loaded(request_id, fifth_channel, &[]);
        assert!(state.notification_inbox_is_loading_more_unreads());
        assert_eq!(state.notification_inbox_items().len(), UNREAD_PAGE_SIZE);

        let (_, sixth_channel) = take_unread_history_request(&mut state);
        assert_eq!(sixth_channel, Id::new(6));
        state.apply_inbox_channel_messages_load_failed(request_id, sixth_channel);

        assert!(!state.notification_inbox_is_loading_more_unreads());
        let visible_channels = state
            .notification_inbox_items()
            .into_iter()
            .filter_map(|item| match item {
                NotificationInboxItem::Unread(item) => Some(item.channel_id),
                NotificationInboxItem::Mention(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            visible_channels,
            (1..=6).map(Id::<ChannelMarker>::new).collect::<Vec<_>>()
        );
        assert!(state.drain_pending_commands().is_empty());
    }
}
