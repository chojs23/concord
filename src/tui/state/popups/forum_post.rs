use crate::discord::ids::{Id, marker::ChannelMarker};
use crate::discord::{
    AppCommand, ForumPostCreate, MAX_UPLOAD_ATTACHMENT_COUNT, MessageAttachmentUpload,
};
use ratatui_image::protocol::Protocol;

use super::super::local_upload_preview::{
    LocalUploadPreviewState, LocalUploadPreviewStatus, local_upload_preview_view,
};
use super::super::{
    DashboardState, FocusPane, ForumPostAttachmentPreviewView, ForumPostComposerAttachmentView,
    ForumPostComposerField, ForumPostComposerTagView, ForumPostComposerView,
};
use super::{
    ActiveModalPopupKind, ForumPostComposerFieldState, ForumPostComposerState, ModalPopup,
};

impl DashboardState {
    pub fn open_forum_post_composer(&mut self, channel_id: Id<ChannelMarker>) {
        let can_create = self
            .discord
            .cache
            .channel(channel_id)
            .is_some_and(|channel| {
                channel.is_forum() && self.discord.cache.can_send_in_channel(channel)
            });
        if !can_create {
            return;
        }

        self.cancel_composer();
        self.popups.modal = Some(ModalPopup::ForumPostComposer(ForumPostComposerState::new(
            channel_id,
        )));
        self.navigation.focus = FocusPane::Messages;
    }

