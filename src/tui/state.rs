use std::collections::{HashMap, HashSet, VecDeque};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};

use crate::config::DisplayOptions;
use crate::discord::{
    AppCommand, AppEvent, ChannelUnreadState, DiscordState, ForumPostArchiveState, MentionInfo,
    MessageAttachmentUpload, MessageInfo, MessageSnapshotInfo, MessageState, PresenceStatus,
};
use unicode_width::UnicodeWidthStr;

use super::format::{
    MentionTarget, RenderedText, TextHighlightKind, render_user_mentions,
    render_user_mentions_with_highlights, replace_custom_emoji_markup,
};
use super::{media, message_format, ui};

mod channels;
mod composer;
mod composer_state;
mod diagnostics;
mod emoji;
mod guilds;
mod image_viewer;
mod member_grouping;
mod message_actions;
mod message_render;
mod model;
mod options;
mod polls;
mod popups;
mod presentation;
mod reactions;
mod scroll;
mod subscriptions;
mod user;

use composer::MentionCompletion;
use message_render::{add_literal_mention_highlights, normalize_text_highlights};
use popups::{
    ChannelActionMenuState, GuildActionMenuState, ImageViewerState, MemberActionMenuState,
    UserProfilePopupState,
};
use scroll::{
    SCROLL_OFF, clamp_list_scroll, clamp_list_viewport, clamp_selected_index, last_index,
    move_index_down, move_index_down_by, move_index_up, move_index_up_by,
    normalize_message_line_scroll, pane_content_height, scroll_list_down, scroll_list_up,
    scroll_message_row_down, scroll_message_row_up,
};

