use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::PathBuf,
};

use crate::discord::ReactionEmoji;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
};
use crate::discord::{PresenceStatus, ProfileAvatarUpload};

use crate::discord::ReactionUsersInfo;
use crate::tui::keybindings::{KeyChord, LeaderShortcutItem, SelectionAction};

mod attachment_viewer;
mod channel_actions;
mod channel_switcher;
mod diagnostics;
mod guild_actions;
mod message_actions;
mod options;
mod polls;
mod reactions;
mod search;
mod user;

use super::{DashboardState, EmojiReactionItem, FocusPane, MessageUrlItem, PollVotePickerItem};
use channel_switcher::ChannelSwitcherState;
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
}

#[derive(Debug)]
pub(super) enum ModalPopup {
    MessageActionMenu(MessageActionMenuState),
    MessageUrlPicker(MessageUrlPickerState),
    MessageDeleteConfirmation(MessageDeleteConfirmationState),
    MessagePinConfirmation(MessagePinConfirmationState),
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
    Search(SearchPopupState),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ActiveModalPopupKind {
    MessageActionMenu,
    MessageUrlPicker,
    MessageDeleteConfirmation,
    MessagePinConfirmation,
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
    Search,
}

impl ModalPopup {
    fn kind(&self) -> ActiveModalPopupKind {
        match self {
            Self::MessageActionMenu(_) => ActiveModalPopupKind::MessageActionMenu,
            Self::MessageUrlPicker(_) => ActiveModalPopupKind::MessageUrlPicker,
            Self::MessageDeleteConfirmation(_) => ActiveModalPopupKind::MessageDeleteConfirmation,
            Self::MessagePinConfirmation(_) => ActiveModalPopupKind::MessagePinConfirmation,
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
            Self::Search(_) => ActiveModalPopupKind::Search,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivePopupPageTarget {
    Scrollable(ScrollablePopupPageTarget),
    Selectable(SelectablePopupPageTarget),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScrollablePopupPageTarget {
    Keymap,
    ReactionUsers,
    UserProfile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectablePopupPageTarget {
    Options,
    ChannelSwitcher,
    PollVotePicker,
    EmojiReactionPicker,
    MessageUrlPicker,
    MessageActionMenu,
}

impl ActivePopupPageTarget {
    fn from_modal(modal: &ModalPopup) -> Option<Self> {
        Some(match modal {
            ModalPopup::Keymap(_) => Self::Scrollable(ScrollablePopupPageTarget::Keymap),
            ModalPopup::ReactionUsers(_) => {
                Self::Scrollable(ScrollablePopupPageTarget::ReactionUsers)
            }
            ModalPopup::UserProfile(_) => Self::Scrollable(ScrollablePopupPageTarget::UserProfile),
            ModalPopup::Options(_) => Self::Selectable(SelectablePopupPageTarget::Options),
            ModalPopup::ChannelSwitcher(_) => {
                Self::Selectable(SelectablePopupPageTarget::ChannelSwitcher)
            }
            ModalPopup::PollVotePicker(_) => {
                Self::Selectable(SelectablePopupPageTarget::PollVotePicker)
            }
            ModalPopup::EmojiReactionPicker(_) => {
                Self::Selectable(SelectablePopupPageTarget::EmojiReactionPicker)
            }
            ModalPopup::MessageUrlPicker(_) => {
                Self::Selectable(SelectablePopupPageTarget::MessageUrlPicker)
            }
            ModalPopup::MessageActionMenu(_) => {
                Self::Selectable(SelectablePopupPageTarget::MessageActionMenu)
            }
            ModalPopup::Search(_) => return None,
            ModalPopup::MessageDeleteConfirmation(_)
            | ModalPopup::MessagePinConfirmation(_)
            | ModalPopup::QuitConfirmation
            | ModalPopup::GuildLeaveConfirmation(_)
            | ModalPopup::AttachmentViewer(_)
            // Leader action popups are shortcut-only. They can render message
            // actions, but they intentionally do not expose selection paging.
            | ModalPopup::Leader(_)
            | ModalPopup::DebugLog => return None,
        })
    }

    fn page(self, state: &mut DashboardState, action: SelectionAction) {
        match self {
            Self::Scrollable(target) => state.page_scrollable_popup(target, action),
            Self::Selectable(target) => state.page_selectable_popup(target, action),
        }
    }
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageDeleteConfirmationState {
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
    pub(super) author: String,
    pub(super) content: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessagePinConfirmationState {
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
    pub(super) pinned: bool,
    pub(super) author: String,
    pub(super) content: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildLeaveConfirmationState {
    pub(super) guild_id: Id<GuildMarker>,
    pub(super) name: String,
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
    pub(super) download_message: Option<String>,
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
    pub(super) edit_buffer: String,
    pub(super) edit_cursor_byte_index: usize,
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
        self.edit_buffer.clear();
        self.edit_cursor_byte_index = 0;
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
    Threads {
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

    pub(super) fn message_url_picker(&self) -> Option<&MessageUrlPickerState> {
        match &self.modal {
            Some(ModalPopup::MessageUrlPicker(picker)) => Some(picker),
            _ => None,
        }
    }

    pub(super) fn message_url_picker_mut(&mut self) -> Option<&mut MessageUrlPickerState> {
        match &mut self.modal {
            Some(ModalPopup::MessageUrlPicker(picker)) => Some(picker),
            _ => None,
        }
    }

    pub(super) fn message_delete_confirmation(&self) -> Option<&MessageDeleteConfirmationState> {
        match &self.modal {
            Some(ModalPopup::MessageDeleteConfirmation(confirmation)) => Some(confirmation),
            _ => None,
        }
    }

    pub(super) fn take_message_delete_confirmation(
        &mut self,
    ) -> Option<MessageDeleteConfirmationState> {
        match self.modal.take() {
            Some(ModalPopup::MessageDeleteConfirmation(confirmation)) => Some(confirmation),
            other => {
                self.modal = other;
                None
            }
        }
    }

    pub(super) fn message_pin_confirmation(&self) -> Option<&MessagePinConfirmationState> {
        match &self.modal {
            Some(ModalPopup::MessagePinConfirmation(confirmation)) => Some(confirmation),
            _ => None,
        }
    }

    pub(super) fn take_message_pin_confirmation(&mut self) -> Option<MessagePinConfirmationState> {
        match self.modal.take() {
            Some(ModalPopup::MessagePinConfirmation(confirmation)) => Some(confirmation),
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

    pub(super) fn options_popup(&self) -> Option<&OptionsPopupState> {
        match &self.modal {
            Some(ModalPopup::Options(popup)) => Some(popup),
            _ => None,
        }
    }

    pub(super) fn options_popup_mut(&mut self) -> Option<&mut OptionsPopupState> {
        match &mut self.modal {
            Some(ModalPopup::Options(popup)) => Some(popup),
            _ => None,
        }
    }

    pub(super) fn attachment_viewer(&self) -> Option<&AttachmentViewerState> {
        match &self.modal {
            Some(ModalPopup::AttachmentViewer(viewer)) => Some(viewer),
            _ => None,
        }
    }

    pub(super) fn attachment_viewer_mut(&mut self) -> Option<&mut AttachmentViewerState> {
        match &mut self.modal {
            Some(ModalPopup::AttachmentViewer(viewer)) => Some(viewer),
            _ => None,
        }
    }

    pub(super) fn leader(&self) -> Option<&LeaderPopupState> {
        match &self.modal {
            Some(ModalPopup::Leader(leader)) => Some(leader),
            _ => None,
        }
    }

    pub(super) fn leader_mut(&mut self) -> Option<&mut LeaderPopupState> {
        match &mut self.modal {
            Some(ModalPopup::Leader(leader)) => Some(leader),
            _ => None,
        }
    }

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

    pub(super) fn user_profile_popup(&self) -> Option<&UserProfilePopupState> {
        match &self.modal {
            Some(ModalPopup::UserProfile(popup)) => Some(popup),
            _ => None,
        }
    }

    pub(super) fn user_profile_popup_mut(&mut self) -> Option<&mut UserProfilePopupState> {
        match &mut self.modal {
            Some(ModalPopup::UserProfile(popup)) => Some(popup),
            _ => None,
        }
    }

    pub(super) fn emoji_reaction_picker(&self) -> Option<&EmojiReactionPickerState> {
        match &self.modal {
            Some(ModalPopup::EmojiReactionPicker(picker)) => Some(picker),
            _ => None,
        }
    }

    pub(super) fn emoji_reaction_picker_mut(&mut self) -> Option<&mut EmojiReactionPickerState> {
        match &mut self.modal {
            Some(ModalPopup::EmojiReactionPicker(picker)) => Some(picker),
            _ => None,
        }
    }

    pub(super) fn poll_vote_picker(&self) -> Option<&PollVotePickerState> {
        match &self.modal {
            Some(ModalPopup::PollVotePicker(picker)) => Some(picker),
            _ => None,
        }
    }

    pub(super) fn poll_vote_picker_mut(&mut self) -> Option<&mut PollVotePickerState> {
        match &mut self.modal {
            Some(ModalPopup::PollVotePicker(picker)) => Some(picker),
            _ => None,
        }
    }

    pub(super) fn take_poll_vote_picker(&mut self) -> Option<PollVotePickerState> {
        match self.modal.take() {
            Some(ModalPopup::PollVotePicker(picker)) => Some(picker),
            other => {
                self.modal = other;
                None
            }
        }
    }

    pub(super) fn reaction_users_popup(&self) -> Option<&ReactionUsersPopupState> {
        match &self.modal {
            Some(ModalPopup::ReactionUsers(popup)) => Some(popup),
            _ => None,
        }
    }

    pub(super) fn reaction_users_popup_mut(&mut self) -> Option<&mut ReactionUsersPopupState> {
        match &mut self.modal {
            Some(ModalPopup::ReactionUsers(popup)) => Some(popup),
            _ => None,
        }
    }

    pub(super) fn keymap_popup(&self) -> Option<&KeymapPopupState> {
        match &self.modal {
            Some(ModalPopup::Keymap(popup)) => Some(popup),
            _ => None,
        }
    }

    pub(super) fn keymap_popup_mut(&mut self) -> Option<&mut KeymapPopupState> {
        match &mut self.modal {
            Some(ModalPopup::Keymap(popup)) => Some(popup),
            _ => None,
        }
    }

    pub(super) fn channel_switcher(&self) -> Option<&ChannelSwitcherState> {
        match &self.modal {
            Some(ModalPopup::ChannelSwitcher(switcher)) => Some(switcher),
            _ => None,
        }
    }

    pub(super) fn channel_switcher_mut(&mut self) -> Option<&mut ChannelSwitcherState> {
        match &mut self.modal {
            Some(ModalPopup::ChannelSwitcher(switcher)) => Some(switcher),
            _ => None,
        }
    }

    pub(super) fn search_popup(&self) -> Option<&SearchPopupState> {
        match &self.modal {
            Some(ModalPopup::Search(search)) => Some(search),
            _ => None,
        }
    }

    pub(super) fn search_popup_mut(&mut self) -> Option<&mut SearchPopupState> {
        match &mut self.modal {
            Some(ModalPopup::Search(search)) => Some(search),
            _ => None,
        }
    }
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
        let Some(target) = self.active_popup_page_target() else {
            return false;
        };

        target.page(self, action);
        true
    }

    fn page_scrollable_popup(
        &mut self,
        target: ScrollablePopupPageTarget,
        action: SelectionAction,
    ) {
        match target {
            ScrollablePopupPageTarget::Keymap => {
                if let Some(popup) = self.popups.keymap_popup_mut() {
                    popup.scroll.page(action);
                }
            }
            ScrollablePopupPageTarget::ReactionUsers => {
                if let Some(popup) = self.popups.reaction_users_popup_mut() {
                    popup.scroll.page(action);
                }
            }
            ScrollablePopupPageTarget::UserProfile => {
                if let Some(popup) = self.popups.user_profile_popup_mut() {
                    popup.scroll.page(action);
                }
            }
        }
    }

    fn page_selectable_popup(
        &mut self,
        target: SelectablePopupPageTarget,
        action: SelectionAction,
    ) {
        match target {
            SelectablePopupPageTarget::Options => {
                let len = self.options_popup_item_count();
                if let Some(popup) = self.popups.options_popup_mut() {
                    popup.selection.page(len, action);
                }
            }
            SelectablePopupPageTarget::ChannelSwitcher => {
                self.page_channel_switcher_selection(action)
            }
            SelectablePopupPageTarget::PollVotePicker => {
                if let Some(picker) = self.popups.poll_vote_picker_mut() {
                    picker.selection.page(picker.answers.len(), action);
                }
            }
            SelectablePopupPageTarget::EmojiReactionPicker => {
                let len = self.filtered_emoji_reaction_items().len();
                if let Some(picker) = self.popups.emoji_reaction_picker_mut() {
                    picker.selection.page(len, action);
                }
            }
            SelectablePopupPageTarget::MessageUrlPicker => {
                if let Some(picker) = self.popups.message_url_picker_mut() {
                    picker.selection.page(picker.items.len(), action);
                }
            }
            SelectablePopupPageTarget::MessageActionMenu => {
                let len = self.selected_message_action_items().len();
                if let Some(menu) = self.popups.message_action_menu_mut() {
                    menu.selection.page(len, action);
                }
            }
        }
    }

    fn active_popup_page_target(&self) -> Option<ActivePopupPageTarget> {
        self.popups
            .modal
            .as_ref()
            .and_then(ActivePopupPageTarget::from_modal)
    }

    pub fn is_any_action_context_active(&self) -> bool {
        self.popups.message_action_menu().is_some()
            || self.popups.guild_leader_action().is_some()
            || self.popups.channel_leader_action().is_some()
            || self.popups.member_leader_action().is_some()
    }
}

impl DashboardState {
    pub fn is_channel_leader_action_active(&self) -> bool {
        self.popups.channel_leader_action().is_some()
    }

    pub fn is_guild_leader_action_active(&self) -> bool {
        self.popups.guild_leader_action().is_some()
    }

    pub fn is_channel_action_threads_phase(&self) -> bool {
        matches!(
            self.popups.channel_leader_action(),
            Some(ChannelLeaderActionState::Threads { .. })
        )
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
