use std::{collections::HashSet, time::Instant};

use crate::discord::ids::marker::{GuildMarker, UserMarker};

use crate::discord::{
    AppCommand, AppEvent, DownloadAttachmentSource, ForumPostArchiveState, MentionInfo,
    MessageSnapshotInfo, VoiceConnectionStatus,
};

mod channel_switcher;
mod channels;
mod composer;
mod dashboard;
mod diagnostics;
mod discord_ui;
mod emoji;
mod guilds;
mod image_viewer;
mod layout_cache;
mod member_grouping;
mod message_actions;
mod message_layout;
mod message_render;
mod message_viewport;
mod model;
mod navigation;
mod options;
mod pane_filter;
mod polls;
mod popups;
mod presentation;
mod reactions;
mod request_tracking;
mod runtime_state;
mod scroll;
mod subscriptions;
mod toast;
mod user;
mod voice_actions;

use composer::ComposerUiState;
use discord_ui::DiscordUiState;
use layout_cache::{LayoutCacheState, MessageRowContentMetrics, MessageRowContentMetricsCacheKey};
use message_render::{add_literal_mention_highlights, normalize_text_highlights};
use message_viewport::{MessageViewportState, ThreadReturnTarget};
use navigation::{ActiveGuildScope, FolderKey, NavigationState};
use options::OptionsUiState;
use pane_filter::PaneFilterState;
use popups::PopupUiState;
use request_tracking::RequestTrackingState;
use runtime_state::{RuntimeUiState, ToastMessage, VoiceConnectionUiState};
use scroll::clamp_selected_index;

