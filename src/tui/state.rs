use std::collections::{HashMap, HashSet};

use ratatui::style::Color;
use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
};
use unicode_width::UnicodeWidthStr;

use crate::discord::{
    AppCommand, AppEvent, AttachmentInfo, ChannelRecipientState, ChannelState, CustomEmojiInfo,
    DiscordState, GuildFolder, GuildMemberState, GuildState, MentionInfo, MessageInfo,
    MessageSnapshotInfo, MessageState, PresenceStatus, ReactionEmoji, ReactionInfo,
    ReactionUsersInfo, RoleState,
};
use crate::logging;

use super::format::{
    RenderedText, TextHighlight, render_user_mentions, render_user_mentions_with_highlights,
};

const SCROLL_OFF: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusPane {
    Guilds,
    Channels,
    Messages,
    Members,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageActionKind {
    Reply,
    OpenThread,
    DownloadImage,
    AddReaction,
    RemoveReaction(usize),
    ShowReactionUsers,
    LoadPinnedMessages,
    SetPinned(bool),
    VotePollAnswer(u8),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageActionItem {
    pub kind: MessageActionKind,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmojiReactionItem {
    pub emoji: ReactionEmoji,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadSummary {
    pub channel_id: Id<ChannelMarker>,
    pub name: String,
    pub message_count: Option<u64>,
    pub total_message_sent: Option<u64>,
    pub archived: Option<bool>,
    pub locked: Option<bool>,
}

impl EmojiReactionItem {
    pub fn custom_image_url(&self) -> Option<String> {
        let ReactionEmoji::Custom { id, animated, .. } = self.emoji else {
            return None;
        };
        let extension = if animated { "gif" } else { "png" };
        Some(format!(
            "https://cdn.discordapp.com/emojis/{}.{}",
            id.get(),
            extension
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UnicodeEmojiReactionItem {
    emoji: &'static str,
    label: &'static str,
}

const EMOJI_REACTION_ITEMS: &[UnicodeEmojiReactionItem] = &[
    UnicodeEmojiReactionItem {
        emoji: "👍",
        label: "Thumbs up",
    },
    UnicodeEmojiReactionItem {
        emoji: "❤️",
        label: "Heart",
    },
    UnicodeEmojiReactionItem {
        emoji: "😂",
        label: "Laugh",
    },
    UnicodeEmojiReactionItem {
        emoji: "🎉",
        label: "Celebrate",
    },
    UnicodeEmojiReactionItem {
        emoji: "😮",
        label: "Surprised",
    },
    UnicodeEmojiReactionItem {
        emoji: "😢",
        label: "Sad",
    },
    UnicodeEmojiReactionItem {
        emoji: "🙏",
        label: "Thanks",
    },
    UnicodeEmojiReactionItem {
        emoji: "👀",
        label: "Looking",
    },
];

fn custom_emoji_reaction_item(emoji: &CustomEmojiInfo) -> EmojiReactionItem {
    EmojiReactionItem {
        emoji: ReactionEmoji::Custom {
            id: emoji.id,
            name: Some(emoji.name.clone()),
            animated: emoji.animated,
        },
        label: custom_emoji_label(&emoji.name),
    }
}

fn custom_emoji_label(name: &str) -> String {
    let words: Vec<String> = name
        .split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect();

    if words.is_empty() {
        name.to_owned()
    } else {
        words.join(" ")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageActionMenuState {
    selected: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmojiReactionPickerState {
    selected: usize,
    guild_id: Option<Id<GuildMarker>>,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReactionUsersPopupState {
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    reactions: Vec<ReactionUsersInfo>,
    scroll: usize,
}

impl ReactionUsersPopupState {
    pub fn reactions(&self) -> &[ReactionUsersInfo] {
        &self.reactions
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OlderHistoryRequestState {
    Requested { before: Id<MessageMarker> },
    Exhausted { before: Id<MessageMarker> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveGuildScope {
    Unset,
    DirectMessages,
    Guild(Id<GuildMarker>),
}

#[derive(Debug)]
pub struct DashboardState {
    discord: DiscordState,
    focus: FocusPane,
    active_guild: ActiveGuildScope,
    active_channel_id: Option<Id<ChannelMarker>>,
    selected_guild: usize,
    guild_scroll: usize,
    guild_view_height: usize,
    selected_channel: usize,
    channel_scroll: usize,
    channel_view_height: usize,
    selected_message: usize,
    message_scroll: usize,
    message_line_scroll: usize,
    message_keep_selection_visible: bool,
    message_auto_follow: bool,
    message_view_height: usize,
    message_content_width: usize,
    message_preview_width: u16,
    message_max_preview_height: u16,
    selected_member: usize,
    member_scroll: usize,
    member_view_height: usize,
    composer_input: String,
    composer_active: bool,
    reply_target_message_id: Option<Id<MessageMarker>>,
    message_action_menu: Option<MessageActionMenuState>,
    emoji_reaction_picker: Option<EmojiReactionPickerState>,
    reaction_users_popup: Option<ReactionUsersPopupState>,
    current_user: Option<String>,
    current_user_id: Option<Id<UserMarker>>,
    last_error: Option<String>,
    last_status: Option<String>,
    skipped_events: u64,
    should_quit: bool,
    older_history_requests: HashMap<Id<ChannelMarker>, OlderHistoryRequestState>,
    /// Folder IDs the user has collapsed in the guild pane. Single-guild
    /// "folders" (id = None) are never collapsible since they have no header.
    collapsed_folders: HashSet<FolderKey>,
    collapsed_channel_categories: HashSet<Id<ChannelMarker>>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum FolderKey {
    Id(u64),
    Guilds(Vec<Id<GuildMarker>>),
}

impl DashboardState {
    pub fn new() -> Self {
        Self {
            discord: DiscordState::default(),
            focus: FocusPane::Guilds,
            active_guild: ActiveGuildScope::Unset,
            active_channel_id: None,
            // Index 0 is the virtual "Direct Messages" entry. Start on the
            // first real guild when one exists; the bounds clamp inside
            // `selected_guild()` falls back to the DM entry while the guild
            // list is still empty.
            selected_guild: 1,
            guild_scroll: 0,
            guild_view_height: 1,
            selected_channel: 0,
            channel_scroll: 0,
            channel_view_height: 1,
            selected_message: 0,
            message_scroll: 0,
            message_line_scroll: 0,
            message_keep_selection_visible: true,
            message_auto_follow: true,
            message_view_height: 1,
            message_content_width: usize::MAX,
            message_preview_width: 0,
            message_max_preview_height: 0,
            selected_member: 0,
            member_scroll: 0,
            member_view_height: 1,
            composer_input: String::new(),
            composer_active: false,
            reply_target_message_id: None,
            message_action_menu: None,
            emoji_reaction_picker: None,
            reaction_users_popup: None,
            current_user: None,
            current_user_id: None,
            last_error: None,
            last_status: None,
            skipped_events: 0,
            should_quit: false,
            older_history_requests: HashMap::new(),
            collapsed_folders: HashSet::new(),
            collapsed_channel_categories: HashSet::new(),
        }
    }

    pub fn push_event(&mut self, event: AppEvent) {
        let selected_message_id = (!self.message_auto_follow)
            .then(|| {
                self.messages()
                    .get(self.selected_message())
                    .map(|message| message.id)
            })
            .flatten();
        let scroll_message_id = (!self.message_auto_follow)
            .then(|| {
                self.messages()
                    .get(self.message_scroll)
                    .map(|message| message.id)
            })
            .flatten();
        let channel_cursor_id = self.selected_channel_cursor_id();

        match &event {
            AppEvent::Ready { user, user_id } => {
                self.current_user = Some(user.clone());
                self.current_user_id = *user_id;
            }
            AppEvent::GatewayError { message } => {
                logging::error("app_event", message);
                self.last_error = Some(message.clone());
            }
            AppEvent::StatusMessage { message } => {
                self.last_status = Some(message.clone());
                self.last_error = None;
            }
            AppEvent::ReactionUsersLoaded {
                channel_id,
                message_id,
                reactions,
            } => {
                self.reaction_users_popup = Some(ReactionUsersPopupState {
                    channel_id: *channel_id,
                    message_id: *message_id,
                    reactions: reactions.clone(),
                    scroll: 0,
                });
                self.last_status = Some("loaded reacted users".to_owned());
                self.last_error = None;
            }
            AppEvent::MessageHistoryLoadFailed {
                channel_id,
                message,
            } => {
                logging::error("history", message);
                self.last_error = Some(message.clone());
                self.older_history_requests.remove(channel_id);
            }
            AppEvent::MessageHistoryLoaded {
                channel_id,
                before,
                messages,
            } => self.record_older_history_loaded(*channel_id, *before, messages),
            AppEvent::GatewayClosed => {
                self.last_error = Some("gateway closed".to_owned());
            }
            _ => {}
        }
        self.discord.apply_event(&event);
        self.clamp_active_selection();
        self.restore_channel_cursor(channel_cursor_id);
        self.clamp_selection_indices();
        if self.message_auto_follow {
            self.follow_latest_message();
        } else {
            self.restore_message_position(selected_message_id, scroll_message_id);
        }
        self.clamp_list_viewports();
        self.clamp_message_viewport();
    }

    pub fn record_lag(&mut self, skipped: u64) {
        self.skipped_events += skipped;
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn focus(&self) -> FocusPane {
        self.focus
    }

    pub fn current_user(&self) -> Option<&str> {
        self.current_user.as_deref()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn last_status(&self) -> Option<&str> {
        self.last_status.as_deref()
    }

    pub fn skipped_events(&self) -> u64 {
        self.skipped_events
    }

    pub fn is_composing(&self) -> bool {
        self.composer_active
    }

    pub fn is_message_action_menu_open(&self) -> bool {
        self.message_action_menu.is_some()
    }

    pub fn is_emoji_reaction_picker_open(&self) -> bool {
        self.emoji_reaction_picker.is_some()
    }

    pub fn is_reaction_users_popup_open(&self) -> bool {
        self.reaction_users_popup.is_some()
    }

    pub fn reaction_users_popup(&self) -> Option<&ReactionUsersPopupState> {
        self.reaction_users_popup.as_ref()
    }

    pub fn emoji_reaction_items(&self) -> Vec<EmojiReactionItem> {
        let mut items: Vec<EmojiReactionItem> = EMOJI_REACTION_ITEMS
            .iter()
            .map(|item| EmojiReactionItem {
                emoji: ReactionEmoji::Unicode(item.emoji.to_owned()),
                label: item.label.to_owned(),
            })
            .collect();

        let guild_id = self.picker_guild_id();

        if let Some(guild_id) = guild_id {
            items.extend(
                self.discord
                    .custom_emojis_for_guild(guild_id)
                    .iter()
                    .filter(|emoji| emoji.available)
                    .map(custom_emoji_reaction_item),
            );
        }

        items
    }

    pub fn open_selected_message_actions(&mut self) {
        if self.focus == FocusPane::Messages && self.selected_message_state().is_some() {
            self.message_action_menu = Some(MessageActionMenuState { selected: 0 });
        }
    }

    pub fn close_message_action_menu(&mut self) {
        self.message_action_menu = None;
    }

    pub fn close_emoji_reaction_picker(&mut self) {
        self.emoji_reaction_picker = None;
    }

    pub fn close_reaction_users_popup(&mut self) {
        self.reaction_users_popup = None;
    }

    pub fn scroll_reaction_users_popup_down(&mut self) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.scroll = popup.scroll.saturating_add(1);
        }
    }

    pub fn scroll_reaction_users_popup_up(&mut self) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.scroll = popup.scroll.saturating_sub(1);
        }
    }

    pub fn page_reaction_users_popup_down(&mut self) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.scroll = popup.scroll.saturating_add(10);
        }
    }

    pub fn page_reaction_users_popup_up(&mut self) {
        if let Some(popup) = &mut self.reaction_users_popup {
            popup.scroll = popup.scroll.saturating_sub(10);
        }
    }

    pub fn move_message_action_down(&mut self) {
        let actions_len = self.selected_message_action_items().len();
        if actions_len == 0 {
            return;
        }
        if let Some(menu) = &mut self.message_action_menu {
            menu.selected = (menu.selected + 1).min(actions_len - 1);
        }
    }

    pub fn move_message_action_up(&mut self) {
        if let Some(menu) = &mut self.message_action_menu {
            menu.selected = menu.selected.saturating_sub(1);
        }
    }

    pub fn move_emoji_reaction_down(&mut self) {
        let reactions_len = self.emoji_reaction_items().len();
        if reactions_len == 0 {
            return;
        }
        if let Some(picker) = &mut self.emoji_reaction_picker {
            picker.selected = (picker.selected + 1).min(reactions_len - 1);
        }
    }

    pub fn move_emoji_reaction_up(&mut self) {
        if let Some(picker) = &mut self.emoji_reaction_picker {
            picker.selected = picker.selected.saturating_sub(1);
        }
    }

    pub fn selected_message_action_items(&self) -> Vec<MessageActionItem> {
        let Some(message) = self.selected_message_state() else {
            return Vec::new();
        };
        let mut actions = vec![MessageActionItem {
            kind: MessageActionKind::Reply,
            label: "Reply".to_owned(),
            enabled: true,
        }];

        let capabilities = message.capabilities();
        if self.thread_summary_for_message(message).is_some() {
            actions.push(MessageActionItem {
                kind: MessageActionKind::OpenThread,
                label: "Open thread".to_owned(),
                enabled: true,
            });
        }
        if capabilities.has_image {
            actions.push(MessageActionItem {
                kind: MessageActionKind::DownloadImage,
                label: "Download image".to_owned(),
                enabled: true,
            });
        }
        actions.push(MessageActionItem {
            kind: MessageActionKind::AddReaction,
            label: "Add reaction".to_owned(),
            enabled: true,
        });
        actions.push(MessageActionItem {
            kind: MessageActionKind::LoadPinnedMessages,
            label: "Show pinned messages".to_owned(),
            enabled: true,
        });
        actions.push(MessageActionItem {
            kind: MessageActionKind::SetPinned(!message.pinned),
            label: if message.pinned {
                "Unpin message".to_owned()
            } else {
                "Pin message".to_owned()
            },
            enabled: true,
        });
        if !message.reactions.is_empty() {
            actions.push(MessageActionItem {
                kind: MessageActionKind::ShowReactionUsers,
                label: "Show reacted users".to_owned(),
                enabled: true,
            });
        }
        for (index, reaction) in message.reactions.iter().enumerate() {
            if reaction.me {
                actions.push(MessageActionItem {
                    kind: MessageActionKind::RemoveReaction(index),
                    label: format!("Remove {} reaction", reaction.emoji.status_label()),
                    enabled: true,
                });
            }
        }
        if let Some(poll) = &message.poll
            && !poll.results_finalized.unwrap_or(false)
        {
            for answer in &poll.answers {
                actions.push(MessageActionItem {
                    kind: MessageActionKind::VotePollAnswer(answer.answer_id),
                    label: if answer.me_voted {
                        format!("Remove poll vote: {}", answer.text)
                    } else {
                        format!("Vote poll: {}", answer.text)
                    },
                    enabled: true,
                });
            }
        }
        actions
    }

    pub fn selected_message_action_index(&self) -> Option<usize> {
        self.message_action_menu.as_ref().map(|menu| {
            menu.selected
                .min(self.selected_message_action_items().len().saturating_sub(1))
        })
    }

    pub fn selected_message_action(&self) -> Option<MessageActionItem> {
        let index = self.selected_message_action_index()?;
        self.selected_message_action_items().get(index).cloned()
    }

    pub fn selected_emoji_reaction_index(&self) -> Option<usize> {
        self.emoji_reaction_picker.as_ref().map(|picker| {
            picker
                .selected
                .min(self.emoji_reaction_items().len().saturating_sub(1))
        })
    }

    pub fn selected_emoji_reaction(&self) -> Option<EmojiReactionItem> {
        let index = self.selected_emoji_reaction_index()?;
        self.emoji_reaction_items().get(index).cloned()
    }

    pub fn activate_selected_message_action(&mut self) -> Option<AppCommand> {
        let action = self.selected_message_action()?;
        if !action.enabled {
            return None;
        }

        match action.kind {
            MessageActionKind::Reply => {
                self.start_reply_composer();
                self.close_message_action_menu();
                None
            }
            MessageActionKind::OpenThread => {
                let channel_id = self
                    .selected_message_state()
                    .and_then(|message| self.thread_summary_for_message(message))?
                    .channel_id;
                self.activate_channel(channel_id);
                self.close_message_action_menu();
                None
            }
            MessageActionKind::DownloadImage => {
                let (url, filename) =
                    self.selected_message_image_attachment()
                        .and_then(|attachment| {
                            attachment
                                .preferred_url()
                                .map(|url| (url.to_owned(), attachment.filename.clone()))
                        })?;
                self.close_message_action_menu();
                Some(AppCommand::DownloadAttachment { url, filename })
            }
            MessageActionKind::AddReaction => {
                self.open_emoji_reaction_picker();
                self.close_message_action_menu();
                None
            }
            MessageActionKind::RemoveReaction(index) => {
                let message = self.selected_message_state()?;
                let channel_id = message.channel_id;
                let message_id = message.id;
                let reaction = message.reactions.get(index)?.clone();
                self.close_message_action_menu();
                Some(AppCommand::RemoveReaction {
                    channel_id,
                    message_id,
                    emoji: reaction.emoji,
                })
            }
            MessageActionKind::ShowReactionUsers => {
                let message = self.selected_message_state()?;
                let channel_id = message.channel_id;
                let message_id = message.id;
                let reactions = message
                    .reactions
                    .iter()
                    .map(|reaction| reaction.emoji.clone())
                    .collect::<Vec<_>>();
                if reactions.is_empty() {
                    self.close_message_action_menu();
                    return None;
                }
                self.close_message_action_menu();
                Some(AppCommand::LoadReactionUsers {
                    channel_id,
                    message_id,
                    reactions,
                })
            }
            MessageActionKind::LoadPinnedMessages => {
                let channel_id = self.selected_message_state()?.channel_id;
                self.close_message_action_menu();
                Some(AppCommand::LoadPinnedMessages { channel_id })
            }
            MessageActionKind::SetPinned(pinned) => {
                let message = self.selected_message_state()?;
                let channel_id = message.channel_id;
                let message_id = message.id;
                self.close_message_action_menu();
                Some(AppCommand::SetMessagePinned {
                    channel_id,
                    message_id,
                    pinned,
                })
            }
            MessageActionKind::VotePollAnswer(answer_id) => {
                let message = self.selected_message_state()?;
                let channel_id = message.channel_id;
                let message_id = message.id;
                let poll = message.poll.as_ref()?;
                let mut answer_ids = if poll.allow_multiselect {
                    poll.answers
                        .iter()
                        .filter(|answer| answer.me_voted && answer.answer_id != answer_id)
                        .map(|answer| answer.answer_id)
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                if !poll
                    .answers
                    .iter()
                    .any(|answer| answer.answer_id == answer_id && answer.me_voted)
                {
                    answer_ids.push(answer_id);
                }
                self.close_message_action_menu();
                Some(AppCommand::VotePoll {
                    channel_id,
                    message_id,
                    answer_ids,
                })
            }
        }
    }

    pub fn activate_selected_emoji_reaction(&mut self) -> Option<AppCommand> {
        let picker = self.emoji_reaction_picker.clone()?;
        let reaction = self.selected_emoji_reaction()?;
        let command = AppCommand::AddReaction {
            channel_id: picker.channel_id,
            message_id: picker.message_id,
            emoji: reaction.emoji,
        };
        self.close_emoji_reaction_picker();
        Some(command)
    }

    fn open_emoji_reaction_picker(&mut self) {
        if let Some(message) = self.selected_message_state() {
            self.emoji_reaction_picker = Some(EmojiReactionPickerState {
                selected: 0,
                guild_id: message
                    .guild_id
                    .or_else(|| self.selected_channel_guild_id()),
                channel_id: message.channel_id,
                message_id: message.id,
            });
        }
    }

    fn picker_guild_id(&self) -> Option<Id<GuildMarker>> {
        self.emoji_reaction_picker
            .as_ref()
            .and_then(|picker| picker.guild_id)
            .or_else(|| {
                self.selected_message_state()
                    .and_then(|message| message.guild_id)
            })
            .or_else(|| self.selected_channel_guild_id())
    }

    fn selected_channel_guild_id(&self) -> Option<Id<GuildMarker>> {
        self.selected_channel_state()
            .and_then(|channel| channel.guild_id)
    }

    fn start_reply_composer(&mut self) {
        let Some(message_id) = self.selected_message_state().map(|message| message.id) else {
            return;
        };
        self.composer_input.clear();
        self.reply_target_message_id = Some(message_id);
        self.composer_active = true;
        self.focus = FocusPane::Messages;
    }

    pub fn composer_input(&self) -> &str {
        &self.composer_input
    }

    /// Builds the guild pane in display order: a virtual "Direct Messages"
    /// row, then each `guild_folders` entry expanded into either a single
    /// guild row (`id == None`, one member) or a folder header followed by
    /// indented children. Collapsed folders hide their children. Guilds that
    /// the user is in but the folder list omits get appended at the bottom.
    pub fn guild_pane_entries(&self) -> Vec<GuildPaneEntry<'_>> {
        let mut entries: Vec<GuildPaneEntry<'_>> = vec![GuildPaneEntry::DirectMessages];
        let by_id: HashMap<Id<GuildMarker>, &GuildState> = self
            .discord
            .guilds()
            .into_iter()
            .map(|guild| (guild.id, guild))
            .collect();
        let mut placed: HashSet<Id<GuildMarker>> = HashSet::new();
        let folders = self.discord.guild_folders();

        if folders.is_empty() {
            for guild in by_id.values() {
                entries.push(GuildPaneEntry::Guild {
                    state: guild,
                    branch: GuildBranch::None,
                });
            }
            return entries;
        }

        for folder in folders {
            let is_single_container = folder.id.is_none() && folder.guild_ids.len() == 1;
            if is_single_container {
                if let Some(guild) = by_id.get(&folder.guild_ids[0]) {
                    entries.push(GuildPaneEntry::Guild {
                        state: guild,
                        branch: GuildBranch::None,
                    });
                    placed.insert(folder.guild_ids[0]);
                }
                continue;
            }

            let folder_key = Self::folder_key(folder);
            let collapsed = folder_key
                .as_ref()
                .is_some_and(|key| self.collapsed_folders.contains(key));
            entries.push(GuildPaneEntry::FolderHeader { folder, collapsed });

            // Always mark children as placed even when collapsed so we don't
            // duplicate them in the trailing "ungrouped" loop.
            for guild_id in &folder.guild_ids {
                placed.insert(*guild_id);
            }

            if collapsed {
                continue;
            }

            let child_guilds: Vec<&GuildState> = folder
                .guild_ids
                .iter()
                .filter_map(|guild_id| by_id.get(guild_id).copied())
                .collect();
            let last_child_index = child_guilds.len().saturating_sub(1);
            for (index, guild) in child_guilds.into_iter().enumerate() {
                let branch = if index == last_child_index {
                    GuildBranch::Last
                } else {
                    GuildBranch::Middle
                };
                entries.push(GuildPaneEntry::Guild {
                    state: guild,
                    branch,
                });
            }
        }

        for guild in by_id.values() {
            if !placed.contains(&guild.id) {
                entries.push(GuildPaneEntry::Guild {
                    state: guild,
                    branch: GuildBranch::None,
                });
            }
        }

        entries
    }

    pub fn selected_guild(&self) -> usize {
        self.selected_guild
            .min(self.guild_pane_entries().len().saturating_sub(1))
    }

    #[cfg(test)]
    pub fn guild_scroll(&self) -> usize {
        self.guild_scroll
    }

    pub fn visible_guild_pane_entries(&self) -> Vec<GuildPaneEntry<'_>> {
        self.guild_pane_entries()
            .into_iter()
            .skip(self.guild_scroll)
            .take(pane_content_height(self.guild_view_height))
            .collect()
    }

    pub fn focused_guild_selection(&self) -> Option<usize> {
        if self.focus == FocusPane::Guilds && !self.guild_pane_entries().is_empty() {
            Some(self.selected_guild().saturating_sub(self.guild_scroll))
        } else {
            None
        }
    }

    pub fn set_guild_view_height(&mut self, height: usize) {
        self.guild_view_height = height;
        self.clamp_guild_viewport();
    }

    pub fn selected_guild_id(&self) -> Option<Id<GuildMarker>> {
        match self.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::Unset | ActiveGuildScope::DirectMessages => None,
        }
    }

    pub fn is_active_guild_entry(&self, entry: &GuildPaneEntry<'_>) -> bool {
        match (self.active_guild, entry) {
            (ActiveGuildScope::DirectMessages, GuildPaneEntry::DirectMessages) => true,
            (ActiveGuildScope::Guild(active_id), GuildPaneEntry::Guild { state, .. }) => {
                state.id == active_id
            }
            (ActiveGuildScope::Unset, _)
            | (ActiveGuildScope::DirectMessages, _)
            | (ActiveGuildScope::Guild(_), _) => false,
        }
    }

    /// Toggles the collapse state of the folder under the selection. Does
    /// nothing if the cursor isn't on a folder header.
    pub fn toggle_selected_folder(&mut self) {
        let folder_key = self.selected_folder_key();
        if let Some(key) = folder_key
            && !self.collapsed_folders.insert(key.clone())
        {
            self.collapsed_folders.remove(&key);
        }
    }

    pub fn open_selected_folder(&mut self) {
        if let Some(key) = self.selected_folder_key() {
            self.collapsed_folders.remove(&key);
        }
    }

    pub fn close_selected_folder(&mut self) {
        if let Some(key) = self.selected_folder_key() {
            self.collapsed_folders.insert(key);
        }
    }

    pub fn confirm_selected_guild(&mut self) {
        match self.guild_pane_entries().get(self.selected_guild()) {
            Some(GuildPaneEntry::DirectMessages) => {
                self.activate_guild(ActiveGuildScope::DirectMessages)
            }
            Some(GuildPaneEntry::Guild { state, .. }) => {
                self.activate_guild(ActiveGuildScope::Guild(state.id))
            }
            Some(GuildPaneEntry::FolderHeader { .. }) => self.toggle_selected_folder(),
            None => {}
        }
    }

    fn activate_guild(&mut self, scope: ActiveGuildScope) {
        self.active_guild = scope;
        self.selected_channel = 0;
        self.channel_scroll = 0;
        self.active_channel_id = None;
        self.selected_message = 0;
        self.message_scroll = 0;
        self.message_line_scroll = 0;
        self.message_keep_selection_visible = true;
        self.message_auto_follow = true;
        self.selected_member = 0;
    }

    fn selected_folder_key(&self) -> Option<FolderKey> {
        let entries = self.guild_pane_entries();
        let selected = self.selected_guild();
        match entries.get(selected) {
            Some(GuildPaneEntry::FolderHeader { folder, .. }) => Self::folder_key(folder),
            Some(GuildPaneEntry::Guild { branch, .. }) if branch.is_folder_child() => entries
                .get(..selected)?
                .iter()
                .rev()
                .find_map(|entry| match entry {
                    GuildPaneEntry::FolderHeader { folder, .. } => Self::folder_key(folder),
                    _ => None,
                }),
            _ => None,
        }
    }

    fn folder_key(folder: &GuildFolder) -> Option<FolderKey> {
        if let Some(id) = folder.id {
            Some(FolderKey::Id(id))
        } else if folder.guild_ids.len() > 1 {
            Some(FolderKey::Guilds(folder.guild_ids.clone()))
        } else {
            None
        }
    }

    pub fn channels(&self) -> Vec<&ChannelState> {
        match self.active_guild {
            ActiveGuildScope::Unset => Vec::new(),
            ActiveGuildScope::DirectMessages => self.discord.channels_for_guild(None),
            ActiveGuildScope::Guild(guild_id) => self.discord.channels_for_guild(Some(guild_id)),
        }
    }

    pub fn channel_pane_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        let mut channels = self.channels();
        if self.active_guild == ActiveGuildScope::DirectMessages {
            sort_direct_message_channels(&mut channels);
            return channels
                .into_iter()
                .map(|state| ChannelPaneEntry::Channel {
                    state,
                    branch: ChannelBranch::None,
                })
                .collect();
        }

        let category_ids: HashSet<Id<ChannelMarker>> = channels
            .iter()
            .filter(|channel| channel.is_category())
            .map(|channel| channel.id)
            .collect();

        let mut roots: Vec<&ChannelState> = channels
            .iter()
            .copied()
            .filter(|channel| {
                channel.is_category()
                    || channel
                        .parent_id
                        .is_none_or(|parent_id| !category_ids.contains(&parent_id))
            })
            .collect();
        sort_channels(&mut roots);

        let mut entries = Vec::new();
        for root in roots {
            if !root.is_category() {
                entries.push(ChannelPaneEntry::Channel {
                    state: root,
                    branch: ChannelBranch::None,
                });
                continue;
            }

            let collapsed = self.collapsed_channel_categories.contains(&root.id);
            entries.push(ChannelPaneEntry::CategoryHeader {
                state: root,
                collapsed,
            });
            if collapsed {
                continue;
            }

            let mut children: Vec<&ChannelState> = channels
                .iter()
                .copied()
                .filter(|channel| !channel.is_category() && channel.parent_id == Some(root.id))
                .collect();
            sort_channels(&mut children);
            let last_child_index = children.len().saturating_sub(1);
            for (index, child) in children.into_iter().enumerate() {
                let branch = if index == last_child_index {
                    ChannelBranch::Last
                } else {
                    ChannelBranch::Middle
                };
                entries.push(ChannelPaneEntry::Channel {
                    state: child,
                    branch,
                });
            }
        }

        entries
    }

    pub fn selected_channel(&self) -> usize {
        self.selected_channel
            .min(self.channel_pane_entries().len().saturating_sub(1))
    }

    fn selected_channel_cursor_id(&self) -> Option<Id<ChannelMarker>> {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::Channel { state, .. }) => Some(state.id),
            Some(ChannelPaneEntry::CategoryHeader { .. }) | None => None,
        }
    }

    #[cfg(test)]
    pub fn channel_scroll(&self) -> usize {
        self.channel_scroll
    }

    pub fn visible_channel_pane_entries(&self) -> Vec<ChannelPaneEntry<'_>> {
        self.channel_pane_entries()
            .into_iter()
            .skip(self.channel_scroll)
            .take(pane_content_height(self.channel_view_height))
            .collect()
    }

    pub fn focused_channel_selection(&self) -> Option<usize> {
        if self.focus == FocusPane::Channels && !self.channel_pane_entries().is_empty() {
            Some(self.selected_channel().saturating_sub(self.channel_scroll))
        } else {
            None
        }
    }

    pub fn set_channel_view_height(&mut self, height: usize) {
        self.channel_view_height = height;
        self.clamp_channel_viewport();
    }

    fn restore_channel_cursor(&mut self, channel_id: Option<Id<ChannelMarker>>) {
        let Some(channel_id) = channel_id else {
            return;
        };
        if let Some(index) = self.channel_pane_entries().iter().position(|entry| {
            matches!(entry, ChannelPaneEntry::Channel { state, .. } if state.id == channel_id)
        }) {
            self.selected_channel = index;
        }
    }

    pub fn selected_channel_id(&self) -> Option<Id<ChannelMarker>> {
        self.active_channel_id
    }

    pub fn selected_channel_state(&self) -> Option<&ChannelState> {
        self.active_channel_id
            .and_then(|channel_id| self.discord.channel(channel_id))
    }

    pub fn channel_label(&self, channel_id: Id<ChannelMarker>) -> String {
        self.discord
            .channel(channel_id)
            .map(|channel| match channel.kind.as_str() {
                "dm" | "Private" => format!("@{}", channel.name),
                "group-dm" | "Group" => channel.name.clone(),
                _ => format!("#{}", channel.name),
            })
            .unwrap_or_else(|| format!("#channel-{}", channel_id.get()))
    }

    pub(crate) fn thread_summary_for_message(
        &self,
        message: &MessageState,
    ) -> Option<ThreadSummary> {
        if message.message_kind.code() != 18 {
            return None;
        }
        let referenced_thread = message
            .reference
            .as_ref()
            .and_then(|reference| reference.channel_id)
            .and_then(|channel_id| self.discord.channel(channel_id))
            .filter(|channel| channel.is_thread());
        let thread = referenced_thread.or_else(|| {
            let thread_name = message.content.as_deref()?.trim();
            if thread_name.is_empty() {
                return None;
            }
            self.discord
                .channels_for_guild(message.guild_id)
                .into_iter()
                .find(|channel| {
                    channel.is_thread()
                        && channel.parent_id == Some(message.channel_id)
                        && channel.name == thread_name
                })
        });
        thread.map(|channel| ThreadSummary {
            channel_id: channel.id,
            name: channel.name.clone(),
            message_count: channel.message_count,
            total_message_sent: channel.total_message_sent,
            archived: channel.thread_archived,
            locked: channel.thread_locked,
        })
    }

    pub(crate) fn render_user_mentions(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        mentions: &[MentionInfo],
        value: &str,
    ) -> String {
        render_user_mentions(value, |user_id| {
            self.resolve_mention_display_name(guild_id, mentions, user_id)
        })
    }

    pub(crate) fn render_user_mentions_with_highlights(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        mentions: &[MentionInfo],
        value: &str,
    ) -> RenderedText {
        let current_user_id = self.current_user_id.map(|id| id.get());
        let mut rendered = render_user_mentions_with_highlights(
            value,
            |user_id| self.resolve_mention_display_name(guild_id, mentions, user_id),
            |user_id| current_user_id == Some(user_id),
        );
        if current_user_id.is_some() {
            add_literal_mention_highlights(&mut rendered, "@everyone");
            add_literal_mention_highlights(&mut rendered, "@here");
        }
        normalize_text_highlights(&mut rendered.highlights);
        rendered
    }

    fn resolve_mention_display_name(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        mentions: &[MentionInfo],
        user_id: u64,
    ) -> Option<String> {
        if let Some(display_name) = guild_id.and_then(|guild_id| {
            let user_id = Id::<UserMarker>::new(user_id);
            self.discord.member_display_name(guild_id, user_id)
        }) {
            return Some(display_name.to_owned());
        }
        mentions
            .iter()
            .find(|mention| mention.user_id.get() == user_id)
            .map(|mention| mention.display_name.clone())
    }

    pub(crate) fn forwarded_snapshot_mention_guild_id(
        &self,
        snapshot: &MessageSnapshotInfo,
    ) -> Option<Id<GuildMarker>> {
        snapshot
            .source_channel_id
            .and_then(|channel_id| self.discord.channel(channel_id))
            .and_then(|channel| channel.guild_id)
    }

    pub fn is_active_channel_entry(&self, entry: &ChannelPaneEntry<'_>) -> bool {
        matches!(
            entry,
            ChannelPaneEntry::Channel { state, .. } if Some(state.id) == self.active_channel_id
        )
    }

    pub fn toggle_selected_channel_category(&mut self) {
        let Some(category_id) = self.selected_channel_category_id() else {
            return;
        };
        if !self.collapsed_channel_categories.insert(category_id) {
            self.collapsed_channel_categories.remove(&category_id);
        }
    }

    pub fn open_selected_channel_category(&mut self) {
        if let Some(category_id) = self.selected_channel_category_id() {
            self.collapsed_channel_categories.remove(&category_id);
        }
    }

    pub fn close_selected_channel_category(&mut self) {
        if let Some(category_id) = self.selected_channel_category_id() {
            self.collapsed_channel_categories.insert(category_id);
        }
    }

    #[cfg(test)]
    pub fn confirm_selected_channel(&mut self) {
        let _ = self.confirm_selected_channel_command();
    }

    pub fn confirm_selected_channel_command(&mut self) -> Option<AppCommand> {
        match self.channel_pane_entries().get(self.selected_channel()) {
            Some(ChannelPaneEntry::CategoryHeader { .. }) => {
                self.toggle_selected_channel_category();
                None
            }
            Some(ChannelPaneEntry::Channel { state, .. }) => {
                let channel_id = state.id;
                let command = if is_direct_message_channel(state) {
                    Some(AppCommand::SubscribeDirectMessage { channel_id })
                } else {
                    state
                        .guild_id
                        .map(|guild_id| AppCommand::SubscribeGuildChannel {
                            guild_id,
                            channel_id,
                        })
                };
                self.activate_channel(channel_id);
                command
            }
            None => None,
        }
    }

    fn activate_channel(&mut self, channel_id: Id<ChannelMarker>) {
        self.active_channel_id = Some(channel_id);
        self.message_auto_follow = true;
        self.message_line_scroll = 0;
        self.message_keep_selection_visible = true;
        self.selected_message = self.messages().len().saturating_sub(1);
        self.clamp_message_viewport();
    }

    fn record_older_history_loaded(
        &mut self,
        channel_id: Id<ChannelMarker>,
        response_before: Option<Id<MessageMarker>>,
        messages: &[MessageInfo],
    ) {
        let Some(OlderHistoryRequestState::Requested { before }) =
            self.older_history_requests.get(&channel_id).copied()
        else {
            return;
        };
        if response_before != Some(before) {
            return;
        }

        if messages.is_empty() {
            self.older_history_requests
                .insert(channel_id, OlderHistoryRequestState::Exhausted { before });
        } else {
            self.older_history_requests.remove(&channel_id);
        }
    }

    fn selected_channel_category_id(&self) -> Option<Id<ChannelMarker>> {
        let entries = self.channel_pane_entries();
        let selected = self.selected_channel();
        match entries.get(selected) {
            Some(ChannelPaneEntry::CategoryHeader { state, .. }) => Some(state.id),
            Some(ChannelPaneEntry::Channel { branch, .. }) if branch.is_category_child() => entries
                .get(..selected)?
                .iter()
                .rev()
                .find_map(|entry| match entry {
                    ChannelPaneEntry::CategoryHeader { state, .. } => Some(state.id),
                    _ => None,
                }),
            _ => None,
        }
    }

    pub fn messages(&self) -> Vec<&MessageState> {
        self.selected_channel_id()
            .map(|channel_id| self.discord.messages_for_channel(channel_id))
            .unwrap_or_default()
    }

    pub fn selected_message(&self) -> usize {
        self.selected_message
            .min(self.messages().len().saturating_sub(1))
    }

    pub fn selected_message_state(&self) -> Option<&MessageState> {
        let channel_id = self.selected_channel_id()?;
        self.discord
            .messages_for_channel(channel_id)
            .get(self.selected_message())
            .copied()
    }

    pub(crate) fn reply_target_message_state(&self) -> Option<&MessageState> {
        let message_id = self.reply_target_message_id?;
        self.messages()
            .into_iter()
            .find(|message| message.id == message_id)
    }

    pub fn next_older_history_command(&mut self) -> Option<AppCommand> {
        let channel_id = self.selected_channel_id()?;
        let before = self.older_history_cursor()?;
        match self.older_history_requests.get(&channel_id) {
            Some(OlderHistoryRequestState::Requested { .. }) => return None,
            Some(OlderHistoryRequestState::Exhausted { before: exhausted })
                if *exhausted == before =>
            {
                return None;
            }
            _ => {}
        }

        self.older_history_requests
            .insert(channel_id, OlderHistoryRequestState::Requested { before });
        Some(AppCommand::LoadMessageHistory {
            channel_id,
            before: Some(before),
        })
    }

    fn older_history_cursor(&self) -> Option<Id<MessageMarker>> {
        if self.focus != FocusPane::Messages
            || self.messages().is_empty()
            || self.selected_message() != 0
        {
            return None;
        }

        self.messages().first().map(|message| message.id)
    }

    #[cfg(test)]
    pub fn message_scroll(&self) -> usize {
        self.message_scroll
    }

    #[cfg(test)]
    pub fn message_auto_follow(&self) -> bool {
        self.message_auto_follow
    }

    #[cfg(test)]
    pub fn message_view_height(&self) -> usize {
        self.message_view_height
    }

    pub fn visible_messages(&self) -> Vec<&MessageState> {
        self.messages()
            .into_iter()
            .skip(self.message_scroll)
            .take(self.message_content_height())
            .collect()
    }

    pub fn message_line_scroll(&self) -> usize {
        self.message_line_scroll
    }

    pub fn set_message_view_height(&mut self, height: usize) {
        self.message_view_height = height;
        self.clamp_message_viewport();
    }

    pub fn clamp_message_viewport_for_image_previews(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        self.message_content_width = content_width;
        self.message_preview_width = preview_width;
        self.message_max_preview_height = max_preview_height;
        self.clamp_message_viewport();
        if self.message_auto_follow {
            if self.message_view_height <= 1 {
                self.message_scroll = self.selected_message();
                self.message_line_scroll = 0;
            } else {
                self.align_message_viewport_to_bottom(
                    content_width,
                    preview_width,
                    max_preview_height,
                );
            }
            return;
        }
        self.normalize_message_line_scroll(content_width, preview_width, max_preview_height);
        if self.messages().is_empty() || !self.message_keep_selection_visible {
            return;
        }

        if self.center_selected_message(content_width, preview_width, max_preview_height) {
            return;
        }

        let height = self.message_content_height();
        let upper_scrolloff = SCROLL_OFF.min(height.saturating_sub(1) / 2);
        let max_iterations = self
            .messages()
            .into_iter()
            .map(|message| {
                self.message_rendered_height(
                    message,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum::<usize>()
            .max(1);

        for _ in 0..max_iterations {
            let lower_scrolloff = self
                .following_message_rendered_rows(
                    content_width,
                    preview_width,
                    max_preview_height,
                    SCROLL_OFF,
                )
                .min(height.saturating_sub(1));
            let lower_bound = height.saturating_sub(1).saturating_sub(lower_scrolloff);
            let selected_row = self.selected_message_rendered_row(
                content_width,
                preview_width,
                max_preview_height,
            );
            let selected_bottom = selected_row.saturating_add(
                self.selected_message_rendered_height(
                    content_width,
                    preview_width,
                    max_preview_height,
                )
                .saturating_sub(1),
            );
            if selected_bottom > lower_bound && self.message_scroll < self.selected_message {
                self.scroll_message_viewport_down_one_row(
                    content_width,
                    preview_width,
                    max_preview_height,
                );
                continue;
            }

            if selected_row < upper_scrolloff && self.message_scroll > 0 {
                let previous_height = self
                    .messages()
                    .get(self.message_scroll.saturating_sub(1))
                    .map(|message| {
                        self.message_rendered_height(
                            message,
                            content_width,
                            preview_width,
                            max_preview_height,
                        )
                    })
                    .unwrap_or(0);
                let candidate_bottom = selected_bottom.saturating_add(previous_height);
                if candidate_bottom < height {
                    self.scroll_message_viewport_up_one_row(
                        content_width,
                        preview_width,
                        max_preview_height,
                    );
                    continue;
                }
            }

            break;
        }
    }

    pub fn focused_message_selection(&self) -> Option<usize> {
        if self.focus == FocusPane::Messages && !self.messages().is_empty() {
            let selected = self.selected_message();
            let visible_count = self.visible_messages().len();
            if selected >= self.message_scroll && selected < self.message_scroll + visible_count {
                Some(selected - self.message_scroll)
            } else {
                None
            }
        } else {
            None
        }
    }

    fn selected_message_image_attachment(&self) -> Option<&crate::discord::AttachmentInfo> {
        self.selected_message_attachment_matching(|attachment| attachment.is_image())
    }

    fn selected_message_attachment_matching(
        &self,
        predicate: impl Fn(&crate::discord::AttachmentInfo) -> bool,
    ) -> Option<&crate::discord::AttachmentInfo> {
        let channel_id = self.selected_channel_id()?;
        let messages = self.discord.messages_for_channel(channel_id);
        let message = messages.get(self.selected_message())?;
        message
            .attachments_in_display_order()
            .find(|attachment| predicate(attachment))
    }

    pub fn members_grouped(&self) -> Vec<MemberGroup<'_>> {
        let Some(guild_id) = self.selected_guild_id() else {
            return self.selected_channel_recipient_group();
        };
        let members = self.discord.members_for_guild(guild_id);
        let roles = self.discord.roles_for_guild(guild_id);
        let hoisted_roles = sorted_hoisted_roles(&roles);
        let mut groups: Vec<MemberGroup<'_>> = Vec::new();
        let mut grouped_members: HashSet<Id<UserMarker>> = HashSet::new();

        for role in hoisted_roles {
            let mut entries: Vec<&GuildMemberState> = members
                .iter()
                .filter(|member| primary_hoisted_role(member, &roles) == Some(role.id))
                .copied()
                .collect();
            if entries.is_empty() {
                continue;
            }
            sort_member_entries(&mut entries);
            grouped_members.extend(entries.iter().map(|member| member.user_id));
            groups.push(MemberGroup {
                label: role.name.clone(),
                color: role.color,
                entries: entries.into_iter().map(MemberEntry::Guild).collect(),
            });
        }

        let mut ungrouped: Vec<&GuildMemberState> = members
            .into_iter()
            .filter(|member| !grouped_members.contains(&member.user_id))
            .collect();
        if !ungrouped.is_empty() {
            sort_member_entries(&mut ungrouped);
            groups.push(MemberGroup {
                label: "Members".to_owned(),
                color: None,
                entries: ungrouped.into_iter().map(MemberEntry::Guild).collect(),
            });
        }

        groups
    }

    fn selected_channel_recipient_group(&self) -> Vec<MemberGroup<'_>> {
        let Some(channel) = self.selected_channel_state() else {
            return Vec::new();
        };
        if !is_direct_message_channel(channel) || channel.recipients.is_empty() {
            return Vec::new();
        }

        let mut recipients: Vec<&ChannelRecipientState> = channel.recipients.iter().collect();
        sort_recipient_entries(&mut recipients);
        vec![MemberGroup {
            label: "Members".to_owned(),
            color: None,
            entries: recipients.into_iter().map(MemberEntry::Recipient).collect(),
        }]
    }

    pub fn flattened_members(&self) -> Vec<MemberEntry<'_>> {
        self.members_grouped()
            .into_iter()
            .flat_map(|group| group.entries)
            .collect()
    }

    pub fn selected_member(&self) -> usize {
        self.selected_member
            .min(self.flattened_members().len().saturating_sub(1))
    }

    pub fn focused_member_selection_line(&self) -> Option<usize> {
        if self.focus == FocusPane::Members && !self.flattened_members().is_empty() {
            Some(
                self.selected_member_line()
                    .saturating_sub(self.member_scroll),
            )
        } else {
            None
        }
    }

    pub fn member_scroll(&self) -> usize {
        self.member_scroll
    }

    pub fn member_content_height(&self) -> usize {
        pane_content_height(self.member_view_height)
    }

    #[cfg(test)]
    pub fn selected_member_line_for_test(&self) -> usize {
        self.selected_member_line()
    }

    pub fn set_member_view_height(&mut self, height: usize) {
        self.member_view_height = height;
        self.clamp_member_viewport();
    }

    pub fn move_down(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = self
                    .selected_guild
                    .saturating_add(1)
                    .min(self.guild_pane_entries().len().saturating_sub(1));
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.selected_channel = self
                    .selected_channel
                    .saturating_add(1)
                    .min(self.channel_pane_entries().len().saturating_sub(1));
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.selected_message = self
                    .selected_message
                    .saturating_add(1)
                    .min(self.messages().len().saturating_sub(1));
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                self.selected_member = self
                    .selected_member
                    .saturating_add(1)
                    .min(self.flattened_members().len().saturating_sub(1));
                self.clamp_member_viewport();
            }
        }
    }

    pub fn move_up(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = self.selected_guild.saturating_sub(1);
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.selected_channel = self.selected_channel.saturating_sub(1);
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.message_auto_follow = false;
                self.selected_message = self.selected_message.saturating_sub(1);
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                self.selected_member = self.selected_member.saturating_sub(1);
                self.clamp_member_viewport();
            }
        }
    }

    pub fn jump_top(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = 0;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.selected_channel = 0;
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.message_auto_follow = false;
                self.selected_message = 0;
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                self.selected_member = 0;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn jump_bottom(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = self.guild_pane_entries().len().saturating_sub(1);
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.selected_channel = self.channel_pane_entries().len().saturating_sub(1);
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.selected_message = self.messages().len().saturating_sub(1);
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                self.selected_member = self.flattened_members().len().saturating_sub(1);
                self.clamp_member_viewport();
            }
        }
    }

    pub fn half_page_down(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                let distance = pane_content_height(self.guild_view_height) / 2;
                self.selected_guild = self
                    .selected_guild
                    .saturating_add(distance.max(1))
                    .min(self.guild_pane_entries().len().saturating_sub(1));
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                let distance = pane_content_height(self.channel_view_height) / 2;
                self.selected_channel = self
                    .selected_channel
                    .saturating_add(distance.max(1))
                    .min(self.channel_pane_entries().len().saturating_sub(1));
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                let distance = self.message_content_height() / 2;
                self.selected_message = self
                    .selected_message
                    .saturating_add(distance.max(1))
                    .min(self.messages().len().saturating_sub(1));
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                let distance = pane_content_height(self.member_view_height) / 2;
                self.select_member_near_line(
                    self.selected_member_line().saturating_add(distance.max(1)),
                );
                self.clamp_member_viewport();
            }
        }
    }

    pub fn half_page_up(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                let distance = pane_content_height(self.guild_view_height) / 2;
                self.selected_guild = self.selected_guild.saturating_sub(distance.max(1));
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                let distance = pane_content_height(self.channel_view_height) / 2;
                self.selected_channel = self.selected_channel.saturating_sub(distance.max(1));
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.message_auto_follow = false;
                let distance = self.message_content_height() / 2;
                self.selected_message = self.selected_message.saturating_sub(distance.max(1));
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
            }
            FocusPane::Members => {
                let distance = pane_content_height(self.member_view_height) / 2;
                self.select_member_near_line(
                    self.selected_member_line().saturating_sub(distance.max(1)),
                );
                self.clamp_member_viewport();
            }
        }
    }

    pub fn toggle_message_auto_follow(&mut self) {
        if self.focus != FocusPane::Messages {
            return;
        }

        self.message_auto_follow = !self.message_auto_follow;
        if self.message_auto_follow {
            self.message_keep_selection_visible = true;
            self.follow_latest_message();
        }
        self.clamp_message_viewport();
    }

    pub fn scroll_message_viewport_down(&mut self) {
        if self.focus != FocusPane::Messages || self.message_content_width == usize::MAX {
            return;
        }
        self.message_auto_follow = false;
        self.message_keep_selection_visible = false;
        self.scroll_message_viewport_down_one_row(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
        );
    }

    pub fn scroll_message_viewport_up(&mut self) {
        if self.focus != FocusPane::Messages || self.message_content_width == usize::MAX {
            return;
        }
        self.message_auto_follow = false;
        self.message_keep_selection_visible = false;
        self.scroll_message_viewport_up_one_row(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
        );
    }

    pub fn scroll_message_viewport_top(&mut self) {
        if self.focus != FocusPane::Messages {
            return;
        }
        self.message_auto_follow = false;
        self.message_keep_selection_visible = false;
        self.message_scroll = 0;
        self.message_line_scroll = 0;
    }

    pub fn scroll_message_viewport_bottom(&mut self) {
        if self.focus != FocusPane::Messages || self.message_content_width == usize::MAX {
            return;
        }
        self.message_auto_follow = false;
        self.message_keep_selection_visible = false;
        let height = self.message_content_height();
        let mut remaining = height;
        for index in (0..self.messages().len()).rev() {
            let message_height = self
                .messages()
                .get(index)
                .map(|message| {
                    self.message_rendered_height(
                        message,
                        self.message_content_width,
                        self.message_preview_width,
                        self.message_max_preview_height,
                    )
                    .max(1)
                })
                .unwrap_or(1);
            if message_height >= remaining {
                self.message_scroll = index;
                self.message_line_scroll = message_height.saturating_sub(remaining);
                return;
            }
            remaining = remaining.saturating_sub(message_height);
        }
        self.message_scroll = 0;
        self.message_line_scroll = 0;
    }

    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Guilds => FocusPane::Channels,
            FocusPane::Channels => FocusPane::Messages,
            FocusPane::Messages => FocusPane::Members,
            FocusPane::Members => FocusPane::Guilds,
        };
    }

    pub fn focus_pane(&mut self, pane: FocusPane) {
        self.focus = pane;
    }

    pub fn start_composer(&mut self) {
        if self.selected_channel_id().is_none() {
            return;
        }
        self.reply_target_message_id = None;
        self.composer_active = true;
        self.focus = FocusPane::Messages;
    }

    pub fn cancel_composer(&mut self) {
        self.composer_active = false;
        self.composer_input.clear();
        self.reply_target_message_id = None;
    }

    pub fn push_composer_char(&mut self, value: char) {
        self.composer_input.push(value);
    }

    pub fn pop_composer_char(&mut self) {
        self.composer_input.pop();
    }

    pub fn submit_composer(&mut self) -> Option<AppCommand> {
        let channel_id = self.selected_channel_id()?;
        let content = self.composer_input.trim().to_owned();
        if content.is_empty() {
            return None;
        }

        self.composer_input.clear();
        self.composer_active = false;
        let reply_to = self.reply_target_message_id.take();
        Some(AppCommand::SendMessage {
            channel_id,
            content,
            reply_to,
        })
    }

    fn clamp_selection_indices(&mut self) {
        self.selected_guild = self.selected_guild();
        self.selected_channel = self.selected_channel();
        self.selected_message = self.selected_message();
        self.selected_member = self.selected_member();
        self.clamp_list_viewports();
        self.clamp_message_viewport();
    }

    fn clamp_active_selection(&mut self) {
        if let ActiveGuildScope::Guild(guild_id) = self.active_guild
            && !self
                .discord
                .guilds()
                .iter()
                .any(|guild| guild.id == guild_id)
        {
            self.active_guild = ActiveGuildScope::Unset;
        }

        let active_channel_is_valid = self
            .active_channel_id
            .and_then(|channel_id| self.discord.channel(channel_id))
            .is_some_and(|channel| match self.active_guild {
                ActiveGuildScope::Unset => false,
                ActiveGuildScope::DirectMessages => {
                    channel.guild_id.is_none() && !channel.is_category()
                }
                ActiveGuildScope::Guild(guild_id) => {
                    channel.guild_id == Some(guild_id) && !channel.is_category()
                }
            });
        if self.active_channel_id.is_some() && !active_channel_is_valid {
            self.active_channel_id = None;
        }
    }

    fn clamp_list_viewports(&mut self) {
        self.clamp_guild_viewport();
        self.clamp_channel_viewport();
        self.clamp_member_viewport();
    }

    fn clamp_guild_viewport(&mut self) {
        let entries_len = self.guild_pane_entries().len();
        self.selected_guild = self.selected_guild.min(entries_len.saturating_sub(1));
        self.guild_scroll = clamp_list_scroll(
            self.selected_guild,
            self.guild_scroll,
            pane_content_height(self.guild_view_height),
            entries_len,
        );
    }

    fn clamp_channel_viewport(&mut self) {
        let entries_len = self.channel_pane_entries().len();
        self.selected_channel = self.selected_channel.min(entries_len.saturating_sub(1));
        self.channel_scroll = clamp_list_scroll(
            self.selected_channel,
            self.channel_scroll,
            pane_content_height(self.channel_view_height),
            entries_len,
        );
    }

    fn clamp_member_viewport(&mut self) {
        let members_len = self.flattened_members().len();
        if members_len == 0 {
            self.selected_member = 0;
            self.member_scroll = 0;
            return;
        }

        self.selected_member = self.selected_member.min(members_len - 1);
        self.member_scroll = clamp_list_scroll(
            self.selected_member_line(),
            self.member_scroll,
            pane_content_height(self.member_view_height),
            self.member_line_count(),
        );
    }

    fn selected_member_line(&self) -> usize {
        let selected_member = self.selected_member();
        let mut member_index = 0usize;
        let mut line_index = 0usize;
        for group in self.members_grouped() {
            if line_index > 0 {
                line_index += 1;
            }
            line_index += 1;
            for _member in group.entries {
                if member_index == selected_member {
                    return line_index;
                }
                member_index += 1;
                line_index += 1;
            }
        }
        0
    }

    fn select_member_near_line(&mut self, target_line: usize) {
        let mut last_member = None;
        for (member_index, line_index) in self.member_line_indices() {
            if line_index >= target_line {
                self.selected_member = member_index;
                return;
            }
            last_member = Some(member_index);
        }

        if let Some(member_index) = last_member {
            self.selected_member = member_index;
        }
    }

    fn member_line_indices(&self) -> Vec<(usize, usize)> {
        let mut indices = Vec::new();
        let mut member_index = 0usize;
        let mut line_index = 0usize;
        for group in self.members_grouped() {
            if line_index > 0 {
                line_index += 1;
            }
            line_index += 1;
            for _member in group.entries {
                indices.push((member_index, line_index));
                member_index += 1;
                line_index += 1;
            }
        }
        indices
    }

    fn member_line_count(&self) -> usize {
        let mut lines = 0usize;
        for group in self.members_grouped() {
            if lines > 0 {
                lines += 1;
            }
            lines += 1 + group.entries.len();
        }
        lines
    }

    fn follow_latest_message(&mut self) {
        self.selected_message = self.messages().len().saturating_sub(1);
        self.message_scroll = self.selected_message;
        self.message_line_scroll = 0;
        self.message_keep_selection_visible = true;
    }

    fn align_message_viewport_to_bottom(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        let height = self.message_content_height();
        let mut remaining = height;
        for index in (0..self.messages().len()).rev() {
            let message_height = self
                .messages()
                .get(index)
                .map(|message| {
                    self.message_rendered_height(
                        message,
                        content_width,
                        preview_width,
                        max_preview_height,
                    )
                    .max(1)
                })
                .unwrap_or(1);
            if message_height >= remaining {
                self.message_scroll = index;
                self.message_line_scroll = message_height.saturating_sub(remaining);
                return;
            }
            remaining = remaining.saturating_sub(message_height);
        }
        self.message_scroll = 0;
        self.message_line_scroll = 0;
    }

    fn restore_message_position(
        &mut self,
        selected_message_id: Option<Id<MessageMarker>>,
        scroll_message_id: Option<Id<MessageMarker>>,
    ) {
        let message_ids: Vec<_> = self
            .messages()
            .into_iter()
            .map(|message| message.id)
            .collect();
        if let Some(message_id) = selected_message_id
            && let Some(index) = message_ids.iter().position(|id| *id == message_id)
        {
            self.selected_message = index;
        }
        if let Some(message_id) = scroll_message_id
            && let Some(index) = message_ids.iter().position(|id| *id == message_id)
        {
            self.message_scroll = index;
        }
    }

    fn clamp_message_viewport(&mut self) {
        let messages_len = self.messages().len();
        if messages_len == 0 {
            self.selected_message = 0;
            self.message_scroll = 0;
            self.message_line_scroll = 0;
            return;
        }

        self.selected_message = self.selected_message.min(messages_len - 1);
        self.message_scroll = self.message_scroll.min(messages_len - 1);
        if self.message_content_width == usize::MAX {
            self.message_scroll = clamp_list_scroll(
                self.selected_message,
                self.message_scroll,
                self.message_content_height(),
                messages_len,
            );
            if self.message_scroll != self.selected_message {
                self.message_line_scroll = 0;
            }
        }
    }

    fn center_selected_message(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> bool {
        let selected = self.selected_message();
        let height = self.message_content_height();
        let Some(selected_message) = self.messages().get(selected).copied() else {
            return false;
        };
        let selected_height = self
            .message_rendered_height(
                selected_message,
                content_width,
                preview_width,
                max_preview_height,
            )
            .max(1);
        let mut top = selected;
        let mut offset = 0usize;
        let mut remaining = (height / 2).saturating_sub(selected_height / 2);

        while remaining > 0 && top > 0 {
            let previous_index = top.saturating_sub(1);
            let Some(previous_message) = self.messages().get(previous_index).copied() else {
                break;
            };
            let previous_height = self
                .message_rendered_height(
                    previous_message,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
                .max(1);
            if remaining >= previous_height {
                remaining = remaining.saturating_sub(previous_height);
                top = previous_index;
                offset = 0;
            } else {
                top = previous_index;
                offset = previous_height.saturating_sub(remaining);
                remaining = 0;
            }
        }

        if remaining > 0 || !self.message_viewport_has_rows_below(top, offset, height) {
            return false;
        }

        self.message_scroll = top;
        self.message_line_scroll = offset;
        true
    }

    fn message_viewport_has_rows_below(&self, top: usize, offset: usize, height: usize) -> bool {
        let mut visible_rows = 0usize;
        for (index, message) in self.messages().into_iter().skip(top).enumerate() {
            let message_height = self
                .message_rendered_height(
                    message,
                    self.message_content_width,
                    self.message_preview_width,
                    self.message_max_preview_height,
                )
                .max(1);
            let visible_height = if index == 0 {
                message_height.saturating_sub(offset)
            } else {
                message_height
            };
            visible_rows = visible_rows.saturating_add(visible_height);
            if visible_rows >= height {
                return true;
            }
        }
        false
    }

    fn scroll_message_viewport_down_one_row(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        let messages_len = self.messages().len();
        let Some(message) = self.messages().get(self.message_scroll).copied() else {
            self.message_line_scroll = 0;
            return;
        };
        let height = self
            .message_rendered_height(message, content_width, preview_width, max_preview_height)
            .max(1);
        if self.message_line_scroll.saturating_add(1) < height {
            self.message_line_scroll = self.message_line_scroll.saturating_add(1);
        } else if self.message_scroll < messages_len.saturating_sub(1) {
            self.message_scroll = self.message_scroll.saturating_add(1);
            self.message_line_scroll = 0;
        }
    }

    fn scroll_message_viewport_up_one_row(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        if self.message_line_scroll > 0 {
            self.message_line_scroll = self.message_line_scroll.saturating_sub(1);
            return;
        }
        if self.message_scroll == 0 {
            return;
        }
        self.message_scroll = self.message_scroll.saturating_sub(1);
        self.message_line_scroll = self
            .messages()
            .get(self.message_scroll)
            .map(|message| {
                self.message_rendered_height(
                    message,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
                .saturating_sub(1)
            })
            .unwrap_or(0);
    }

    fn normalize_message_line_scroll(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        let Some(message) = self.messages().get(self.message_scroll).copied() else {
            self.message_line_scroll = 0;
            return;
        };

        let height = self
            .message_rendered_height(message, content_width, preview_width, max_preview_height)
            .max(1);
        self.message_line_scroll = self.message_line_scroll.min(height.saturating_sub(1));
    }

    fn message_content_height(&self) -> usize {
        pane_content_height(self.message_view_height)
    }

    fn selected_message_rendered_row(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        let messages = self.messages();
        let row = messages
            .iter()
            .skip(self.message_scroll)
            .take(self.selected_message.saturating_sub(self.message_scroll))
            .map(|message| {
                self.message_rendered_height(
                    message,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum::<usize>();
        row.saturating_sub(self.message_line_scroll)
    }

    fn selected_message_rendered_height(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        self.messages()
            .get(self.selected_message)
            .map(|message| {
                self.message_rendered_height(
                    message,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .unwrap_or(1)
    }

    fn following_message_rendered_rows(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
        count: usize,
    ) -> usize {
        self.messages()
            .iter()
            .skip(self.selected_message.saturating_add(1))
            .take(count)
            .map(|message| {
                self.message_rendered_height(
                    message,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum()
    }

    pub(crate) fn message_base_line_count_for_width(
        &self,
        message: &MessageState,
        content_width: usize,
    ) -> usize {
        message_base_line_count_for_width_with_mentions(
            message,
            content_width,
            |value| self.render_user_mentions(message.guild_id, &message.mentions, value),
            |snapshot, value| {
                self.render_user_mentions(
                    self.forwarded_snapshot_mention_guild_id(snapshot),
                    &snapshot.mentions,
                    value,
                )
            },
        )
    }

    fn message_rendered_height(
        &self,
        message: &MessageState,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        message_rendered_height_with_mentions(
            message,
            content_width,
            preview_width,
            max_preview_height,
            |value| self.render_user_mentions(message.guild_id, &message.mentions, value),
            |snapshot, value| {
                self.render_user_mentions(
                    self.forwarded_snapshot_mention_guild_id(snapshot),
                    &snapshot.mentions,
                    value,
                )
            },
        )
    }
}

#[cfg(test)]
fn message_rendered_height(
    message: &MessageState,
    content_width: usize,
    preview_width: u16,
    max_preview_height: u16,
) -> usize {
    message_rendered_height_with_mentions(
        message,
        content_width,
        preview_width,
        max_preview_height,
        str::to_owned,
        |_, value| value.to_owned(),
    )
}

fn message_rendered_height_with_mentions<F, G>(
    message: &MessageState,
    content_width: usize,
    preview_width: u16,
    max_preview_height: u16,
    render_text: F,
    render_snapshot_text: G,
) -> usize
where
    F: Fn(&str) -> String,
    G: Fn(&MessageSnapshotInfo, &str) -> String,
{
    let preview_height = message
        .attachments_in_display_order()
        .find(|attachment| attachment.inline_preview_url().is_some())
        .map(|attachment| {
            super::image_preview_height_for_dimensions(
                preview_width,
                max_preview_height,
                attachment.width,
                attachment.height,
            )
        })
        .unwrap_or(0);
    message_base_line_count_for_width_with_mentions(
        message,
        content_width,
        render_text,
        render_snapshot_text,
    ) + usize::from(preview_height)
}

fn message_base_line_count_for_width_with_mentions<F, G>(
    message: &MessageState,
    content_width: usize,
    render_text: F,
    render_snapshot_text: G,
) -> usize
where
    F: Fn(&str) -> String,
    G: Fn(&MessageSnapshotInfo, &str) -> String,
{
    if let Some(system_lines) = system_message_line_count(message) {
        return 1 + system_lines.max(1);
    }

    let primary_lines = message_primary_line_count(
        message.content.as_deref(),
        &message.attachments,
        content_width,
        &render_text,
    );
    let kind_line = usize::from(
        message.reply.is_none() && message.poll.is_none() && !message.message_kind.is_regular(),
    );
    let reply_line = usize::from(message.reply.is_some());
    let poll_lines = if message.reply.is_none() {
        message
            .poll
            .as_ref()
            .map(|poll| 3 + poll.answers.len())
            .unwrap_or(0)
    } else {
        0
    };
    let reaction_lines = reaction_line_count(&message.reactions, content_width);

    if let Some(snapshot) = message.forwarded_snapshots.first() {
        let metadata_line =
            usize::from(snapshot.source_channel_id.is_some() || snapshot.timestamp.is_some());
        return 1
            + (reply_line
                + poll_lines
                + kind_line
                + primary_lines
                + forwarded_snapshot_line_count(snapshot, content_width, &render_snapshot_text)
                + metadata_line
                + reaction_lines)
                .max(1);
    }

    1 + (reply_line + poll_lines + kind_line + primary_lines + reaction_lines).max(1)
}

fn reaction_line_count(reactions: &[ReactionInfo], width: usize) -> usize {
    let chips = reactions
        .iter()
        .filter(|reaction| reaction.count > 0)
        .map(|reaction| {
            let marker = if reaction.me { "●" } else { "○" };
            format!(
                "[{marker} {} {}]",
                reaction.emoji.status_label(),
                reaction.count
            )
        })
        .collect::<Vec<_>>()
        .join("  ");
    if chips.is_empty() {
        0
    } else {
        reaction_text_line_count(&chips, width)
    }
}

fn reaction_text_line_count(value: &str, width: usize) -> usize {
    let width = width.max(1);
    let mut lines = 1;
    let mut current_width = 0;
    for grapheme in unicode_segmentation::UnicodeSegmentation::graphemes(value, true) {
        let grapheme_width = grapheme.width();
        if current_width > 0 && current_width + grapheme_width > width {
            lines += 1;
            current_width = grapheme_width;
        } else {
            current_width += grapheme_width;
        }
    }
    lines
}

fn system_message_line_count(message: &MessageState) -> Option<usize> {
    match message.message_kind.code() {
        8..=11 => Some(1),
        18 => Some(3),
        21 => Some(2),
        46 => Some(match message.poll.as_ref() {
            Some(poll) if poll.total_votes.is_some() => 4,
            Some(_) => 3,
            None => 2,
        }),
        _ => None,
    }
}

fn message_primary_line_count(
    content: Option<&str>,
    attachments: &[AttachmentInfo],
    content_width: usize,
    render_text: &dyn Fn(&str) -> String,
) -> usize {
    content
        .filter(|value| !value.is_empty())
        .map(|value| super::ui::wrapped_text_line_count(&render_text(value), content_width))
        .unwrap_or(0)
        + usize::from(!attachments.is_empty())
}

fn forwarded_snapshot_line_count(
    snapshot: &MessageSnapshotInfo,
    content_width: usize,
    render_text: &dyn Fn(&MessageSnapshotInfo, &str) -> String,
) -> usize {
    let forwarded_content_width = content_width.saturating_sub(2).max(1);
    let content_lines = snapshot
        .content
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(|value| {
            super::ui::wrapped_text_line_count(
                &render_text(snapshot, value),
                forwarded_content_width,
            )
        })
        .unwrap_or(0);
    let attachment_line = usize::from(!snapshot.attachments.is_empty());

    1 + content_lines.saturating_add(attachment_line).max(1)
}

fn add_literal_mention_highlights(rendered: &mut RenderedText, mention: &str) {
    let mut cursor = 0usize;
    while let Some(relative_start) = rendered.text[cursor..].find(mention) {
        let start = cursor.saturating_add(relative_start);
        let end = start.saturating_add(mention.len());
        if is_literal_mention_boundary(&rendered.text, start, end) {
            rendered.highlights.push(TextHighlight { start, end });
        }
        cursor = end;
    }
}

fn normalize_text_highlights(highlights: &mut Vec<TextHighlight>) {
    highlights.sort_by_key(|highlight| (highlight.start, highlight.end));
    let mut normalized: Vec<TextHighlight> = Vec::new();
    for highlight in highlights.drain(..) {
        let Some(last) = normalized.last_mut() else {
            normalized.push(highlight);
            continue;
        };
        if highlight.start <= last.end {
            last.end = last.end.max(highlight.end);
        } else {
            normalized.push(highlight);
        }
    }
    *highlights = normalized;
}

fn is_literal_mention_boundary(value: &str, start: usize, end: usize) -> bool {
    let before = value[..start].chars().next_back();
    let after = value[end..].chars().next();
    !before.is_some_and(is_literal_mention_word_char)
        && !after.is_some_and(is_literal_mention_word_char)
}

fn is_literal_mention_word_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_'
}

fn pane_content_height(height: usize) -> usize {
    height.max(1)
}

fn clamp_list_scroll(cursor: usize, mut scroll: usize, height: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }

    let max_scroll = len.saturating_sub(height);
    scroll = scroll.min(max_scroll);
    let scrolloff = SCROLL_OFF.min(height.saturating_sub(1) / 2);

    let lower_bound = scroll
        .saturating_add(height)
        .saturating_sub(1)
        .saturating_sub(scrolloff);
    if cursor > lower_bound {
        scroll = cursor
            .saturating_add(1)
            .saturating_add(scrolloff)
            .saturating_sub(height);
    }

    let upper_bound = scroll.saturating_add(scrolloff);
    if cursor < upper_bound {
        scroll = cursor.saturating_sub(scrolloff);
    }

    scroll.min(max_scroll)
}

impl Default for DashboardState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct MemberGroup<'a> {
    pub label: String,
    pub color: Option<u32>,
    pub entries: Vec<MemberEntry<'a>>,
}

#[derive(Debug, Clone, Copy)]
pub enum MemberEntry<'a> {
    Guild(&'a GuildMemberState),
    Recipient(&'a ChannelRecipientState),
}

impl MemberEntry<'_> {
    pub fn display_name(self) -> String {
        match self {
            Self::Guild(member) => member.display_name.clone(),
            Self::Recipient(recipient) => recipient.display_name.clone(),
        }
    }

    pub fn is_bot(self) -> bool {
        match self {
            Self::Guild(member) => member.is_bot,
            Self::Recipient(recipient) => recipient.is_bot,
        }
    }

    pub fn status(self) -> PresenceStatus {
        match self {
            Self::Guild(member) => member.status,
            Self::Recipient(recipient) => recipient.status,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ChannelPaneEntry<'a> {
    CategoryHeader {
        state: &'a ChannelState,
        collapsed: bool,
    },
    Channel {
        state: &'a ChannelState,
        branch: ChannelBranch,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ChannelBranch {
    None,
    Middle,
    Last,
}

impl ChannelBranch {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Middle => "├ ",
            Self::Last => "└ ",
        }
    }

    fn is_category_child(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum GuildPaneEntry<'a> {
    DirectMessages,
    FolderHeader {
        folder: &'a GuildFolder,
        collapsed: bool,
    },
    Guild {
        state: &'a GuildState,
        branch: GuildBranch,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GuildBranch {
    None,
    Middle,
    Last,
}

impl GuildBranch {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Middle => "├ ",
            Self::Last => "└ ",
        }
    }

    fn is_folder_child(self) -> bool {
        !matches!(self, Self::None)
    }
}

impl GuildPaneEntry<'_> {
    pub fn label(&self) -> &str {
        match self {
            Self::DirectMessages => "Direct Messages",
            Self::FolderHeader { folder, .. } => folder.name.as_deref().unwrap_or("Folder"),
            Self::Guild { state, .. } => state.name.as_str(),
        }
    }
}

/// Convert a Discord folder color (24-bit RGB integer) to a ratatui color.
/// Falls back to a neutral cyan when the color is missing or zero so
/// uncolored folders still read as folder headers.
pub fn folder_color(color: Option<u32>) -> Color {
    match color {
        Some(value) if value != 0 => {
            let r = ((value >> 16) & 0xFF) as u8;
            let g = ((value >> 8) & 0xFF) as u8;
            let b = (value & 0xFF) as u8;
            Color::Rgb(r, g, b)
        }
        _ => Color::Cyan,
    }
}

pub fn presence_color(status: PresenceStatus) -> Color {
    match status {
        PresenceStatus::Online => Color::Green,
        PresenceStatus::Idle => Color::Rgb(180, 140, 0),
        PresenceStatus::DoNotDisturb => Color::Red,
        PresenceStatus::Offline => Color::DarkGray,
        PresenceStatus::Unknown => Color::DarkGray,
    }
}

fn sorted_hoisted_roles<'a>(roles: &'a [&'a RoleState]) -> Vec<&'a RoleState> {
    let mut roles: Vec<&RoleState> = roles.iter().copied().filter(|role| role.hoist).collect();
    roles.sort_by(|left, right| role_display_order(left, right));
    roles
}

fn primary_hoisted_role(member: &GuildMemberState, roles: &[&RoleState]) -> Option<Id<RoleMarker>> {
    member
        .role_ids
        .iter()
        .filter_map(|role_id| roles.iter().find(|role| role.id == *role_id).copied())
        .filter(|role| role.hoist)
        .min_by(|left, right| role_display_order(left, right))
        .map(|role| role.id)
}

fn role_display_order(left: &RoleState, right: &RoleState) -> std::cmp::Ordering {
    right
        .position
        .cmp(&left.position)
        .then(left.id.get().cmp(&right.id.get()))
}

fn sort_member_entries(entries: &mut [&GuildMemberState]) {
    entries.sort_by(|left, right| {
        member_status_rank(left.status)
            .cmp(&member_status_rank(right.status))
            .then_with(|| {
                left.display_name
                    .to_lowercase()
                    .cmp(&right.display_name.to_lowercase())
            })
    });
}

fn sort_recipient_entries(entries: &mut [&ChannelRecipientState]) {
    entries.sort_by(|left, right| {
        member_status_rank(left.status)
            .cmp(&member_status_rank(right.status))
            .then_with(|| {
                left.display_name
                    .to_lowercase()
                    .cmp(&right.display_name.to_lowercase())
            })
    });
}

fn is_direct_message_channel(channel: &ChannelState) -> bool {
    matches!(
        channel.kind.as_str(),
        "dm" | "Private" | "group-dm" | "Group"
    )
}

fn member_status_rank(status: PresenceStatus) -> u8 {
    match status {
        PresenceStatus::Online => 0,
        PresenceStatus::Idle => 1,
        PresenceStatus::DoNotDisturb => 2,
        PresenceStatus::Offline => 3,
        PresenceStatus::Unknown => 4,
    }
}

pub fn presence_marker(status: PresenceStatus) -> char {
    match status {
        PresenceStatus::Online => '●',
        PresenceStatus::Idle => '◐',
        PresenceStatus::DoNotDisturb => '⊘',
        PresenceStatus::Offline => '○',
        PresenceStatus::Unknown => ' ',
    }
}

fn sort_channels(channels: &mut [&ChannelState]) {
    channels.sort_by_key(|channel| (channel.position.unwrap_or(i32::MAX), channel.id));
}

fn sort_direct_message_channels(channels: &mut [&ChannelState]) {
    channels.sort_by(|left, right| {
        right
            .last_message_id
            .cmp(&left.last_message_id)
            .then_with(|| right.id.cmp(&left.id))
    });
}

#[cfg(test)]
mod tests {
    use twilight_model::id::{Id, marker::ChannelMarker, marker::UserMarker};

    use super::{
        ChannelBranch, ChannelPaneEntry, DashboardState, FocusPane, GuildBranch, GuildPaneEntry,
        MessageActionKind, MessageState, message_rendered_height, presence_marker,
    };
    use crate::discord::{
        AppCommand, AppEvent, AttachmentInfo, ChannelInfo, ChannelRecipientInfo, CustomEmojiInfo,
        GuildFolder, MemberInfo, MessageInfo, MessageKind, MessageReferenceInfo,
        MessageSnapshotInfo, PollAnswerInfo, PollInfo, PresenceStatus, ReactionEmoji, ReactionInfo,
        ReactionUserInfo, ReactionUsersInfo, ReplyInfo, RoleInfo,
    };

    #[test]
    fn tracks_current_user_from_ready() {
        let mut state = DashboardState::new();
        state.push_event(AppEvent::Ready {
            user: "neo".to_owned(),
            user_id: Some(Id::new(10)),
        });
        assert_eq!(state.current_user(), Some("neo"));
        assert_eq!(state.current_user_id, Some(Id::new(10)));
    }

    #[test]
    fn captures_last_gateway_error() {
        let mut state = DashboardState::new();
        state.push_event(AppEvent::GatewayError {
            message: "boom".to_owned(),
        });
        assert_eq!(state.last_error(), Some("boom"));
    }

    #[test]
    fn dashboard_starts_without_message_focus() {
        let state = DashboardState::new();

        assert_eq!(state.focus(), FocusPane::Guilds);
        assert_eq!(state.focused_message_selection(), None);
    }

    #[test]
    fn cycle_focus_uses_four_top_level_panes() {
        let mut state = DashboardState::new();

        assert_eq!(state.focus(), FocusPane::Guilds);
        state.cycle_focus();
        assert_eq!(state.focus(), FocusPane::Channels);
        state.cycle_focus();
        assert_eq!(state.focus(), FocusPane::Messages);
        state.cycle_focus();
        assert_eq!(state.focus(), FocusPane::Members);
        state.cycle_focus();
        assert_eq!(state.focus(), FocusPane::Guilds);
    }

    #[test]
    fn loaded_messages_are_unselected_until_message_pane_is_focused() {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
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
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        for id in 1..=2u64 {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some(format!("msg {id}")),
                mentions: Vec::new(),
                attachments: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }

        assert_eq!(state.selected_message(), 1);
        assert_eq!(state.focused_message_selection(), None);

        while state.focus() != FocusPane::Messages {
            state.cycle_focus();
        }
        assert_eq!(state.focused_message_selection(), Some(0));
    }

    #[test]
    fn startup_events_do_not_auto_open_direct_messages() {
        let channel_id: Id<ChannelMarker> = Id::new(20);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
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
        state.push_event(AppEvent::MessageCreate {
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
            content: Some("hello".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(state.selected_channel_id(), None);
        assert_eq!(state.selected_channel_state(), None);
        assert!(state.channel_pane_entries().is_empty());
        assert!(state.messages().is_empty());
    }

    #[test]
    fn member_groups_use_roles_and_status_sorted_entries() {
        let guild_id = Id::new(1);
        let alice: Id<UserMarker> = Id::new(10);
        let bob: Id<UserMarker> = Id::new(20);
        let admin_role = Id::new(100);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
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
                recipients: None,
            }],
            members: vec![
                MemberInfo {
                    user_id: bob,
                    display_name: "bob".to_owned(),
                    is_bot: false,
                    avatar_url: None,
                    role_ids: vec![admin_role],
                },
                MemberInfo {
                    user_id: alice,
                    display_name: "alice".to_owned(),
                    is_bot: false,
                    avatar_url: None,
                    role_ids: vec![admin_role],
                },
            ],
            presences: vec![(alice, PresenceStatus::Online), (bob, PresenceStatus::Idle)],
            roles: vec![RoleInfo {
                id: admin_role,
                name: "Admin".to_owned(),
                color: Some(0xFFAA00),
                position: 10,
                hoist: true,
            }],
            emojis: Vec::new(),
        });
        state.confirm_selected_guild();

        let groups = state.members_grouped();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].label, "Admin");
        assert_eq!(groups[0].color, Some(0xFFAA00));
        assert_eq!(
            groups[0]
                .entries
                .iter()
                .map(|member| member.display_name())
                .collect::<Vec<_>>(),
            vec!["alice".to_owned(), "bob".to_owned()],
        );
    }

    #[test]
    fn member_groups_show_selected_group_dm_recipients() {
        let mut state = DashboardState::new();
        let channel_id = Id::new(20);
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
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
            recipients: Some(vec![
                ChannelRecipientInfo {
                    user_id: Id::new(30),
                    display_name: "bob".to_owned(),
                    is_bot: false,
                    avatar_url: None,
                    status: Some(PresenceStatus::Idle),
                },
                ChannelRecipientInfo {
                    user_id: Id::new(10),
                    display_name: "alice".to_owned(),
                    is_bot: false,
                    avatar_url: None,
                    status: Some(PresenceStatus::Online),
                },
            ]),
        }));

        state.confirm_selected_guild();
        state.confirm_selected_channel();

        let groups = state.members_grouped();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].label, "Members");
        assert_eq!(
            groups[0]
                .entries
                .iter()
                .map(|member| (member.display_name(), member.status()))
                .collect::<Vec<_>>(),
            vec![
                ("alice".to_owned(), PresenceStatus::Online),
                ("bob".to_owned(), PresenceStatus::Idle),
            ],
        );
    }

    #[test]
    fn unknown_presence_uses_neutral_member_marker() {
        assert_eq!(presence_marker(PresenceStatus::Unknown), ' ');
    }

    #[test]
    fn member_groups_keep_unroled_guild_members_in_members_group() {
        let guild_id = Id::new(1);
        let admin_role = Id::new(100);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
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
                recipients: None,
            }],
            members: vec![
                MemberInfo {
                    user_id: Id::new(10),
                    display_name: "alice".to_owned(),
                    is_bot: false,
                    avatar_url: None,
                    role_ids: vec![admin_role],
                },
                MemberInfo {
                    user_id: Id::new(20),
                    display_name: "bob".to_owned(),
                    is_bot: false,
                    avatar_url: None,
                    role_ids: Vec::new(),
                },
            ],
            presences: vec![
                (Id::new(10), PresenceStatus::Online),
                (Id::new(20), PresenceStatus::Offline),
            ],
            roles: vec![RoleInfo {
                id: admin_role,
                name: "Admin".to_owned(),
                color: Some(0xFFAA00),
                position: 10,
                hoist: true,
            }],
            emojis: Vec::new(),
        });
        state.confirm_selected_guild();

        let groups = state.members_grouped();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].label, "Admin");
        assert_eq!(groups[0].entries[0].display_name(), "alice");
        assert_eq!(groups[1].label, "Members");
        assert_eq!(groups[1].entries[0].display_name(), "bob");
    }

    #[test]
    fn member_groups_show_selected_dm_recipient() {
        let mut state = DashboardState::new();
        let channel_id = Id::new(20);
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
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
                user_id: Id::new(10),
                display_name: "alice".to_owned(),
                is_bot: false,
                avatar_url: None,
                status: Some(PresenceStatus::DoNotDisturb),
            }]),
        }));

        state.confirm_selected_guild();
        state.confirm_selected_channel();

        let groups = state.members_grouped();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].label, "Members");
        assert_eq!(groups[0].entries.len(), 1);
        assert_eq!(groups[0].entries[0].display_name(), "alice");
        assert_eq!(groups[0].entries[0].status(), PresenceStatus::DoNotDisturb);
    }

    #[test]
    fn emoji_picker_items_include_available_custom_emojis_for_selected_message_guild() {
        let state = state_with_custom_emojis();

        let items = state.emoji_reaction_items();

        assert_eq!(items.len(), 9);
        assert_eq!(items[0].emoji, ReactionEmoji::Unicode("👍".to_owned()));
        assert_eq!(items[8].label, "Party Time");
        assert_eq!(
            items[8].emoji,
            ReactionEmoji::Custom {
                id: Id::new(50),
                name: Some("party_time".to_owned()),
                animated: true,
            }
        );
    }

    #[test]
    fn custom_emoji_reaction_items_expose_cdn_image_url() {
        let state = state_with_custom_emojis();

        let items = state.emoji_reaction_items();

        assert_eq!(
            items[8].custom_image_url().as_deref(),
            Some("https://cdn.discordapp.com/emojis/50.gif")
        );
        assert_eq!(items[0].custom_image_url(), None);
    }

    #[test]
    fn emoji_picker_items_include_custom_emojis_from_update_event() {
        let guild_id = Id::new(1);
        let mut state = state_with_messages(1);

        state.push_event(AppEvent::GuildEmojisUpdate {
            guild_id,
            emojis: vec![CustomEmojiInfo {
                id: Id::new(60),
                name: "wave".to_owned(),
                animated: false,
                available: true,
            }],
        });

        let items = state.emoji_reaction_items();

        assert_eq!(items.len(), 9);
        assert_eq!(items[8].label, "Wave");
        assert_eq!(
            items[8].emoji,
            ReactionEmoji::Custom {
                id: Id::new(60),
                name: Some("wave".to_owned()),
                animated: false,
            }
        );
    }

    #[test]
    fn emoji_picker_uses_channel_guild_when_selected_message_lacks_guild_id() {
        let mut state = state_with_custom_emojis();

        state.push_event(AppEvent::MessageCreate {
            guild_id: None,
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("history message without guild".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let items = state.emoji_reaction_items();

        assert_eq!(items.len(), 9);
        assert_eq!(items[8].label, "Party Time");
    }

    #[test]
    fn emoji_picker_items_stay_unicode_only_for_direct_messages() {
        let mut state = DashboardState::new();
        let channel_id = Id::new(20);
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: None,
            channel_id,
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "neo".to_owned(),
            kind: "dm".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            recipients: None,
        }));
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        state.push_event(AppEvent::MessageCreate {
            guild_id: None,
            channel_id,
            message_id: Id::new(1),
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

        assert_eq!(state.emoji_reaction_items().len(), 8);
    }

    #[test]
    fn message_creation_keeps_viewport_on_latest() {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
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
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        for id in 1..=3u64 {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some(format!("msg {id}")),
                mentions: Vec::new(),
                attachments: Vec::new(),
                forwarded_snapshots: Vec::new(),
            });
        }

        assert_eq!(state.selected_message(), 2);
    }

    #[test]
    fn message_scroll_preserves_position_when_not_following() {
        let mut state = state_with_messages(5);
        focus_messages(&mut state);
        state.set_message_view_height(6);

        assert_eq!(state.selected_message(), 4);
        assert!(state.message_auto_follow());

        state.move_up();
        assert_eq!(state.selected_message(), 3);
        assert!(!state.message_auto_follow());

        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(6),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("msg 6".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(state.selected_message(), 3);
        assert_eq!(state.messages()[state.selected_message()].id, Id::new(4));
        assert!(!state.message_auto_follow());
    }

    #[test]
    fn message_auto_follow_can_jump_back_to_latest() {
        let mut state = state_with_messages(5);
        focus_messages(&mut state);
        state.set_message_view_height(6);

        state.move_up();
        assert!(!state.message_auto_follow());

        state.toggle_message_auto_follow();

        assert!(state.message_auto_follow());
        assert_eq!(state.selected_message(), 4);
    }

    #[test]
    fn image_preview_rows_keep_latest_message_visible_when_auto_following() {
        let mut state = state_with_image_messages(6, &[1]);
        focus_messages(&mut state);
        state.set_message_view_height(6);

        assert_eq!(state.message_scroll(), 0);

        state.clamp_message_viewport_for_image_previews(200, 16, 3);

        assert!(state.message_scroll() > 0 || state.message_line_scroll() > 0);
        let selected_bottom = state
            .selected_message_rendered_row(200, 16, 3)
            .saturating_add(
                state
                    .selected_message_rendered_height(200, 16, 3)
                    .saturating_sub(1),
            );
        assert!(selected_bottom < state.message_view_height());
    }

    #[test]
    fn image_preview_scrolloff_keeps_selected_message_visible() {
        let mut state = state_with_image_messages(8, &[5, 6, 7]);
        focus_messages(&mut state);
        state.set_message_view_height(14);

        while state.selected_message() > 3 {
            state.move_up();
        }
        state.clamp_message_viewport_for_image_previews(200, 16, 3);

        assert_eq!(state.following_message_rendered_rows(200, 16, 3, 3), 18);
        let selected_bottom = state
            .selected_message_rendered_row(200, 16, 3)
            .saturating_add(
                state
                    .selected_message_rendered_height(200, 16, 3)
                    .saturating_sub(1),
            );
        assert!(selected_bottom < state.message_view_height());
    }

    #[test]
    fn video_attachment_does_not_reserve_image_preview_rows() {
        let message = MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some("clip".to_owned()),
            mentions: Vec::new(),
            attachments: vec![video_attachment(1)],
            forwarded_snapshots: Vec::new(),
        };

        assert_eq!(message_rendered_height(&message, 200, 16, 3), 3);
    }

    #[test]
    fn explicit_newlines_increase_message_rendered_height() {
        let message = MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some("hello\nworld".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        };

        assert_eq!(message_rendered_height(&message, 200, 16, 3), 3);
    }

    #[test]
    fn wrapped_content_increases_message_rendered_height() {
        let message = MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some("abcdefghijkl".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        };

        assert_eq!(message_rendered_height(&message, 5, 16, 3), 4);
    }

    #[test]
    fn rendered_mentions_affect_message_height() {
        let mut state = state_with_single_message_content("<@10><@10>");
        state.push_event(AppEvent::GuildMemberUpsert {
            guild_id: Id::new(1),
            member: MemberInfo {
                user_id: Id::new(10),
                display_name: "a".to_owned(),
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });
        let message = state.messages()[0];

        assert_eq!(message_rendered_height(message, 5, 16, 3), 3);
        assert_eq!(state.message_base_line_count_for_width(message, 5), 2);
    }

    #[test]
    fn forwarded_mentions_affect_height_from_source_channel_guild() {
        let mut state = DashboardState::new();
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            guild_id: Some(Id::new(2)),
            channel_id: Id::new(9),
            parent_id: None,
            position: None,
            last_message_id: None,
            name: "source".to_owned(),
            kind: "GuildText".to_owned(),
            message_count: None,
            total_message_sent: None,
            thread_archived: None,
            thread_locked: None,
            recipients: None,
        }));
        state.push_event(AppEvent::GuildMemberUpsert {
            guild_id: Id::new(2),
            member: MemberInfo {
                user_id: Id::new(10),
                display_name: "a".to_owned(),
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            },
        });
        let message = MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: vec![MessageSnapshotInfo {
                content: Some("<@10><@10>".to_owned()),
                mentions: Vec::new(),
                attachments: Vec::new(),
                source_channel_id: Some(Id::new(9)),
                timestamp: None,
            }],
        };

        assert_eq!(state.message_base_line_count_for_width(&message, 7), 4);
    }

    #[test]
    fn wide_content_increases_message_rendered_height_by_terminal_width() {
        let message = MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some("漢字仮名交じ".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        };

        assert_eq!(message_rendered_height(&message, 10, 16, 3), 3);
    }

    #[test]
    fn image_attachment_summary_reserves_text_row_before_preview() {
        let message = MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some("look".to_owned()),
            mentions: Vec::new(),
            attachments: vec![image_attachment(1)],
            forwarded_snapshots: Vec::new(),
        };

        assert_eq!(message_rendered_height(&message, 200, 16, 3), 6);
    }

    #[test]
    fn forwarded_image_attachment_reserves_preview_rows() {
        let message = MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: vec![forwarded_snapshot(1)],
        };

        assert_eq!(message_rendered_height(&message, 200, 16, 3), 7);
    }

    #[test]
    fn forwarded_snapshot_wrapped_content_increases_rendered_height() {
        let message = MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: vec![MessageSnapshotInfo {
                content: Some("abcdefghijkl".to_owned()),
                mentions: Vec::new(),
                attachments: vec![image_attachment(1)],
                source_channel_id: None,
                timestamp: None,
            }],
        };

        assert_eq!(message_rendered_height(&message, 7, 16, 3), 9);
    }

    #[test]
    fn forwarded_snapshot_wide_content_uses_terminal_width() {
        let message = MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: vec![MessageSnapshotInfo {
                content: Some("漢字仮名交じ".to_owned()),
                mentions: Vec::new(),
                attachments: vec![image_attachment(1)],
                source_channel_id: None,
                timestamp: None,
            }],
        };

        assert_eq!(message_rendered_height(&message, 12, 16, 3), 8);
    }

    #[test]
    fn forwarded_metadata_reserves_card_row() {
        let mut snapshot = forwarded_snapshot(1);
        snapshot.source_channel_id = Some(Id::new(2));
        snapshot.timestamp = Some("2026-04-30T12:34:56.000000+00:00".to_owned());
        let message = MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: vec![snapshot],
        };

        assert_eq!(message_rendered_height(&message, 200, 16, 3), 8);
    }

    #[test]
    fn non_default_message_kind_reserves_label_row() {
        let mut message = MessageState {
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
            content: Some("reply body".to_owned()),
            mentions: Vec::new(),
            attachments: vec![image_attachment(1)],
            forwarded_snapshots: Vec::new(),
        };

        assert_eq!(message_rendered_height(&message, 200, 16, 3), 6);

        message.message_kind = MessageKind::new(19);

        assert_eq!(message_rendered_height(&message, 200, 16, 3), 7);
    }

    #[test]
    fn reply_preview_reserves_connector_row_without_extra_type_label() {
        let message = MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::new(19),
            reference: None,
            reply: Some(ReplyInfo {
                author: "casey".to_owned(),
                content: Some("looks good".to_owned()),
                mentions: Vec::new(),
            }),
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: Some("asdf".to_owned()),
            mentions: Vec::new(),
            attachments: vec![image_attachment(1)],
            forwarded_snapshots: Vec::new(),
        };

        assert_eq!(message_rendered_height(&message, 200, 16, 3), 7);
    }

    #[test]
    fn poll_message_reserves_question_and_answer_rows() {
        let message = MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info(false)),
            pinned: false,
            reactions: Vec::new(),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        };

        assert_eq!(message_rendered_height(&message, 200, 16, 3), 6);
    }

    #[test]
    fn thread_created_message_reserves_system_card_rows() {
        let mut message = height_test_message("release notes");
        message.message_kind = MessageKind::new(18);

        assert_eq!(message_rendered_height(&message, 200, 16, 3), 4);
    }

    #[test]
    fn poll_result_message_reserves_result_card_rows() {
        let mut message = height_test_message("");
        message.message_kind = MessageKind::new(46);
        message.poll = Some(poll_info(false));

        assert_eq!(message_rendered_height(&message, 200, 16, 3), 5);
    }

    #[test]
    fn thread_starter_message_reserves_system_card_rows() {
        let mut message = height_test_message("");
        message.message_kind = MessageKind::new(21);
        message.reply = Some(ReplyInfo {
            author: "alice".to_owned(),
            content: Some("original topic".to_owned()),
            mentions: Vec::new(),
        });

        assert_eq!(message_rendered_height(&message, 200, 16, 3), 3);
    }

    #[test]
    fn multiselect_poll_message_uses_same_card_height() {
        let message = MessageState {
            id: Id::new(1),
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info(true)),
            pinned: false,
            reactions: Vec::new(),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        };

        assert_eq!(message_rendered_height(&message, 200, 16, 3), 6);
    }

    #[test]
    fn message_action_items_reflect_selected_message_capabilities() {
        let mut state = state_with_image_messages(1, &[1]);
        focus_messages(&mut state);

        let actions = state.selected_message_action_items();

        assert!(
            actions.iter().any(|action| {
                action.kind == MessageActionKind::DownloadImage && action.enabled
            })
        );
        assert!(!actions.iter().any(|action| action.label.contains("poll")));
    }

    #[test]
    fn normal_message_actions_do_not_include_poll_or_image_actions() {
        let mut state = state_with_messages(1);
        focus_messages(&mut state);

        let actions = state.selected_message_action_items();

        assert_eq!(
            actions.iter().map(|action| action.kind).collect::<Vec<_>>(),
            vec![
                MessageActionKind::Reply,
                MessageActionKind::AddReaction,
                MessageActionKind::LoadPinnedMessages,
                MessageActionKind::SetPinned(true),
            ]
        );
    }

    #[test]
    fn reaction_message_actions_use_single_reacted_users_item() {
        let mut state = state_with_reaction_message();
        focus_messages(&mut state);

        let actions = state.selected_message_action_items();

        assert_eq!(
            actions.iter().map(|action| action.kind).collect::<Vec<_>>(),
            vec![
                MessageActionKind::Reply,
                MessageActionKind::AddReaction,
                MessageActionKind::LoadPinnedMessages,
                MessageActionKind::SetPinned(true),
                MessageActionKind::ShowReactionUsers,
                MessageActionKind::RemoveReaction(0),
            ]
        );
        assert_eq!(
            actions
                .iter()
                .filter(|action| action.label == "Show reacted users")
                .count(),
            1
        );
        assert!(!actions.iter().any(|action| action.label == "Show 👍 users"));
    }

    #[test]
    fn show_reacted_users_action_loads_all_reaction_emojis() {
        let mut state = state_with_reaction_message();
        focus_messages(&mut state);
        state.open_selected_message_actions();
        for _ in 0..4 {
            state.move_message_action_down();
        }

        let command = state.activate_selected_message_action();

        assert_eq!(
            command,
            Some(AppCommand::LoadReactionUsers {
                channel_id: Id::new(2),
                message_id: Id::new(1),
                reactions: vec![
                    ReactionEmoji::Unicode("👍".to_owned()),
                    ReactionEmoji::Custom {
                        id: Id::new(50),
                        name: Some("party".to_owned()),
                        animated: false,
                    },
                ],
            })
        );
        assert!(!state.is_message_action_menu_open());
    }

    #[test]
    fn reaction_users_loaded_opens_popup_state() {
        let mut state = state_with_messages(1);

        state.push_event(AppEvent::ReactionUsersLoaded {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            reactions: vec![ReactionUsersInfo {
                emoji: ReactionEmoji::Unicode("👍".to_owned()),
                users: vec![ReactionUserInfo {
                    user_id: Id::new(10),
                    display_name: "neo".to_owned(),
                }],
            }],
        });

        assert!(state.is_reaction_users_popup_open());
        assert_eq!(state.last_status(), Some("loaded reacted users"));
        assert_eq!(
            state
                .reaction_users_popup()
                .map(|popup| popup.reactions()[0].users[0].display_name.as_str()),
            Some("neo")
        );
    }

    #[test]
    fn thread_created_message_action_opens_cached_thread() {
        let mut state = state_with_thread_created_message();
        focus_messages(&mut state);

        let actions = state.selected_message_action_items();
        assert_eq!(
            actions.iter().map(|action| action.kind).collect::<Vec<_>>(),
            vec![
                MessageActionKind::Reply,
                MessageActionKind::OpenThread,
                MessageActionKind::AddReaction,
                MessageActionKind::LoadPinnedMessages,
                MessageActionKind::SetPinned(true),
            ]
        );

        state.open_selected_message_actions();
        state.move_message_action_down();
        let command = state.activate_selected_message_action();

        assert_eq!(state.selected_channel_id(), Some(Id::new(10)));
        assert_eq!(command, None);
    }

    #[test]
    fn history_loaded_thread_created_message_opens_reference_thread_after_rename() {
        let mut state = state_with_thread_created_message();
        state.push_event(AppEvent::MessageHistoryLoaded {
            channel_id: Id::new(2),
            before: None,
            messages: vec![MessageInfo {
                message_kind: MessageKind::new(18),
                reference: Some(MessageReferenceInfo {
                    guild_id: Some(Id::new(1)),
                    channel_id: Some(Id::new(10)),
                    message_id: None,
                }),
                pinned: false,
                reactions: Vec::new(),
                content: Some("old thread name".to_owned()),
                ..message_info(Id::new(2), 2)
            }],
        });
        focus_messages(&mut state);
        state.jump_bottom();

        let actions = state.selected_message_action_items();
        assert!(
            actions
                .iter()
                .any(|action| action.kind == MessageActionKind::OpenThread)
        );

        state.open_selected_message_actions();
        state.move_message_action_down();
        state.activate_selected_message_action();

        assert_eq!(state.selected_channel_id(), Some(Id::new(10)));
    }

    #[test]
    fn composer_sends_to_opened_thread_channel() {
        let mut state = state_with_thread_created_message();
        focus_messages(&mut state);
        state.open_selected_message_actions();
        state.move_message_action_down();
        state.activate_selected_message_action();

        state.start_composer();
        state.push_composer_char('h');
        state.push_composer_char('i');

        assert_eq!(
            state.submit_composer(),
            Some(AppCommand::SendMessage {
                channel_id: Id::new(10),
                content: "hi".to_owned(),
                reply_to: None,
            })
        );
    }

    #[test]
    fn poll_vote_actions_are_available_by_default() {
        let mut state = state_with_messages(1);
        focus_messages(&mut state);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info(false)),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        let actions = state.selected_message_action_items();

        assert_eq!(
            actions.iter().map(|action| action.kind).collect::<Vec<_>>(),
            vec![
                MessageActionKind::Reply,
                MessageActionKind::AddReaction,
                MessageActionKind::LoadPinnedMessages,
                MessageActionKind::SetPinned(true),
                MessageActionKind::VotePollAnswer(1),
                MessageActionKind::VotePollAnswer(2),
            ]
        );
        assert_eq!(actions[4].label, "Remove poll vote: Soup");
        assert_eq!(actions[5].label, "Vote poll: Noodles");
    }

    #[test]
    fn message_action_items_keep_image_action_for_poll_messages() {
        let mut state = state_with_image_messages(1, &[1]);
        focus_messages(&mut state);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info(false)),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: vec![image_attachment(1)],
            forwarded_snapshots: Vec::new(),
        });

        let actions = state.selected_message_action_items();

        assert_eq!(
            actions.iter().map(|action| action.kind).collect::<Vec<_>>(),
            vec![
                MessageActionKind::Reply,
                MessageActionKind::DownloadImage,
                MessageActionKind::AddReaction,
                MessageActionKind::LoadPinnedMessages,
                MessageActionKind::SetPinned(true),
                MessageActionKind::VotePollAnswer(1),
                MessageActionKind::VotePollAnswer(2),
            ]
        );
    }

    #[test]
    fn poll_vote_action_can_remove_existing_vote() {
        let mut state = state_with_messages(1);
        focus_messages(&mut state);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: Some(poll_info(false)),
            content: Some(String::new()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state.open_selected_message_actions();
        for _ in 0..4 {
            state.move_message_action_down();
        }

        let command = state.activate_selected_message_action();

        assert_eq!(
            command,
            Some(AppCommand::VotePoll {
                channel_id: Id::new(2),
                message_id: Id::new(1),
                answer_ids: Vec::new(),
            })
        );
    }

    #[test]
    fn message_scroll_uses_scrolloff() {
        let mut state = state_with_messages(12);
        focus_messages(&mut state);
        state.set_message_view_height(7);

        assert_eq!(state.message_scroll(), 5);

        state.move_up();
        state.move_up();
        assert_eq!(state.selected_message(), 9);
        assert_eq!(state.message_scroll(), 5);

        state.move_up();
        assert_eq!(state.selected_message(), 8);
        assert_eq!(state.message_scroll(), 5);
    }

    #[test]
    fn message_auto_follow_keeps_latest_message_at_bottom_after_rendered_clamp() {
        let mut state = state_with_messages(12);
        focus_messages(&mut state);
        state.set_message_view_height(7);

        state.clamp_message_viewport_for_image_previews(200, 16, 3);

        assert!(state.message_auto_follow());
        assert_eq!(state.selected_message(), 11);
        assert_eq!(state.message_scroll(), 8);
        assert_eq!(state.message_line_scroll(), 1);
        assert_eq!(state.selected_message_rendered_row(200, 16, 3), 5);
    }

    #[test]
    fn message_selection_centers_selected_message_when_possible() {
        let mut state = state_with_messages(12);
        focus_messages(&mut state);
        state.set_message_view_height(7);
        state.clamp_message_viewport_for_image_previews(200, 16, 3);

        for _ in 0..4 {
            state.move_up();
            state.clamp_message_viewport_for_image_previews(200, 16, 3);
        }

        assert_eq!(state.selected_message(), 7);
        assert_eq!(state.message_scroll(), 6);
        assert_eq!(state.message_line_scroll(), 0);
        assert_eq!(state.selected_message_rendered_row(200, 16, 3), 2);
    }

    #[test]
    fn message_selection_centers_with_line_offset_inside_previous_message() {
        let mut state = state_with_single_message_content("abcdefghijkl");
        for id in 2..=5 {
            push_text_message(&mut state, id, &format!("msg {id}"));
        }
        focus_messages(&mut state);
        state.set_message_view_height(5);
        state.jump_top();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);

        state.move_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);

        assert_eq!(state.selected_message(), 1);
        assert_eq!(state.message_scroll(), 0);
        assert_eq!(state.message_line_scroll(), 3);
        assert_eq!(state.selected_message_rendered_row(5, 16, 3), 1);
    }

    #[test]
    fn message_selection_centers_with_image_preview_height() {
        let mut state = state_with_image_messages(8, &[4]);
        focus_messages(&mut state);
        state.set_message_view_height(9);
        state.jump_top();
        state.clamp_message_viewport_for_image_previews(200, 16, 3);

        for _ in 0..3 {
            state.move_down();
            state.clamp_message_viewport_for_image_previews(200, 16, 3);
        }

        assert_eq!(state.messages()[state.selected_message()].id, Id::new(4));
        assert_eq!(state.selected_message_rendered_height(200, 16, 3), 6);
        assert_eq!(state.message_scroll(), 2);
        assert_eq!(state.message_line_scroll(), 1);
        assert_eq!(state.selected_message_rendered_row(200, 16, 3), 1);
    }

    #[test]
    fn message_viewport_scrolls_by_rendered_line() {
        let mut state = state_with_single_message_content("abcdefghijkl");
        focus_messages(&mut state);
        state.set_message_view_height(3);
        state.clamp_message_viewport_for_image_previews(5, 16, 3);

        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);

        assert_eq!(state.message_scroll(), 0);
        assert_eq!(state.message_line_scroll(), 2);
        assert_eq!(state.selected_message(), 0);

        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);

        assert_eq!(state.message_scroll(), 0);
        assert_eq!(state.message_line_scroll(), 3);
    }

    #[test]
    fn viewport_scroll_moves_to_next_message_after_current_message() {
        let mut state = state_with_single_message_content("abcdefghijkl");
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("next".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        focus_messages(&mut state);
        state.set_message_view_height(3);
        state.jump_top();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);

        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);

        assert_eq!(state.message_scroll(), 1);
        assert_eq!(state.message_line_scroll(), 0);
        assert_eq!(state.selected_message(), 0);
    }

    #[test]
    fn focused_message_selection_returns_none_when_viewport_scrolled_past_selection() {
        let mut state = state_with_single_message_content("abcdefghijkl");
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("next".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        focus_messages(&mut state);
        state.set_message_view_height(3);
        state.jump_top();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);

        for _ in 0..4 {
            state.scroll_message_viewport_down();
            state.clamp_message_viewport_for_image_previews(5, 16, 3);
        }

        assert_eq!(state.message_scroll(), 1);
        assert_eq!(state.selected_message(), 0);
        assert_eq!(state.focused_message_selection(), None);
    }

    #[test]
    fn viewport_scrolls_by_rendered_line_when_selected_message_is_below_top() {
        let mut state = state_with_single_message_content("abcdefghijkl");
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("next".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        focus_messages(&mut state);
        state.set_message_view_height(3);
        state.jump_top();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);

        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);

        assert_eq!(state.message_scroll(), 0);
        assert_eq!(state.message_line_scroll(), 2);
        assert_eq!(state.selected_message(), 0);

        state.move_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);

        assert_eq!(state.selected_message(), 1);
        let selected_bottom = state
            .selected_message_rendered_row(5, 16, 3)
            .saturating_add(
                state
                    .selected_message_rendered_height(5, 16, 3)
                    .saturating_sub(1),
            );
        assert!(selected_bottom < state.message_view_height());
    }

    #[test]
    fn tall_message_clamp_keeps_next_selected_message_visible() {
        let mut state = state_with_single_message_content(
            "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz",
        );
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("next".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        focus_messages(&mut state);
        state.set_message_view_height(3);
        state.jump_top();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);

        state.move_down();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);

        let selected_bottom = state
            .selected_message_rendered_row(5, 16, 3)
            .saturating_add(
                state
                    .selected_message_rendered_height(5, 16, 3)
                    .saturating_sub(1),
            );
        assert!(selected_bottom < state.message_view_height());
        assert!(state.message_line_scroll() > 1);
    }

    #[test]
    fn viewport_scroll_up_enters_previous_long_message_at_last_line() {
        let mut state = state_with_single_message_content("abcdefghijkl");
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("next".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        focus_messages(&mut state);
        state.set_message_view_height(3);
        state.jump_top();
        state.clamp_message_viewport_for_image_previews(5, 16, 3);
        for _ in 0..3 {
            state.scroll_message_viewport_down();
            state.clamp_message_viewport_for_image_previews(5, 16, 3);
        }

        state.scroll_message_viewport_up();

        assert_eq!(state.message_scroll(), 0);
        assert_eq!(state.message_line_scroll(), 2);
        assert_eq!(state.selected_message(), 0);
    }

    #[test]
    fn shared_scroll_helper_keeps_three_rows_below_cursor_when_scrolling_starts() {
        let height = 10;
        let scroll = super::clamp_list_scroll(7, 0, height, 20);

        assert_eq!(scroll, 1);
        assert_eq!(height - 1 - (7 - scroll), 3);
    }

    #[test]
    fn shared_scroll_helper_moves_one_row_near_bottom() {
        let mut scroll = 0usize;

        for cursor in 0..20 {
            let next_scroll = super::clamp_list_scroll(cursor, scroll, 7, 20);
            assert!(
                next_scroll <= scroll.saturating_add(1),
                "cursor {cursor} moved scroll from {scroll} to {next_scroll}",
            );
            scroll = next_scroll;
        }
    }

    #[test]
    fn guild_scroll_uses_scrolloff() {
        let mut state = state_with_many_guilds(8);
        focus_guilds(&mut state);
        state.set_guild_view_height(7);

        state.jump_bottom();
        assert_eq!(state.selected_guild(), 8);
        assert_eq!(state.guild_scroll(), 2);

        state.move_up();
        state.move_up();
        assert_eq!(state.selected_guild(), 6);
        assert_eq!(state.guild_scroll(), 2);

        state.move_up();
        assert_eq!(state.selected_guild(), 5);
        assert_eq!(state.guild_scroll(), 2);
    }

    #[test]
    fn channel_scroll_uses_scrolloff() {
        let mut state = state_with_many_channels(8);
        focus_channels(&mut state);
        state.set_channel_view_height(7);

        state.jump_bottom();
        assert_eq!(state.selected_channel(), 7);
        assert_eq!(state.channel_scroll(), 1);

        state.move_up();
        state.move_up();
        assert_eq!(state.selected_channel(), 5);
        assert_eq!(state.channel_scroll(), 1);

        state.move_up();
        assert_eq!(state.selected_channel(), 4);
        assert_eq!(state.channel_scroll(), 1);
    }

    #[test]
    fn member_scroll_uses_scrolloff() {
        let mut state = state_with_members(8);
        focus_members(&mut state);
        state.set_member_view_height(7);

        state.jump_bottom();
        assert_eq!(state.selected_member(), 7);
        assert_eq!(state.member_scroll(), 2);

        state.move_up();
        state.move_up();
        assert_eq!(state.selected_member(), 5);
        assert_eq!(state.member_scroll(), 2);

        state.move_up();
        assert_eq!(state.selected_member(), 4);
        assert_eq!(state.member_scroll(), 2);
    }

    #[test]
    fn member_half_page_scrolls_by_rendered_lines() {
        let mut state = state_with_grouped_members();
        focus_members(&mut state);
        state.set_member_view_height(9);

        assert_eq!(state.selected_member(), 0);
        assert_eq!(state.selected_member_line_for_test(), 1);

        state.half_page_down();
        assert_eq!(state.selected_member(), 2);
        assert_eq!(state.selected_member_line_for_test(), 5);

        state.half_page_up();
        assert_eq!(state.selected_member(), 0);
        assert_eq!(state.selected_member_line_for_test(), 1);
    }

    #[test]
    fn half_page_scrolls_all_list_panes() {
        let mut guild_state = state_with_many_guilds(8);
        focus_guilds(&mut guild_state);
        guild_state.set_guild_view_height(9);
        guild_state.half_page_down();
        assert_eq!(guild_state.selected_guild(), 5);

        let mut channel_state = state_with_many_channels(8);
        focus_channels(&mut channel_state);
        channel_state.set_channel_view_height(9);
        channel_state.half_page_down();
        assert_eq!(channel_state.selected_channel(), 4);

        let mut member_state = state_with_members(8);
        focus_members(&mut member_state);
        member_state.set_member_view_height(9);
        member_state.half_page_down();
        assert_eq!(member_state.selected_member(), 4);
    }

    #[test]
    fn message_half_page_up_disables_follow() {
        let mut state = state_with_messages(10);
        focus_messages(&mut state);
        state.set_message_view_height(9);

        state.half_page_up();

        assert_eq!(state.selected_message(), 5);
        assert!(!state.message_auto_follow());
    }

    #[test]
    fn message_jump_bottom_does_not_enable_auto_follow() {
        let mut state = state_with_messages(10);
        focus_messages(&mut state);
        state.set_message_view_height(9);

        state.move_up();
        assert!(!state.message_auto_follow());

        state.jump_bottom();

        assert_eq!(state.selected_message(), 9);
        assert!(!state.message_auto_follow());
    }

    #[test]
    fn message_half_page_down_keeps_follow_state() {
        let mut state = state_with_messages(10);
        focus_messages(&mut state);
        state.set_message_view_height(9);

        state.half_page_down();
        assert!(state.message_auto_follow());

        state.move_up();
        assert!(!state.message_auto_follow());

        state.half_page_down();
        assert!(!state.message_auto_follow());
    }

    #[test]
    fn history_load_preserves_manual_scroll_position_by_message_id() {
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = state_with_message_ids([10, 11, 12, 13, 14]);
        focus_messages(&mut state);
        state.set_message_view_height(3);
        state.move_up();
        state.move_up();

        let selected_id = state.messages()[state.selected_message()].id;
        let scroll_id = state.messages()[state.message_scroll()].id;

        state.push_event(AppEvent::MessageHistoryLoaded {
            channel_id,
            before: None,
            messages: vec![message_info(channel_id, 5)],
        });

        assert_eq!(state.messages()[state.selected_message()].id, selected_id);
        assert_eq!(state.messages()[state.message_scroll()].id, scroll_id);
        assert!(!state.message_auto_follow());
    }

    #[test]
    fn older_history_request_waits_for_loaded_page() {
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = state_with_message_ids([10, 11, 12]);
        focus_messages(&mut state);
        state.jump_top();

        assert_eq!(
            state.next_older_history_command(),
            Some(AppCommand::LoadMessageHistory {
                channel_id,
                before: Some(Id::new(10)),
            })
        );
        assert_eq!(state.next_older_history_command(), None);

        state.push_event(AppEvent::MessageHistoryLoaded {
            channel_id,
            before: Some(Id::new(10)),
            messages: vec![message_info(channel_id, 5)],
        });

        state.move_up();
        assert_eq!(
            state.next_older_history_command(),
            Some(AppCommand::LoadMessageHistory {
                channel_id,
                before: Some(Id::new(5)),
            })
        );
    }

    #[test]
    fn older_history_request_advances_after_cache_limit_retention() {
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = state_with_message_ids(10..=209);
        focus_messages(&mut state);
        state.jump_top();

        assert_eq!(
            state.next_older_history_command(),
            Some(AppCommand::LoadMessageHistory {
                channel_id,
                before: Some(Id::new(10)),
            })
        );
        state.push_event(AppEvent::MessageHistoryLoaded {
            channel_id,
            before: Some(Id::new(10)),
            messages: vec![message_info(channel_id, 5)],
        });

        assert_eq!(
            state.messages().last().map(|message| message.id),
            Some(Id::new(209))
        );

        state.move_up();

        assert_eq!(
            state.next_older_history_command(),
            Some(AppCommand::LoadMessageHistory {
                channel_id,
                before: Some(Id::new(5)),
            })
        );
    }

    #[test]
    fn empty_older_history_page_marks_cursor_exhausted() {
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = state_with_message_ids([10, 11, 12]);
        focus_messages(&mut state);
        state.jump_top();

        assert_eq!(
            state.next_older_history_command(),
            Some(AppCommand::LoadMessageHistory {
                channel_id,
                before: Some(Id::new(10)),
            })
        );

        state.push_event(AppEvent::MessageHistoryLoaded {
            channel_id,
            before: Some(Id::new(10)),
            messages: Vec::new(),
        });

        assert_eq!(state.next_older_history_command(), None);
    }

    #[test]
    fn direct_messages_are_sorted_by_latest_message_id() {
        let mut state = state_with_direct_messages();
        state.confirm_selected_guild();

        assert_eq!(channel_entry_names(&state), vec!["new", "old", "empty"]);
    }

    #[test]
    fn direct_message_selection_waits_for_channel_confirmation() {
        let mut state = state_with_direct_messages();

        state.confirm_selected_guild();
        assert_eq!(state.selected_channel_id(), None);

        state.confirm_selected_channel();
        assert_eq!(state.selected_channel_id(), Some(Id::new(20)));
    }

    #[test]
    fn direct_message_sorting_uses_channel_id_fallback() {
        let mut state = DashboardState::new();
        for (channel_id, name) in [(Id::new(10), "older-id"), (Id::new(30), "newer-id")] {
            state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
                guild_id: None,
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: name.to_owned(),
                kind: "dm".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
            }));
        }
        state.confirm_selected_guild();

        assert_eq!(channel_entry_names(&state), vec!["newer-id", "older-id"]);
    }

    #[test]
    fn direct_message_cursor_stays_on_same_channel_after_recency_sort() {
        let mut state = state_with_direct_messages();
        state.confirm_selected_guild();
        focus_channels(&mut state);
        state.move_down();

        assert_eq!(state.selected_channel(), 1);
        assert_eq!(channel_entry_names(&state), vec!["new", "old", "empty"]);

        state.push_event(AppEvent::MessageCreate {
            guild_id: None,
            channel_id: Id::new(30),
            message_id: Id::new(300),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some("new empty dm".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });

        assert_eq!(channel_entry_names(&state), vec!["empty", "new", "old"]);
        assert_eq!(state.selected_channel(), 2);
    }

    #[test]
    fn channel_tree_groups_category_children() {
        let state = state_with_channel_tree();
        let entries = state.channel_pane_entries();

        assert!(matches!(
            entries[0],
            ChannelPaneEntry::CategoryHeader {
                collapsed: false,
                ..
            }
        ));
        assert!(matches!(
            entries[1],
            ChannelPaneEntry::Channel {
                branch: ChannelBranch::Middle,
                ..
            }
        ));
        assert!(matches!(
            entries[2],
            ChannelPaneEntry::Channel {
                branch: ChannelBranch::Last,
                ..
            }
        ));
    }

    #[test]
    fn selected_channel_category_can_be_closed_and_opened() {
        let mut state = state_with_channel_tree();

        assert_eq!(state.channel_pane_entries().len(), 3);
        assert_eq!(state.selected_channel_id(), None);

        state.close_selected_channel_category();
        let closed_entries = state.channel_pane_entries();
        assert_eq!(closed_entries.len(), 1);
        assert!(matches!(
            closed_entries[0],
            ChannelPaneEntry::CategoryHeader {
                collapsed: true,
                ..
            }
        ));

        state.open_selected_channel_category();
        assert_eq!(state.channel_pane_entries().len(), 3);
    }

    #[test]
    fn selected_channel_child_can_close_parent_category() {
        let mut state = state_with_channel_tree();
        state.selected_channel = 1;

        state.toggle_selected_channel_category();
        let entries = state.channel_pane_entries();
        assert_eq!(entries.len(), 1);
        assert!(matches!(
            entries[0],
            ChannelPaneEntry::CategoryHeader {
                collapsed: true,
                ..
            }
        ));
    }

    #[test]
    fn moving_guild_cursor_does_not_activate_guild() {
        let mut state = state_with_two_guilds();
        focus_guilds(&mut state);

        state.confirm_selected_guild();
        let active_guild = state.selected_guild_id();
        assert!(active_guild.is_some());

        state.move_down();
        assert_eq!(state.selected_guild, 2);
        assert_eq!(state.selected_guild_id(), active_guild);

        state.confirm_selected_guild();
        assert_ne!(state.selected_guild_id(), active_guild);
    }

    #[test]
    fn active_guild_entry_tracks_confirmed_guild() {
        let mut state = state_with_two_guilds();
        focus_guilds(&mut state);

        {
            let entries = state.guild_pane_entries();
            assert!(!state.is_active_guild_entry(&entries[0]));
            assert!(!state.is_active_guild_entry(&entries[1]));
            assert!(!state.is_active_guild_entry(&entries[2]));
        }

        state.confirm_selected_guild();
        {
            let entries = state.guild_pane_entries();
            assert!(!state.is_active_guild_entry(&entries[0]));
            assert!(state.is_active_guild_entry(&entries[1]));
            assert!(!state.is_active_guild_entry(&entries[2]));
        }

        state.move_down();
        {
            let entries = state.guild_pane_entries();
            assert!(state.is_active_guild_entry(&entries[1]));
            assert!(!state.is_active_guild_entry(&entries[2]));
        }

        state.confirm_selected_guild();
        let entries = state.guild_pane_entries();
        assert!(!state.is_active_guild_entry(&entries[1]));
        assert!(state.is_active_guild_entry(&entries[2]));
    }

    #[test]
    fn moving_channel_cursor_does_not_activate_channel() {
        let mut state = state_with_channel_tree();
        let random_id = Id::new(12);
        focus_channels(&mut state);

        assert_eq!(state.selected_channel_id(), None);

        state.move_down();
        state.move_down();
        assert_eq!(state.selected_channel, 2);
        assert_eq!(state.selected_channel_id(), None);

        state.confirm_selected_channel();
        assert_eq!(state.selected_channel_id(), Some(random_id));
    }

    #[test]
    fn active_channel_entry_tracks_confirmed_channel() {
        let mut state = state_with_channel_tree();
        focus_channels(&mut state);

        {
            let entries = state.channel_pane_entries();
            assert!(!state.is_active_channel_entry(&entries[0]));
            assert!(!state.is_active_channel_entry(&entries[1]));
            assert!(!state.is_active_channel_entry(&entries[2]));
        }

        state.move_down();
        state.confirm_selected_channel();
        {
            let entries = state.channel_pane_entries();
            assert!(!state.is_active_channel_entry(&entries[0]));
            assert!(state.is_active_channel_entry(&entries[1]));
            assert!(!state.is_active_channel_entry(&entries[2]));
        }

        state.move_down();
        {
            let entries = state.channel_pane_entries();
            assert!(state.is_active_channel_entry(&entries[1]));
            assert!(!state.is_active_channel_entry(&entries[2]));
        }

        state.confirm_selected_channel();
        let entries = state.channel_pane_entries();
        assert!(!state.is_active_channel_entry(&entries[1]));
        assert!(state.is_active_channel_entry(&entries[2]));
    }

    #[test]
    fn selected_folder_can_be_closed_and_opened() {
        let mut state = state_with_folder(Some(42));

        assert_eq!(state.guild_pane_entries().len(), 4);
        state.close_selected_folder();
        let closed_entries = state.guild_pane_entries();
        assert_eq!(closed_entries.len(), 2);
        assert!(matches!(
            closed_entries[1],
            GuildPaneEntry::FolderHeader {
                collapsed: true,
                ..
            }
        ));

        state.open_selected_folder();
        let open_entries = state.guild_pane_entries();
        assert_eq!(open_entries.len(), 4);
        assert!(matches!(
            open_entries[1],
            GuildPaneEntry::FolderHeader {
                collapsed: false,
                ..
            }
        ));
    }

    #[test]
    fn folder_children_use_middle_and_last_branches() {
        let state = state_with_folder(Some(42));

        let entries = state.guild_pane_entries();
        assert!(matches!(
            entries[2],
            GuildPaneEntry::Guild {
                branch: GuildBranch::Middle,
                ..
            }
        ));
        assert!(matches!(
            entries[3],
            GuildPaneEntry::Guild {
                branch: GuildBranch::Last,
                ..
            }
        ));
    }

    #[test]
    fn folder_without_id_can_be_closed() {
        let mut state = state_with_folder(None);

        state.close_selected_folder();
        let entries = state.guild_pane_entries();
        assert_eq!(entries.len(), 2);
        assert!(matches!(
            entries[1],
            GuildPaneEntry::FolderHeader {
                collapsed: true,
                ..
            }
        ));
    }

    #[test]
    fn selected_folder_child_can_close_parent() {
        let mut state = state_with_folder(Some(42));
        state.selected_guild = 2;

        state.toggle_selected_folder();
        let entries = state.guild_pane_entries();
        assert_eq!(entries.len(), 2);
        assert!(matches!(
            entries[1],
            GuildPaneEntry::FolderHeader {
                collapsed: true,
                ..
            }
        ));
    }

    fn state_with_folder(folder_id: Option<u64>) -> DashboardState {
        let first_guild = Id::new(1);
        let second_guild = Id::new(2);
        let mut state = DashboardState::new();

        for (guild_id, name) in [(first_guild, "first"), (second_guild, "second")] {
            state.push_event(AppEvent::GuildCreate {
                guild_id,
                name: name.to_owned(),
                channels: Vec::new(),
                members: Vec::new(),
                presences: Vec::new(),
                roles: Vec::new(),
                emojis: Vec::new(),
            });
        }
        state.push_event(AppEvent::GuildFoldersUpdate {
            folders: vec![GuildFolder {
                id: folder_id,
                name: Some("folder".to_owned()),
                color: None,
                guild_ids: vec![first_guild, second_guild],
            }],
        });
        state
    }

    fn state_with_many_guilds(count: u64) -> DashboardState {
        let mut state = DashboardState::new();
        for id in 1..=count {
            state.push_event(AppEvent::GuildCreate {
                guild_id: Id::new(id),
                name: format!("guild {id}"),
                channels: Vec::new(),
                members: Vec::new(),
                presences: Vec::new(),
                roles: Vec::new(),
                emojis: Vec::new(),
            });
        }
        state
    }

    fn state_with_many_channels(count: u64) -> DashboardState {
        let guild_id = Id::new(1);
        let mut state = DashboardState::new();
        let channels = (1..=count)
            .map(|id| ChannelInfo {
                guild_id: Some(guild_id),
                channel_id: Id::new(id),
                parent_id: None,
                position: Some(id as i32),
                last_message_id: None,
                name: format!("channel {id}"),
                kind: "text".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
            })
            .collect();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels,
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.confirm_selected_guild();
        state
    }

    fn state_with_members(count: u64) -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();
        let members = (1..=count)
            .map(|id| MemberInfo {
                user_id: Id::new(id),
                display_name: format!("member {id}"),
                is_bot: false,
                avatar_url: None,
                role_ids: Vec::new(),
            })
            .collect();
        let presences = (1..=count)
            .map(|id| (Id::new(id), PresenceStatus::Online))
            .collect();

        state.push_event(AppEvent::GuildCreate {
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
            members,
            presences,
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.confirm_selected_guild();
        state
    }

    fn state_with_grouped_members() -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let role_id = Id::new(100);
        let mut state = DashboardState::new();
        let members = (1..=4)
            .map(|id| MemberInfo {
                user_id: Id::new(id),
                display_name: format!("member {id}"),
                is_bot: false,
                avatar_url: None,
                role_ids: (id <= 2).then_some(role_id).into_iter().collect(),
            })
            .collect();

        state.push_event(AppEvent::GuildCreate {
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
            members,
            presences: vec![
                (Id::new(1), PresenceStatus::Online),
                (Id::new(2), PresenceStatus::Online),
                (Id::new(3), PresenceStatus::Offline),
                (Id::new(4), PresenceStatus::Offline),
            ],
            roles: vec![RoleInfo {
                id: role_id,
                name: "Role".to_owned(),
                color: None,
                position: 1,
                hoist: true,
            }],
            emojis: Vec::new(),
        });
        state.confirm_selected_guild();
        state
    }

    fn state_with_channel_tree() -> DashboardState {
        let guild_id = Id::new(1);
        let category_id = Id::new(10);
        let general_id = Id::new(11);
        let random_id = Id::new(12);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: category_id,
                    parent_id: None,
                    position: Some(0),
                    last_message_id: None,
                    name: "Text Channels".to_owned(),
                    kind: "category".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    recipients: None,
                },
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: general_id,
                    parent_id: Some(category_id),
                    position: Some(0),
                    last_message_id: None,
                    name: "general".to_owned(),
                    kind: "text".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    recipients: None,
                },
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: random_id,
                    parent_id: Some(category_id),
                    position: Some(1),
                    last_message_id: None,
                    name: "random".to_owned(),
                    kind: "text".to_owned(),
                    message_count: None,
                    total_message_sent: None,
                    thread_archived: None,
                    thread_locked: None,
                    recipients: None,
                },
            ],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.confirm_selected_guild();
        state
    }

    fn state_with_direct_messages() -> DashboardState {
        let mut state = DashboardState::new();
        for (channel_id, name, last_message_id) in [
            (Id::new(10), "old", Some(Id::new(100))),
            (Id::new(20), "new", Some(Id::new(200))),
            (Id::new(30), "empty", None),
        ] {
            state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
                guild_id: None,
                channel_id,
                parent_id: None,
                position: None,
                last_message_id,
                name: name.to_owned(),
                kind: "dm".to_owned(),
                message_count: None,
                total_message_sent: None,
                thread_archived: None,
                thread_locked: None,
                recipients: None,
            }));
        }
        state
    }

    fn state_with_messages(count: u64) -> DashboardState {
        state_with_message_ids(1..=count)
    }

    fn state_with_reaction_message() -> DashboardState {
        let mut state = state_with_messages(1);
        state.push_event(AppEvent::MessageHistoryLoaded {
            channel_id: Id::new(2),
            before: None,
            messages: vec![MessageInfo {
                reactions: vec![
                    ReactionInfo {
                        emoji: ReactionEmoji::Unicode("👍".to_owned()),
                        count: 2,
                        me: true,
                    },
                    ReactionInfo {
                        emoji: ReactionEmoji::Custom {
                            id: Id::new(50),
                            name: Some("party".to_owned()),
                            animated: false,
                        },
                        count: 1,
                        me: false,
                    },
                ],
                ..message_info(Id::new(2), 1)
            }],
        });
        state
    }

    fn state_with_custom_emojis() -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
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
            emojis: vec![
                CustomEmojiInfo {
                    id: Id::new(50),
                    name: "party_time".to_owned(),
                    animated: true,
                    available: true,
                },
                CustomEmojiInfo {
                    id: Id::new(51),
                    name: "gone".to_owned(),
                    animated: false,
                    available: false,
                },
            ],
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(1),
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
        state
    }

    fn state_with_single_message_content(content: &str) -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
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
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id,
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: crate::discord::MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(content.to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state
    }

    fn state_with_thread_created_message() -> DashboardState {
        let guild_id = Id::new(1);
        let parent_id: Id<ChannelMarker> = Id::new(2);
        let thread_id: Id<ChannelMarker> = Id::new(10);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: parent_id,
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
                },
                ChannelInfo {
                    guild_id: Some(guild_id),
                    channel_id: thread_id,
                    parent_id: Some(parent_id),
                    position: None,
                    last_message_id: None,
                    name: "release notes".to_owned(),
                    kind: "thread".to_owned(),
                    message_count: Some(12),
                    total_message_sent: Some(14),
                    thread_archived: Some(false),
                    thread_locked: Some(false),
                    recipients: None,
                },
            ],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(guild_id),
            channel_id: parent_id,
            message_id: Id::new(1),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::new(18),
            reference: Some(MessageReferenceInfo {
                guild_id: Some(guild_id),
                channel_id: Some(thread_id),
                message_id: None,
            }),
            reply: None,
            poll: None,
            content: Some("release notes".to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
        state
    }

    fn height_test_message(content: &str) -> MessageState {
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

    fn state_with_image_messages(count: u64, image_message_ids: &[u64]) -> DashboardState {
        state_with_messages_matching(1..=count, |id| image_message_ids.contains(&id))
    }

    fn state_with_message_ids(message_ids: impl IntoIterator<Item = u64>) -> DashboardState {
        state_with_messages_matching(message_ids, |_| false)
    }

    fn state_with_messages_matching(
        message_ids: impl IntoIterator<Item = u64>,
        has_image: impl Fn(u64) -> bool,
    ) -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
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
        state.confirm_selected_guild();
        state.confirm_selected_channel();
        for id in message_ids {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                author_avatar_url: None,
                message_kind: crate::discord::MessageKind::regular(),
                reference: None,
                reply: None,
                poll: None,
                content: Some(format!("msg {id}")),
                mentions: Vec::new(),
                attachments: has_image(id)
                    .then(|| image_attachment(id))
                    .into_iter()
                    .collect(),
                forwarded_snapshots: Vec::new(),
            });
        }
        state
    }

    fn push_text_message(state: &mut DashboardState, message_id: u64, content: &str) {
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(message_id),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            author_avatar_url: None,
            message_kind: MessageKind::regular(),
            reference: None,
            reply: None,
            poll: None,
            content: Some(content.to_owned()),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        });
    }

    fn image_attachment(id: u64) -> AttachmentInfo {
        AttachmentInfo {
            id: Id::new(id),
            filename: format!("image-{id}.png"),
            url: format!("https://cdn.discordapp.com/image-{id}.png"),
            proxy_url: format!("https://media.discordapp.net/image-{id}.png"),
            content_type: Some("image/png".to_owned()),
            size: 2048,
            width: Some(640),
            height: Some(480),
            description: None,
        }
    }

    fn video_attachment(id: u64) -> AttachmentInfo {
        AttachmentInfo {
            id: Id::new(id),
            filename: format!("clip-{id}.mp4"),
            url: format!("https://cdn.discordapp.com/clip-{id}.mp4"),
            proxy_url: format!("https://media.discordapp.net/clip-{id}.mp4"),
            content_type: Some("video/mp4".to_owned()),
            size: 78_364_758,
            width: Some(1920),
            height: Some(1080),
            description: None,
        }
    }

    fn forwarded_snapshot(id: u64) -> MessageSnapshotInfo {
        MessageSnapshotInfo {
            content: Some(format!("forwarded {id}")),
            mentions: Vec::new(),
            attachments: vec![image_attachment(id)],
            source_channel_id: None,
            timestamp: None,
        }
    }

    fn message_info(channel_id: Id<ChannelMarker>, message_id: u64) -> MessageInfo {
        MessageInfo {
            guild_id: Some(Id::new(1)),
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
            content: Some(format!("msg {message_id}")),
            mentions: Vec::new(),
            attachments: Vec::new(),
            forwarded_snapshots: Vec::new(),
        }
    }

    fn channel_entry_names(state: &DashboardState) -> Vec<&str> {
        state
            .channel_pane_entries()
            .into_iter()
            .filter_map(|entry| match entry {
                ChannelPaneEntry::Channel { state, .. } => Some(state.name.as_str()),
                ChannelPaneEntry::CategoryHeader { .. } => None,
            })
            .collect()
    }

    fn poll_info(allow_multiselect: bool) -> PollInfo {
        PollInfo {
            question: "What should we eat?".to_owned(),
            answers: vec![
                PollAnswerInfo {
                    answer_id: 1,
                    text: "Soup".to_owned(),
                    vote_count: Some(2),
                    me_voted: true,
                },
                PollAnswerInfo {
                    answer_id: 2,
                    text: "Noodles".to_owned(),
                    vote_count: Some(1),
                    me_voted: false,
                },
            ],
            allow_multiselect,
            results_finalized: Some(false),
            total_votes: Some(3),
        }
    }

    fn state_with_two_guilds() -> DashboardState {
        let mut state = DashboardState::new();
        let first_guild = Id::new(1);
        let second_guild = Id::new(2);
        for (guild_id, name) in [(first_guild, "first"), (second_guild, "second")] {
            state.push_event(AppEvent::GuildCreate {
                guild_id,
                name: name.to_owned(),
                channels: Vec::new(),
                members: Vec::new(),
                presences: Vec::new(),
                roles: Vec::new(),
                emojis: Vec::new(),
            });
        }
        state.push_event(AppEvent::GuildFoldersUpdate {
            folders: vec![
                GuildFolder {
                    id: None,
                    name: None,
                    color: None,
                    guild_ids: vec![first_guild],
                },
                GuildFolder {
                    id: None,
                    name: None,
                    color: None,
                    guild_ids: vec![second_guild],
                },
            ],
        });
        state
    }

    fn focus_guilds(state: &mut DashboardState) {
        while state.focus() != FocusPane::Guilds {
            state.cycle_focus();
        }
    }

    fn focus_channels(state: &mut DashboardState) {
        while state.focus() != FocusPane::Channels {
            state.cycle_focus();
        }
    }

    fn focus_members(state: &mut DashboardState) {
        while state.focus() != FocusPane::Members {
            state.cycle_focus();
        }
    }

    fn focus_messages(state: &mut DashboardState) {
        while state.focus() != FocusPane::Messages {
            state.cycle_focus();
        }
    }
}
