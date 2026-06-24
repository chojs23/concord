use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker},
};
use crate::discord::{
    AppCommand, EmbedInfo, MESSAGE_FLAG_SUPPRESS_EMBEDS, MediaPlaybackSource, MediaPlaybackTarget,
    MessageState, ReactionEmoji,
};
use crate::tui::format::detected_urls;
use crate::tui::keybindings::KeyChord;

use super::super::{
    ActiveGuildScope, DashboardState, FocusPane, MessageActionItem, MessageActionKind,
    MessageActionMenuState, MessageConfirmationKind, MessageUrlItem, MessageUrlPickerState, popups,
};
use crate::tui::state::popups::{ActiveModalPopupKind, LeaderActionState, ModalPopup};

const PLAYABLE_VIDEO_EXTENSIONS: &[&str] = &["m4v", "mov", "mp4", "webm"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ReferencedMessageTarget {
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
}

impl DashboardState {
    pub fn activate_selected_message_pane_item(&mut self) -> Option<AppCommand> {
        if self.message_pane_uses_thread_cards() {
            return self.activate_selected_thread_card();
        }
        self.open_selected_message_actions();
        None
    }

    pub fn open_selected_message_actions(&mut self) {
        if self.navigation.focus == FocusPane::Messages && self.selected_message_state().is_some() {
            self.popups.modal = Some(ModalPopup::MessageActionMenu(
                MessageActionMenuState::default(),
            ));
        }
    }

    pub fn close_message_action_menu(&mut self) {
        if matches!(
            self.popups.modal,
            Some(ModalPopup::MessageActionMenu(_))
                | Some(ModalPopup::Leader(super::LeaderPopupState {
                    action: Some(LeaderActionState::Message(_)),
                    ..
                }))
        ) {
            self.popups.clear_modal();
        }
    }

    pub fn move_message_action_down(&mut self) {
        let actions_len = self.selected_message_action_items().len();
        if let Some(menu) = self.popups.message_action_menu_mut() {
            menu.selection.move_down(actions_len);
        }
    }

    pub fn move_message_action_up(&mut self) {
        if let Some(menu) = self.popups.message_action_menu_mut() {
            menu.selection.move_up();
        }
    }

    pub fn select_message_action_row(&mut self, row: usize) -> bool {
        if row >= self.selected_message_action_items().len() {
            return false;
        }
        if let Some(menu) = self.popups.message_action_menu_mut() {
            menu.selection.select(row);
            return true;
        }
        false
    }

    pub fn selected_message_action_items(&self) -> Vec<MessageActionItem> {
        let Some(message) = self.selected_message_state() else {
            return Vec::new();
        };
        let poll_voting_enabled = message
            .poll
            .as_ref()
            .is_some_and(|poll| !poll.results_finalized.unwrap_or(false));
        vec![
            MessageActionItem {
                kind: MessageActionKind::CopyContent,
                label: "copy message".to_owned(),
                enabled: message.content.is_some(),
            },
            MessageActionItem {
                kind: MessageActionKind::OpenReactionPicker,
                label: "react".to_owned(),
                enabled: self.can_open_reaction_picker(message),
            },
            MessageActionItem {
                kind: MessageActionKind::Reply,
                label: "reply".to_owned(),
                enabled: self.can_send_in_selected_channel(),
            },
            MessageActionItem {
                kind: MessageActionKind::OpenDeleteConfirmation,
                label: "delete message".to_owned(),
                enabled: self.can_delete_message(message),
            },
            MessageActionItem {
                kind: MessageActionKind::Edit,
                label: "edit message".to_owned(),
                enabled: self.can_edit_message(message),
            },
            MessageActionItem {
                kind: MessageActionKind::OpenUrl,
                label: "open URL".to_owned(),
                enabled: !message_url_items(message).is_empty(),
            },
            MessageActionItem {
                kind: MessageActionKind::RemoveEmbeds,
                label: "remove embeds".to_owned(),
                enabled: self.can_remove_message_embeds(message),
            },
            MessageActionItem {
                kind: MessageActionKind::PlayMedia,
                label: "play media".to_owned(),
                enabled: !message_media_playback_items(message).is_empty(),
            },
            MessageActionItem {
                kind: MessageActionKind::ViewAttachment,
                label: "view attachment".to_owned(),
                enabled: message.attachments_in_display_order().next().is_some(),
            },
            MessageActionItem {
                kind: MessageActionKind::GoToReferencedMessage,
                label: "go to referenced message".to_owned(),
                enabled: self.referenced_message_target(message).is_some(),
            },
            MessageActionItem {
                kind: MessageActionKind::ShowProfile,
                label: "show message sender profile".to_owned(),
                enabled: true,
            },
            MessageActionItem {
                kind: MessageActionKind::OpenPinConfirmation,
                label: "pin message".to_owned(),
                enabled: self.can_pin_messages_for_message(message),
            },
            MessageActionItem {
                kind: MessageActionKind::OpenThread,
                label: "open thread".to_owned(),
                enabled: self.thread_summary_for_message(message).is_some(),
            },
            MessageActionItem {
                kind: MessageActionKind::ShowReactionUsers,
                label: "show reacted users".to_owned(),
                enabled: !message.reactions.is_empty()
                    && self.can_show_reaction_users_for_message(message),
            },
            MessageActionItem {
                kind: MessageActionKind::OpenPollVotePicker,
                label: "choose poll votes".to_owned(),
                enabled: poll_voting_enabled,
            },
        ]
    }

    pub fn selected_message_action_index(&self) -> Option<usize> {
        self.popups.message_action_menu().map(|menu| {
            menu.selection
                .selected_for_len(self.selected_message_action_items().len())
        })
    }

    pub fn selected_message_url_items(&self) -> Vec<MessageUrlItem> {
        if let Some(picker) = self.popups.message_url_picker() {
            return picker.items.clone();
        }
        self.selected_message_state()
            .map(message_url_items)
            .unwrap_or_default()
    }

    pub fn selected_message_url_index(&self) -> Option<usize> {
        self.popups
            .message_url_picker()
            .map(|picker| picker.selection.selected_for_len(picker.items.len()))
    }

    pub fn move_message_url_picker_down(&mut self) {
        if let Some(picker) = self.popups.message_url_picker_mut() {
            picker.selection.move_down(picker.items.len());
        }
    }

    pub fn move_message_url_picker_up(&mut self) {
        if let Some(picker) = self.popups.message_url_picker_mut() {
            picker.selection.move_up();
        }
    }

    pub fn select_message_url_row(&mut self, row: usize) -> bool {
        let Some(picker) = self.popups.message_url_picker_mut() else {
            return false;
        };
        if row >= picker.items.len() {
            return false;
        }
        picker.selection.select(row);
        true
    }

    pub fn selected_message_action(&self) -> Option<MessageActionItem> {
        let index = self.selected_message_action_index()?;
        self.selected_message_action_items().get(index).cloned()
    }

    pub fn activate_selected_message_action(&mut self) -> Option<AppCommand> {
        let action = self.selected_message_action()?;
        if !action.enabled {
            return None;
        }
        self.close_message_action_menu();
        self.run_message_action_kind(action.kind)
    }

    pub(super) fn can_add_reaction_to_message(
        &self,
        message: &MessageState,
        emoji: &ReactionEmoji,
    ) -> bool {
        let Some(channel) = self.discord.cache.channel(message.channel_id) else {
            return true;
        };
        if !self
            .discord
            .cache
            .can_read_message_history_in_channel(channel)
        {
            return false;
        }
        message
            .reactions
            .iter()
            .any(|reaction| &reaction.emoji == emoji)
            || self.discord.cache.can_add_reactions_in_channel(channel)
    }

    pub(super) fn can_open_reaction_picker(&self, message: &MessageState) -> bool {
        let Some(channel) = self.discord.cache.channel(message.channel_id) else {
            return true;
        };
        self.discord
            .cache
            .can_read_message_history_in_channel(channel)
            && (self.discord.cache.can_add_reactions_in_channel(channel)
                || !message.reactions.is_empty())
    }

    pub(super) fn can_add_new_reaction_for_message(&self, message: &MessageState) -> bool {
        let Some(channel) = self.discord.cache.channel(message.channel_id) else {
            return true;
        };
        self.discord.cache.can_add_reactions_in_channel(channel)
    }

    fn can_show_reaction_users_for_message(&self, message: &MessageState) -> bool {
        let Some(channel) = self.discord.cache.channel(message.channel_id) else {
            return true;
        };
        self.discord
            .cache
            .can_read_message_history_in_channel(channel)
    }

    fn can_delete_message(&self, message: &MessageState) -> bool {
        if Some(message.author_id) == self.discord.current_user_id {
            return true;
        }
        let Some(channel) = self.discord.cache.channel(message.channel_id) else {
            return true;
        };
        self.discord.cache.can_manage_messages_in_channel(channel)
    }

    fn can_edit_message(&self, message: &MessageState) -> bool {
        Some(message.author_id) == self.discord.current_user_id
            && message.message_kind.is_regular_or_reply()
            && message.content.is_some()
    }

    fn can_remove_message_embeds(&self, message: &MessageState) -> bool {
        if message.embeds.is_empty() || message.flags & MESSAGE_FLAG_SUPPRESS_EMBEDS != 0 {
            return false;
        }
        if Some(message.author_id) == self.discord.current_user_id {
            return true;
        }
        let Some(channel) = self.discord.cache.channel(message.channel_id) else {
            return true;
        };
        self.discord.cache.can_manage_messages_in_channel(channel)
    }

    fn can_pin_messages_for_message(&self, message: &MessageState) -> bool {
        let Some(channel) = self.discord.cache.channel(message.channel_id) else {
            return true;
        };
        self.discord.cache.can_pin_messages_in_channel(channel)
    }

    fn referenced_message_target(&self, message: &MessageState) -> Option<ReferencedMessageTarget> {
        let reference = message.reference.as_ref()?;
        let message_id = reference.message_id?;
        let channel_id = reference.channel_id.unwrap_or(message.channel_id);
        let channel = self.discord.cache.channel(channel_id)?;
        if !self.discord.cache.can_view_channel(channel)
            || !self
                .discord
                .cache
                .can_read_message_history_in_channel(channel)
        {
            return None;
        }
        Some(ReferencedMessageTarget {
            channel_id,
            message_id,
        })
    }

    pub fn activate_message_action_shortcut(&mut self, shortcut: KeyChord) -> Option<AppCommand> {
        let actions = self.selected_message_action_items();
        let index = self.options.key_bindings().matching_action_shortcut_index(
            &actions,
            shortcut,
            |key_bindings, actions, index| key_bindings.message_action_shortcuts(actions, index),
            |action| action.enabled,
        )?;
        self.select_message_action_row(index);
        self.activate_selected_message_action()
    }

    pub fn activate_message_action_kind(&mut self, kind: MessageActionKind) -> Option<AppCommand> {
        let action = self
            .selected_message_action_items()
            .into_iter()
            .find(|action| action.kind == kind)?;
        if !action.enabled {
            return None;
        }
        self.close_message_action_menu();
        self.run_message_action_kind(kind)
    }

    fn run_message_action_kind(&mut self, kind: MessageActionKind) -> Option<AppCommand> {
        match kind {
            MessageActionKind::CopyContent => {
                self.direct_copy_selected_message_content();
                None
            }
            MessageActionKind::OpenReactionPicker => {
                self.direct_open_selected_message_reaction_picker();
                None
            }
            MessageActionKind::Reply => {
                self.direct_reply_to_selected_message();
                None
            }
            MessageActionKind::OpenDeleteConfirmation => {
                self.open_selected_message_delete_confirmation();
                None
            }
            MessageActionKind::Edit => {
                self.direct_edit_selected_message();
                None
            }
            MessageActionKind::OpenUrl => self.direct_open_selected_message_url(),
            MessageActionKind::RemoveEmbeds => {
                self.direct_open_selected_message_remove_embeds_confirmation();
                None
            }
            MessageActionKind::PlayMedia => self.direct_play_selected_message_media(),
            MessageActionKind::ViewAttachment => {
                self.direct_open_selected_message_attachment_viewer();
                None
            }
            MessageActionKind::ShowProfile => self.direct_show_selected_message_profile(),
            MessageActionKind::OpenPinConfirmation => {
                self.direct_open_selected_message_pin_confirmation();
                None
            }
            MessageActionKind::OpenThread => {
                let channel_id = self
                    .selected_message_state()
                    .and_then(|message| self.thread_summary_for_message(message))?
                    .channel_id;
                self.record_thread_return_target(channel_id);
                self.activate_channel(channel_id);
                None
            }
            MessageActionKind::ShowReactionUsers => {
                let message = self.selected_message_state()?;
                if !self.can_show_reaction_users_for_message(message) {
                    return None;
                }
                let channel_id = message.channel_id;
                let message_id = message.id;
                let reactions = message
                    .reactions
                    .iter()
                    .map(|reaction| reaction.emoji.clone())
                    .collect::<Vec<_>>();
                if reactions.is_empty() {
                    return None;
                }
                Some(AppCommand::LoadReactionUsers {
                    channel_id,
                    message_id,
                    reactions,
                })
            }
            MessageActionKind::OpenPollVotePicker => {
                self.open_poll_vote_picker();
                None
            }
            MessageActionKind::GoToReferencedMessage => self.go_to_selected_referenced_message(),
        }
    }

    pub fn activate_selected_message_url(&mut self) -> Option<AppCommand> {
        let index = self.selected_message_url_index()?;
        let url = self.selected_message_url_items().get(index)?.url.clone();
        self.close_message_url_picker();
        Some(AppCommand::OpenUrl { url })
    }

    pub fn activate_message_url_shortcut(&mut self, shortcut: KeyChord) -> Option<AppCommand> {
        let urls = self.selected_message_url_items();
        let index = self
            .options
            .key_bindings()
            .matching_indexed_shortcut_index(shortcut, urls.len())?;
        self.select_message_url_row(index);
        self.activate_selected_message_url()
    }

    pub fn close_message_url_picker(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::MessageUrlPicker) {
            self.popups.clear_modal();
        }
    }

    fn open_message_url_picker(&mut self, items: Vec<MessageUrlItem>) {
        self.popups.modal = Some(ModalPopup::MessageUrlPicker(MessageUrlPickerState {
            selection: Default::default(),
            items,
        }));
    }

    pub fn direct_copy_selected_message_content(&mut self) {
        let Some(content) = self
            .selected_message_state()
            .and_then(|message| message.content.as_ref())
        else {
            return;
        };
        self.runtime.copy_text_requested = Some((content.clone(), "Message copied"));
    }

    pub(in crate::tui) fn take_copy_text_request(&mut self) -> Option<(String, &'static str)> {
        self.runtime.copy_text_requested.take()
    }

    pub fn direct_open_selected_message_reaction_picker(&mut self) {
        self.open_emoji_reaction_picker();
    }

    pub fn direct_reply_to_selected_message(&mut self) {
        self.start_reply_composer();
    }

    pub fn direct_edit_selected_message(&mut self) {
        self.start_edit_composer();
    }

    pub fn direct_open_selected_message_attachment_viewer(&mut self) {
        self.open_attachment_viewer_for_selected_message();
    }

    pub fn direct_open_selected_message_url(&mut self) -> Option<AppCommand> {
        let message = self.selected_message_state()?;
        let urls = message_url_items(message);

        match urls.as_slice() {
            [] => None,
            [item] => Some(AppCommand::OpenUrl {
                url: item.url.clone(),
            }),
            _ => {
                self.open_message_url_picker(urls);
                None
            }
        }
    }

    pub fn direct_open_selected_message_remove_embeds_confirmation(&mut self) {
        let Some(message) = self.selected_message_state() else {
            return;
        };
        if !self.can_remove_message_embeds(message) {
            return;
        }
        self.open_message_confirmation(popups::MessageConfirmationState::remove_embeds(
            message.channel_id,
            message.id,
            message.author.clone(),
            message.content.clone(),
        ));
    }

    pub fn direct_play_selected_message_media(&mut self) -> Option<AppCommand> {
        let message = self.selected_message_state()?;
        message_media_playback_items(message)
            .into_iter()
            .next()
            .map(|target| AppCommand::PlayMedia {
                target,
                request_id: None,
            })
    }

    pub fn go_to_selected_referenced_message(&mut self) -> Option<AppCommand> {
        let target = self
            .selected_message_state()
            .and_then(|message| self.referenced_message_target(message))?;
        let scope = self
            .discord
            .cache
            .channel(target.channel_id)
            .map(|channel| match channel.guild_id {
                Some(guild_id) => ActiveGuildScope::Guild(guild_id),
                None => ActiveGuildScope::DirectMessages,
            })?;
        self.activate_guild(scope);
        self.activate_channel(target.channel_id);
        self.focus_pane(FocusPane::Messages);
        Some(AppCommand::LoadMessageHistoryAround {
            channel_id: target.channel_id,
            message_id: target.message_id,
        })
    }

    pub fn direct_show_selected_message_profile(&mut self) -> Option<AppCommand> {
        let message = self.selected_message_state()?;
        self.open_user_profile_popup(message.author_id, message.guild_id)
    }

    pub fn direct_open_selected_message_pin_confirmation(&mut self) {
        let Some(message) = self.selected_message_state() else {
            return;
        };
        self.open_selected_message_pin_confirmation(!message.pinned);
    }

    pub fn open_selected_message_delete_confirmation(&mut self) {
        let Some(message) = self.selected_message_state() else {
            return;
        };
        if !self.can_delete_message(message) {
            return;
        }
        self.open_message_confirmation(popups::MessageConfirmationState::delete(
            message.channel_id,
            message.id,
            message.author.clone(),
            message.content.clone(),
        ));
    }

    pub fn open_selected_message_pin_confirmation(&mut self, pinned: bool) {
        let Some(message) = self.selected_message_state() else {
            return;
        };
        if !self.can_pin_messages_for_message(message) {
            return;
        }
        self.open_message_confirmation(popups::MessageConfirmationState::pin(
            message.channel_id,
            message.id,
            pinned,
            message.author.clone(),
            message.content.clone(),
        ));
    }

    pub fn close_message_confirmation(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::MessageConfirmation) {
            self.popups.clear_modal();
        }
    }

    pub fn confirm_message_confirmation(&mut self) -> Option<AppCommand> {
        let confirmation = self.popups.take_message_confirmation()?;
        match confirmation.kind {
            MessageConfirmationKind::Delete => Some(AppCommand::DeleteMessage {
                channel_id: confirmation.channel_id,
                message_id: confirmation.message_id,
            }),
            MessageConfirmationKind::RemoveEmbeds => Some(AppCommand::RemoveMessageEmbeds {
                channel_id: confirmation.channel_id,
                message_id: confirmation.message_id,
            }),
            MessageConfirmationKind::Pin { pinned } => Some(AppCommand::SetMessagePinned {
                channel_id: confirmation.channel_id,
                message_id: confirmation.message_id,
                pinned,
            }),
        }
    }

    pub fn message_confirmation_lines(
        &self,
    ) -> Option<(MessageConfirmationKind, String, Option<String>)> {
        let confirmation = self.popups.message_confirmation()?;
        Some((
            confirmation.kind,
            confirmation.author.clone(),
            confirmation.content.clone(),
        ))
    }

    fn open_message_confirmation(&mut self, confirmation: popups::MessageConfirmationState) {
        self.popups.modal = Some(ModalPopup::MessageConfirmation(confirmation));
    }
}