pub use composer::{
    CommandPickerEntry, EmojiPickerEntry, MAX_MENTION_PICKER_VISIBLE, MentionPickerEntry,
};
pub use dashboard::DashboardState;
pub use member_grouping::{MemberEntry, MemberGroup};
pub use model::{
    ChannelActionItem, ChannelPaneEntry, ChannelSwitcherItem, ChannelThreadItem, EmojiReactionItem,
    FORUM_POST_CARD_HEIGHT, FocusPane, GuildActionItem, GuildPaneEntry, ImageViewerItem,
    MemberActionItem, MessageActionItem, MessageActionKind, MuteActionDurationItem,
    PollVotePickerItem, ThreadMessagePreview, ThreadSummary,
};
pub use model::{
    ChannelActionKind, GuildActionKind, MemberActionKind, MessageActionMenuPhase, MessageUrlItem,
};
pub use options::DisplayOptionItem;
pub use popups::{
    EmojiReactionPickerState, MessageActionMenuState, PollVotePickerState, ReactionUsersPopupState,
};
pub use presentation::{discord_color, folder_color, presence_color, presence_marker};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToastKind {
    Success,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToastView<'a> {
    pub text: &'a str,
    pub kind: ToastKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopNotification {
    pub title: String,
    pub body: String,
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
    #[cfg(test)]
    pub fn push_event(&mut self, event: AppEvent) {
        self.push_event_inner(event, true);
    }

    pub fn push_effect(&mut self, event: AppEvent) {
        if let AppEvent::ChannelUpsert(channel) = &event {
            self.record_thread_channel_upserted(channel);
            return;
        }
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
        //   was already following the latest message. Self-sent messages no longer force-follow.
        //   If the user is reading older history, sending a message keeps the
        //   viewport parked.
        //
        // Both modes share `message_auto_follow`. It means the next render
        // should align the viewport to the bottom. Auto-follow also jumps
        // the cursor.
        let was_auto_follow = self.messages.message_auto_follow;
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
                    .get(self.messages.message_scroll)
                    .map(|message| message.id)
            })
            .flatten();
        let mut channel_cursor_id = self.selected_channel_cursor_id();

        match &event {
            AppEvent::Ready { user, user_id } => {
                self.discord.current_user = Some(user.clone());
                self.discord.current_user_id = *user_id;
            }
            AppEvent::CurrentUserCapabilities {
                can_use_animated_custom_emojis,
            } => {
                self.discord.current_user_can_use_animated_custom_emojis =
                    Some(*can_use_animated_custom_emojis);
            }
            AppEvent::ApplicationCommandsLoaded { guild_id, commands } => {
                self.discord
                    .application_commands
                    .insert(*guild_id, commands.clone());
                self.refresh_active_mention_query();
            }
            AppEvent::AttachmentDownloadCompleted { path, source }
                if *source == DownloadAttachmentSource::ImageViewer =>
            {
                self.record_image_viewer_download_completed(path);
            }
            AppEvent::UpdateAvailable { latest_version } => {
                self.discord.update_available_version = Some(latest_version.clone());
            }
            AppEvent::ReactionUsersLoaded {
                channel_id,
                message_id,
                reactions,
            } => {
                self.popups.reaction_users_popup = Some(ReactionUsersPopupState {
                    channel_id: *channel_id,
                    message_id: *message_id,
                    reactions: reactions.clone(),
                    scroll: 0,
                    view_height: 0,
                });
            }
            AppEvent::MessageHistoryLoadFailed { .. } => {}
            AppEvent::ForumPostsLoaded {
                channel_id,
                archive_state,
                offset,
                next_offset: _,
                threads,
                has_more,
                ..
            } => {
                self.record_forum_posts_loaded(
                    *channel_id,
                    *archive_state,
                    *offset,
                    threads,
                    *has_more,
                );
            }
            AppEvent::MessageHistoryLoaded { .. } => {}
            AppEvent::UserProfileLoadFailed {
                user_id,
                guild_id,
                message,
            } => {
                if let Some(popup) = self.popups.user_profile_popup.as_mut()
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
                        .cache
                        .channel(channel_id)
                        .map(|channel| match channel.guild_id {
                            Some(guild_id) => ActiveGuildScope::Guild(guild_id),
                            None => ActiveGuildScope::DirectMessages,
                        });
                if let Some(scope) = scope {
                    self.activate_guild(scope);
                    self.activate_channel(channel_id);
                    self.navigation.channel_keep_selection_visible = true;
                    channel_cursor_id = Some(channel_id);
                }
            }
            AppEvent::VoiceConnectionStatusChanged {
                guild_id,
                channel_id,
                status,
                message,
            } => match status {
                VoiceConnectionStatus::Connecting => {
                    self.runtime.voice_connection = Some(VoiceConnectionUiState {
                        guild_id: *guild_id,
                        channel_id: *channel_id,
                    });
                    self.show_success_toast(
                        message.as_deref().unwrap_or("Voice join requested"),
                        Instant::now(),
                    );
                }
                VoiceConnectionStatus::Connected => {
                    self.runtime.voice_connection = Some(VoiceConnectionUiState {
                        guild_id: *guild_id,
                        channel_id: *channel_id,
                    });
                    self.show_success_toast(
                        message.as_deref().unwrap_or("Voice connected"),
                        Instant::now(),
                    );
                }
                VoiceConnectionStatus::Disconnected => {
                    if self
                        .runtime
                        .voice_connection
                        .is_some_and(|voice| voice.guild_id == *guild_id)
                    {
                        self.runtime.voice_connection = None;
                    }
                    self.show_success_toast(
                        message.as_deref().unwrap_or("Voice leave requested"),
                        Instant::now(),
                    );
                }
                VoiceConnectionStatus::Failed => {
                    if self
                        .runtime
                        .voice_connection
                        .is_some_and(|voice| voice.guild_id == *guild_id)
                    {
                        self.runtime.voice_connection = None;
                    }
                    self.show_error_toast(
                        message.as_deref().unwrap_or("Voice request failed"),
                        Instant::now(),
                    );
                }
            },
            AppEvent::ChannelUpsert(channel) => {
                self.record_thread_channel_upserted(channel);
            }
            _ => {}
        }
        if apply_discord {
            let discord_event = self.discord_event_for_apply(&event);
            self.discord.cache.apply_event(&discord_event);
            if Self::event_affects_message_row_content_metrics(&discord_event) {
                self.clear_message_row_content_metrics_cache();
            }
        }
        if matches!(
            &event,
            AppEvent::CurrentUserCapabilities { .. } | AppEvent::GuildEmojisUpdate { .. }
        ) {
            self.refresh_composer_emoji_candidates_for_current_query();
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
            self.messages.message_auto_follow = true;
            self.clear_new_messages_marker();
            if let Some((channel_id, _)) = active_new_message {
                if user_just_sent {
                    self.messages.unread_divider_last_acked_id = None;
                    self.messages.pending_unread_anchor_scroll = false;
                } else {
                    self.schedule_channel_ack(channel_id);
                }
            }
        } else if in_message_view
            && !was_at_latest
            && !user_just_sent
            && self.messages.new_messages_marker_message_id.is_none()
        {
            self.messages.new_messages_marker_message_id =
                active_new_message.map(|(_, message_id)| message_id);
        }
        self.clamp_list_viewports();
        self.clamp_message_viewport();
        if !should_scroll {
            self.refresh_message_auto_follow();
        }
    }
}

#[cfg(test)]
mod tests;