pub use composer::{MAX_MENTION_PICKER_VISIBLE, MentionPickerEntry};
pub use member_grouping::{MemberEntry, MemberGroup};
pub use model::{
    ChannelActionItem, ChannelPaneEntry, ChannelThreadItem, EmojiReactionItem,
    FORUM_POST_CARD_HEIGHT, FocusPane, GuildActionItem, GuildPaneEntry, ImageViewerItem,
    MemberActionItem, MessageActionItem, MessageActionKind, PollVotePickerItem,
    ThreadMessagePreview, ThreadSummary, channel_action_shortcut, guild_action_shortcut,
    indexed_shortcut, member_action_shortcut, message_action_shortcut,
};
#[allow(unused_imports)]
pub use model::{ChannelActionKind, ChannelBranch, GuildActionKind, GuildBranch, MemberActionKind};
pub use options::DisplayOptionItem;
pub use popups::{
    EmojiReactionPickerState, MessageActionMenuState, PollVotePickerState, ReactionUsersPopupState,
};
pub use presentation::{discord_color, folder_color, presence_color, presence_marker};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OlderHistoryRequestState {
    Requested { before: Id<MessageMarker> },
    Exhausted { before: Id<MessageMarker> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnreadBanner {
    pub since_message_id: Id<MessageMarker>,
    pub unread_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopNotification {
    pub title: String,
    pub body: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveGuildScope {
    Unset,
    DirectMessages,
    Guild(Id<GuildMarker>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ThreadReturnTarget {
    thread_channel_id: Id<ChannelMarker>,
    channel_id: Id<ChannelMarker>,
    selected_message: usize,
    message_scroll: usize,
    message_line_scroll: usize,
    message_keep_selection_visible: bool,
    message_auto_follow: bool,
    new_messages_marker_message_id: Option<Id<MessageMarker>>,
    unread_divider_last_acked_id: Option<Id<MessageMarker>>,
    pending_unread_anchor_scroll: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PinnedMessageViewReturnTarget {
    channel_id: Id<ChannelMarker>,
    selected_message: usize,
    message_scroll: usize,
    message_line_scroll: usize,
    message_keep_selection_visible: bool,
    message_auto_follow: bool,
    new_messages_marker_message_id: Option<Id<MessageMarker>>,
    unread_divider_last_acked_id: Option<Id<MessageMarker>>,
    pending_unread_anchor_scroll: bool,
}

#[derive(Debug, Default)]
struct ForumPostListState {
    active_post_ids: Vec<Id<ChannelMarker>>,
    archived_post_ids: Vec<Id<ChannelMarker>>,
    has_more: bool,
}

#[derive(Debug)]
pub struct DashboardState {
    discord: DiscordState,
    focus: FocusPane,
    active_guild: ActiveGuildScope,
    active_channel_id: Option<Id<ChannelMarker>>,
    selected_guild: usize,
    guild_scroll: usize,
    guild_horizontal_scroll: usize,
    guild_keep_selection_visible: bool,
    guild_view_height: usize,
    selected_channel: usize,
    channel_scroll: usize,
    channel_horizontal_scroll: usize,
    channel_keep_selection_visible: bool,
    channel_view_height: usize,
    selected_message: usize,
    message_scroll: usize,
    message_line_scroll: usize,
    message_keep_selection_visible: bool,
    message_auto_follow: bool,
    new_messages_marker_message_id: Option<Id<MessageMarker>>,
    /// Snowflake of the last message the user had acked at the moment the
    /// active channel was opened. Captured *before* the activation-time
    /// ack so it survives the immediate ack flush, lets the renderer place
    /// a Discord-style red divider just above the first unread message,
    /// and lets the scroll math anchor the viewport to the user's
    /// last-read position once history arrives. `None` when the channel
    /// had no unread state at activation.
    unread_divider_last_acked_id: Option<Id<MessageMarker>>,
    /// Set on activation when an unread anchor needs to be applied to the
    /// viewport once history is available. Cleared the first frame the
    /// anchor is found among the loaded messages, so subsequent navigation
    /// is not pinned to the original anchor position.
    pending_unread_anchor_scroll: bool,
    message_view_height: usize,
    message_content_width: usize,
    message_preview_width: u16,
    message_max_preview_height: u16,
    pinned_message_view_channel_id: Option<Id<ChannelMarker>>,
    pinned_message_view_return_target: Option<PinnedMessageViewReturnTarget>,
    thread_return_target: Option<ThreadReturnTarget>,
    selected_member: usize,
    member_scroll: usize,
    member_horizontal_scroll: usize,
    member_keep_selection_visible: bool,
    member_view_height: usize,
    composer_input: String,
    composer_cursor_byte_index: usize,
    pending_composer_attachments: Vec<MessageAttachmentUpload>,
    composer_active: bool,
    reply_target_message_id: Option<Id<MessageMarker>>,
    edit_target_message: Option<(Id<ChannelMarker>, Id<MessageMarker>)>,
    /// Set when the user is in the middle of an `@mention` autocomplete. The
    /// stored string is the characters typed *after* the `@` and is used to
    /// filter the candidate list. `None` means the picker is closed.
    composer_mention_query: Option<String>,
    composer_mention_start: Option<usize>,
    composer_mention_selected: usize,
    /// Records `@displayname` substrings that the picker inserted, so the
    /// composer can rewrite them to Discord's `<@USER_ID>` wire format on
    /// submit even though the visible text is still the friendly form.
    composer_mention_completions: Vec<MentionCompletion>,
    message_action_menu: Option<MessageActionMenuState>,
    options_popup: Option<popups::OptionsPopupState>,
    image_viewer: Option<ImageViewerState>,
    guild_action_menu: Option<GuildActionMenuState>,
    channel_action_menu: Option<ChannelActionMenuState>,
    member_action_menu: Option<MemberActionMenuState>,
    user_profile_popup: Option<UserProfilePopupState>,
    emoji_reaction_picker: Option<EmojiReactionPickerState>,
    poll_vote_picker: Option<PollVotePickerState>,
    reaction_users_popup: Option<ReactionUsersPopupState>,
    debug_log_popup_open: bool,
    display_options: DisplayOptions,
    display_options_save_pending: bool,
    current_user: Option<String>,
    current_user_id: Option<Id<UserMarker>>,
    last_status: Option<String>,
    should_quit: bool,
    older_history_requests: HashMap<Id<ChannelMarker>, OlderHistoryRequestState>,
    forum_post_lists: HashMap<Id<ChannelMarker>, ForumPostListState>,
    /// Folder IDs the user has collapsed in the guild pane. Single-guild
    /// "folders" (id = None) are never collapsible since they have no header.
    collapsed_folders: HashSet<FolderKey>,
    collapsed_channel_categories: HashSet<Id<ChannelMarker>>,
    pending_commands: VecDeque<AppCommand>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum FolderKey {
    Id(u64),
    Guilds(Vec<Id<GuildMarker>>),
}

fn message_notification_body(
    content: Option<&str>,
    sticker_count: usize,
    attachment_count: usize,
    embed_count: usize,
) -> String {
    let content = content.unwrap_or_default().trim();
    if !content.is_empty() {
        let single_line = content.split_whitespace().collect::<Vec<_>>().join(" ");
        return truncate_notification_text(&single_line, 200);
    }
    if attachment_count > 0 {
        return format!("sent {attachment_count} attachment(s)");
    }
    if sticker_count > 0 {
        return format!("sent {sticker_count} sticker(s)");
    }
    if embed_count > 0 {
        return format!("sent {embed_count} embed(s)");
    }
    "sent a message".to_owned()
}

fn truncate_notification_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
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
            guild_horizontal_scroll: 0,
            guild_keep_selection_visible: true,
            guild_view_height: 1,
            selected_channel: 0,
            channel_scroll: 0,
            channel_horizontal_scroll: 0,
            channel_keep_selection_visible: true,
            channel_view_height: 1,
            selected_message: 0,
            message_scroll: 0,
            message_line_scroll: 0,
            message_keep_selection_visible: true,
            message_auto_follow: true,
            new_messages_marker_message_id: None,
            unread_divider_last_acked_id: None,
            pending_unread_anchor_scroll: false,
            message_view_height: 1,
            message_content_width: usize::MAX,
            message_preview_width: 0,
            message_max_preview_height: 0,
            pinned_message_view_channel_id: None,
            pinned_message_view_return_target: None,
            thread_return_target: None,
            selected_member: 0,
            member_scroll: 0,
            member_horizontal_scroll: 0,
            member_keep_selection_visible: true,
            member_view_height: 1,
            composer_input: String::new(),
            composer_cursor_byte_index: 0,
            pending_composer_attachments: Vec::new(),
            composer_active: false,
            reply_target_message_id: None,
            edit_target_message: None,
            composer_mention_query: None,
            composer_mention_start: None,
            composer_mention_selected: 0,
            composer_mention_completions: Vec::new(),
            message_action_menu: None,
            options_popup: None,
            image_viewer: None,
            guild_action_menu: None,
            channel_action_menu: None,
            member_action_menu: None,
            user_profile_popup: None,
            emoji_reaction_picker: None,
            poll_vote_picker: None,
            reaction_users_popup: None,
            debug_log_popup_open: false,
            display_options: DisplayOptions::default(),
            display_options_save_pending: false,
            current_user: None,
            current_user_id: None,
            last_status: None,
            should_quit: false,
            older_history_requests: HashMap::new(),
            forum_post_lists: HashMap::new(),
            collapsed_folders: HashSet::new(),
            collapsed_channel_categories: HashSet::new(),
            pending_commands: VecDeque::new(),
        }
    }

    pub fn drain_pending_commands(&mut self) -> Vec<AppCommand> {
        self.pending_commands.drain(..).collect()
    }

    #[cfg(test)]
    pub fn push_event(&mut self, event: AppEvent) {
        self.push_event_inner(event, true);
    }

    pub fn push_effect(&mut self, event: AppEvent) {
        self.push_event_inner(event, false);
    }

    fn push_event_inner(&mut self, event: AppEvent, apply_discord: bool) {
        // Two layered behaviours run on every event:
        //
        // * Auto-scroll: when the user is already viewing the latest message
        //   (the bottom of the last message is visible in the viewport, even
        //   if the cursor is parked on an older one), keep the viewport
        //   tracking the latest after the event applies. The cursor is
        //   preserved by message id.
        // * Auto-follow: a superset of auto-scroll that also moves the
        //   cursor to the new latest message. Triggers only when the user
        //   was already following the latest message (cursor on last AND
        //   viewport at bottom). Self-sent messages no longer force-follow;
        //   if the user is reading older history, sending a message keeps
        //   the viewport parked.
        //
        // Both share the `message_auto_follow` flag — that flag really means
        // "next render should align the viewport to the bottom" and applies
        // to both modes. Auto-follow simply adds the cursor jump.
        let was_auto_follow = self.message_auto_follow;
        let was_at_latest = was_auto_follow || self.is_viewport_at_latest_message();
        let was_cursor_on_last = self.cursor_on_last_message();
        let was_following_cursor = was_at_latest && was_cursor_on_last;
        let user_just_sent = self.event_is_self_message_in_active_channel(&event);
        let active_new_message = self.active_channel_message_create(&event);
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
                    .get(self.message_scroll)
                    .map(|message| message.id)
            })
            .flatten();
        let mut channel_cursor_id = self.selected_channel_cursor_id();

        match &event {
            AppEvent::Ready { user, user_id } => {
                self.current_user = Some(user.clone());
                self.current_user_id = *user_id;
            }
            AppEvent::StatusMessage { message } => {
                self.last_status = Some(message.clone());
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
                    view_height: 0,
                });
            }
            AppEvent::MessageHistoryLoadFailed { channel_id, .. } => {
                self.older_history_requests.remove(channel_id);
            }
            AppEvent::ForumPostsLoaded {
                channel_id,
                archive_state,
                offset,
                next_offset: _,
                posts,
                has_more,
                ..
            } => {
                self.record_forum_posts_loaded(
                    *channel_id,
                    *archive_state,
                    *offset,
                    posts,
                    *has_more,
                );
            }
            AppEvent::MessageHistoryLoaded {
                channel_id,
                before,
                messages,
            } => self.record_older_history_loaded(*channel_id, *before, messages),
            AppEvent::UserProfileLoadFailed {
                user_id,
                guild_id,
                message,
            } => {
                if let Some(popup) = self.user_profile_popup.as_mut()
                    && popup.user_id == *user_id
                    && popup.guild_id == *guild_id
                {
                    popup.load_error = Some(message.clone());
                }
            }
            AppEvent::ActivateChannel { channel_id } => {
                let channel_id = *channel_id;
                let scope =
                    self.discord
                        .channel(channel_id)
                        .map(|channel| match channel.guild_id {
                            Some(guild_id) => ActiveGuildScope::Guild(guild_id),
                            None => ActiveGuildScope::DirectMessages,
                        });
                if let Some(scope) = scope {
                    self.activate_guild(scope);
                    self.activate_channel(channel_id);
                    self.channel_keep_selection_visible = true;
                    channel_cursor_id = Some(channel_id);
                }
            }
            _ => {}
        }
        if apply_discord {
            let discord_event = self.discord_event_for_apply(&event);
            self.discord.apply_event(&discord_event);
        }
        self.clamp_active_selection();
        self.restore_channel_cursor(channel_cursor_id);
        self.clamp_selection_indices();
        self.clear_missing_new_messages_marker();
        let in_message_view =
            !self.selected_channel_is_forum() && !self.is_pinned_message_view_active();
        let should_follow = was_following_cursor && in_message_view;
        let should_scroll = should_follow || (was_at_latest && in_message_view);
        if should_follow {
            self.follow_latest_message();
        } else {
            self.restore_message_position(selected_message_id, scroll_message_id);
        }
        if should_scroll {
            // Keep the bottom-align intent across to the next render so
            // `clamp_message_viewport_for_image_previews` snaps to the new
            // last message even when only the viewport (not the cursor)
            // moves.
            self.message_auto_follow = true;
            self.clear_new_messages_marker();
            if let Some((channel_id, _)) = active_new_message {
                if user_just_sent {
                    self.unread_divider_last_acked_id = None;
                    self.pending_unread_anchor_scroll = false;
                } else {
                    self.mark_channel_as_read(channel_id);
                }
            }
        } else if in_message_view
            && !was_at_latest
            && !user_just_sent
            && self.new_messages_marker_message_id.is_none()
        {
            self.new_messages_marker_message_id =
                active_new_message.map(|(_, message_id)| message_id);
        }
        self.clamp_list_viewports();
        self.clamp_message_viewport();
        if !should_scroll {
            self.refresh_message_auto_follow();
        }
    }

    fn discord_event_for_apply(&self, event: &AppEvent) -> AppEvent {
        let AppEvent::ForumPostsLoaded {
            channel_id,
            archive_state: ForumPostArchiveState::Archived,
            offset,
            next_offset,
            posts,
            preview_messages,
            has_more,
        } = event
        else {
            return event.clone();
        };

        let Some(list) = self.forum_post_lists.get(channel_id) else {
            return event.clone();
        };
        AppEvent::ForumPostsLoaded {
            channel_id: *channel_id,
            archive_state: ForumPostArchiveState::Archived,
            offset: *offset,
            next_offset: *next_offset,
            posts: posts
                .iter()
                .filter(|post| !list.active_post_ids.contains(&post.channel_id))
                .cloned()
                .collect(),
            preview_messages: preview_messages
                .iter()
                .filter(|message| !list.active_post_ids.contains(&message.channel_id))
                .cloned()
                .collect(),
            has_more: *has_more,
        }
    }

    pub fn restore_discord_snapshot(&mut self, discord: DiscordState) {
        let was_auto_follow = self.message_auto_follow;
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
                    .get(self.message_scroll)
                    .map(|message| message.id)
            })
            .flatten();
        let channel_cursor_id = self.selected_channel_cursor_id();

        self.discord = discord;
        if let Some(user) = self.discord.current_user() {
            self.current_user = Some(user.to_owned());
        }
        if let Some(user_id) = self.discord.current_user_id() {
            self.current_user_id = Some(user_id);
        }
        self.clamp_active_selection();
        self.restore_channel_cursor(channel_cursor_id);
        self.clamp_selection_indices();
        let in_message_view =
            !self.selected_channel_is_forum() && !self.is_pinned_message_view_active();
        let should_follow = was_following_cursor && in_message_view;
        let should_scroll = should_follow || (was_at_latest && in_message_view);
        if should_follow {
            self.follow_latest_message();
        } else {
            self.restore_message_position(selected_message_id, scroll_message_id);
        }
        if should_scroll {
            self.message_auto_follow = true;
        }
        self.clamp_list_viewports();
        self.clamp_message_viewport();
        if !should_scroll {
            self.refresh_message_auto_follow();
        }
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

    pub fn channel_unread(&self, channel_id: Id<ChannelMarker>) -> ChannelUnreadState {
        self.discord.channel_unread(channel_id)
    }

    pub fn guild_unread(&self, guild_id: Id<GuildMarker>) -> ChannelUnreadState {
        self.discord.guild_unread(guild_id)
    }

    pub fn direct_message_unread_count(&self) -> usize {
        self.discord.direct_message_unread_count()
    }

    pub fn channel_unread_message_count(&self, channel_id: Id<ChannelMarker>) -> usize {
        self.discord.channel_unread_message_count(channel_id)
    }

    pub(crate) fn desktop_notification_for_event(
        &self,
        event: &AppEvent,
    ) -> Option<DesktopNotification> {
        let AppEvent::MessageCreate {
            guild_id,
            channel_id,
            author,
            content,
            sticker_names,
            attachments,
            embeds,
            ..
        } = event
        else {
            return None;
        };
        if !self.desktop_notifications_enabled() || self.active_channel_id == Some(*channel_id) {
            return None;
        }
        if !self.discord.message_event_triggers_notification(event) {
            return None;
        }

        let channel = self.discord.channel(*channel_id);
        let guild_id = guild_id.or_else(|| channel.and_then(|channel| channel.guild_id));
        let title = match guild_id.and_then(|guild_id| self.discord.guild(guild_id)) {
            Some(guild) => {
                let channel_name = channel
                    .map(|channel| channel.name.as_str())
                    .unwrap_or("unknown-channel");
                format!("{author} in {} #{channel_name}", guild.name)
            }
            None => author.clone(),
        };
        let body = message_notification_body(
            content.as_deref(),
            sticker_names.len(),
            attachments.len(),
            embeds.len(),
        );
        Some(DesktopNotification { title, body })
    }

    pub fn current_user(&self) -> Option<&str> {
        self.current_user.as_deref()
    }

    pub fn is_channel_action_menu_open(&self) -> bool {
        self.channel_action_menu.is_some()
    }

    pub fn is_guild_action_menu_open(&self) -> bool {
        self.guild_action_menu.is_some()
    }

    pub fn open_actions_for_focused_target(&mut self) {
        match self.focus {
            FocusPane::Guilds => self.open_selected_guild_actions(),
            FocusPane::Channels => self.open_selected_channel_actions(),
            FocusPane::Messages => self.open_active_channel_actions(),
            FocusPane::Members => self.open_selected_member_actions(),
        }
    }

    pub fn is_channel_action_threads_phase(&self) -> bool {
        matches!(
            self.channel_action_menu,
            Some(ChannelActionMenuState::Threads { .. })
        )
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
            .filter(|channel| channel.is_thread() && self.discord.can_view_channel(channel));
        let thread = referenced_thread.or_else(|| {
            let thread_name = message.content.as_deref()?.trim();
            if thread_name.is_empty() {
                return None;
            }
            self.discord
                .viewable_channels_for_guild(message.guild_id)
                .into_iter()
                .find(|channel| {
                    channel.is_thread()
                        && channel.parent_id == Some(message.channel_id)
                        && channel.name == thread_name
                })
        });
        thread.map(|channel| {
            let latest_cached_message = self
                .discord
                .messages_for_channel(channel.id)
                .into_iter()
                .max_by_key(|message| message.id);
            let latest_message_id = channel
                .last_message_id
                .or_else(|| latest_cached_message.map(|message| message.id));
            let latest_message_preview = latest_cached_message
                .filter(|message| Some(message.id) == latest_message_id)
                .map(|message| ThreadMessagePreview {
                    author: message.author.clone(),
                    content: self.thread_message_preview_text(message),
                });
            ThreadSummary {
                channel_id: channel.id,
                name: channel.name.clone(),
                message_count: channel.message_count,
                total_message_sent: channel.total_message_sent,
                archived: channel.thread_archived,
                locked: channel.thread_locked,
                latest_message_id,
                latest_message_preview,
            }
        })
    }

    fn thread_message_preview_text(&self, message: &MessageState) -> String {
        if let Some(content) =
            message_preview_text(message.content.as_deref(), &message.sticker_names)
        {
            return self
                .render_user_mentions(message.guild_id, &message.mentions, &content)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
        }

        if !message.attachments.is_empty() {
            return "[attachment]".to_owned();
        }

        if message.content.is_some() {
            "<empty message>".to_owned()
        } else {
            "<message content unavailable>".to_owned()
        }
    }

    pub(crate) fn render_user_mentions(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        mentions: &[MentionInfo],
        value: &str,
    ) -> String {
        let value = if self.show_custom_emoji() {
            replace_custom_emoji_markup(value)
        } else {
            super::format::replace_custom_emoji_markup_with_ids(value)
        };
        render_user_mentions(
            &value,
            |user_id| self.resolve_mention_display_name(guild_id, mentions, user_id),
            |role_id| self.resolve_role_mention_name(guild_id, role_id),
            |channel_id| self.resolve_channel_mention_name(channel_id),
        )
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
            |role_id| self.resolve_role_mention_name(guild_id, role_id),
            |channel_id| self.resolve_channel_mention_name(channel_id),
            |target| match target {
                MentionTarget::User(user_id) => {
                    if current_user_id == Some(user_id) {
                        Some(TextHighlightKind::SelfMention)
                    } else {
                        Some(TextHighlightKind::OtherMention)
                    }
                }
                // Discord notifies role members on a role mention, but
                // computing the membership check here would require the
                // current user's role list. For the highlight pass we treat
                // every role mention as informational; the message-level
                // mention notification still drives self-targeted styling
                // through the literal `@everyone`/`@here` pass below when
                // those are used.
                MentionTarget::Role(_) => Some(TextHighlightKind::OtherMention),
                // Channel mentions never notify, but we highlight them so
                // the rendered `#channel-name` stays visually distinct from
                // surrounding text — same treatment as role mentions.
                MentionTarget::Channel(_) => Some(TextHighlightKind::OtherMention),
            },
        );
        if current_user_id.is_some() {
            add_literal_mention_highlights(&mut rendered, "@everyone");
            add_literal_mention_highlights(&mut rendered, "@here");
        }
        normalize_text_highlights(&mut rendered.highlights);
        super::format::replace_custom_emoji_markup_in_rendered_with_images(
            rendered,
            self.show_custom_emoji(),
        )
    }

    fn resolve_role_mention_name(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        role_id: u64,
    ) -> Option<String> {
        let guild_id = guild_id?;
        self.discord
            .roles_for_guild(guild_id)
            .into_iter()
            .find(|role| role.id.get() == role_id)
            .map(|role| role.name.clone())
    }

    fn resolve_channel_mention_name(&self, channel_id: u64) -> Option<String> {
        // `parse_mention` already rejects zero ids, so the `Id::new` call
        // never sees the forbidden value.
        let id = Id::<ChannelMarker>::new(channel_id);
        self.discord.channel(id).map(|channel| channel.name.clone())
    }

    fn resolve_mention_display_name(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        mentions: &[MentionInfo],
        user_id: u64,
    ) -> Option<String> {
        let mention = mentions
            .iter()
            .find(|mention| mention.user_id.get() == user_id);
        if let Some(guild_nick) = mention.and_then(|mention| mention.guild_nick.as_deref()) {
            return Some(guild_nick.to_owned());
        }
        if let Some(display_name) = guild_id.and_then(|guild_id| {
            let user_id = Id::<UserMarker>::new(user_id);
            self.discord.member_display_name(guild_id, user_id)
        }) {
            return Some(display_name.to_owned());
        }
        mention.map(|mention| mention.display_name.clone())
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

    fn record_forum_posts_loaded(
        &mut self,
        channel_id: Id<ChannelMarker>,
        archive_state: ForumPostArchiveState,
        offset: usize,
        posts: &[crate::discord::ChannelInfo],
        has_more: bool,
    ) {
        let list = self.forum_post_lists.entry(channel_id).or_default();
        if archive_state == ForumPostArchiveState::Active && offset == 0 {
            list.active_post_ids.clear();
            if self.active_channel_id == Some(channel_id) {
                self.selected_message = 0;
                self.message_scroll = 0;
                self.message_line_scroll = 0;
                self.message_auto_follow = false;
            }
        } else if archive_state == ForumPostArchiveState::Archived && offset == 0 {
            list.archived_post_ids.clear();
        }
        for post in posts {
            match archive_state {
                ForumPostArchiveState::Active => {
                    list.archived_post_ids.retain(|id| *id != post.channel_id);
                    if !list.active_post_ids.contains(&post.channel_id) {
                        list.active_post_ids.push(post.channel_id);
                    }
                }
                ForumPostArchiveState::Archived => {
                    if !list.active_post_ids.contains(&post.channel_id)
                        && !list.archived_post_ids.contains(&post.channel_id)
                    {
                        list.archived_post_ids.push(post.channel_id);
                    }
                }
            }
        }
        list.has_more = match archive_state {
            // Once active search is exhausted, the archived search stream may
            // still have old forum posts. Keep the UI asking for more until an
            // archived page says it is exhausted.
            ForumPostArchiveState::Active => true,
            ForumPostArchiveState::Archived => has_more,
        };
    }

    pub fn messages(&self) -> Vec<&MessageState> {
        if self.selected_channel_is_forum() {
            return Vec::new();
        }
        if self.pinned_message_view_channel_id == self.selected_channel_id() {
            return self.pinned_messages();
        }
        self.channel_messages()
    }

    pub fn pinned_messages(&self) -> Vec<&MessageState> {
        if self.selected_channel_is_forum() {
            return Vec::new();
        }
        self.selected_channel_id()
            .map(|channel_id| self.discord.pinned_messages_for_channel(channel_id))
            .unwrap_or_default()
    }

    fn channel_messages(&self) -> Vec<&MessageState> {
        self.selected_channel_id()
            .map(|channel_id| self.discord.messages_for_channel(channel_id))
            .unwrap_or_default()
    }

    pub fn enter_pinned_message_view(&mut self, channel_id: Id<ChannelMarker>) {
        if !self.is_pinned_message_view_active() {
            self.record_pinned_message_view_return_target(channel_id);
        }
        self.pinned_message_view_channel_id = Some(channel_id);
        self.selected_message = 0;
        self.message_scroll = 0;
        self.message_line_scroll = 0;
        self.message_auto_follow = false;
        self.clear_new_messages_marker();
        self.message_keep_selection_visible = true;
        self.clamp_message_viewport();
    }

    fn record_pinned_message_view_return_target(&mut self, channel_id: Id<ChannelMarker>) {
        if self.selected_channel_id() != Some(channel_id) {
            return;
        }
        self.pinned_message_view_return_target = Some(PinnedMessageViewReturnTarget {
            channel_id,
            selected_message: self.selected_message,
            message_scroll: self.message_scroll,
            message_line_scroll: self.message_line_scroll,
            message_keep_selection_visible: self.message_keep_selection_visible,
            message_auto_follow: self.message_auto_follow,
            new_messages_marker_message_id: self.new_messages_marker_message_id,
            unread_divider_last_acked_id: self.unread_divider_last_acked_id,
            pending_unread_anchor_scroll: self.pending_unread_anchor_scroll,
        });
    }

    pub fn return_from_pinned_message_view(&mut self) -> bool {
        if !self.is_pinned_message_view_active() {
            return false;
        }
        let Some(target) = self.pinned_message_view_return_target else {
            return false;
        };
        if self.selected_channel_id() != Some(target.channel_id) {
            self.pinned_message_view_return_target = None;
            return false;
        }

        self.pinned_message_view_channel_id = None;
        self.pinned_message_view_return_target = None;
        self.selected_message = target.selected_message;
        self.message_scroll = target.message_scroll;
        self.message_line_scroll = target.message_line_scroll;
        self.message_keep_selection_visible = target.message_keep_selection_visible;
        self.message_auto_follow = target.message_auto_follow;
        self.new_messages_marker_message_id = target.new_messages_marker_message_id;
        self.unread_divider_last_acked_id = target.unread_divider_last_acked_id;
        self.pending_unread_anchor_scroll = target.pending_unread_anchor_scroll;
        self.clamp_message_viewport();
        true
    }

    fn is_pinned_message_view_active(&self) -> bool {
        self.pinned_message_view_channel_id
            .is_some_and(|channel_id| Some(channel_id) == self.selected_channel_id())
    }

    #[cfg(test)]
    pub fn is_pinned_message_view(&self) -> bool {
        self.is_pinned_message_view_active()
    }

    pub fn selected_message(&self) -> usize {
        clamp_selected_index(self.selected_message, self.message_pane_item_count())
    }

    pub fn selected_message_state(&self) -> Option<&MessageState> {
        if self.selected_channel_is_forum() {
            return None;
        }
        self.messages().get(self.selected_message()).copied()
    }

    pub(crate) fn reply_target_message_state(&self) -> Option<&MessageState> {
        let message_id = self.reply_target_message_id?;
        self.messages()
            .into_iter()
            .find(|message| message.id == message_id)
    }

    pub fn next_older_history_command(&mut self) -> Option<AppCommand> {
        if self.is_pinned_message_view_active() {
            return None;
        }
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

    pub(crate) fn message_scroll(&self) -> usize {
        self.message_scroll
    }

    pub(crate) fn message_scroll_row_position(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        if self.selected_channel_is_forum() {
            return self
                .selected_forum_post_items()
                .into_iter()
                .take(self.message_scroll)
                .map(|post| post.rendered_height())
                .sum();
        }
        (0..self.message_scroll)
            .map(|index| {
                self.message_rendered_height_at(
                    index,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum::<usize>()
            .saturating_add(self.message_line_scroll)
    }

    pub(crate) fn message_total_rendered_rows(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        if self.selected_channel_is_forum() {
            return self
                .selected_forum_post_items()
                .into_iter()
                .map(|post| post.rendered_height())
                .sum();
        }
        (0..self.messages().len())
            .map(|index| {
                self.message_rendered_height_at(
                    index,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum()
    }

    /// Returns true when the message at `index` (within `self.messages()`)
    /// should be preceded by a date separator because its local date differs
    /// from the previous message's, or because it is the first loaded message
    /// and needs day context at the top of the pane.
    pub(crate) fn message_starts_new_day_at(&self, index: usize) -> bool {
        let messages = self.messages();
        let Some(current) = messages.get(index) else {
            return false;
        };
        let previous_id = index
            .checked_sub(1)
            .and_then(|prev_index| messages.get(prev_index).map(|message| message.id));
        super::ui::message_starts_new_day(current.id, previous_id)
    }

    pub(crate) fn new_messages_count(&self) -> usize {
        let Some(marker_id) = self.new_messages_marker_message_id else {
            return 0;
        };
        let messages = self.messages();
        messages
            .iter()
            .position(|message| message.id == marker_id)
            .map(|index| messages.len().saturating_sub(index))
            .unwrap_or(0)
    }

    /// Number of extra rows that the message at `index` reserves above its
    /// avatar/header line. These rows are painted by `message_viewport_lines`
    /// before the message body, so scroll and media-target math must use the
    /// same count as the renderer.
    pub(crate) fn message_extra_top_lines(&self, index: usize) -> usize {
        let mut extra = usize::from(self.message_starts_new_day_at(index));
        if self.should_draw_unread_divider_at(index) {
            extra += 1;
        }
        extra
    }

    /// Index of the first loaded message whose snowflake is newer than the
    /// captured `unread_divider_last_acked_id`. Snowflake IDs encode message
    /// ordering, so the comparison resolves the divider position even when
    /// the originally-acked message is no longer in the loaded slice (e.g.
    /// because history was trimmed). Returns `None` when no anchor is
    /// captured or every loaded message is at-or-before the anchor.
    pub(crate) fn unread_divider_message_index(&self) -> Option<usize> {
        if self.is_pinned_message_view_active() {
            return None;
        }
        let last_acked = self.unread_divider_last_acked_id?;
        let messages = self.messages();
        messages.iter().position(|message| message.id > last_acked)
    }

    pub(crate) fn should_draw_unread_divider_at(&self, index: usize) -> bool {
        self.unread_divider_message_index() == Some(index)
    }

    /// Returns the captured snapshot together with the number of currently
    /// loaded messages newer than it. The renderer uses this to draw the
    /// Discord-style "since {time} you have {count} unread messages"
    /// banner above the message pane. `None` when no anchor is captured
    /// or no loaded message is newer than the snapshot.
    pub(crate) fn unread_banner(&self) -> Option<UnreadBanner> {
        if self.is_pinned_message_view_active() {
            return None;
        }
        let last_acked = self.unread_divider_last_acked_id?;
        let messages = self.messages();
        let unread_count = messages.iter().filter(|m| m.id > last_acked).count();
        if unread_count == 0 {
            return None;
        }
        Some(UnreadBanner {
            since_message_id: last_acked,
            unread_count,
        })
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn unread_divider_last_acked_id(&self) -> Option<Id<MessageMarker>> {
        self.unread_divider_last_acked_id
    }

    #[cfg(test)]
    pub fn new_messages_marker_message_id(&self) -> Option<Id<MessageMarker>> {
        self.new_messages_marker_message_id
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

    pub fn missing_thread_preview_load_requests(
        &self,
    ) -> Vec<(Id<ChannelMarker>, Id<MessageMarker>)> {
        self.visible_messages()
            .into_iter()
            .filter_map(|message| {
                let summary = self.thread_summary_for_message(message)?;
                let latest_message_id = summary.latest_message_id?;
                summary
                    .latest_message_preview
                    .is_none()
                    .then_some((summary.channel_id, latest_message_id))
            })
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
        // Retry the unread-anchor snap until the originally-acked message
        // is loaded. After it fires once, the pending flag clears and this
        // is a cheap no-op.
        self.try_apply_unread_anchor_scroll();
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
        if self.selected_message() == 0 {
            self.message_scroll = 0;
            self.message_line_scroll = 0;
            return;
        }

        let height = self.message_content_height();
        if self.selected_message() == 1 && self.message_scroll == 0 && self.message_line_scroll == 0
        {
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
            if selected_bottom < height {
                return;
            }
        }

        if self.center_selected_message(content_width, preview_width, max_preview_height) {
            return;
        }

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
                let previous_height = self.message_rendered_height_at(
                    self.message_scroll.saturating_sub(1),
                    content_width,
                    preview_width,
                    max_preview_height,
                );
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
        if self.selected_channel_is_forum() {
            return self.focused_forum_post_selection();
        }
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

    pub fn selected_member(&self) -> usize {
        clamp_selected_index(self.selected_member, self.flattened_members().len())
    }

    pub fn focused_member_selection_line(&self) -> Option<usize> {
        if self.focus == FocusPane::Members && !self.flattened_members().is_empty() {
            let selected_line = self.selected_member_line();
            if selected_line >= self.member_scroll
                && selected_line < self.member_scroll + self.member_content_height()
            {
                Some(selected_line - self.member_scroll)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn member_scroll(&self) -> usize {
        self.member_scroll
    }

    pub fn guild_horizontal_scroll(&self) -> usize {
        self.guild_horizontal_scroll
    }

    pub fn channel_horizontal_scroll(&self) -> usize {
        self.channel_horizontal_scroll
    }

    pub fn member_horizontal_scroll(&self) -> usize {
        self.member_horizontal_scroll
    }

    pub fn member_content_height(&self) -> usize {
        pane_content_height(self.member_view_height)
    }

    pub fn member_line_count(&self) -> usize {
        self.count_member_lines()
    }

    pub fn set_member_view_height(&mut self, height: usize) {
        self.member_view_height = height;
        let selected_line = self.selected_member_line();
        let height = pane_content_height(self.member_view_height);
        let len = self.count_member_lines();
        clamp_list_viewport(
            selected_line,
            &mut self.member_scroll,
            height,
            len,
            self.member_keep_selection_visible,
        );
    }

    pub fn move_down(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                let len = self.guild_pane_entries().len();
                move_index_down(&mut self.selected_guild, len);
                self.guild_keep_selection_visible = true;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                let len = self.channel_pane_entries().len();
                move_index_down(&mut self.selected_channel, len);
                self.channel_keep_selection_visible = true;
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                let len = self.message_pane_item_count();
                move_index_down(&mut self.selected_message, len);
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusPane::Members => {
                let len = self.flattened_members().len();
                move_index_down(&mut self.selected_member, len);
                self.member_keep_selection_visible = true;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn move_up(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                move_index_up(&mut self.selected_guild);
                self.guild_keep_selection_visible = true;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                move_index_up(&mut self.selected_channel);
                self.channel_keep_selection_visible = true;
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                move_index_up(&mut self.selected_message);
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusPane::Members => {
                move_index_up(&mut self.selected_member);
                self.member_keep_selection_visible = true;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn jump_top(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = 0;
                self.guild_keep_selection_visible = true;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.selected_channel = 0;
                self.channel_keep_selection_visible = true;
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.selected_message = 0;
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusPane::Members => {
                self.selected_member = 0;
                self.member_keep_selection_visible = true;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn jump_bottom(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.selected_guild = last_index(self.guild_pane_entries().len());
                self.guild_keep_selection_visible = true;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                self.selected_channel = last_index(self.channel_pane_entries().len());
                self.channel_keep_selection_visible = true;
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                self.selected_message = last_index(self.message_pane_item_count());
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusPane::Members => {
                self.selected_member = last_index(self.flattened_members().len());
                self.member_keep_selection_visible = true;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn half_page_down(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                let distance = pane_content_height(self.guild_view_height) / 2;
                let len = self.guild_pane_entries().len();
                move_index_down_by(&mut self.selected_guild, len, distance.max(1));
                self.guild_keep_selection_visible = true;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                let distance = pane_content_height(self.channel_view_height) / 2;
                let len = self.channel_pane_entries().len();
                move_index_down_by(&mut self.selected_channel, len, distance.max(1));
                self.channel_keep_selection_visible = true;
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                let distance = self.message_content_height() / 2;
                let len = self.message_pane_item_count();
                move_index_down_by(&mut self.selected_message, len, distance.max(1));
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusPane::Members => {
                let distance = pane_content_height(self.member_view_height) / 2;
                self.select_member_near_line(
                    self.selected_member_line().saturating_add(distance.max(1)),
                );
                self.member_keep_selection_visible = true;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn half_page_up(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                let distance = pane_content_height(self.guild_view_height) / 2;
                move_index_up_by(&mut self.selected_guild, distance.max(1));
                self.guild_keep_selection_visible = true;
                self.clamp_guild_viewport();
            }
            FocusPane::Channels => {
                let distance = pane_content_height(self.channel_view_height) / 2;
                move_index_up_by(&mut self.selected_channel, distance.max(1));
                self.channel_keep_selection_visible = true;
                self.clamp_channel_viewport();
            }
            FocusPane::Messages => {
                let distance = self.message_content_height() / 2;
                self.selected_message = self.selected_message.saturating_sub(distance.max(1));
                self.message_keep_selection_visible = true;
                self.clamp_message_viewport();
                self.refresh_message_auto_follow();
            }
            FocusPane::Members => {
                let distance = pane_content_height(self.member_view_height) / 2;
                self.select_member_near_line(
                    self.selected_member_line().saturating_sub(distance.max(1)),
                );
                self.member_keep_selection_visible = true;
                self.clamp_member_viewport();
            }
        }
    }

    pub fn scroll_message_viewport_down(&mut self) {
        if self.focus != FocusPane::Messages || self.message_content_width == usize::MAX {
            return;
        }
        if self.selected_channel_is_forum() {
            let len = self.selected_forum_post_items().len();
            move_index_down(&mut self.message_scroll, len);
            self.message_auto_follow = false;
            self.message_keep_selection_visible = false;
            return;
        }
        // Viewport scrolling intentionally drops auto-follow so that the
        // user can over-scroll without the next render re-aligning to the
        // natural bottom. The event handler still re-engages follow when a
        // new message arrives and the viewport actually shows the latest,
        // via `is_viewport_at_latest_message()`.
        self.message_auto_follow = false;
        self.message_keep_selection_visible = false;
        self.scroll_message_viewport_down_one_row(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
        );
        if self.is_viewport_at_latest_message() {
            self.clear_new_messages_marker();
            self.normalize_message_line_scroll(
                self.message_content_width,
                self.message_preview_width,
                self.message_max_preview_height,
            );
        }
    }

    pub fn scroll_message_viewport_up(&mut self) {
        if self.focus != FocusPane::Messages || self.message_content_width == usize::MAX {
            return;
        }
        if self.selected_channel_is_forum() {
            move_index_up(&mut self.message_scroll);
            self.message_auto_follow = false;
            self.message_keep_selection_visible = false;
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
        self.clear_new_messages_marker();
        self.align_message_viewport_to_bottom(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
        );
        self.refresh_message_auto_follow();
    }

    pub fn scroll_focused_pane_viewport_down(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                let height = pane_content_height(self.guild_view_height);
                let len = self.guild_pane_entries().len();
                self.guild_keep_selection_visible = false;
                scroll_list_down(&mut self.guild_scroll, height, len);
            }
            FocusPane::Channels => {
                let height = pane_content_height(self.channel_view_height);
                let len = self.channel_pane_entries().len();
                self.channel_keep_selection_visible = false;
                scroll_list_down(&mut self.channel_scroll, height, len);
            }
            FocusPane::Messages => self.scroll_message_viewport_down(),
            FocusPane::Members => {
                let height = pane_content_height(self.member_view_height);
                let len = self.count_member_lines();
                self.member_keep_selection_visible = false;
                scroll_list_down(&mut self.member_scroll, height, len);
            }
        }
    }

    pub fn scroll_focused_pane_viewport_up(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.guild_keep_selection_visible = false;
                scroll_list_up(&mut self.guild_scroll);
            }
            FocusPane::Channels => {
                self.channel_keep_selection_visible = false;
                scroll_list_up(&mut self.channel_scroll);
            }
            FocusPane::Messages => self.scroll_message_viewport_up(),
            FocusPane::Members => {
                self.member_keep_selection_visible = false;
                scroll_list_up(&mut self.member_scroll);
            }
        }
    }

    pub fn scroll_focused_pane_horizontal_right(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.guild_horizontal_scroll = self
                    .guild_horizontal_scroll
                    .saturating_add(1)
                    .min(self.max_guild_horizontal_scroll());
            }
            FocusPane::Channels => {
                self.channel_horizontal_scroll = self
                    .channel_horizontal_scroll
                    .saturating_add(1)
                    .min(self.max_channel_horizontal_scroll());
            }
            FocusPane::Members => {
                self.member_horizontal_scroll = self
                    .member_horizontal_scroll
                    .saturating_add(1)
                    .min(self.max_member_horizontal_scroll());
            }
            FocusPane::Messages => {}
        }
    }

    fn max_guild_horizontal_scroll(&self) -> usize {
        self.guild_pane_entries()
            .into_iter()
            .map(|entry| entry.label().width().saturating_sub(1))
            .max()
            .unwrap_or_default()
    }

    fn max_channel_horizontal_scroll(&self) -> usize {
        self.channel_pane_entries()
            .into_iter()
            .map(|entry| match entry {
                ChannelPaneEntry::CategoryHeader { state, .. }
                | ChannelPaneEntry::Channel { state, .. } => state.name.width().saturating_sub(1),
            })
            .max()
            .unwrap_or_default()
    }

    fn max_member_horizontal_scroll(&self) -> usize {
        self.flattened_members()
            .into_iter()
            .map(|member| member.display_name().width().saturating_sub(1))
            .max()
            .unwrap_or_default()
    }

    pub fn scroll_focused_pane_horizontal_left(&mut self) {
        match self.focus {
            FocusPane::Guilds => {
                self.guild_horizontal_scroll = self.guild_horizontal_scroll.saturating_sub(1)
            }
            FocusPane::Channels => {
                self.channel_horizontal_scroll = self.channel_horizontal_scroll.saturating_sub(1)
            }
            FocusPane::Members => {
                self.member_horizontal_scroll = self.member_horizontal_scroll.saturating_sub(1)
            }
            FocusPane::Messages => {}
        }
    }

    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Guilds => FocusPane::Channels,
            FocusPane::Channels => FocusPane::Messages,
            FocusPane::Messages => FocusPane::Members,
            FocusPane::Members => FocusPane::Guilds,
        };
    }

    pub fn cycle_focus_backward(&mut self) {
        self.focus = match self.focus {
            FocusPane::Guilds => FocusPane::Members,
            FocusPane::Channels => FocusPane::Guilds,
            FocusPane::Messages => FocusPane::Channels,
            FocusPane::Members => FocusPane::Messages,
        };
    }

    pub fn focus_pane(&mut self, pane: FocusPane) {
        self.focus = pane;
    }

    pub fn select_visible_pane_row(&mut self, pane: FocusPane, row: usize) -> bool {
        match pane {
            FocusPane::Guilds => self.select_visible_guild_row(row),
            FocusPane::Channels => self.select_visible_channel_row(row),
            FocusPane::Messages => self.select_visible_message_row(row),
            FocusPane::Members => self.select_visible_member_line(row),
        }
    }

    fn select_visible_guild_row(&mut self, row: usize) -> bool {
        let index = self.guild_scroll.saturating_add(row);
        if index >= self.guild_pane_entries().len() {
            return false;
        }
        self.selected_guild = index;
        self.guild_keep_selection_visible = true;
        true
    }

    fn select_visible_channel_row(&mut self, row: usize) -> bool {
        let index = self.channel_scroll.saturating_add(row);
        if index >= self.channel_pane_entries().len() {
            return false;
        }
        self.selected_channel = index;
        self.channel_keep_selection_visible = true;
        true
    }

    fn select_visible_message_row(&mut self, row: usize) -> bool {
        if self.selected_channel_is_forum() {
            return self.select_visible_forum_post_row(row);
        }
        if self.message_content_width == usize::MAX {
            return false;
        }

        let mut rendered_row = 0usize;
        for local_index in 0..self.visible_messages().len() {
            let index = self.message_scroll.saturating_add(local_index);
            let rendered_height = self
                .message_rendered_height_at(
                    index,
                    self.message_content_width,
                    self.message_preview_width,
                    self.message_max_preview_height,
                )
                .max(1);
            let visible_height = if local_index == 0 {
                rendered_height.saturating_sub(self.message_line_scroll)
            } else {
                rendered_height
            };
            if row < rendered_row.saturating_add(visible_height) {
                self.selected_message = index;
                self.message_auto_follow = false;
                self.message_keep_selection_visible = false;
                return true;
            }
            rendered_row = rendered_row.saturating_add(visible_height);
        }
        false
    }

    fn select_visible_forum_post_row(&mut self, row: usize) -> bool {
        let mut rendered_row = 0usize;
        for (visible_index, post) in self.visible_forum_post_items().into_iter().enumerate() {
            if post.section_label.is_some() {
                if row == rendered_row {
                    return false;
                }
                rendered_row = rendered_row.saturating_add(1);
            }
            if row < rendered_row.saturating_add(FORUM_POST_CARD_HEIGHT) {
                let index = self.message_scroll.saturating_add(visible_index);
                if index >= self.selected_forum_post_items().len() {
                    return false;
                }
                self.selected_message = index;
                self.message_auto_follow = false;
                self.message_keep_selection_visible = false;
                return true;
            }
            rendered_row = rendered_row.saturating_add(FORUM_POST_CARD_HEIGHT);
        }
        false
    }

    fn select_visible_member_line(&mut self, row: usize) -> bool {
        let target_line = self.member_scroll.saturating_add(row);
        for (member_index, line_index) in self.member_line_indices() {
            if line_index == target_line {
                self.selected_member = member_index;
                self.member_keep_selection_visible = true;
                return true;
            }
        }
        false
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
                    channel.guild_id == Some(guild_id)
                        && !channel.is_category()
                        && self.discord.can_view_channel(channel)
                }
            });
        if self.active_channel_id.is_some() && !active_channel_is_valid {
            self.clear_active_channel();
        }
    }

    fn clear_active_channel(&mut self) {
        self.active_channel_id = None;
        self.selected_message = 0;
        self.message_scroll = 0;
        self.message_line_scroll = 0;
        self.message_keep_selection_visible = true;
        self.message_auto_follow = true;
        self.clear_new_messages_marker();
        self.channel_keep_selection_visible = true;
        self.member_keep_selection_visible = true;
        self.cancel_composer();
        self.close_message_action_menu();
        self.close_channel_action_menu();
        self.close_emoji_reaction_picker();
        self.close_poll_vote_picker();
        self.close_reaction_users_popup();
        self.thread_return_target = None;
    }

    fn clamp_list_viewports(&mut self) {
        self.clamp_guild_viewport();
        self.clamp_channel_viewport();
        self.clamp_member_viewport();
    }

    fn clamp_guild_viewport(&mut self) {
        let entries_len = self.guild_pane_entries().len();
        self.selected_guild = clamp_selected_index(self.selected_guild, entries_len);
        clamp_list_viewport(
            self.selected_guild,
            &mut self.guild_scroll,
            pane_content_height(self.guild_view_height),
            entries_len,
            self.guild_keep_selection_visible,
        );
    }

    fn clamp_channel_viewport(&mut self) {
        let entries_len = self.channel_pane_entries().len();
        self.selected_channel = clamp_selected_index(self.selected_channel, entries_len);
        clamp_list_viewport(
            self.selected_channel,
            &mut self.channel_scroll,
            pane_content_height(self.channel_view_height),
            entries_len,
            self.channel_keep_selection_visible,
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
        let selected_line = self.selected_member_line();
        let height = pane_content_height(self.member_view_height);
        let len = self.count_member_lines();
        clamp_list_viewport(
            selected_line,
            &mut self.member_scroll,
            height,
            len,
            self.member_keep_selection_visible,
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
            for member in group.entries {
                if member_index == selected_member {
                    return line_index;
                }
                member_index += 1;
                line_index += 1;
                if self.member_has_activity_row(member) {
                    line_index += 1;
                }
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
            for member in group.entries {
                indices.push((member_index, line_index));
                member_index += 1;
                line_index += 1;
                if self.member_has_activity_row(member) {
                    line_index += 1;
                }
            }
        }
        indices
    }

    fn count_member_lines(&self) -> usize {
        let mut lines = 0usize;
        for group in self.members_grouped() {
            if lines > 0 {
                lines += 1;
            }
            lines += 1; // group header
            for member in group.entries {
                lines += 1; // member name row
                if self.member_has_activity_row(member) {
                    lines += 1; // activity sub-row
                }
            }
        }
        lines
    }

    /// Must mirror `tui::ui::panes::render_members` — line counting and
    /// selection drift apart silently if the two predicates diverge.
    fn member_has_activity_row(&self, member: MemberEntry<'_>) -> bool {
        if matches!(
            member.status(),
            PresenceStatus::Offline | PresenceStatus::Unknown
        ) {
            return false;
        }
        !self.discord.user_activities(member.user_id()).is_empty()
    }

    /// Returns true when the cursor sits on the last message in the active
    /// channel. This is the auto-follow trigger condition: when an event
    /// arrives, follow (cursor jump + scroll) only fires if the cursor was
    /// already on the latest message and the viewport was at the latest.
    fn cursor_on_last_message(&self) -> bool {
        if self.selected_channel_is_forum() || self.is_pinned_message_view_active() {
            return false;
        }
        let messages = self.messages();
        if messages.is_empty() {
            return true;
        }
        self.selected_message >= messages.len().saturating_sub(1)
    }

    /// Returns true when the rendered viewport currently shows the latest
    /// message — that is, the user can see the bottom of the last message,
    /// regardless of where the cursor is parked. This is the auto-scroll
    /// trigger condition. With no rendered width yet (unit-test setups),
    /// falls back to a simple item-count check against the configured view
    /// height.
    fn is_viewport_at_latest_message(&self) -> bool {
        if self.selected_channel_is_forum() || self.is_pinned_message_view_active() {
            return false;
        }
        let messages = self.messages();
        if messages.is_empty() {
            return true;
        }
        let viewport = self.message_content_height();
        if self.message_content_width == usize::MAX {
            return self.message_scroll.saturating_add(viewport) >= messages.len();
        }
        let total = self.message_total_rendered_rows(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
        );
        let pos = self.message_scroll_row_position(
            self.message_content_width,
            self.message_preview_width,
            self.message_max_preview_height,
        );
        total.saturating_sub(pos) <= viewport
    }

    /// Re-engages auto-follow only when both invariants hold: the cursor is
    /// on the last message and the viewport is currently showing it. Either
    /// condition alone is not enough — if the user has scrolled the viewport
    /// off the bottom while the cursor remains on the last message, the
    /// next render must not snap the viewport back. Moving the cursor away
    /// from the last message also disengages, so the bottom-snap inside
    /// `clamp_message_viewport_for_image_previews` won't fight
    /// cursor-visibility centering.
    fn refresh_message_auto_follow(&mut self) {
        self.message_auto_follow =
            self.cursor_on_last_message() && self.is_viewport_at_latest_message();
        if self.message_auto_follow {
            self.clear_new_messages_marker();
            // Once the user has caught up (cursor + viewport on the
            // latest), retire the unread divider/banner so the indicator
            // doesn't linger after every unread message has been read.
            self.unread_divider_last_acked_id = None;
            self.pending_unread_anchor_scroll = false;
        }
    }

    fn clear_new_messages_marker(&mut self) {
        self.new_messages_marker_message_id = None;
    }

    fn clear_missing_new_messages_marker(&mut self) {
        if let Some(marker_id) = self.new_messages_marker_message_id
            && !self
                .messages()
                .iter()
                .any(|message| message.id == marker_id)
        {
            self.clear_new_messages_marker();
        }
    }

    fn active_channel_message_create(
        &self,
        event: &AppEvent,
    ) -> Option<(Id<ChannelMarker>, Id<MessageMarker>)> {
        let AppEvent::MessageCreate {
            channel_id,
            message_id,
            ..
        } = event
        else {
            return None;
        };
        (Some(*channel_id) == self.active_channel_id).then_some((*channel_id, *message_id))
    }

    fn event_is_self_message_in_active_channel(&self, event: &AppEvent) -> bool {
        let AppEvent::MessageCreate {
            author_id,
            channel_id,
            ..
        } = event
        else {
            return false;
        };
        Some(*author_id) == self.current_user_id && Some(*channel_id) == self.active_channel_id
    }

    fn follow_latest_message(&mut self) {
        // Only updates the selection; scroll position is left for
        // `align_message_viewport_to_bottom` to recompute on the next render.
        // Touching scroll/line_scroll here would briefly collapse the viewport
        // to a single-message state, and a key press (e.g. `k`) landing in
        // that window flips auto_follow off before alignment runs again,
        // stranding the viewport with empty space below the last message.
        self.selected_message = self.message_pane_item_count().saturating_sub(1);
        self.message_keep_selection_visible = true;
    }

    /// Snap the viewport so the user's last-read message sits at the top of
    /// the message pane and the unread divider is visible just below it.
    /// No-op until the captured `last_acked` snowflake is resolvable from
    /// the loaded slice; the call is retried each frame so the snap fires
    /// once history streams in. Once applied, the pending flag clears so
    /// subsequent navigation is not pinned to the anchor.
    pub(crate) fn try_apply_unread_anchor_scroll(&mut self) {
        if !self.pending_unread_anchor_scroll {
            return;
        }
        let Some(divider_index) = self.unread_divider_message_index() else {
            return;
        };
        let item_count = self.message_pane_item_count();
        if item_count == 0 {
            return;
        }
        // Anchor: place the last-read message (one row above the divider)
        // at the top of the viewport. Park the cursor on the first unread
        // so j/k navigation begins where the user left off, and disable
        // selection-keep so the next frame's centering pass does not pull
        // the viewport away from the anchor.
        self.message_scroll = divider_index.saturating_sub(1);
        self.message_line_scroll = 0;
        self.selected_message = divider_index.min(item_count.saturating_sub(1));
        self.message_keep_selection_visible = false;
        self.message_auto_follow = false;
        self.pending_unread_anchor_scroll = false;
    }

    fn align_message_viewport_to_bottom(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        if self.selected_channel_is_forum() {
            self.clamp_forum_post_viewport();
            self.message_line_scroll = 0;
            return;
        }
        let height = self.message_content_height();
        let mut remaining = height;
        for index in (0..self.messages().len()).rev() {
            let message_height = self
                .message_rendered_height_at(index, content_width, preview_width, max_preview_height)
                .max(1);
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
        let item_count = self.message_pane_item_count();
        if item_count == 0 {
            self.selected_message = 0;
            self.message_scroll = 0;
            self.message_line_scroll = 0;
            return;
        }

        self.selected_message = self.selected_message.min(item_count - 1);
        self.message_scroll = self.message_scroll.min(item_count - 1);
        if self.selected_channel_is_forum() {
            self.clamp_forum_post_viewport();
            self.message_line_scroll = 0;
            return;
        }
        if self.message_content_width == usize::MAX {
            self.message_scroll = clamp_list_scroll(
                self.selected_message,
                self.message_scroll,
                self.message_content_height(),
                item_count,
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
        if self.messages().get(selected).is_none() {
            return false;
        }
        let selected_height = self
            .message_rendered_height_at(selected, content_width, preview_width, max_preview_height)
            .max(1);
        let mut top = selected;
        let mut offset = 0usize;
        let mut remaining = (height / 2).saturating_sub(selected_height / 2);

        while remaining > 0 && top > 0 {
            let previous_index = top.saturating_sub(1);
            if self.messages().get(previous_index).is_none() {
                break;
            }
            let previous_height = self
                .message_rendered_height_at(
                    previous_index,
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
        for offset_from_top in 0..self.messages().len().saturating_sub(top) {
            let global_index = top + offset_from_top;
            let message_height = self
                .message_rendered_height_at(
                    global_index,
                    self.message_content_width,
                    self.message_preview_width,
                    self.message_max_preview_height,
                )
                .max(1);
            let visible_height = if offset_from_top == 0 {
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
        let current_message_height = self.messages().get(self.message_scroll).map(|_| {
            self.message_rendered_height_at(
                self.message_scroll,
                content_width,
                preview_width,
                max_preview_height,
            )
        });
        scroll_message_row_down(
            &mut self.message_scroll,
            &mut self.message_line_scroll,
            messages_len,
            current_message_height,
        );
    }

    fn scroll_message_viewport_up_one_row(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        if self.message_line_scroll > 0 {
            scroll_message_row_up(
                &mut self.message_scroll,
                &mut self.message_line_scroll,
                None,
            );
            return;
        }
        let previous_message_index = self.message_scroll.checked_sub(1);
        let previous_message_height = previous_message_index.map(|index| {
            self.message_rendered_height_at(index, content_width, preview_width, max_preview_height)
        });
        scroll_message_row_up(
            &mut self.message_scroll,
            &mut self.message_line_scroll,
            previous_message_height,
        );
    }

    fn normalize_message_line_scroll(
        &mut self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) {
        let current_message_height = self.messages().get(self.message_scroll).map(|_| {
            self.message_rendered_height_at(
                self.message_scroll,
                content_width,
                preview_width,
                max_preview_height,
            )
        });
        normalize_message_line_scroll(&mut self.message_line_scroll, current_message_height);
    }

    fn message_content_height(&self) -> usize {
        pane_content_height(self.message_view_height)
    }

    fn clamp_forum_post_viewport(&mut self) {
        let posts = self.selected_forum_post_items();
        if posts.is_empty() {
            self.message_scroll = 0;
            return;
        }

        let selected = self.selected_message.min(posts.len() - 1);
        self.message_scroll = self.message_scroll.min(selected);
        let height = self.message_content_height().max(1);
        while self.message_scroll < selected {
            let rendered_rows: usize = posts[self.message_scroll..=selected]
                .iter()
                .map(|post| post.rendered_height())
                .sum();
            if rendered_rows <= height {
                break;
            }
            self.message_scroll = self.message_scroll.saturating_add(1);
        }
    }

    fn message_pane_item_count(&self) -> usize {
        if self.selected_channel_is_forum() {
            self.selected_forum_post_items().len()
        } else {
            self.messages().len()
        }
    }

    fn selected_message_rendered_row(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        let span = self.selected_message.saturating_sub(self.message_scroll);
        let row: usize = (0..span)
            .map(|offset| {
                self.message_rendered_height_at(
                    self.message_scroll + offset,
                    content_width,
                    preview_width,
                    max_preview_height,
                )
            })
            .sum();
        row.saturating_sub(self.message_line_scroll)
    }

    fn selected_message_rendered_height(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        if self.messages().get(self.selected_message).is_none() {
            return 1;
        }
        self.message_rendered_height_at(
            self.selected_message,
            content_width,
            preview_width,
            max_preview_height,
        )
    }

    fn following_message_rendered_rows(
        &self,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
        count: usize,
    ) -> usize {
        let messages_len = self.messages().len();
        let start = self.selected_message.saturating_add(1);
        (0..count)
            .map(|offset| start + offset)
            .take_while(|&index| index < messages_len)
            .map(|index| {
                self.message_rendered_height_at(
                    index,
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
        let (body_lines, reaction_lines) =
            message_format::format_message_content_sections(message, self, content_width);
        1 + body_lines.len() + reaction_lines.len()
    }

    pub(crate) fn message_body_line_count_for_width(
        &self,
        message: &MessageState,
        content_width: usize,
    ) -> usize {
        let (body_lines, _) =
            message_format::format_message_content_sections(message, self, content_width);
        1 + body_lines.len()
    }

    fn message_rendered_height(
        &self,
        message: &MessageState,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        let previews = message.inline_previews();
        let album = media::image_preview_album_layout(&previews, preview_width, max_preview_height);
        let preview_height = album
            .height
            .saturating_add(usize::from(album.overflow_count > 0));
        self.message_base_line_count_for_width(message, content_width)
            + preview_height
            + ui::MESSAGE_ROW_GAP
    }

    /// Same as `message_rendered_height` but also accounts for an optional
    /// date-separator line above the message body. Use this everywhere the
    /// caller knows the message's index inside `self.messages()` so scroll
    /// math stays consistent with what the renderer actually paints.
    fn message_rendered_height_at(
        &self,
        index: usize,
        content_width: usize,
        preview_width: u16,
        max_preview_height: u16,
    ) -> usize {
        let messages = self.messages();
        let Some(message) = messages.get(index).copied() else {
            return 0;
        };
        self.message_rendered_height(message, content_width, preview_width, max_preview_height)
            + self.message_extra_top_lines(index)
    }
}

fn message_preview_text(content: Option<&str>, sticker_names: &[String]) -> Option<String> {
    content
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            sticker_names
                .first()
                .map(|name| format!("[Sticker: {name}]"))
        })
}

#[cfg(test)]
fn message_rendered_height(
    message: &MessageState,
    content_width: usize,
    preview_width: u16,
    max_preview_height: u16,
) -> usize {
    DashboardState::new().message_rendered_height(
        message,
        content_width,
        preview_width,
        max_preview_height,
    )
}

impl Default for DashboardState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