fn message_url_items(message: &MessageState) -> Vec<MessageUrlItem> {
    message_urls(message)
        .into_iter()
        .map(|url| MessageUrlItem {
            label: url.clone(),
            url,
        })
        .collect()
}

fn message_media_playback_items(message: &MessageState) -> Vec<MediaPlaybackTarget> {
    let mut targets = message
        .attachments_in_display_order()
        .filter(|attachment| attachment.is_video())
        .filter_map(|attachment| {
            Some(MediaPlaybackTarget {
                url: attachment.preferred_url()?.to_owned(),
                label: attachment.filename.clone(),
                source: MediaPlaybackSource::Message,
            })
        })
        .collect::<Vec<_>>();

    targets.extend(message_playable_media_urls(message).into_iter().map(|url| {
        MediaPlaybackTarget {
            label: "media URL".to_owned(),
            url,
            source: MediaPlaybackSource::Message,
        }
    }));
    dedupe_media_targets(targets)
}

fn message_playable_media_urls(message: &MessageState) -> Vec<String> {
    let mut urls = message_urls(message)
        .into_iter()
        .filter(|url| is_playable_media_url(url))
        .collect::<Vec<_>>();
    urls.extend(playable_embed_video_urls(&message.embeds));
    for snapshot in &message.forwarded_snapshots {
        urls.extend(playable_embed_video_urls(&snapshot.embeds));
    }
    dedupe_urls(urls)
}

