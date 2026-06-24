use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, ForumTagMarker},
};
use crate::discord::{
    AppCommand, ForumPostCreate, MAX_UPLOAD_ATTACHMENT_COUNT, MessageAttachmentUpload,
};
use crate::tui::keybindings::ScrollAction;
use ratatui_image::protocol::Protocol;

use super::super::composer::expand_emoji_shortcodes;
use super::super::local_upload_preview::{
    LocalUploadPreviewState, LocalUploadPreviewStatus, local_upload_preview_candidate,
    local_upload_preview_view,
};
use super::super::{
    DashboardState, FocusPane, ForumPostComposerAttachmentView, ForumPostComposerField,
    ForumPostComposerTagView, ForumPostComposerView, LocalUploadPreviewView,
};
use super::{
    ActiveModalPopupKind, ForumPostComposerFieldState, ForumPostComposerState, ModalPopup,
};

/// Discord allows at most five tags applied to a single forum post.
const MAX_FORUM_POST_TAGS: usize = 5;

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

    /// Ask the runtime loop to open `$EDITOR` for the post body. Only honored
    /// while the body field is being edited so the title and other fields keep
    /// their inline editing.
    pub fn request_open_forum_post_body_in_editor(&mut self) {
        if self
            .popups
            .forum_post_composer()
            .is_some_and(|popup| popup.editing == Some(ForumPostComposerFieldState::Body))
        {
            self.runtime.open_forum_post_body_in_editor_requested = true;
        }
    }

    pub fn take_open_forum_post_body_in_editor_request(&mut self) -> bool {
        std::mem::take(&mut self.runtime.open_forum_post_body_in_editor_requested)
    }

    /// Current body text to seed the external editor with, or `None` when the
    /// body is not being edited. The body lives in `edit_input` while editing.
    pub fn forum_post_body_for_editor(&self) -> Option<String> {
        self.popups
            .forum_post_composer()
            .filter(|popup| popup.editing == Some(ForumPostComposerFieldState::Body))
            .map(|popup| popup.edit_input.value().to_owned())
    }

    /// Apply the text returned by the external editor back into the body edit
    /// buffer, keeping the user in body editing mode.
    pub fn replace_forum_post_body_from_editor(&mut self, content: String) {
        if let Some(popup) = self.popups.forum_post_composer_mut()
            && popup.editing == Some(ForumPostComposerFieldState::Body)
        {
            popup.edit_input.set_value(content);
            popup.status = None;
        }
    }

    /// Top-of-viewport row for the scrolling composer body. Used by the renderer.
    pub fn forum_post_composer_scroll(&self) -> usize {
        self.popups
            .forum_post_composer()
            .map(|popup| popup.scroll.scroll())
            .unwrap_or(0)
    }

    pub fn scroll_forum_post_composer(&mut self, action: ScrollAction) {
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            match action {
                ScrollAction::Down => popup.scroll.scroll_down(),
                ScrollAction::Up => popup.scroll.scroll_up(),
            }
        }
    }

    /// Ask the next render to scroll the focused field or text cursor back into
    /// view. Called after any focus/edit change, but not after a manual scroll.
    pub fn request_forum_post_scroll_reveal(&mut self) {
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            popup.pending_scroll_reveal = true;
        }
    }

    /// Renderer hook: stash the viewport height and laid-out content height so
    /// scroll clamping stays in sync with what is actually drawn.
    pub fn set_forum_post_composer_metrics(&mut self, view_height: usize, total_lines: usize) {
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            popup.scroll.set_view_height(view_height);
            popup.scroll.set_total_lines(total_lines);
        }
    }

    /// Renderer hook: when a reveal is pending, scroll just enough to show rows
    /// `[start, end)` (the focused field or cursor), then clear the request.
    pub fn reveal_forum_post_composer_rows(&mut self, start: usize, end: usize) {
        if let Some(popup) = self.popups.forum_post_composer_mut()
            && popup.pending_scroll_reveal
        {
            popup.scroll.reveal(start, end);
            popup.pending_scroll_reveal = false;
        }
    }

    pub fn forum_post_composer_view(&self) -> Option<ForumPostComposerView> {
        let popup = self.popups.forum_post_composer()?;
        let channel = self.discord.cache.channel(popup.channel_id)?;
        let editing_tags = popup.editing == Some(ForumPostComposerFieldState::Tags);
        // While the picker is open we render the snapshot order captured on
        // entry so toggling a tag does not reshuffle the list under the cursor.
        // Otherwise (collapsed summary) we sort selected tags to the top live.
        let display_ids: Vec<Id<ForumTagMarker>> = if editing_tags && !popup.tag_order.is_empty() {
            popup.tag_order.clone()
        } else {
            channel
                .available_tags
                .iter()
                .map(|tag| tag.id)
                .filter(|id| popup.selected_tag_ids.contains(id))
                .chain(
                    channel
                        .available_tags
                        .iter()
                        .map(|tag| tag.id)
                        .filter(|id| !popup.selected_tag_ids.contains(id)),
                )
                .collect()
        };
        // Once the cap is hit only the already-selected tags stay toggleable.
        // The rest are reported as not selectable so the picker can dim them.
        let cap_reached = popup.selected_tag_ids.len() >= MAX_FORUM_POST_TAGS;
        let guild_id = channel.guild_id;
        let tags = display_ids
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                let tag = channel.available_tags.iter().find(|tag| tag.id == *id)?;
                let selected = popup.selected_tag_ids.contains(&tag.id);
                let emoji = self.forum_tag_emoji_fields(tag, guild_id);
                Some(ForumPostComposerTagView {
                    name: tag.name.clone(),
                    unicode_emoji: emoji.unicode_emoji,
                    custom_emoji_url: emoji.custom_emoji_url,
                    custom_emoji_label: emoji.custom_emoji_label,
                    selected,
                    active: editing_tags && index == popup.selected_tag_index,
                    selectable: selected || !cap_reached,
                })
            })
            .collect();
        let attachments = popup
            .attachments
            .iter()
            .map(|attachment| ForumPostComposerAttachmentView {
                filename: attachment.filename.clone(),
                size_bytes: attachment.size_bytes,
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
                ForumPostComposerFieldState::Tags => ForumPostComposerFieldState::Submit,
                ForumPostComposerFieldState::Submit => ForumPostComposerFieldState::Cancel,
                // Selection stops at the last field instead of wrapping around.
                ForumPostComposerFieldState::Cancel => ForumPostComposerFieldState::Cancel,
            };
        }
    }

    pub fn cycle_forum_post_field_previous(&mut self) {
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            if popup.editing.is_some() {
                return;
            }
            popup.active_field = match popup.active_field {
                // Selection stops at the first field instead of wrapping around.
                ForumPostComposerFieldState::Title => ForumPostComposerFieldState::Title,
                ForumPostComposerFieldState::Body => ForumPostComposerFieldState::Title,
                ForumPostComposerFieldState::Attachments => ForumPostComposerFieldState::Body,
                ForumPostComposerFieldState::Tags => ForumPostComposerFieldState::Attachments,
                ForumPostComposerFieldState::Submit => ForumPostComposerFieldState::Tags,
                ForumPostComposerFieldState::Cancel => ForumPostComposerFieldState::Submit,
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
                | Some(ForumPostComposerFieldState::Submit)
                | Some(ForumPostComposerFieldState::Cancel)
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
            | Some(ForumPostComposerFieldState::Submit)
            | Some(ForumPostComposerFieldState::Cancel)
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
                    ForumPostComposerFieldState::Attachments
                    | ForumPostComposerFieldState::Tags
                    | ForumPostComposerFieldState::Submit
                    | ForumPostComposerFieldState::Cancel,
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
                    ForumPostComposerFieldState::Attachments
                    | ForumPostComposerFieldState::Tags
                    | ForumPostComposerFieldState::Submit
                    | ForumPostComposerFieldState::Cancel,
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

    pub fn move_forum_post_cursor_up(&mut self) {
        self.with_forum_post_active_text_input(|input| input.move_up());
    }

    pub fn move_forum_post_cursor_down(&mut self) {
        self.with_forum_post_active_text_input(|input| input.move_down());
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
            Some(
                ForumPostComposerFieldState::Attachments
                | ForumPostComposerFieldState::Tags
                | ForumPostComposerFieldState::Submit
                | ForumPostComposerFieldState::Cancel,
            )
            | None => {}
        }
    }

    pub fn move_forum_post_selection_down(&mut self) {
        let Some((editing, tag_count)) = self
            .popups
            .forum_post_composer()
            .map(|popup| (popup.editing, popup.tag_order.len()))
        else {
            return;
        };
        match editing {
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
        let Some(tag_id) = self
            .popups
            .forum_post_composer()
            .and_then(|popup| popup.tag_order.get(popup.selected_tag_index).copied())
        else {
            return;
        };
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            if let Some(position) = popup.selected_tag_ids.iter().position(|id| *id == tag_id) {
                popup.selected_tag_ids.remove(position);
                popup.status = None;
            } else if popup.selected_tag_ids.len() < MAX_FORUM_POST_TAGS {
                // Discord caps applied tags at five, so extra selections are ignored.
                popup.selected_tag_ids.push(tag_id);
                popup.status = None;
            }
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
        popup.editing == Some(ForumPostComposerFieldState::Body)
            && self.forum_post_composer_accepts_attachments()
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
            popup.status = None;
        }
        self.refresh_forum_post_attachment_previews();
    }

    pub fn pop_pending_forum_post_attachment(&mut self) {
        if let Some(popup) = self.popups.forum_post_composer_mut() {
            if popup.attachments.pop().is_none() {
                return;
            }
            popup.status = None;
        }
        self.refresh_forum_post_attachment_previews();
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
                ForumPostComposerFieldState::Tags => popup.selected_tag_ids.clear(),
                ForumPostComposerFieldState::Attachments => {
                    popup.attachments.clear();
                    popup.attachment_previews.clear();
                }
                ForumPostComposerFieldState::Submit | ForumPostComposerFieldState::Cancel => {}
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
            ForumPostComposerFieldState::Tags => self.start_forum_post_tag_selection(),
            ForumPostComposerFieldState::Submit => return self.save_forum_post_composer(),
            ForumPostComposerFieldState::Cancel => self.close_forum_post_composer(),
            // The attachments cell only displays previews; uploads come from
            // pasting into the body, like the main composer.
            ForumPostComposerFieldState::Attachments => {}
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
            Some(
                ForumPostComposerFieldState::Attachments
                | ForumPostComposerFieldState::Tags
                | ForumPostComposerFieldState::Submit
                | ForumPostComposerFieldState::Cancel,
            ) => {
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

    pub fn forum_post_attachment_previews(&self) -> Vec<LocalUploadPreviewView<'_>> {
        self.popups
            .forum_post_composer()
            .map(|popup| {
                popup
                    .attachment_previews
                    .iter()
                    .map(local_upload_preview_view)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(in crate::tui) fn take_pending_forum_post_attachment_preview(
        &mut self,
    ) -> Option<(usize, u64, String, MessageAttachmentUpload)> {
        if !self.show_images() {
            return None;
        }
        let popup = self.popups.forum_post_composer_mut()?;
        let preview = popup
            .attachment_previews
            .iter_mut()
            .find(|preview| matches!(preview.state, LocalUploadPreviewStatus::Pending))?;
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
        let Some(preview) = popup.attachment_previews.iter_mut().find(|preview| {
            preview.attachment_index == attachment_index && preview.generation == generation
        }) else {
            return;
        };
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
            ForumPostComposerFieldState::Attachments
            | ForumPostComposerFieldState::Tags
            | ForumPostComposerFieldState::Submit
            | ForumPostComposerFieldState::Cancel => return,
        };
        popup.editing = Some(field);
        popup.edit_input.set_value(value);
        popup.status = None;
    }

    fn start_forum_post_tag_selection(&mut self) {
        let Some(channel_id) = self
            .popups
            .forum_post_composer()
            .map(|popup| popup.channel_id)
        else {
            return;
        };
        // Snapshot the display order once on entry: selected tags first (in the
        // channel's tag order), then the rest. Keeping this fixed while the
        // picker is open stops the cursor from jumping as tags are toggled.
        let selected_ids = self
            .popups
            .forum_post_composer()
            .map(|popup| popup.selected_tag_ids.clone())
            .unwrap_or_default();
        let ordered: Vec<Id<ForumTagMarker>> = self
            .discord
            .cache
            .channel(channel_id)
            .map(|channel| {
                channel
                    .available_tags
                    .iter()
                    .map(|tag| tag.id)
                    .filter(|id| selected_ids.contains(id))
                    .chain(
                        channel
                            .available_tags
                            .iter()
                            .map(|tag| tag.id)
                            .filter(|id| !selected_ids.contains(id)),
                    )
                    .collect()
            })
            .unwrap_or_default();
        let Some(popup) = self.popups.forum_post_composer_mut() else {
            return;
        };
        if ordered.is_empty() {
            popup.status = Some("no tags available".to_owned());
            return;
        }
        popup.tag_order = ordered;
        popup.selected_tag_index = 0;
        popup.editing = Some(ForumPostComposerFieldState::Tags);
        popup.edit_input.clear();
        popup.status = None;
    }

    fn refresh_forum_post_attachment_previews(&mut self) {
        let show_images = self.show_images();
        let Some(popup) = self.popups.forum_post_composer_mut() else {
            return;
        };
        if !show_images {
            popup.attachment_previews.clear();
            return;
        }
        // Keep already-resolved previews when their attachment stays in place;
        // only newly added image attachments schedule fresh preview work.
        let mut previous = std::mem::take(&mut popup.attachment_previews);
        let mut previews = Vec::new();
        for (index, attachment) in popup
            .attachments
            .iter()
            .enumerate()
            .filter(|(_, attachment)| local_upload_preview_candidate(attachment))
        {
            if let Some(previous_index) = previous.iter().position(|preview| {
                preview.attachment_index == index && preview.filename == attachment.filename
            }) {
                previews.push(previous.remove(previous_index));
                continue;
            }
            popup.attachment_preview_generation =
                popup.attachment_preview_generation.saturating_add(1);
            previews.push(LocalUploadPreviewState {
                attachment_index: index,
                generation: popup.attachment_preview_generation,
                filename: attachment.filename.clone(),
                state: LocalUploadPreviewStatus::Pending,
            });
        }
        popup.attachment_previews = previews;
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
            ForumPostComposerFieldState::Attachments
            | ForumPostComposerFieldState::Tags
            | ForumPostComposerFieldState::Submit
            | ForumPostComposerFieldState::Cancel => {}
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
        // Forum bodies have no mentions or commands, so only `:shortcode:` emoji
        // are expanded on submit.
        let content = expand_emoji_shortcodes(popup.body.value())
            .trim()
            .to_owned();
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
            ForumPostComposerFieldState::Submit => Self::Submit,
            ForumPostComposerFieldState::Cancel => Self::Cancel,
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
        ForumPostComposerFieldState::Attachments
        | ForumPostComposerFieldState::Tags
        | ForumPostComposerFieldState::Submit
        | ForumPostComposerFieldState::Cancel => "",
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
        ForumPostComposerFieldState::Attachments
        | ForumPostComposerFieldState::Tags
        | ForumPostComposerFieldState::Submit
        | ForumPostComposerFieldState::Cancel => 0,
    }
}
