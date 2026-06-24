use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::PathBuf,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, ForumTagMarker, GuildMarker, MessageMarker, UserMarker},
};
use crate::discord::{AppCommand, MessageAttachmentUpload, ReactionEmoji};
use crate::discord::{PresenceStatus, ProfileAvatarUpload};

use crate::discord::ReactionUsersInfo;
use crate::tui::keybindings::{KeyBindings, KeyChord, LeaderShortcutItem, SelectionAction};
use crate::tui::text_input::TextInputState;

mod attachment_viewer;
mod channel_actions;
mod channel_switcher;
mod diagnostics;
mod forum_post;
mod guild_actions;
mod message_actions;
mod notification_inbox;
mod options;
mod polls;
mod reactions;
mod search;
mod thread_actions;
mod thread_edit;
mod user;

use super::{
    DashboardState, EmojiReactionItem, FocusPane, MessageUrlItem, PollVotePickerItem,
    ThreadEditField,
};
use channel_switcher::ChannelSwitcherState;
use notification_inbox::NotificationInboxState;
pub use notification_inbox::{
    NotificationInboxChannelLoad, NotificationInboxItem, NotificationInboxLoad,
    NotificationInboxMessage, NotificationInboxTab,
};
use search::SearchPopupState;

const SELECTABLE_POPUP_PAGE_STEP: usize = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LeaderMode {
    Root,
    Actions,
}

#[derive(Debug, Default)]
pub(super) struct PopupUiState {
    pub(super) modal: Option<ModalPopup>,
    /// Bumped per inbox open so a previous open's late responses are ignored.
    pub(super) inbox_request_generation: u64,
}

#[derive(Debug)]
pub(in crate::tui) struct ActionShortcutActivation {
    pub(in crate::tui) matched: bool,
    pub(in crate::tui) command: Option<AppCommand>,
}