    pub fn close_forum_post_composer(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::ForumPostComposer) {
            self.popups.clear_modal();
            self.runtime.clipboard_paste_pending = false;
        }
    }

    pub fn is_forum_post_composer_active(&self) -> bool {
        self.is_active_modal_popup(ActiveModalPopupKind::ForumPostComposer)
    }

    pub fn forum_post_composer_view(&self) -> Option<ForumPostComposerView> {
        let popup = self.popups.forum_post_composer()?;
        let channel = self.discord.cache.channel(popup.channel_id)?;
        let tags = channel
            .available_tags
            .iter()
            .enumerate()
            .map(|(index, tag)| ForumPostComposerTagView {
                name: tag.name.clone(),
                emoji: forum_tag_emoji_label(tag.emoji_id.is_some(), tag.emoji_name.as_deref()),
                selected: popup.selected_tag_ids.contains(&tag.id),
                active: popup.editing == Some(ForumPostComposerFieldState::Tags)
                    && index == popup.selected_tag_index,
            })
            .collect();
        let attachments = popup
            .attachments
            .iter()
            .enumerate()
            .map(|attachment| ForumPostComposerAttachmentView {
                filename: attachment.1.filename.clone(),
                size_bytes: attachment.1.size_bytes,
                active: popup.editing == Some(ForumPostComposerFieldState::Attachments)
                    && attachment.0 == popup.selected_attachment_index,
                preview: forum_post_attachment_preview(attachment.1),
            })
            .collect();
        Some(ForumPostComposerView {
            channel_label: format!("#{}", channel.name),
            active_field: popup.active_field.into(),
            editing_field: popup.editing.map(Into::into),
            title: forum_post_text_field_value(popup, ForumPostComposerFieldState::Title)
                .to_owned(),
            title_cursor: forum_post_text_field_cursor(popup, ForumPostComposerFieldState::Title),
            body: forum_post_text_field_value(popup, ForumPostComposerFieldState::Body).to_owned(),
            body_cursor: forum_post_text_field_cursor(popup, ForumPostComposerFieldState::Body),
            attachments,
            tags,
            requires_tag: channel.requires_forum_tag(),
            paste_pending: self.runtime.clipboard_paste_pending,
            status: popup.status.clone(),
        })
    }

    pub fn cycle_forum_post_field_next(&mut self) {
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            if popup.editing.is_some() {
                return;
            }
            popup.active_field = match popup.active_field {
                ForumPostComposerFieldState::Title => ForumPostComposerFieldState::Body,
                ForumPostComposerFieldState::Body => ForumPostComposerFieldState::Attachments,
                ForumPostComposerFieldState::Attachments => ForumPostComposerFieldState::Tags,
                ForumPostComposerFieldState::Tags => ForumPostComposerFieldState::Title,
            };
        }
    }

    pub fn cycle_forum_post_field_previous(&mut self) {
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            if popup.editing.is_some() {
                return;
            }
            popup.active_field = match popup.active_field {
                ForumPostComposerFieldState::Title => ForumPostComposerFieldState::Tags,
                ForumPostComposerFieldState::Body => ForumPostComposerFieldState::Title,
                ForumPostComposerFieldState::Attachments => ForumPostComposerFieldState::Body,
                ForumPostComposerFieldState::Tags => ForumPostComposerFieldState::Attachments,
            };
        }
    }

    pub fn push_forum_post_char(&mut self, value: char) {
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            match popup.editing {
                Some(ForumPostComposerFieldState::Title) if value != '\n' => {
                    popup.edit_input.insert_char(value);
                    popup.status = None;
                }
                Some(ForumPostComposerFieldState::Body) => {
                    popup.edit_input.insert_char(value);
                    popup.status = None;
                }
                Some(ForumPostComposerFieldState::Title)
                | Some(ForumPostComposerFieldState::Attachments)
                | Some(ForumPostComposerFieldState::Tags)
                | None => {}
            }
        }
    }

    pub fn insert_forum_post_text(&mut self, value: &str) -> bool {
        let Some(popup) = self.popups.forum_post_composer_mut() else {
            return false;
        };
        let pasted: String = value.chars().filter(|value| *value != '\r').collect();
        if pasted.is_empty() {
            return false;
        }
        match popup.editing {
            Some(ForumPostComposerFieldState::Title) => {
                let single_line = pasted.lines().next().unwrap_or_default();
                if single_line.is_empty() {
                    return false;
                }
                popup.edit_input.insert_str(single_line);
            }
            Some(ForumPostComposerFieldState::Body) => popup.edit_input.insert_str(&pasted),
            Some(ForumPostComposerFieldState::Attachments)
            | Some(ForumPostComposerFieldState::Tags)
            | None => {
                return false;
            }
        }
        popup.status = None;
        true
    }

    pub fn delete_forum_post_previous_char(&mut self) {
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            let changed = match popup.editing {
                Some(ForumPostComposerFieldState::Title | ForumPostComposerFieldState::Body) => {
                    popup.edit_input.delete_previous_grapheme()
                }
                Some(
                    ForumPostComposerFieldState::Attachments | ForumPostComposerFieldState::Tags,
                )
                | None => false,
            };
            if changed {
                popup.status = None;
            }
        }
    }

    pub fn delete_forum_post_previous_word(&mut self) {
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            let changed = match popup.editing {
                Some(ForumPostComposerFieldState::Title | ForumPostComposerFieldState::Body) => {
                    popup.edit_input.delete_previous_word()
                }
                Some(
                    ForumPostComposerFieldState::Attachments | ForumPostComposerFieldState::Tags,
                )
                | None => false,
            };
            if changed {
                popup.status = None;
            }
        }
    }

    pub fn move_forum_post_cursor_left(&mut self) {
        self.with_forum_post_active_text_input(|input| input.move_left());
    }

    pub fn move_forum_post_cursor_right(&mut self) {
        self.with_forum_post_active_text_input(|input| input.move_right());
    }

    pub fn move_forum_post_cursor_word_left(&mut self) {
        self.with_forum_post_active_text_input(|input| input.move_word_left());
    }

    pub fn move_forum_post_cursor_word_right(&mut self) {
        self.with_forum_post_active_text_input(|input| input.move_word_right());
    }

    pub fn move_forum_post_cursor_home(&mut self) {
        self.with_forum_post_active_text_input(|input| input.move_home());
    }

    pub fn move_forum_post_cursor_end(&mut self) {
        self.with_forum_post_active_text_input(|input| input.move_end());
    }

    fn with_forum_post_active_text_input(
        &mut self,
        action: impl FnOnce(&mut crate::tui::text_input::TextInputState),
    ) {
        let Some(popup) = self.popups.forum_post_composer_mut() else {
            return;
        };
        match popup.editing {
            Some(ForumPostComposerFieldState::Title | ForumPostComposerFieldState::Body) => {
                action(&mut popup.edit_input)
            }
            Some(ForumPostComposerFieldState::Attachments | ForumPostComposerFieldState::Tags)
            | None => {}
        }
    }

    pub fn move_forum_post_selection_down(&mut self) {
        let Some((channel_id, editing)) = self
            .popups
            .forum_post_composer()
            .map(|popup| (popup.channel_id, popup.editing))
        else {
            return;
        };
        let (attachment_count, tag_count) = self
            .discord
            .cache
            .channel(channel_id)
            .map(|channel| {
                let attachment_count = self
                    .popups
                    .forum_post_composer()
                    .map(|popup| popup.attachments.len())
                    .unwrap_or_default();
                (attachment_count, channel.available_tags.len())
            })
            .unwrap_or_default();
        match editing {
            Some(ForumPostComposerFieldState::Attachments) if attachment_count > 0 => {
                if let Some(popup) = self.popups.forum_post_composer_mut() {
                    popup.selected_attachment_index = (popup.selected_attachment_index + 1)
                        .min(attachment_count.saturating_sub(1));
                }
                self.refresh_forum_post_attachment_preview();
            }
            Some(ForumPostComposerFieldState::Tags) if tag_count > 0 => {
                if let Some(popup) = self.popups.forum_post_composer_mut() {
                    popup.selected_tag_index =
                        (popup.selected_tag_index + 1).min(tag_count.saturating_sub(1));
                }
            }
            Some(_) => {}
            None => self.cycle_forum_post_field_next(),
        }
    }

    pub fn move_forum_post_selection_up(&mut self) {
        match self
            .popups
            .forum_post_composer()
            .and_then(|popup| popup.editing)
        {
            Some(ForumPostComposerFieldState::Attachments) => {
                if let Some(popup) = self.popups.forum_post_composer_mut() {
                    popup.selected_attachment_index =
                        popup.selected_attachment_index.saturating_sub(1);
                }
                self.refresh_forum_post_attachment_preview();
            }
            Some(ForumPostComposerFieldState::Tags) => {
                if let Some(popup) = self.popups.forum_post_composer_mut() {
                    popup.selected_tag_index = popup.selected_tag_index.saturating_sub(1);
                }
            }
            Some(_) => {}
            None => self.cycle_forum_post_field_previous(),
        }
    }

    pub fn toggle_selected_forum_post_tag(&mut self) {
        let Some((channel_id, selected_tag_index)) = self
            .popups
            .forum_post_composer()
            .map(|popup| (popup.channel_id, popup.selected_tag_index))
        else {
            return;
        };
        let Some(tag_id) = self
            .discord
            .cache
            .channel(channel_id)
            .and_then(|channel| channel.available_tags.get(selected_tag_index))
            .map(|tag| tag.id)
        else {
            return;
        };
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            if let Some(position) = popup.selected_tag_ids.iter().position(|id| *id == tag_id) {
                popup.selected_tag_ids.remove(position);
            } else {
                popup.selected_tag_ids.push(tag_id);
            }
            popup.status = None;
        }
    }

    pub fn forum_post_composer_accepts_attachments(&self) -> bool {
        let Some(popup) = self.popups.forum_post_composer() else {
            return false;
        };
        self.discord
            .cache
            .channel(popup.channel_id)
            .is_some_and(|channel| {
                channel.is_forum() && self.discord.cache.can_attach_in_channel(channel)
            })
    }

    pub fn forum_post_composer_accepts_attachment_paste(&self) -> bool {
        let Some(popup) = self.popups.forum_post_composer() else {
            return false;
        };
        matches!(
            popup.editing,
            Some(ForumPostComposerFieldState::Body | ForumPostComposerFieldState::Attachments)
        ) && self.forum_post_composer_accepts_attachments()
    }

    pub fn add_pending_forum_post_attachments(
        &mut self,
        attachments: Vec<MessageAttachmentUpload>,
    ) {
        if attachments.is_empty() || !self.forum_post_composer_accepts_attachments() {
            return;
        }
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            let available = MAX_UPLOAD_ATTACHMENT_COUNT.saturating_sub(popup.attachments.len());
            popup
                .attachments
                .extend(attachments.into_iter().take(available));
            if !popup.attachments.is_empty() {
                popup.selected_attachment_index = popup
                    .selected_attachment_index
                    .min(popup.attachments.len().saturating_sub(1));
            }
            popup.status = None;
        }
        self.refresh_forum_post_attachment_preview();
    }

    pub fn pop_pending_forum_post_attachment(&mut self) {
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            if popup.attachments.is_empty() {
                return;
            }
            let index = popup
                .selected_attachment_index
                .min(popup.attachments.len().saturating_sub(1));
            popup.attachments.remove(index);
            popup.selected_attachment_index = popup
                .selected_attachment_index
                .min(popup.attachments.len().saturating_sub(1));
            popup.attachment_preview = None;
            popup.status = None;
        }
        self.refresh_forum_post_attachment_preview();
    }

    pub fn clear_forum_post_active_field(&mut self) {
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            if popup.editing.is_some() {
                popup.edit_input.clear();
                popup.status = None;
                return;
            }
            match popup.active_field {
                ForumPostComposerFieldState::Title => popup.title.clear(),
                ForumPostComposerFieldState::Body => popup.body.clear(),
                ForumPostComposerFieldState::Attachments => {
                    popup.attachments.clear();
                    popup.selected_attachment_index = 0;
                    popup.attachment_preview = None;
                }
                ForumPostComposerFieldState::Tags => popup.selected_tag_ids.clear(),
            }
            popup.status = None;
        }
    }

    pub fn activate_forum_post_composer(&mut self) -> Option<AppCommand> {
        let (active_field, editing) = self
            .popups
            .forum_post_composer()
            .map(|popup| (popup.active_field, popup.editing))?;
        if editing == Some(ForumPostComposerFieldState::Tags)
            && active_field == ForumPostComposerFieldState::Tags
        {
            self.toggle_selected_forum_post_tag();
            return None;
        }
        if matches!(
            editing,
            Some(ForumPostComposerFieldState::Title | ForumPostComposerFieldState::Body)
        ) && editing == Some(active_field)
        {
            self.commit_forum_post_edit();
            return None;
        }
        match active_field {
            ForumPostComposerFieldState::Title | ForumPostComposerFieldState::Body => {
                self.start_forum_post_edit(active_field);
            }
            ForumPostComposerFieldState::Attachments => {
                self.start_forum_post_attachment_selection();
            }
            ForumPostComposerFieldState::Tags => self.start_forum_post_tag_selection(),
        }
        None
    }

    pub fn close_or_cancel_forum_post_composer(&mut self) {
        match self
            .popups
            .forum_post_composer()
            .and_then(|popup| popup.editing)
        {
            Some(ForumPostComposerFieldState::Title | ForumPostComposerFieldState::Body) => {
                self.commit_forum_post_edit();
                return;
            }
            Some(ForumPostComposerFieldState::Attachments | ForumPostComposerFieldState::Tags) => {
                if let Some(popup) = self.popups.forum_post_composer_mut() {
                    popup.editing = None;
                    popup.edit_input.clear();
                    popup.status = None;
                }
                return;
            }
            None => {}
        }
        self.close_forum_post_composer();
    }

    pub fn is_forum_post_composer_editing(&self) -> bool {
        self.popups
            .forum_post_composer()
            .is_some_and(|popup| popup.editing.is_some())
    }

    pub fn is_forum_post_attachment_picker_active(&self) -> bool {
        self.popups
            .forum_post_composer()
            .is_some_and(|popup| popup.editing == Some(ForumPostComposerFieldState::Attachments))
    }

    pub fn forum_post_attachment_preview(&self) -> Option<ForumPostAttachmentPreviewView<'_>> {
        let popup = self.popups.forum_post_composer()?;
        let preview = popup.attachment_preview.as_ref()?;
        if popup.editing != Some(ForumPostComposerFieldState::Attachments)
            || preview.attachment_index != popup.selected_attachment_index
        {
            return None;
        }
        Some(local_upload_preview_view(preview))
    }

    pub(in crate::tui) fn forum_post_attachment_preview_has_image_surface(&self) -> bool {
        let Some(popup) = self.popups.forum_post_composer() else {
            return false;
        };
        let Some(preview) = popup.attachment_preview.as_ref() else {
            return false;
        };
        popup.editing == Some(ForumPostComposerFieldState::Attachments)
            && preview.attachment_index == popup.selected_attachment_index
            && matches!(preview.state, LocalUploadPreviewStatus::Ready(_))
    }

    pub(in crate::tui) fn take_pending_forum_post_attachment_preview(
        &mut self,
    ) -> Option<(usize, u64, String, MessageAttachmentUpload)> {
        let popup = self.popups.forum_post_composer_mut()?;
        if popup.editing != Some(ForumPostComposerFieldState::Attachments) {
            return None;
        }
        let preview = popup.attachment_preview.as_mut()?;
        if !matches!(preview.state, LocalUploadPreviewStatus::Pending) {
            return None;
        }
        let attachment = popup.attachments.get(preview.attachment_index)?.clone();
        preview.state = LocalUploadPreviewStatus::Loading;
        Some((
            preview.attachment_index,
            preview.generation,
            preview.filename.clone(),
            attachment,
        ))
    }

    pub(in crate::tui) fn store_forum_post_attachment_preview_result(
        &mut self,
        attachment_index: usize,
        generation: u64,
        filename: String,
        result: std::result::Result<Protocol, String>,
    ) {
        let Some(popup) = self.popups.forum_post_composer_mut() else {
            return;
        };
        let Some(preview) = popup.attachment_preview.as_mut() else {
            return;
        };
        if preview.attachment_index != attachment_index || preview.generation != generation {
            return;
        }
        preview.filename = filename;
        preview.state = match result {
            Ok(protocol) => LocalUploadPreviewStatus::Ready(protocol),
            Err(message) => LocalUploadPreviewStatus::Failed(message),
        };
    }

    pub fn is_forum_post_tag_picker_active(&self) -> bool {
        self.popups
            .forum_post_composer()
            .is_some_and(|popup| popup.editing == Some(ForumPostComposerFieldState::Tags))
    }

    pub fn save_forum_post_composer(&mut self) -> Option<AppCommand> {
        if let Some(popup) = self.popups.forum_post_composer_mut()
            && let Some(editing) = popup.editing
        {
            let message = if editing == ForumPostComposerFieldState::Tags {
                "Press Esc to finish selecting tags first"
            } else {
                "Press Enter to finish editing first"
            };
            popup.status = Some(message.to_owned());
            return None;
        }
        self.submit_forum_post_composer()
    }

    fn start_forum_post_edit(&mut self, field: ForumPostComposerFieldState) {
        let Some(popup) = self.popups.forum_post_composer_mut() else {
            return;
        };
        let value = match field {
            ForumPostComposerFieldState::Title => popup.title.value().to_owned(),
            ForumPostComposerFieldState::Body => popup.body.value().to_owned(),
            ForumPostComposerFieldState::Attachments | ForumPostComposerFieldState::Tags => return,
        };
        popup.editing = Some(field);
        popup.edit_input.set_value(value);
        popup.status = None;
    }

    fn start_forum_post_attachment_selection(&mut self) {
        let Some(popup) = self.popups.forum_post_composer_mut() else {
            return;
        };
        if popup.attachments.is_empty() {
            popup.status = Some("no attachments pasted yet".to_owned());
            return;
        }
        popup.selected_attachment_index = popup
            .selected_attachment_index
            .min(popup.attachments.len().saturating_sub(1));
        popup.editing = Some(ForumPostComposerFieldState::Attachments);
        popup.edit_input.clear();
        popup.status = None;
        self.refresh_forum_post_attachment_preview();
    }

    fn start_forum_post_tag_selection(&mut self) {
        let Some(channel_id) = self
            .popups
            .forum_post_composer()
            .map(|popup| popup.channel_id)
        else {
            return;
        };
        let tag_count = self
            .discord
            .cache
            .channel(channel_id)
            .map(|channel| channel.available_tags.len())
            .unwrap_or_default();
        let Some(popup) = self.popups.forum_post_composer_mut() else {
            return;
        };
        if tag_count == 0 {
            popup.status = Some("no tags available".to_owned());
            return;
        }
        popup.selected_tag_index = popup.selected_tag_index.min(tag_count - 1);
        popup.editing = Some(ForumPostComposerFieldState::Tags);
        popup.edit_input.clear();
        popup.status = None;
    }

    fn refresh_forum_post_attachment_preview(&mut self) {
        let show_images = self.show_images();
        let Some(popup) = self.popups.forum_post_composer_mut() else {
            return;
        };
        if popup.editing != Some(ForumPostComposerFieldState::Attachments) || !show_images {
            popup.attachment_preview = None;
            return;
        }
        let index = popup
            .selected_attachment_index
            .min(popup.attachments.len().saturating_sub(1));
        if popup.attachment_preview.as_ref().is_some_and(|preview| {
            preview.attachment_index == index
                && !matches!(preview.state, LocalUploadPreviewStatus::Failed(_))
        }) {
            return;
        }
        let Some(attachment) = popup.attachments.get(index) else {
            popup.attachment_preview = None;
            return;
        };
        popup.attachment_preview_generation = popup.attachment_preview_generation.saturating_add(1);
        popup.attachment_preview = Some(LocalUploadPreviewState {
            attachment_index: index,
            generation: popup.attachment_preview_generation,
            filename: attachment.filename.clone(),
            state: LocalUploadPreviewStatus::Pending,
        });
    }

    fn commit_forum_post_edit(&mut self) {
        let Some(popup) = self.popups.forum_post_composer_mut() else {
            return;
        };
        let Some(field) = popup.editing else {
            return;
        };
        let value = popup.edit_input.value().to_owned();
        match field {
            ForumPostComposerFieldState::Title => popup.title.set_value(value),
            ForumPostComposerFieldState::Body => popup.body.set_value(value),
            ForumPostComposerFieldState::Attachments | ForumPostComposerFieldState::Tags => {}
        }
        popup.editing = None;
        popup.edit_input.clear();
        popup.status = None;
    }

    pub fn submit_forum_post_composer(&mut self) -> Option<AppCommand> {
        let result = self.build_forum_post_create();
        match result {
            Ok(post) => {
                self.close_forum_post_composer();
                Some(AppCommand::CreateForumPost { post })
            }
            Err(message) => {
                if let Some(popup) = self.popups.forum_post_composer_mut() {
                    popup.status = Some(message);
                }
                None
            }
        }
    }

    fn build_forum_post_create(&mut self) -> Result<ForumPostCreate, String> {
        let Some(popup) = self.popups.forum_post_composer() else {
            return Err("forum post composer is not open".to_owned());
        };
        let channel_id = popup.channel_id;
        let title = popup.title.value().trim().to_owned();
        let content = popup.body.value().trim().to_owned();
        let applied_tags = popup.selected_tag_ids.clone();

        if title.is_empty() {
            return Err("title is required".to_owned());
        }
        if title.chars().count() > 100 {
            return Err("title must be 100 characters or fewer".to_owned());
        }
        if content.is_empty() {
            return Err("body is required".to_owned());
        }
        let Some(channel) = self.discord.cache.channel(channel_id) else {
            return Err("forum channel is no longer available".to_owned());
        };
        if !channel.is_forum() || !self.discord.cache.can_send_in_channel(channel) {
            return Err("cannot create posts in this channel".to_owned());
        }
        if channel.requires_forum_tag() && applied_tags.is_empty() {
            return Err("at least one tag is required".to_owned());
        }
        if !popup.attachments.is_empty() && !self.discord.cache.can_attach_in_channel(channel) {
            return Err("attachments are not allowed in this channel".to_owned());
        }

        let attachments = self
            .popups
            .forum_post_composer_mut()
            .map(|popup| std::mem::take(&mut popup.attachments))
            .unwrap_or_default();
        Ok(ForumPostCreate {
            channel_id,
            title,
            content,
            applied_tags,
            attachments,
        })
    }
}