fn playable_embed_video_urls(embeds: &[EmbedInfo]) -> Vec<String> {
    embeds
        .iter()
        .filter_map(|embed| embed.video_url.clone())
        .filter(|url| is_playable_media_url(url))
        .collect()
}

fn message_urls(message: &MessageState) -> Vec<String> {
    let mut urls = Vec::new();
    if let Some(content) = &message.content {
        urls.extend(detected_urls(content));
    }
    urls.extend(embed_urls(&message.embeds));
    // URLs in a reply quote or a forwarded message are shown to the user too.
    if let Some(reply) = &message.reply
        && let Some(content) = &reply.content
    {
        urls.extend(detected_urls(content));
    }
    for snapshot in &message.forwarded_snapshots {
        if let Some(content) = &snapshot.content {
            urls.extend(detected_urls(content));
        }
        urls.extend(embed_urls(&snapshot.embeds));
    }
    dedupe_urls(urls)
}

fn embed_urls(embeds: &[EmbedInfo]) -> Vec<String> {
    embeds
        .iter()
        .filter_map(|embed| embed.url.clone())
        .collect()
}

fn dedupe_urls(urls: Vec<String>) -> Vec<String> {
    let mut unique = Vec::new();
    for url in urls {
        if !unique.contains(&url) {
            unique.push(url);
        }
    }
    unique
}

fn dedupe_media_targets(targets: Vec<MediaPlaybackTarget>) -> Vec<MediaPlaybackTarget> {
    let mut unique = Vec::new();
    for target in targets {
        if !unique
            .iter()
            .any(|candidate: &MediaPlaybackTarget| candidate.url == target.url)
        {
            unique.push(target);
        }
    }
    unique
}

fn is_playable_media_url(url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if is_youtube_host(&host) {
        return true;
    }
    let path = url.path().to_ascii_lowercase();
    PLAYABLE_VIDEO_EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(&format!(".{extension}")))
}

fn is_youtube_host(host: &str) -> bool {
    matches!(
        host,
        "youtu.be" | "youtube.com" | "www.youtube.com" | "m.youtube.com"
    ) || host.ends_with(".youtube.com")
        || host == "youtube-nocookie.com"
        || host.ends_with(".youtube-nocookie.com")
}