#[derive(Debug)]
pub(super) enum ModalPopup {
    MessageActionMenu(MessageActionMenuState),
    MessageUrlPicker(MessageUrlPickerState),
    MessageConfirmation(MessageConfirmationState),
    QuitConfirmation,
    GuildLeaveConfirmation(GuildLeaveConfirmationState),
    Options(OptionsPopupState),
    AttachmentViewer(AttachmentViewerState),
    Leader(LeaderPopupState),
    UserProfile(UserProfilePopupState),
    EmojiReactionPicker(EmojiReactionPickerState),
    PollVotePicker(PollVotePickerState),
    ReactionUsers(ReactionUsersPopupState),
    DebugLog,
    Keymap(KeymapPopupState),
    ChannelSwitcher(ChannelSwitcherState),
    NotificationInbox(NotificationInboxState),
    Search(SearchPopupState),
    ForumPostComposer(ForumPostComposerState),
    ThreadEdit(ThreadEditState),
    ThreadActionMenu(ThreadActionMenuState),
    ThreadDeleteConfirmation(ThreadDeleteConfirmationState),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ActiveModalPopupKind {
    MessageActionMenu,
    MessageUrlPicker,
    MessageConfirmation,
    QuitConfirmation,
    GuildLeaveConfirmation,
    Options,
    AttachmentViewer,
    Leader,
    UserProfile,
    EmojiReactionPicker,
    PollVotePicker,
    ReactionUsers,
    DebugLog,
    KeymapHelp,
    ChannelSwitcher,
    NotificationInbox,
    Search,
    ForumPostComposer,
    ThreadEdit,
    ThreadActionMenu,
    ThreadDeleteConfirmation,
}

impl ModalPopup {
    fn kind(&self) -> ActiveModalPopupKind {
        match self {
            Self::MessageActionMenu(_) => ActiveModalPopupKind::MessageActionMenu,
            Self::MessageUrlPicker(_) => ActiveModalPopupKind::MessageUrlPicker,
            Self::MessageConfirmation(_) => ActiveModalPopupKind::MessageConfirmation,
            Self::QuitConfirmation => ActiveModalPopupKind::QuitConfirmation,
            Self::GuildLeaveConfirmation(_) => ActiveModalPopupKind::GuildLeaveConfirmation,
            Self::Options(_) => ActiveModalPopupKind::Options,
            Self::AttachmentViewer(_) => ActiveModalPopupKind::AttachmentViewer,
            Self::Leader(_) => ActiveModalPopupKind::Leader,
            Self::UserProfile(_) => ActiveModalPopupKind::UserProfile,
            Self::EmojiReactionPicker(_) => ActiveModalPopupKind::EmojiReactionPicker,
            Self::PollVotePicker(_) => ActiveModalPopupKind::PollVotePicker,
            Self::ReactionUsers(_) => ActiveModalPopupKind::ReactionUsers,
            Self::DebugLog => ActiveModalPopupKind::DebugLog,
            Self::Keymap(_) => ActiveModalPopupKind::KeymapHelp,
            Self::ChannelSwitcher(_) => ActiveModalPopupKind::ChannelSwitcher,
            Self::NotificationInbox(_) => ActiveModalPopupKind::NotificationInbox,
            Self::Search(_) => ActiveModalPopupKind::Search,
            Self::ForumPostComposer(_) => ActiveModalPopupKind::ForumPostComposer,
            Self::ThreadEdit(_) => ActiveModalPopupKind::ThreadEdit,
            Self::ThreadActionMenu(_) => ActiveModalPopupKind::ThreadActionMenu,
            Self::ThreadDeleteConfirmation(_) => ActiveModalPopupKind::ThreadDeleteConfirmation,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui::state) enum ForumPostComposerFieldState {
    Title,
    Body,
    Attachments,
    Tags,
    Submit,
    Cancel,
}

#[derive(Debug)]
pub(super) struct ForumPostComposerState {
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) title: TextInputState,
    pub(super) body: TextInputState,
    pub(super) edit_input: TextInputState,
    pub(super) active_field: ForumPostComposerFieldState,
    pub(super) editing: Option<ForumPostComposerFieldState>,
    pub(super) selected_tag_index: usize,
    /// Display order of tags while the tag picker is open. Captured on entry
    /// (selected tags first) so the cursor does not jump as tags are toggled.
    /// Indexed by `selected_tag_index`.
    pub(super) tag_order: Vec<Id<ForumTagMarker>>,
    pub(super) selected_tag_ids: Vec<Id<ForumTagMarker>>,
    /// Attachments uploaded with the post. Pasted and previewed inline with the
    /// body, mirroring the main message composer.
    pub(super) attachments: Vec<MessageAttachmentUpload>,
    pub(super) attachment_previews: Vec<super::local_upload_preview::LocalUploadPreviewState>,
    pub(super) attachment_preview_generation: u64,
    pub(super) status: Option<String>,
    /// Viewport scroll for the (possibly overflowing) composer body, driven by
    /// the scroll keys. `pending_scroll_reveal` asks the next render to bring the
    /// focused field or text cursor back into view after a focus/edit change.
    pub(super) scroll: ScrollablePopupState,
    pub(super) pending_scroll_reveal: bool,
}

impl ForumPostComposerState {
    fn new(channel_id: Id<ChannelMarker>) -> Self {
        Self {
            channel_id,
            title: TextInputState::default(),
            body: TextInputState::default(),
            edit_input: TextInputState::default(),
            active_field: ForumPostComposerFieldState::Title,
            editing: None,
            selected_tag_index: 0,
            tag_order: Vec::new(),
            selected_tag_ids: Vec::new(),
            attachments: Vec::new(),
            attachment_previews: Vec::new(),
            attachment_preview_generation: 0,
            status: None,
            scroll: ScrollablePopupState::default(),
            pending_scroll_reveal: true,
        }
    }
}

/// Settings popup for editing an existing thread (a regular thread or a forum
/// post). A leaner mirror of [`ForumPostComposerState`]: there is no body or
/// attachments, and the slow-mode and auto-archive selectors replace them. The
/// title edits inline through `edit_input` (like the composer's title), the tag
/// picker (forum posts only) reuses the same snapshot-on-entry order, and the
/// two selectors cycle their option index with the arrow keys.
#[derive(Debug)]
pub(super) struct ThreadEditState {
    pub(super) channel_id: Id<ChannelMarker>,
    /// Whether the edited thread lives under a forum channel. Tags only exist on
    /// forum posts, so for a regular thread the Tags field is hidden entirely.
    pub(super) is_forum_post: bool,
    pub(super) title: TextInputState,
    pub(super) editing_title: bool,
    pub(super) edit_input: TextInputState,
    pub(super) selected_tag_ids: Vec<Id<ForumTagMarker>>,
    /// Display order of tags while the tag picker is open. Captured on entry
    /// (selected tags first) so the cursor does not jump as tags are toggled.
    /// Indexed by `selected_tag_index`.
    pub(super) tag_order: Vec<Id<ForumTagMarker>>,
    pub(super) selected_tag_index: usize,
    pub(super) editing_tags: bool,
    /// Index into [`SLOW_MODE_OPTIONS`] for the current slow-mode value.
    pub(super) rate_limit_index: usize,
    /// Index into [`AUTO_ARCHIVE_OPTIONS`] for the current auto-archive value.
    pub(super) auto_archive_index: usize,
    /// Whether the slow-mode selector may be changed. Gated on the
    /// manage-channel permission, mirroring Discord's General settings panel.
    pub(super) can_set_slow_mode: bool,
    pub(super) active_field: ThreadEditField,
    pub(super) status: Option<String>,
    /// Viewport scroll for the (possibly overflowing) settings form, driven by
    /// the scroll keys. `pending_scroll_reveal` asks the next render to bring
    /// the focused field or text cursor back into view after a focus/edit
    /// change.
    pub(super) scroll: ScrollablePopupState,
    pub(super) pending_scroll_reveal: bool,
}

/// Standalone action menu for a focused thread (a regular thread or a forum
/// post). `Actions` is the top-level list; `MuteDuration` is the mute submenu;
/// `NotificationSettings` is the notification-level submenu. All phases carry
/// `channel_id` and `guild_id` so the actions can act on the thread directly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ThreadActionMenuState {
    Actions {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        selection: SelectablePopupState,
    },
    MuteDuration {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        selection: SelectablePopupState,
    },
    NotificationSettings {
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        selection: SelectablePopupState,
    },
}

#[derive(Debug)]
pub(super) struct LeaderPopupState {
    pub(super) mode: LeaderMode,
    pub(super) keymap_prefix: Vec<KeyChord>,
    pub(super) action: Option<LeaderActionState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum LeaderActionState {
    Message(MessageActionMenuState),
    Guild(GuildLeaderActionState),
    Channel(ChannelLeaderActionState),
    Member(MemberLeaderActionState),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct SelectablePopupState {
    selected: usize,
}

impl SelectablePopupState {
    pub(super) fn selected(&self) -> usize {
        self.selected
    }

    pub(super) fn selected_for_len(&self, len: usize) -> usize {
        self.selected.min(len.saturating_sub(1))
    }

    pub(super) fn select(&mut self, row: usize) {
        self.selected = row;
    }

    pub(super) fn move_down(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        self.selected = self.selected.saturating_add(1).min(len - 1);
    }

    pub(super) fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub(super) fn page(&mut self, len: usize, action: SelectionAction) {
        match action {
            SelectionAction::Next => {
                if len > 0 {
                    self.selected = self
                        .selected
                        .saturating_add(SELECTABLE_POPUP_PAGE_STEP)
                        .min(len - 1);
                }
            }
            SelectionAction::Previous => {
                self.selected = self.selected.saturating_sub(SELECTABLE_POPUP_PAGE_STEP);
            }
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ScrollablePopupState {
    scroll: usize,
    view_height: usize,
    total_lines: usize,
}

impl ScrollablePopupState {
    pub(super) fn scroll(&self) -> usize {
        self.scroll
    }

    pub(super) fn set_view_height(&mut self, height: usize) {
        self.view_height = height;
        self.clamp_scroll();
    }

    pub(super) fn set_total_lines(&mut self, total_lines: usize) {
        self.total_lines = total_lines;
        self.clamp_scroll();
    }

    pub(super) fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
        self.clamp_scroll();
    }

    pub(super) fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub(super) fn page_down(&mut self) {
        self.scroll = self.scroll.saturating_add((self.view_height / 2).max(1));
        self.clamp_scroll();
    }

    pub(super) fn page_up(&mut self) {
        self.scroll = self.scroll.saturating_sub((self.view_height / 2).max(1));
    }

    pub(super) fn page(&mut self, action: SelectionAction) {
        match action {
            SelectionAction::Next => self.page_down(),
            SelectionAction::Previous => self.page_up(),
        }
    }

    /// Adjust the scroll offset just enough to bring rows `[start, end)` into
    /// the viewport, without recentering when the range is already visible.
    pub(super) fn reveal(&mut self, start: usize, end: usize) {
        if start < self.scroll {
            self.scroll = start;
        } else if self.view_height > 0 && end > self.scroll + self.view_height {
            self.scroll = end.saturating_sub(self.view_height);
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let visible = self.view_height.min(self.total_lines);
        self.scroll = self.scroll.min(self.total_lines.saturating_sub(visible));
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MessageActionMenuState {
    pub(super) selection: SelectablePopupState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct KeymapPopupState {
    pub(super) scroll: ScrollablePopupState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageUrlPickerState {
    pub(super) selection: SelectablePopupState,
    pub(super) items: Vec<MessageUrlItem>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MessageConfirmationKind {
    Delete,
    RemoveEmbeds,
    Pin { pinned: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MessageConfirmationState {
    pub(super) kind: MessageConfirmationKind,
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
    pub(super) author: String,
    pub(super) content: Option<String>,
}

impl MessageConfirmationState {
    pub(super) fn delete(
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        author: String,
        content: Option<String>,
    ) -> Self {
        Self {
            kind: MessageConfirmationKind::Delete,
            channel_id,
            message_id,
            author,
            content,
        }
    }

    pub(super) fn pin(
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
        author: String,
        content: Option<String>,
    ) -> Self {
        Self {
            kind: MessageConfirmationKind::Pin { pinned },
            channel_id,
            message_id,
            author,
            content,
        }
    }

    pub(super) fn remove_embeds(
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        author: String,
        content: Option<String>,
    ) -> Self {
        Self {
            kind: MessageConfirmationKind::RemoveEmbeds,
            channel_id,
            message_id,
            author,
            content,
        }
    }
}

impl MessageConfirmationKind {
    pub(in crate::tui) fn title(self) -> &'static str {
        match self {
            Self::Delete => "Delete message?",
            Self::RemoveEmbeds => "Remove embeds?",
            Self::Pin { pinned: true } => "Pin message?",
            Self::Pin { pinned: false } => "Unpin message?",
        }
    }

    pub(in crate::tui) fn prompt(self) -> String {
        match self {
            Self::Delete => "Delete this message?".to_owned(),
            Self::RemoveEmbeds => "Remove embeds from this message?".to_owned(),
            Self::Pin { pinned: true } => "Pin this message?".to_owned(),
            Self::Pin { pinned: false } => "Unpin this message?".to_owned(),
        }
    }

    pub(in crate::tui) fn action_label(self) -> String {
        match self {
            Self::Delete => "delete".to_owned(),
            Self::RemoveEmbeds => "Remove embeds".to_owned(),
            Self::Pin { pinned: true } => "Pin message".to_owned(),
            Self::Pin { pinned: false } => "Unpin message".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildLeaveConfirmationState {
    pub(super) guild_id: Id<GuildMarker>,
    pub(super) name: String,
}

/// Confirmation gate before permanently deleting a thread. Carries the thread
/// id to delete, its display name for the prompt, and whether it is a forum post
/// so the prompt reads "post" instead of "thread".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadDeleteConfirmationState {
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) name: String,
    pub(super) is_forum_post: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionsCategory {
    Display,
    Composer,
    Notifications,
    Voice,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct OptionsPopupState {
    pub(super) selection: SelectablePopupState,
    pub(super) category: Option<OptionsCategory>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AttachmentViewerState {
    pub(super) message_id: Id<MessageMarker>,
    pub(super) selection: SelectablePopupState,
    pub(super) zoom: AttachmentViewerZoom,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AttachmentViewerZoom {
    #[default]
    Default,
    Large,
    Fullscreen,
}

impl AttachmentViewerZoom {
    pub(super) fn zoom_in(self) -> Self {
        match self {
            Self::Default => Self::Large,
            Self::Large | Self::Fullscreen => Self::Fullscreen,
        }
    }

    pub(super) fn zoom_out(self) -> Self {
        match self {
            Self::Fullscreen => Self::Large,
            Self::Large | Self::Default => Self::Default,
        }
    }

    pub(super) fn toggle_fullscreen(self) -> Self {
        match self {
            Self::Fullscreen => Self::Default,
            Self::Default | Self::Large => Self::Fullscreen,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum GuildLeaderActionState {
    Actions { selection: SelectablePopupState },
    MuteDuration { selection: SelectablePopupState },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct UserProfilePopupState {
    pub(super) user_id: Id<UserMarker>,
    pub(super) guild_id: Option<Id<GuildMarker>>,
    pub(super) load_error: Option<String>,
    pub(super) settings: UserProfileSettingsState,
    /// First visible row of the popup body. Behaves like the channel/guild
    /// pane scroll: j/k and the mouse wheel adjust this, never moving a
    /// cursor that the renderer would have to chase.
    pub(super) scroll: ScrollablePopupState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UserProfileSettingsTab {
    #[default]
    Global,
    Guild,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserProfileSettingsField {
    CurrentStatus,
    ManualActivity,
    GlobalDisplayName,
    GlobalPronouns,
    GlobalAvatarPath,
    GuildNickname,
    GuildPronouns,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct UserProfileSettingsState {
    pub(super) tab: UserProfileSettingsTab,
    pub(super) selected_global: usize,
    pub(super) selected_guild: usize,
    pub(super) editing: Option<UserProfileSettingsField>,
    pub(super) edit_input: TextInputState,
    pub(super) global_display_name: Option<String>,
    pub(super) global_pronouns: Option<String>,
    pub(super) global_avatar_path: Option<String>,
    pub(super) global_avatar_upload: Option<ProfileAvatarUpload>,
    pub(super) global_avatar_preview_key: Option<String>,
    pub(super) presence_status: Option<PresenceStatus>,
    pub(super) manual_activity: Option<String>,
    pub(super) status_picker: Option<SelectablePopupState>,
    pub(super) guild_nickname: Option<String>,
    pub(super) guild_pronouns: Option<String>,
    pub(super) saving: bool,
    pub(super) status: Option<String>,
}

impl UserProfileSettingsState {
    const GLOBAL_FIELDS: [UserProfileSettingsField; 5] = [
        UserProfileSettingsField::GlobalDisplayName,
        UserProfileSettingsField::GlobalPronouns,
        UserProfileSettingsField::GlobalAvatarPath,
        UserProfileSettingsField::CurrentStatus,
        UserProfileSettingsField::ManualActivity,
    ];
    const GUILD_FIELDS: [UserProfileSettingsField; 2] = [
        UserProfileSettingsField::GuildNickname,
        UserProfileSettingsField::GuildPronouns,
    ];

    pub(super) fn active_field(&self) -> UserProfileSettingsField {
        match self.tab {
            UserProfileSettingsTab::Global => {
                Self::GLOBAL_FIELDS[self.selected_global.min(Self::GLOBAL_FIELDS.len() - 1)]
            }
            UserProfileSettingsTab::Guild => {
                Self::GUILD_FIELDS[self.selected_guild.min(Self::GUILD_FIELDS.len() - 1)]
            }
        }
    }

    pub(super) fn next_field(&mut self) {
        match self.tab {
            UserProfileSettingsTab::Global => {
                self.selected_global = (self.selected_global + 1) % Self::GLOBAL_FIELDS.len();
            }
            UserProfileSettingsTab::Guild => {
                self.selected_guild = (self.selected_guild + 1) % Self::GUILD_FIELDS.len();
            }
        }
    }

    pub(super) fn previous_field(&mut self) {
        match self.tab {
            UserProfileSettingsTab::Global => {
                self.selected_global = if self.selected_global == 0 {
                    Self::GLOBAL_FIELDS.len() - 1
                } else {
                    self.selected_global - 1
                };
            }
            UserProfileSettingsTab::Guild => {
                self.selected_guild = if self.selected_guild == 0 {
                    Self::GUILD_FIELDS.len() - 1
                } else {
                    self.selected_guild - 1
                };
            }
        }
    }

    pub(super) fn set_field_value(&mut self, field: UserProfileSettingsField, value: String) {
        match field {
            UserProfileSettingsField::CurrentStatus => {}
            UserProfileSettingsField::ManualActivity => self.manual_activity = Some(value),
            UserProfileSettingsField::GlobalDisplayName => self.global_display_name = Some(value),
            UserProfileSettingsField::GlobalPronouns => self.global_pronouns = Some(value),
            UserProfileSettingsField::GlobalAvatarPath => {
                let trimmed = value.trim();
                let upload = (!trimmed.is_empty())
                    .then(|| ProfileAvatarUpload::from_path(PathBuf::from(trimmed)));
                self.global_avatar_preview_key = upload.as_ref().map(profile_avatar_preview_key);
                self.global_avatar_path = Some(value);
                self.global_avatar_upload = None;
            }
            UserProfileSettingsField::GuildNickname => self.guild_nickname = Some(value),
            UserProfileSettingsField::GuildPronouns => self.guild_pronouns = Some(value),
        }
    }

    pub(super) fn set_avatar_upload(&mut self, upload: ProfileAvatarUpload) {
        self.global_avatar_preview_key = Some(profile_avatar_preview_key(&upload));
        self.global_avatar_path = None;
        self.global_avatar_upload = Some(upload);
    }

    pub(super) fn pending_global_avatar_upload(&self) -> Option<ProfileAvatarUpload> {
        self.global_avatar_upload.clone().or_else(|| {
            self.global_avatar_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .map(ProfileAvatarUpload::from_path)
        })
    }

    pub(super) fn pending_global_avatar_preview_key(&self) -> Option<&str> {
        self.global_avatar_preview_key.as_deref()
    }

    pub(super) fn clear_after_save(&mut self) {
        self.global_display_name = None;
        self.global_pronouns = None;
        self.global_avatar_path = None;
        self.global_avatar_upload = None;
        self.global_avatar_preview_key = None;
        self.guild_nickname = None;
        self.guild_pronouns = None;
        self.editing = None;
        self.edit_input.clear();
        self.saving = false;
        self.status = Some("Saved profile changes".to_owned());
    }
}

fn profile_avatar_preview_key(upload: &ProfileAvatarUpload) -> String {
    let mut hasher = DefaultHasher::new();
    upload.filename.hash(&mut hasher);
    upload.size_bytes.hash(&mut hasher);
    if let Some(path) = upload.path() {
        path.hash(&mut hasher);
    }
    if let Some(bytes) = upload.bytes() {
        bytes.hash(&mut hasher);
    }
    format!("concord-profile-avatar-preview://{:016x}", hasher.finish())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MemberLeaderActionState {
    pub(super) user_id: Id<UserMarker>,
    pub(super) guild_id: Option<Id<GuildMarker>>,
    pub(super) selection: SelectablePopupState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ChannelLeaderActionState {
    Actions {
        channel_id: Id<ChannelMarker>,
        selection: SelectablePopupState,
    },
    MuteDuration {
        channel_id: Id<ChannelMarker>,
        selection: SelectablePopupState,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmojiReactionPickerState {
    pub(super) selection: SelectablePopupState,
    pub(super) filter: Option<String>,
    pub(super) filter_editing: bool,
    pub(super) items: Vec<EmojiReactionItem>,
    pub(super) filtered_items: Vec<EmojiReactionItem>,
    pub(super) existing_reactions: Vec<ReactionEmoji>,
    pub(super) own_reactions: Vec<ReactionEmoji>,
    pub(super) guild_id: Option<Id<GuildMarker>>,
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PollVotePickerState {
    pub(super) selection: SelectablePopupState,
    pub(super) allow_multiselect: bool,
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
    pub(super) answers: Vec<PollVotePickerItem>,
}

impl PollVotePickerState {
    pub fn answers(&self) -> &[PollVotePickerItem] {
        &self.answers
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReactionUsersPopupState {
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
    pub(super) reactions: Vec<ReactionUsersInfo>,
    pub(super) scroll: ScrollablePopupState,
}

impl ReactionUsersPopupState {
    pub(super) fn new(
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        reactions: Vec<ReactionUsersInfo>,
    ) -> Self {
        let mut scroll = ScrollablePopupState::default();
        scroll.set_total_lines(reaction_users_line_count(&reactions));
        Self {
            channel_id,
            message_id,
            reactions,
            scroll,
        }
    }

    pub fn reactions(&self) -> &[ReactionUsersInfo] {
        &self.reactions
    }

    pub fn scroll(&self) -> usize {
        self.scroll.scroll()
    }

    /// Total renderable data lines for the current reactions, mirroring the
    /// layout produced by `reaction_users_popup_data_lines` in `ui.rs` so the
    /// scroll bound here stays in sync with what the user actually sees.
    pub fn data_line_count(&self) -> usize {
        reaction_users_line_count(&self.reactions)
    }
}

fn reaction_users_line_count(reactions: &[ReactionUsersInfo]) -> usize {
    if reactions.is_empty() {
        return 1;
    }
    reactions
        .iter()
        .map(|reaction| 1 + reaction.users.len().max(1))
        .sum()
}

macro_rules! modal_popup_accessors {
    ($get:ident, $get_mut:ident, $variant:ident, $state:ty, $binding:ident) => {
        pub(super) fn $get(&self) -> Option<&$state> {
            match &self.modal {
                Some(ModalPopup::$variant($binding)) => Some($binding),
                _ => None,
            }
        }

        pub(super) fn $get_mut(&mut self) -> Option<&mut $state> {
            match &mut self.modal {
                Some(ModalPopup::$variant($binding)) => Some($binding),
                _ => None,
            }
        }
    };
}

impl PopupUiState {
    pub(super) fn clear_modal(&mut self) {
        self.modal = None;
    }

    pub(super) fn message_action_menu(&self) -> Option<&MessageActionMenuState> {
        match &self.modal {
            Some(ModalPopup::MessageActionMenu(menu)) => Some(menu),
            Some(ModalPopup::Leader(LeaderPopupState {
                action: Some(LeaderActionState::Message(menu)),
                ..
            })) => Some(menu),
            _ => None,
        }
    }

    pub(super) fn message_action_menu_mut(&mut self) -> Option<&mut MessageActionMenuState> {
        match &mut self.modal {
            Some(ModalPopup::MessageActionMenu(menu)) => Some(menu),
            Some(ModalPopup::Leader(LeaderPopupState {
                action: Some(LeaderActionState::Message(menu)),
                ..
            })) => Some(menu),
            _ => None,
        }
    }

    modal_popup_accessors!(
        message_url_picker,
        message_url_picker_mut,
        MessageUrlPicker,
        MessageUrlPickerState,
        picker
    );

    pub(super) fn message_confirmation(&self) -> Option<&MessageConfirmationState> {
        match &self.modal {
            Some(ModalPopup::MessageConfirmation(confirmation)) => Some(confirmation),
            _ => None,
        }
    }

    pub(super) fn take_message_confirmation(&mut self) -> Option<MessageConfirmationState> {
        match self.modal.take() {
            Some(ModalPopup::MessageConfirmation(confirmation)) => Some(confirmation),
            other => {
                self.modal = other;
                None
            }
        }
    }

    pub(super) fn guild_leave_confirmation(&self) -> Option<&GuildLeaveConfirmationState> {
        match &self.modal {
            Some(ModalPopup::GuildLeaveConfirmation(confirmation)) => Some(confirmation),
            _ => None,
        }
    }

    pub(super) fn take_guild_leave_confirmation(&mut self) -> Option<GuildLeaveConfirmationState> {
        match self.modal.take() {
            Some(ModalPopup::GuildLeaveConfirmation(confirmation)) => Some(confirmation),
            other => {
                self.modal = other;
                None
            }
        }
    }

    pub(super) fn thread_delete_confirmation(&self) -> Option<&ThreadDeleteConfirmationState> {
        match &self.modal {
            Some(ModalPopup::ThreadDeleteConfirmation(confirmation)) => Some(confirmation),
            _ => None,
        }
    }

    pub(super) fn take_thread_delete_confirmation(
        &mut self,
    ) -> Option<ThreadDeleteConfirmationState> {
        match self.modal.take() {
            Some(ModalPopup::ThreadDeleteConfirmation(confirmation)) => Some(confirmation),
            other => {
                self.modal = other;
                None
            }
        }
    }

    modal_popup_accessors!(
        options_popup,
        options_popup_mut,
        Options,
        OptionsPopupState,
        popup
    );
    modal_popup_accessors!(
        attachment_viewer,
        attachment_viewer_mut,
        AttachmentViewer,
        AttachmentViewerState,
        viewer
    );
    modal_popup_accessors!(leader, leader_mut, Leader, LeaderPopupState, leader);

    pub(super) fn guild_leader_action(&self) -> Option<&GuildLeaderActionState> {
        match self.leader().and_then(|leader| leader.action.as_ref()) {
            Some(LeaderActionState::Guild(action)) => Some(action),
            _ => None,
        }
    }

    pub(super) fn guild_leader_action_mut(&mut self) -> Option<&mut GuildLeaderActionState> {
        match self.leader_mut().and_then(|leader| leader.action.as_mut()) {
            Some(LeaderActionState::Guild(action)) => Some(action),
            _ => None,
        }
    }

    pub(super) fn channel_leader_action(&self) -> Option<&ChannelLeaderActionState> {
        match self.leader().and_then(|leader| leader.action.as_ref()) {
            Some(LeaderActionState::Channel(action)) => Some(action),
            _ => None,
        }
    }

    pub(super) fn channel_leader_action_mut(&mut self) -> Option<&mut ChannelLeaderActionState> {
        match self.leader_mut().and_then(|leader| leader.action.as_mut()) {
            Some(LeaderActionState::Channel(action)) => Some(action),
            _ => None,
        }
    }

    pub(super) fn member_leader_action(&self) -> Option<&MemberLeaderActionState> {
        match self.leader().and_then(|leader| leader.action.as_ref()) {
            Some(LeaderActionState::Member(action)) => Some(action),
            _ => None,
        }
    }

    pub(super) fn member_leader_action_mut(&mut self) -> Option<&mut MemberLeaderActionState> {
        match self.leader_mut().and_then(|leader| leader.action.as_mut()) {
            Some(LeaderActionState::Member(action)) => Some(action),
            _ => None,
        }
    }

    modal_popup_accessors!(
        user_profile_popup,
        user_profile_popup_mut,
        UserProfile,
        UserProfilePopupState,
        popup
    );
    modal_popup_accessors!(
        emoji_reaction_picker,
        emoji_reaction_picker_mut,
        EmojiReactionPicker,
        EmojiReactionPickerState,
        picker
    );
    modal_popup_accessors!(
        poll_vote_picker,
        poll_vote_picker_mut,
        PollVotePicker,
        PollVotePickerState,
        picker
    );

    pub(super) fn take_poll_vote_picker(&mut self) -> Option<PollVotePickerState> {
        match self.modal.take() {
            Some(ModalPopup::PollVotePicker(picker)) => Some(picker),
            other => {
                self.modal = other;
                None
            }
        }
    }

    modal_popup_accessors!(
        reaction_users_popup,
        reaction_users_popup_mut,
        ReactionUsers,
        ReactionUsersPopupState,
        popup
    );
    modal_popup_accessors!(
        keymap_popup,
        keymap_popup_mut,
        Keymap,
        KeymapPopupState,
        popup
    );
    modal_popup_accessors!(
        channel_switcher,
        channel_switcher_mut,
        ChannelSwitcher,
        ChannelSwitcherState,
        switcher
    );
    modal_popup_accessors!(
        notification_inbox,
        notification_inbox_mut,
        NotificationInbox,
        NotificationInboxState,
        inbox
    );
    modal_popup_accessors!(
        search_popup,
        search_popup_mut,
        Search,
        SearchPopupState,
        search
    );
    modal_popup_accessors!(
        forum_post_composer,
        forum_post_composer_mut,
        ForumPostComposer,
        ForumPostComposerState,
        composer
    );
    modal_popup_accessors!(
        thread_edit,
        thread_edit_mut,
        ThreadEdit,
        ThreadEditState,
        popup
    );
    modal_popup_accessors!(
        thread_action_menu,
        thread_action_menu_mut,
        ThreadActionMenu,
        ThreadActionMenuState,
        menu
    );
}

impl DashboardState {
    pub(in crate::tui) fn active_modal_popup_kind(&self) -> Option<ActiveModalPopupKind> {
        self.popups.modal.as_ref().map(ModalPopup::kind)
    }

    pub(in crate::tui) fn is_active_modal_popup(&self, kind: ActiveModalPopupKind) -> bool {
        self.active_modal_popup_kind() == Some(kind)
    }

    pub(in crate::tui) fn is_message_action_context_active(&self) -> bool {
        self.popups.message_action_menu().is_some()
    }

    pub fn is_leader_active(&self) -> bool {
        self.popups.leader().is_some()
    }

    pub fn is_leader_action_mode(&self) -> bool {
        self.popups
            .leader()
            .is_some_and(|leader| leader.mode == LeaderMode::Actions)
    }

    pub fn open_leader(&mut self) {
        self.popups.modal = Some(ModalPopup::Leader(LeaderPopupState {
            mode: LeaderMode::Root,
            keymap_prefix: self.options.key_bindings.leader_keymap_prefix(),
            action: None,
        }));
    }

    pub(in crate::tui) fn open_keymap_prefix(&mut self, prefix: Vec<KeyChord>) {
        self.popups.modal = Some(ModalPopup::Leader(LeaderPopupState {
            mode: LeaderMode::Root,
            keymap_prefix: prefix,
            action: None,
        }));
    }

    pub fn close_leader(&mut self) {
        if self.popups.leader().is_some() {
            self.popups.clear_modal();
        }
    }

    pub(in crate::tui) fn leader_keymap_prefix(&self) -> &[KeyChord] {
        self.popups
            .leader()
            .map(|leader| leader.keymap_prefix.as_slice())
            .unwrap_or_default()
    }

    pub(in crate::tui) fn push_leader_keymap_key(&mut self, key: KeyChord) {
        if let Some(leader) = self.popups.leader_mut() {
            leader.keymap_prefix.push(key);
        }
    }

    pub fn leader_keymap_shortcuts(&self) -> Vec<LeaderShortcutItem> {
        self.options
            .key_bindings
            .leader_keymap_children(self.leader_keymap_prefix())
    }

    pub(in crate::tui) fn leader_keymap_title(&self) -> String {
        self.options
            .key_bindings
            .keymap_prefix_title(self.leader_keymap_prefix())
    }

    pub fn open_leader_actions_for_focused_target(&mut self) {
        self.close_all_action_contexts();
        // A focused forum post opens its own standalone action menu instead of
        // the (empty) message action context, since the messages pane is then
        // showing forum post cards rather than messages.
        if self.open_selected_thread_actions() {
            return;
        }
        let action = match self.navigation.focus {
            FocusPane::Guilds => self
                .selected_guild_action_context()
                .map(LeaderActionState::Guild),
            FocusPane::Channels => self
                .selected_channel_action_context()
                .map(LeaderActionState::Channel),
            FocusPane::Messages => self
                .selected_message_state()
                .map(|_| LeaderActionState::Message(MessageActionMenuState::default())),
            FocusPane::Members => self
                .selected_member_action_context()
                .map(LeaderActionState::Member),
        };
        self.popups.modal = Some(ModalPopup::Leader(LeaderPopupState {
            mode: LeaderMode::Actions,
            keymap_prefix: Vec::new(),
            action,
        }));
    }

    pub fn close_all_action_contexts(&mut self) {
        if matches!(self.popups.modal, Some(ModalPopup::MessageActionMenu(_)))
            || self.is_leader_action_mode()
        {
            self.popups.clear_modal();
        }
    }

    pub fn open_quit_confirmation(&mut self) {
        self.popups.modal = Some(ModalPopup::QuitConfirmation);
    }

    pub fn close_quit_confirmation(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::QuitConfirmation) {
            self.popups.clear_modal();
        }
    }

    pub fn confirm_quit(&mut self) {
        self.close_quit_confirmation();
        self.quit();
    }

    pub(in crate::tui) fn page_active_popup_down(&mut self) -> bool {
        self.page_active_popup(SelectionAction::Next)
    }

    pub(in crate::tui) fn page_active_popup_up(&mut self) -> bool {
        self.page_active_popup(SelectionAction::Previous)
    }

    fn page_active_popup(&mut self, action: SelectionAction) -> bool {
        match self.active_modal_popup_kind() {
            Some(ActiveModalPopupKind::KeymapHelp) => {
                if let Some(popup) = self.popups.keymap_popup_mut() {
                    popup.scroll.page(action);
                }
                true
            }
            Some(ActiveModalPopupKind::ReactionUsers) => {
                if let Some(popup) = self.popups.reaction_users_popup_mut() {
                    popup.scroll.page(action);
                }
                true
            }
            Some(ActiveModalPopupKind::UserProfile) => {
                if let Some(popup) = self.popups.user_profile_popup_mut() {
                    popup.scroll.page(action);
                }
                true
            }
            Some(ActiveModalPopupKind::Options) => {
                let len = self.options_popup_item_count();
                if let Some(popup) = self.popups.options_popup_mut() {
                    popup.selection.page(len, action);
                }
                true
            }
            Some(ActiveModalPopupKind::ChannelSwitcher) => {
                self.page_channel_switcher_selection(action);
                true
            }
            Some(ActiveModalPopupKind::NotificationInbox) => {
                self.page_notification_inbox_selection(action);
                true
            }
            Some(ActiveModalPopupKind::PollVotePicker) => {
                if let Some(picker) = self.popups.poll_vote_picker_mut() {
                    picker.selection.page(picker.answers.len(), action);
                }
                true
            }
            Some(ActiveModalPopupKind::EmojiReactionPicker) => {
                let len = self.filtered_emoji_reaction_items().len();
                if let Some(picker) = self.popups.emoji_reaction_picker_mut() {
                    picker.selection.page(len, action);
                }
                true
            }
            Some(ActiveModalPopupKind::MessageUrlPicker) => {
                if let Some(picker) = self.popups.message_url_picker_mut() {
                    picker.selection.page(picker.items.len(), action);
                }
                true
            }
            Some(ActiveModalPopupKind::MessageActionMenu) => {
                let len = self.selected_message_action_items().len();
                if let Some(menu) = self.popups.message_action_menu_mut() {
                    menu.selection.page(len, action);
                }
                true
            }
            Some(ActiveModalPopupKind::MessageConfirmation)
            | Some(ActiveModalPopupKind::QuitConfirmation)
            | Some(ActiveModalPopupKind::GuildLeaveConfirmation)
            | Some(ActiveModalPopupKind::AttachmentViewer)
            // Leader action popups are shortcut-only. They can render message
            // actions, but they intentionally do not expose selection paging.
            | Some(ActiveModalPopupKind::Leader)
            | Some(ActiveModalPopupKind::DebugLog)
            | Some(ActiveModalPopupKind::Search)
            | Some(ActiveModalPopupKind::ForumPostComposer)
            | Some(ActiveModalPopupKind::ThreadEdit)
            | Some(ActiveModalPopupKind::ThreadActionMenu)
            | Some(ActiveModalPopupKind::ThreadDeleteConfirmation)
            | None => false,
        }
    }

    pub fn is_any_action_context_active(&self) -> bool {
        self.popups.message_action_menu().is_some()
            || self.popups.guild_leader_action().is_some()
            || self.popups.channel_leader_action().is_some()
            || self.popups.member_leader_action().is_some()
    }

    pub(in crate::tui) fn activate_active_action_shortcut(
        &mut self,
        shortcut: KeyChord,
    ) -> ActionShortcutActivation {
        if self.message_action_shortcut_matches(shortcut) {
            return ActionShortcutActivation {
                matched: true,
                command: self.activate_message_action_shortcut(shortcut),
            };
        }
        if self.channel_action_shortcut_matches(shortcut) {
            return ActionShortcutActivation {
                matched: true,
                command: self.activate_channel_action_shortcut(shortcut),
            };
        }
        if self.guild_action_shortcut_matches(shortcut) {
            return ActionShortcutActivation {
                matched: true,
                command: self.activate_guild_action_shortcut(shortcut),
            };
        }
        if self.member_action_shortcut_matches(shortcut) {
            return ActionShortcutActivation {
                matched: true,
                command: self.activate_member_action_shortcut(shortcut),
            };
        }
        ActionShortcutActivation {
            matched: false,
            command: None,
        }
    }

    pub(in crate::tui) fn message_action_shortcut_matches(&self, shortcut: KeyChord) -> bool {
        if !self.is_message_action_context_active() {
            return false;
        }
        let actions = self.selected_message_action_items();
        action_shortcut_matches(
            self.key_bindings(),
            &actions,
            shortcut,
            |key_bindings, actions, index| key_bindings.message_action_shortcuts(actions, index),
            |action| action.enabled,
        )
    }

    pub(in crate::tui) fn thread_action_shortcut_matches(&self, shortcut: KeyChord) -> bool {
        // Shortcuts only act on the top-level action list; the mute-duration and
        // notification submenus navigate by selection only.
        if !matches!(
            self.popups.thread_action_menu(),
            Some(ThreadActionMenuState::Actions { .. })
        ) {
            return false;
        }
        let actions = self.selected_thread_action_items();
        action_shortcut_matches(
            self.key_bindings(),
            &actions,
            shortcut,
            |key_bindings, actions, index| key_bindings.thread_action_shortcuts(actions, index),
            |action| action.enabled,
        )
    }

    fn channel_action_shortcut_matches(&self, shortcut: KeyChord) -> bool {
        if !self.is_channel_leader_action_active() {
            return false;
        }
        if self.is_channel_action_mute_duration_phase() {
            return indexed_shortcut_matches(
                self.key_bindings(),
                shortcut,
                self.selected_channel_mute_duration_items().len(),
            );
        }
        let actions = self.selected_channel_action_items();
        action_shortcut_matches(
            self.key_bindings(),
            &actions,
            shortcut,
            |key_bindings, actions, index| key_bindings.channel_action_shortcuts(actions, index),
            |action| action.enabled,
        )
    }

    fn guild_action_shortcut_matches(&self, shortcut: KeyChord) -> bool {
        if !self.is_guild_leader_action_active() {
            return false;
        }
        if self.is_guild_action_mute_duration_phase() {
            return indexed_shortcut_matches(
                self.key_bindings(),
                shortcut,
                self.selected_guild_mute_duration_items().len(),
            );
        }
        let actions = self.selected_guild_action_items();
        action_shortcut_matches(
            self.key_bindings(),
            &actions,
            shortcut,
            |key_bindings, actions, index| key_bindings.guild_action_shortcuts(actions, index),
            |action| action.enabled,
        )
    }

    fn member_action_shortcut_matches(&self, shortcut: KeyChord) -> bool {
        if !self.is_member_leader_action_active() {
            return false;
        }
        let actions = self.selected_member_action_items();
        action_shortcut_matches(
            self.key_bindings(),
            &actions,
            shortcut,
            |key_bindings, actions, index| key_bindings.member_action_shortcuts(actions, index),
            |action| action.enabled,
        )
    }
}

fn action_shortcut_matches<A>(
    key_bindings: &KeyBindings,
    actions: &[A],
    shortcut: KeyChord,
    shortcuts: impl Fn(&KeyBindings, &[A], usize) -> Vec<KeyChord>,
    is_enabled: impl Fn(&A) -> bool,
) -> bool {
    key_bindings
        .matching_action_shortcut_index(actions, shortcut, shortcuts, is_enabled)
        .is_some()
}

fn indexed_shortcut_matches(key_bindings: &KeyBindings, shortcut: KeyChord, len: usize) -> bool {
    key_bindings
        .matching_indexed_shortcut_index(shortcut, len)
        .is_some()
}

impl DashboardState {
    pub fn is_channel_leader_action_active(&self) -> bool {
        self.popups.channel_leader_action().is_some()
    }

    pub fn is_guild_leader_action_active(&self) -> bool {
        self.popups.guild_leader_action().is_some()
    }

    pub fn is_channel_action_mute_duration_phase(&self) -> bool {
        matches!(
            self.popups.channel_leader_action(),
            Some(ChannelLeaderActionState::MuteDuration { .. })
        )
    }

    pub fn is_guild_action_mute_duration_phase(&self) -> bool {
        matches!(
            self.popups.guild_leader_action(),
            Some(GuildLeaderActionState::MuteDuration { .. })
        )
    }
}