impl From<ForumPostComposerFieldState> for ForumPostComposerField {
    fn from(value: ForumPostComposerFieldState) -> Self {
        match value {
            ForumPostComposerFieldState::Title => Self::Title,
            ForumPostComposerFieldState::Body => Self::Body,
            ForumPostComposerFieldState::Attachments => Self::Attachments,
            ForumPostComposerFieldState::Tags => Self::Tags,
        }
    }
}

fn forum_post_text_field_value(
    popup: &ForumPostComposerState,
    field: ForumPostComposerFieldState,
) -> &str {
    if popup.editing == Some(field) {
        return popup.edit_input.value();
    }
    match field {
        ForumPostComposerFieldState::Title => popup.title.value(),
        ForumPostComposerFieldState::Body => popup.body.value(),
        ForumPostComposerFieldState::Attachments | ForumPostComposerFieldState::Tags => "",
    }
}

fn forum_post_text_field_cursor(
    popup: &ForumPostComposerState,
    field: ForumPostComposerFieldState,
) -> usize {
    if popup.editing == Some(field) {
        return popup.edit_input.cursor_byte_index();
    }
    match field {
        ForumPostComposerFieldState::Title => popup.title.cursor_byte_index(),
        ForumPostComposerFieldState::Body => popup.body.cursor_byte_index(),
        ForumPostComposerFieldState::Attachments | ForumPostComposerFieldState::Tags => 0,
    }
}

fn forum_tag_emoji_label(custom: bool, name: Option<&str>) -> Option<String> {
    let name = name?.trim();
    if name.is_empty() {
        return None;
    }
    if custom {
        Some(format!(":{name}:"))
    } else {
        Some(name.to_owned())
    }
}

fn forum_post_attachment_preview(attachment: &MessageAttachmentUpload) -> String {
    if let Some(path) = attachment.path() {
        return path.display().to_string();
    }
    if attachment.bytes().is_some() {
        return "clipboard image or in-memory upload".to_owned();
    }
    "pending upload".to_owned()
}
