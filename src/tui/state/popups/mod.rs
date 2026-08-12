use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::PathBuf,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, ForumTagMarker, GuildMarker, MessageMarker, UserMarker},
};
use crate::discord::{
    AppCommand, MessageAttachmentUpload, ReactionEmoji, StreamCaptureTarget,
    VoiceParticipantPlaybackSettings, VoiceScope,
};
use crate::discord::{PresenceStatus, ProfileAvatarUpload};

use crate::discord::ReactionUserInfo;
use crate::tui::keybindings::{
    KeyBindings, KeyChord, LeaderShortcutItem, PopupAction, PopupKeymapScope, SelectionAction,
    UiAction,
};
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
mod voice_participant_audio;
pub(in crate::tui) use voice_participant_audio::VoiceParticipantAudioField;
use voice_participant_audio::{
    VOICE_PARTICIPANT_AUDIO_FIELD_COUNT, VoiceParticipantAudioPopupState,
};

use super::scroll::{VerticalScrollState, clamp_list_scroll};
use super::{
    DashboardState, EmojiReactionItem, FocusPane, MessageUrlItem, PollVotePickerItem,
    ThreadEditField,
};
use channel_switcher::ChannelSwitcherState;
use notification_inbox::NotificationInboxState;
pub use notification_inbox::{
    NotificationInboxChannelLoad, NotificationInboxItem, NotificationInboxLoad,
    NotificationInboxMessage, NotificationInboxTab, NotificationInboxUnreadItem,
};
use search::SearchPopupState;

#[derive(Debug, Default)]
pub(super) struct PopupUiState {
    pub(super) modal: Option<ModalPopup>,
    pub(super) confirmation_button: ConfirmationButton,
    key_sequence: Option<KeySequenceState>,
    /// Bumped per inbox open so a previous open's late responses are ignored.
    pub(super) inbox_request_generation: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::tui) enum ConfirmationButton {
    #[default]
    Confirm,
    Cancel,
}

impl ConfirmationButton {
    fn next(self) -> Self {
        match self {
            Self::Confirm => Self::Cancel,
            Self::Cancel => Self::Confirm,
        }
    }
}

/// Declares the modal popup enum and its payload-free companion from one list,
/// the way `define_app_event_kinds!` does for `AppEvent`. Writing the variants
/// once is what keeps the two enums and `kind()` from drifting apart.
macro_rules! define_modal_popups {
    ($($variant:ident $(($state:ty))?,)*) => {
        #[derive(Debug)]
        pub(super) enum ModalPopup {
            $($variant $(($state))?,)*
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(in crate::tui) enum ActiveModalPopupKind {
            $($variant,)*
        }

        impl ModalPopup {
            fn kind(&self) -> ActiveModalPopupKind {
                match self {
                    $(define_modal_popups!(@pattern $variant $(($state))?)
                        => ActiveModalPopupKind::$variant,)*
                }
            }
        }
    };
    (@pattern $variant:ident ($state:ty)) => { Self::$variant(_) };
    (@pattern $variant:ident) => { Self::$variant };
}

define_modal_popups! {
    MessageActionMenu(MessageActionMenuState),
    GuildActionMenu(GuildActionMenuState),
    ChannelActionMenu(ChannelActionMenuState),
    MemberActionMenu(MemberActionMenuState),
    MessageUrlPicker(MessageUrlPickerState),
    MessageConfirmation(MessageConfirmationState),
    QuitConfirmation,
    GuildLeaveConfirmation(GuildLeaveConfirmationState),
    Options(OptionsPopupState),
    AttachmentViewer(AttachmentViewerState),
    UserProfile(UserProfilePopupState),
    EmojiReactionPicker(EmojiReactionPickerState),
    PollVotePicker(PollVotePickerState),
    ReactionUsers(ReactionUsersPopupState),
    DebugLog,
    KeymapHelp(KeymapPopupState),
    ChannelSwitcher(ChannelSwitcherState),
    NotificationInbox(NotificationInboxState),
    Search(SearchPopupState),
    ForumPostComposer(ForumPostComposerState),
    ThreadEdit(ThreadEditState),
    ThreadActionMenu(ThreadActionMenuState),
    ThreadDeleteConfirmation(ThreadDeleteConfirmationState),
    VoiceParticipantAudio(VoiceParticipantAudioPopupState),
}

/// The input behavior of the topmost visible popup layer.
///
/// A modal kind alone is not enough because forms can open nested lists or
/// confirmations without replacing the outer modal. Resolving the interaction
/// first prevents page keys from moving hidden background content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivePopupInteraction {
    SelectableList(SelectablePopupTarget),
    ScrollableDocument(ScrollablePopupTarget),
    EditingDocument(ScrollablePopupTarget),
    Custom(CustomPopupTarget),
    Confirmation,
    NoNavigation,
}

impl ActivePopupInteraction {
    const fn keymap_context(self) -> Option<PopupKeymapContext> {
        match self {
            Self::SelectableList(target) => Some(PopupKeymapContext::Selectable(target)),
            Self::ScrollableDocument(target) => Some(PopupKeymapContext::Scrollable(target)),
            Self::Confirmation => Some(PopupKeymapContext::Confirmation),
            Self::EditingDocument(_) | Self::Custom(_) | Self::NoNavigation => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum PopupInputMode {
    /// Popup-local commands and the shared popup keymap both own input.
    Routed,
    /// Printable keys stay with the editor while modified shared keys may run.
    TextEntry,
    /// Raw shortcut capture bypasses every shared action except bare Esc.
    Exclusive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) struct ActivePopupPolicy {
    pub(in crate::tui) kind: ActiveModalPopupKind,
    interaction: ActivePopupInteraction,
    pub(in crate::tui) input_mode: PopupInputMode,
}

impl ActivePopupPolicy {
    const fn routed(kind: ActiveModalPopupKind, interaction: ActivePopupInteraction) -> Self {
        Self {
            kind,
            interaction,
            input_mode: PopupInputMode::Routed,
        }
    }

    const fn text_entry(kind: ActiveModalPopupKind, interaction: ActivePopupInteraction) -> Self {
        Self {
            kind,
            interaction,
            input_mode: PopupInputMode::TextEntry,
        }
    }

    const fn exclusive(kind: ActiveModalPopupKind) -> Self {
        Self {
            kind,
            interaction: ActivePopupInteraction::NoNavigation,
            input_mode: PopupInputMode::Exclusive,
        }
    }

    const fn selectable(kind: ActiveModalPopupKind, target: SelectablePopupTarget) -> Self {
        Self::routed(kind, ActivePopupInteraction::SelectableList(target))
    }

    const fn scrollable(kind: ActiveModalPopupKind, target: ScrollablePopupTarget) -> Self {
        Self::routed(kind, ActivePopupInteraction::ScrollableDocument(target))
    }

    const fn confirmation(kind: ActiveModalPopupKind) -> Self {
        Self::routed(kind, ActivePopupInteraction::Confirmation)
    }

    pub(in crate::tui) const fn keymap_context(self) -> Option<PopupKeymapContext> {
        match self.input_mode {
            PopupInputMode::Routed => self.interaction.keymap_context(),
            PopupInputMode::TextEntry | PopupInputMode::Exclusive => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum SelectablePopupTarget {
    MessageActions,
    GuildActions,
    ChannelActions,
    MemberActions,
    MessageUrls,
    Options,
    UserProfileStatus,
    UserProfileActivity,
    EmojiReactions,
    PollVotes,
    ReactionList,
    ChannelSwitcher,
    NotificationInbox,
    ForumPostTags,
    ThreadEditTags,
    ThreadActions,
    VoiceParticipantAudio,
    SearchResults,
    SearchSuggestions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum PopupKeymapContext {
    Selectable(SelectablePopupTarget),
    Scrollable(ScrollablePopupTarget),
    Confirmation,
}

impl PopupKeymapContext {
    pub(in crate::tui) const fn scope(self) -> PopupKeymapScope {
        match self {
            Self::Selectable(_) => PopupKeymapScope::Selectable,
            Self::Scrollable(_) => PopupKeymapScope::Scrollable,
            Self::Confirmation => PopupKeymapScope::Confirmation,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeySequenceContext {
    Dashboard,
    Popup(PopupKeymapContext),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct KeySequenceState {
    context: KeySequenceContext,
    keys: Vec<KeyChord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CustomPopupTarget {
    Search,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) struct SelectablePopupSnapshot {
    pub(in crate::tui) target: SelectablePopupTarget,
    pub(in crate::tui) item_count: usize,
    pub(in crate::tui) selected: usize,
    pub(in crate::tui) scroll: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ScrollablePopupTarget {
    KeymapHelp,
    ReactionUsers,
    UserProfile,
    ForumPostComposer,
    ThreadEdit,
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
    pub(super) tag_selection: SelectablePopupState,
    /// Display order of tags while the tag picker is open. Captured on entry
    /// (selected tags first) so the cursor does not jump as tags are toggled.
    /// Indexed by `tag_selection.selected()`.
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
            tag_selection: SelectablePopupState::default(),
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
    /// Indexed by `tag_selection.selected()`.
    pub(super) tag_order: Vec<Id<ForumTagMarker>>,
    pub(super) tag_selection: SelectablePopupState,
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

/// Selectable list with the panes' scrolloff windowing. The UI builds a
/// `SelectablePopupLayout` from the rows it actually renders, then synchronizes
/// the item scroll and visible count here before input is handled.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct SelectablePopupState {
    selected: usize,
    scroll: usize,
    visible_items: usize,
    page_step: usize,
}

impl SelectablePopupState {
    pub(super) fn selected(&self) -> usize {
        self.selected
    }

    pub(super) fn selected_for_len(&self, len: usize) -> usize {
        self.selected.min(len.saturating_sub(1))
    }

    pub(super) fn scroll(&self) -> usize {
        self.scroll
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

    pub(super) fn set_layout(&mut self, scroll: usize, visible_items: usize, len: usize) {
        self.visible_items = visible_items.max(1);
        self.page_step = (self.visible_items / 2).max(1);
        self.scroll = scroll.min(len.saturating_sub(self.visible_items));
    }

    pub(super) fn page(&mut self, len: usize, action: SelectionAction) {
        let step = self.page_step.max(1);
        match action {
            SelectionAction::Next => {
                if len > 0 {
                    self.selected = self.selected.saturating_add(step).min(len - 1);
                }
            }
            SelectionAction::Previous => {
                self.selected = self.selected.saturating_sub(step);
            }
        }
        self.scroll = clamp_list_scroll(
            self.selected_for_len(len),
            self.scroll,
            self.visible_items.max(1),
            len,
        );
    }

    pub(super) fn jump_top(&mut self) {
        self.selected = 0;
        self.scroll = 0;
    }

    pub(super) fn jump_bottom(&mut self, len: usize) {
        self.selected = len.saturating_sub(1);
        self.scroll = len.saturating_sub(self.visible_items.max(1));
    }
}

type ScrollablePopupState = VerticalScrollState;

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
    pub(super) capturing_push_to_talk_shortcut: bool,
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
pub(super) enum GuildActionMenuState {
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
    Save,
    Cancel,
    SignOut,
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
    pub(super) activity_picker: Option<SelectablePopupState>,
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
    const GLOBAL_ACTIONS: [UserProfileSettingsField; 3] = [
        UserProfileSettingsField::Save,
        UserProfileSettingsField::Cancel,
        UserProfileSettingsField::SignOut,
    ];
    const GUILD_FIELDS: [UserProfileSettingsField; 2] = [
        UserProfileSettingsField::GuildNickname,
        UserProfileSettingsField::GuildPronouns,
    ];
    const GUILD_ACTIONS: [UserProfileSettingsField; 3] = [
        UserProfileSettingsField::Save,
        UserProfileSettingsField::Cancel,
        UserProfileSettingsField::SignOut,
    ];

    pub(super) fn active_field(&self) -> UserProfileSettingsField {
        match self.tab {
            UserProfileSettingsTab::Global => profile_settings_field_at(
                self.selected_global,
                &Self::GLOBAL_FIELDS,
                &Self::GLOBAL_ACTIONS,
            ),
            UserProfileSettingsTab::Guild => profile_settings_field_at(
                self.selected_guild,
                &Self::GUILD_FIELDS,
                &Self::GUILD_ACTIONS,
            ),
        }
    }

    pub(super) fn next_field(&mut self) {
        match self.tab {
            UserProfileSettingsTab::Global => {
                self.selected_global = (self.selected_global + 1)
                    % (Self::GLOBAL_FIELDS.len() + Self::GLOBAL_ACTIONS.len());
            }
            UserProfileSettingsTab::Guild => {
                self.selected_guild = (self.selected_guild + 1)
                    % (Self::GUILD_FIELDS.len() + Self::GUILD_ACTIONS.len());
            }
        }
    }

    pub(super) fn previous_field(&mut self) {
        match self.tab {
            UserProfileSettingsTab::Global => {
                self.selected_global = if self.selected_global == 0 {
                    Self::GLOBAL_FIELDS.len() + Self::GLOBAL_ACTIONS.len() - 1
                } else {
                    self.selected_global - 1
                };
            }
            UserProfileSettingsTab::Guild => {
                self.selected_guild = if self.selected_guild == 0 {
                    Self::GUILD_FIELDS.len() + Self::GUILD_ACTIONS.len() - 1
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
            UserProfileSettingsField::Save
            | UserProfileSettingsField::Cancel
            | UserProfileSettingsField::SignOut => {}
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

fn profile_settings_field_at(
    selected: usize,
    fields: &[UserProfileSettingsField],
    actions: &[UserProfileSettingsField],
) -> UserProfileSettingsField {
    let field_count = fields.len();
    let selected = selected.min(field_count + actions.len() - 1);
    if selected < field_count {
        fields[selected]
    } else {
        actions[selected - field_count]
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
pub(super) struct MemberActionMenuState {
    pub(super) user_id: Id<UserMarker>,
    pub(super) guild_id: Option<Id<GuildMarker>>,
    pub(super) selection: SelectablePopupState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ChannelActionMenuState {
    Actions {
        channel_id: Id<ChannelMarker>,
        selection: SelectablePopupState,
    },
    ParticipantActions {
        // Keep the channel in the context so later participant actions, such
        // as watching a stream, do not need a second lookup.
        channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
        display_name: String,
        selection: SelectablePopupState,
    },
    MuteDuration {
        channel_id: Id<ChannelMarker>,
        selection: SelectablePopupState,
    },
    StreamTargets {
        scope: VoiceScope,
        channel_id: Id<ChannelMarker>,
        targets: Vec<StreamCaptureTarget>,
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

const REACTION_USERS_LOAD_MORE_THRESHOLD: usize = 3;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReactionUsersEntry {
    pub(super) emoji: ReactionEmoji,
    pub(super) count: u64,
    pub(super) users: Vec<ReactionUserInfo>,
    pub(super) next_after: Option<Id<UserMarker>>,
    pub(super) loading: bool,
    pub(super) loaded_once: bool,
}

impl ReactionUsersEntry {
    pub fn emoji(&self) -> &ReactionEmoji {
        &self.emoji
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn users(&self) -> &[ReactionUserInfo] {
        &self.users
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn loaded_once(&self) -> bool {
        self.loaded_once
    }

    pub fn has_more(&self) -> bool {
        self.next_after.is_some()
    }
}

/// `viewing` is the opened entry's index, or `None` while the reaction list shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReactionUsersPopupState {
    pub(super) channel_id: Id<ChannelMarker>,
    pub(super) message_id: Id<MessageMarker>,
    pub(super) entries: Vec<ReactionUsersEntry>,
    pub(super) list: SelectablePopupState,
    pub(super) viewing: Option<usize>,
    pub(super) user_scroll: ScrollablePopupState,
}

impl ReactionUsersPopupState {
    pub(in crate::tui) fn channel_id(&self) -> Id<ChannelMarker> {
        self.channel_id
    }

    pub(super) fn new(
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        reactions: Vec<(ReactionEmoji, u64)>,
    ) -> Self {
        let entries = reactions
            .into_iter()
            .map(|(emoji, count)| ReactionUsersEntry {
                emoji,
                count,
                users: Vec::new(),
                next_after: None,
                loading: false,
                loaded_once: false,
            })
            .collect();
        Self {
            channel_id,
            message_id,
            entries,
            list: SelectablePopupState::default(),
            viewing: None,
            user_scroll: ScrollablePopupState::default(),
        }
    }

    pub fn entries(&self) -> &[ReactionUsersEntry] {
        &self.entries
    }

    pub fn is_viewing_users(&self) -> bool {
        self.viewing.is_some()
    }

    pub fn list_selected(&self) -> usize {
        self.list.selected_for_len(self.entries.len())
    }

    pub fn list_scroll(&self) -> usize {
        self.list.scroll()
    }

    pub fn viewed_entry(&self) -> Option<&ReactionUsersEntry> {
        self.viewing.and_then(|index| self.entries.get(index))
    }
    pub fn user_scroll(&self) -> usize {
        self.user_scroll.scroll()
    }

    pub fn user_line_count(&self) -> usize {
        self.viewed_entry()
            .map(|entry| entry.users.len().max(1))
            .unwrap_or(1)
    }

    pub(super) fn set_user_view_height(&mut self, height: usize) {
        let total = self.user_line_count();
        self.user_scroll.set_view_height(height);
        self.user_scroll.set_total_lines(total);
    }

    pub(super) fn open_selected(&mut self) -> Option<ReactionEmoji> {
        let index = self.list_selected();
        if index >= self.entries.len() {
            return None;
        }
        self.viewing = Some(index);
        self.user_scroll.scroll_to_top();
        let total = self.user_line_count();
        self.user_scroll.set_total_lines(total);
        self.begin_load(index)
    }

    pub(super) fn back_to_list(&mut self) -> bool {
        if self.viewing.is_some() {
            self.viewing = None;
            true
        } else {
            false
        }
    }

    fn begin_load(&mut self, index: usize) -> Option<ReactionEmoji> {
        let entry = self.entries.get_mut(index)?;
        if entry.loaded_once || entry.loading {
            return None;
        }
        entry.loading = true;
        Some(entry.emoji.clone())
    }

    pub(super) fn take_load_more(&mut self) -> Option<(ReactionEmoji, Id<UserMarker>)> {
        if !self
            .user_scroll
            .is_near_bottom(REACTION_USERS_LOAD_MORE_THRESHOLD)
        {
            return None;
        }
        let index = self.viewing?;
        let entry = self.entries.get_mut(index)?;
        if entry.loading {
            return None;
        }
        let after = entry.next_after?;
        entry.loading = true;
        Some((entry.emoji.clone(), after))
    }

    pub(super) fn apply_loaded(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
        users: Vec<ReactionUserInfo>,
        next_after: Option<Id<UserMarker>>,
        after: Option<Id<UserMarker>>,
    ) {
        if self.channel_id != channel_id || self.message_id != message_id {
            return;
        }
        let Some(entry) = self.entries.iter_mut().find(|entry| &entry.emoji == emoji) else {
            return;
        };
        // after == None replaces the users (first page). Some appends the next.
        if after.is_none() {
            entry.users = users;
        } else {
            entry.users.extend(users);
        }
        entry.next_after = next_after;
        entry.loading = false;
        entry.loaded_once = true;
        let total = self.user_line_count();
        self.user_scroll.set_total_lines(total);
    }

    pub(super) fn apply_load_failed(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) {
        if self.channel_id != channel_id || self.message_id != message_id {
            return;
        }
        if let Some(entry) = self.entries.iter_mut().find(|entry| &entry.emoji == emoji) {
            entry.loading = false;
        }
    }
}

#[cfg(test)]
type ReactionUsersTestEntry = (
    ReactionEmoji,
    u64,
    Vec<ReactionUserInfo>,
    Option<Id<UserMarker>>,
);

#[cfg(test)]
impl ReactionUsersPopupState {
    pub(crate) fn test_list(
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        reactions: Vec<(ReactionEmoji, u64)>,
    ) -> Self {
        Self::new(channel_id, message_id, reactions)
    }

    /// Tuple order is (emoji, count, users, next_after).
    pub(crate) fn test_viewing(
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        entries: Vec<ReactionUsersTestEntry>,
        viewing: usize,
    ) -> Self {
        let entries = entries
            .into_iter()
            .map(|(emoji, count, users, next_after)| ReactionUsersEntry {
                emoji,
                count,
                users,
                next_after,
                loading: false,
                loaded_once: true,
            })
            .collect();
        let mut state = Self {
            channel_id,
            message_id,
            entries,
            list: SelectablePopupState::default(),
            viewing: Some(viewing),
            user_scroll: ScrollablePopupState::default(),
        };
        let total = state.user_line_count();
        state.user_scroll.set_total_lines(total);
        state
    }
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

macro_rules! take_modal_state {
    ($self:ident, $variant:ident, $binding:ident) => {{
        if !matches!(&$self.modal, Some(ModalPopup::$variant(_))) {
            None
        } else {
            let Some(ModalPopup::$variant($binding)) = $self.modal.take() else {
                unreachable!("modal variant was checked before extraction")
            };
            $self.clear_modal();
            Some($binding)
        }
    }};
}

impl PopupUiState {
    pub(super) fn set_modal(&mut self, modal: ModalPopup) {
        self.modal = Some(modal);
        self.key_sequence = None;
    }

    pub(super) fn clear_modal(&mut self) {
        self.modal = None;
        self.key_sequence = None;
    }

    modal_popup_accessors!(
        message_action_menu,
        message_action_menu_mut,
        MessageActionMenu,
        MessageActionMenuState,
        menu
    );
    modal_popup_accessors!(
        guild_action_menu,
        guild_action_menu_mut,
        GuildActionMenu,
        GuildActionMenuState,
        menu
    );
    modal_popup_accessors!(
        channel_action_menu,
        channel_action_menu_mut,
        ChannelActionMenu,
        ChannelActionMenuState,
        menu
    );
    modal_popup_accessors!(
        member_action_menu,
        member_action_menu_mut,
        MemberActionMenu,
        MemberActionMenuState,
        menu
    );

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
        take_modal_state!(self, MessageConfirmation, confirmation)
    }

    pub(super) fn guild_leave_confirmation(&self) -> Option<&GuildLeaveConfirmationState> {
        match &self.modal {
            Some(ModalPopup::GuildLeaveConfirmation(confirmation)) => Some(confirmation),
            _ => None,
        }
    }

    pub(super) fn take_guild_leave_confirmation(&mut self) -> Option<GuildLeaveConfirmationState> {
        take_modal_state!(self, GuildLeaveConfirmation, confirmation)
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
        take_modal_state!(self, ThreadDeleteConfirmation, confirmation)
    }

    modal_popup_accessors!(
        options_popup,
        options_popup_mut,
        Options,
        OptionsPopupState,
        popup
    );
    modal_popup_accessors!(
        voice_participant_audio,
        voice_participant_audio_mut,
        VoiceParticipantAudio,
        VoiceParticipantAudioPopupState,
        popup
    );
    modal_popup_accessors!(
        attachment_viewer,
        attachment_viewer_mut,
        AttachmentViewer,
        AttachmentViewerState,
        viewer
    );

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
        take_modal_state!(self, PollVotePicker, picker)
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
        KeymapHelp,
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

    pub(in crate::tui) fn is_key_sequence_active(&self) -> bool {
        self.popups.key_sequence.is_some()
    }

    pub fn is_leader_active(&self) -> bool {
        self.popups
            .key_sequence
            .as_ref()
            .is_some_and(|sequence| sequence.context == KeySequenceContext::Dashboard)
    }

    pub fn open_leader(&mut self) {
        self.open_keymap_prefix(self.options.key_bindings.leader_keymap_prefix());
    }

    pub(in crate::tui) fn open_keymap_prefix(&mut self, keys: Vec<KeyChord>) {
        self.popups.key_sequence = Some(KeySequenceState {
            context: KeySequenceContext::Dashboard,
            keys,
        });
    }

    pub(in crate::tui) fn open_popup_keymap_prefix(
        &mut self,
        context: PopupKeymapContext,
        keys: Vec<KeyChord>,
    ) {
        self.popups.key_sequence = Some(KeySequenceState {
            context: KeySequenceContext::Popup(context),
            keys,
        });
    }

    pub(in crate::tui) fn close_key_sequence(&mut self) {
        self.popups.key_sequence = None;
    }

    pub fn close_leader(&mut self) {
        if self.is_leader_active() {
            self.close_key_sequence();
        }
    }

    pub(in crate::tui) fn leader_keymap_prefix(&self) -> &[KeyChord] {
        self.key_sequence_prefix(KeySequenceContext::Dashboard)
            .unwrap_or_default()
    }

    pub(in crate::tui) fn popup_keymap_prefix(
        &self,
        context: PopupKeymapContext,
    ) -> Option<&[KeyChord]> {
        self.key_sequence_prefix(KeySequenceContext::Popup(context))
    }

    fn key_sequence_prefix(&self, context: KeySequenceContext) -> Option<&[KeyChord]> {
        self.popups
            .key_sequence
            .as_ref()
            .filter(|sequence| sequence.context == context)
            .map(|sequence| sequence.keys.as_slice())
    }

    pub(in crate::tui) fn push_key_sequence_key(&mut self, key: KeyChord) {
        if let Some(sequence) = self.popups.key_sequence.as_mut() {
            sequence.keys.push(key);
        }
    }

    pub(in crate::tui) fn key_sequence_shortcuts(&self) -> Vec<LeaderShortcutItem> {
        let Some(sequence) = self.popups.key_sequence.as_ref() else {
            return Vec::new();
        };
        let mut shortcuts = match sequence.context {
            KeySequenceContext::Dashboard => self
                .options
                .key_bindings
                .leader_keymap_children(&sequence.keys),
            KeySequenceContext::Popup(context) => self
                .options
                .key_bindings
                .popup_keymap_children(&sequence.keys, context.scope()),
        };
        for shortcut in &mut shortcuts {
            if shortcut.action == Some(UiAction::ToggleStream)
                && shortcut.label == UiAction::ToggleStream.label()
            {
                shortcut.label = self.current_voice_stream_action_label().to_owned();
            }
        }
        shortcuts
    }

    pub(in crate::tui) fn key_sequence_title(&self) -> String {
        let prefix = self
            .popups
            .key_sequence
            .as_ref()
            .map(|sequence| sequence.keys.as_slice())
            .unwrap_or_default();
        self.options.key_bindings.keymap_prefix_title(prefix)
    }

    /// Open the action menu for the focused pane's selected target. Every
    /// menu is a standalone modal; a pane without an actionable selection
    /// opens nothing.
    pub fn open_focused_pane_actions(&mut self) {
        self.close_all_action_contexts();
        self.close_leader();
        // A focused forum post opens the thread action menu instead of the
        // (empty) message action menu, since the messages pane is then
        // showing forum post cards rather than messages.
        if self.open_selected_thread_actions() {
            return;
        }
        match self.navigation.focus {
            FocusPane::Guilds => {
                if let Some(menu) = self.selected_guild_action_context() {
                    self.popups.set_modal(ModalPopup::GuildActionMenu(menu));
                }
            }
            FocusPane::Channels => {
                if let Some(menu) = self.selected_channel_action_context() {
                    self.popups.set_modal(ModalPopup::ChannelActionMenu(menu));
                }
            }
            FocusPane::Messages => self.open_selected_message_actions(),
            FocusPane::Members => {
                if let Some(menu) = self.selected_member_action_context() {
                    self.popups.set_modal(ModalPopup::MemberActionMenu(menu));
                }
            }
        }
    }

    pub fn close_all_action_contexts(&mut self) {
        if matches!(
            self.popups.modal,
            Some(
                ModalPopup::MessageActionMenu(_)
                    | ModalPopup::GuildActionMenu(_)
                    | ModalPopup::ChannelActionMenu(_)
                    | ModalPopup::MemberActionMenu(_)
            )
        ) {
            self.popups.clear_modal();
        }
    }

    pub fn open_quit_confirmation(&mut self) {
        self.popups.confirmation_button = ConfirmationButton::default();
        self.popups.set_modal(ModalPopup::QuitConfirmation);
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

    pub(in crate::tui) fn active_confirmation_button(&self) -> ConfirmationButton {
        self.popups.confirmation_button
    }

    pub(in crate::tui) fn next_confirmation_button(&mut self) {
        self.popups.confirmation_button = self.popups.confirmation_button.next();
    }

    /// Closes the topmost popup layer using that popup's own back or cancel
    /// behavior. Raw close keys and configured `ClosePopup` bindings both use
    /// this path so nested popup state cannot behave differently by key source.
    pub(in crate::tui) fn close_active_popup(&mut self) {
        let Some(kind) = self.active_modal_popup_kind() else {
            return;
        };

        match kind {
            ActiveModalPopupKind::MessageActionMenu => self.close_message_action_menu(),
            ActiveModalPopupKind::GuildActionMenu => {
                if !self.back_guild_action_menu() {
                    self.close_guild_action_menu();
                }
            }
            ActiveModalPopupKind::ChannelActionMenu => {
                if !self.back_channel_action_menu() {
                    self.close_channel_action_menu();
                }
            }
            ActiveModalPopupKind::MemberActionMenu => self.close_member_action_menu(),
            ActiveModalPopupKind::MessageUrlPicker => self.close_message_url_picker(),
            ActiveModalPopupKind::MessageConfirmation => self.close_message_confirmation(),
            ActiveModalPopupKind::QuitConfirmation => self.close_quit_confirmation(),
            ActiveModalPopupKind::GuildLeaveConfirmation => {
                self.close_guild_leave_confirmation();
            }
            ActiveModalPopupKind::Options if self.is_capturing_push_to_talk_shortcut() => {
                self.cancel_push_to_talk_shortcut_capture();
            }
            ActiveModalPopupKind::Options => self.close_options_popup(),
            ActiveModalPopupKind::AttachmentViewer => self.close_attachment_viewer(),
            ActiveModalPopupKind::UserProfile => self.close_or_cancel_user_profile_popup(),
            ActiveModalPopupKind::EmojiReactionPicker => self.close_emoji_reaction_picker(),
            ActiveModalPopupKind::PollVotePicker => self.close_poll_vote_picker(),
            ActiveModalPopupKind::ReactionUsers => {
                if !self.reaction_users_popup_back() {
                    self.close_reaction_users_popup();
                }
            }
            ActiveModalPopupKind::DebugLog => self.close_debug_log_popup(),
            ActiveModalPopupKind::KeymapHelp => self.close_keymap_popup(),
            ActiveModalPopupKind::ChannelSwitcher => self.close_channel_switcher(),
            ActiveModalPopupKind::NotificationInbox
                if self.notification_inbox_is_confirming_mark_all() =>
            {
                self.cancel_mark_all_notification_inbox_read();
            }
            ActiveModalPopupKind::NotificationInbox => self.close_notification_inbox(),
            ActiveModalPopupKind::Search => self.close_search_popup(),
            ActiveModalPopupKind::ForumPostComposer => {
                self.close_or_cancel_forum_post_composer();
            }
            ActiveModalPopupKind::ThreadEdit => self.close_or_cancel_thread_edit(),
            ActiveModalPopupKind::ThreadActionMenu => {
                if !self.back_thread_action_menu() {
                    self.close_thread_action_menu();
                }
            }
            ActiveModalPopupKind::ThreadDeleteConfirmation => {
                self.close_thread_delete_confirmation();
            }
            ActiveModalPopupKind::VoiceParticipantAudio => {
                self.close_voice_participant_audio_popup();
            }
        }
    }

    pub(in crate::tui) fn execute_popup_keymap_action(
        &mut self,
        action: PopupAction,
    ) -> Option<AppCommand> {
        let context = self
            .active_popup_policy()
            .and_then(ActivePopupPolicy::keymap_context)?;
        if !action.is_allowed_in(context.scope()) {
            return None;
        }

        match action {
            PopupAction::SelectNext | PopupAction::SelectPrevious => match context {
                PopupKeymapContext::Selectable(target) => {
                    let action = if action == PopupAction::SelectNext {
                        SelectionAction::Next
                    } else {
                        SelectionAction::Previous
                    };
                    self.move_selectable_popup(target, action);
                    None
                }
                PopupKeymapContext::Scrollable(target) => {
                    let action = if action == PopupAction::SelectNext {
                        SelectionAction::Next
                    } else {
                        SelectionAction::Previous
                    };
                    self.select_scrollable_popup(target, action)
                }
                PopupKeymapContext::Confirmation => {
                    self.next_confirmation_button();
                    None
                }
            },
            PopupAction::HalfPageDown => {
                self.page_active_popup(SelectionAction::Next);
                self.reaction_users_popup_take_load_more()
            }
            PopupAction::HalfPageUp => {
                self.page_active_popup(SelectionAction::Previous);
                None
            }
            PopupAction::JumpTop | PopupAction::JumpBottom => {
                let PopupKeymapContext::Selectable(target) = context else {
                    return None;
                };
                self.jump_selectable_popup(target, action.ui_action());
                None
            }
        }
    }

    pub(in crate::tui) fn page_active_popup_down(&mut self) -> bool {
        self.page_active_popup(SelectionAction::Next)
    }

    pub(in crate::tui) fn page_active_popup_up(&mut self) -> bool {
        self.page_active_popup(SelectionAction::Previous)
    }

    pub(in crate::tui) fn move_active_popup_down(&mut self) -> Option<AppCommand> {
        self.move_active_popup(SelectionAction::Next)
    }

    pub(in crate::tui) fn move_active_popup_up(&mut self) -> Option<AppCommand> {
        self.move_active_popup(SelectionAction::Previous)
    }

    fn page_active_popup(&mut self, action: SelectionAction) -> bool {
        match self.active_popup_interaction() {
            Some(ActivePopupInteraction::SelectableList(target)) => {
                self.page_selectable_popup(target, action);
                true
            }
            Some(ActivePopupInteraction::ScrollableDocument(target)) => {
                self.page_scrollable_popup(target, action);
                true
            }
            Some(
                ActivePopupInteraction::EditingDocument(_)
                | ActivePopupInteraction::Custom(_)
                | ActivePopupInteraction::Confirmation
                | ActivePopupInteraction::NoNavigation,
            )
            | None => false,
        }
    }

    fn move_active_popup(&mut self, action: SelectionAction) -> Option<AppCommand> {
        match self.active_popup_interaction()? {
            ActivePopupInteraction::SelectableList(target) => {
                self.move_selectable_popup(target, action);
                None
            }
            ActivePopupInteraction::ScrollableDocument(ScrollablePopupTarget::ReactionUsers) => {
                self.navigate_reaction_users_popup(action)
            }
            ActivePopupInteraction::ScrollableDocument(target) => {
                self.scroll_popup_document(target, action);
                None
            }
            ActivePopupInteraction::EditingDocument(target) => {
                self.scroll_popup_document(target, action);
                None
            }
            ActivePopupInteraction::Custom(CustomPopupTarget::Search) => match action {
                SelectionAction::Next => self.move_search_result_down(),
                SelectionAction::Previous => {
                    self.move_search_result_up();
                    None
                }
            },
            ActivePopupInteraction::Confirmation | ActivePopupInteraction::NoNavigation => None,
        }
    }

    fn select_scrollable_popup(
        &mut self,
        target: ScrollablePopupTarget,
        action: SelectionAction,
    ) -> Option<AppCommand> {
        match target {
            ScrollablePopupTarget::KeymapHelp => {
                self.scroll_keymap_popup(action);
                None
            }
            ScrollablePopupTarget::ReactionUsers => self.navigate_reaction_users_popup(action),
            ScrollablePopupTarget::UserProfile => {
                match action {
                    SelectionAction::Next => self.next_user_profile_settings_field(),
                    SelectionAction::Previous => self.previous_user_profile_settings_field(),
                }
                None
            }
            ScrollablePopupTarget::ForumPostComposer => {
                match action {
                    SelectionAction::Next => self.move_forum_post_selection_down(),
                    SelectionAction::Previous => self.move_forum_post_selection_up(),
                }
                None
            }
            ScrollablePopupTarget::ThreadEdit => {
                match action {
                    SelectionAction::Next => self.move_thread_edit_selection_down(),
                    SelectionAction::Previous => self.move_thread_edit_selection_up(),
                }
                None
            }
        }
    }

    pub(in crate::tui) fn active_popup_policy(&self) -> Option<ActivePopupPolicy> {
        let modal = self.popups.modal.as_ref()?;
        let kind = modal.kind();
        let policy = match modal {
            ModalPopup::MessageActionMenu(_) => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::MessageActions)
            }
            ModalPopup::GuildActionMenu(_) => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::GuildActions)
            }
            ModalPopup::ChannelActionMenu(_) => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::ChannelActions)
            }
            ModalPopup::MemberActionMenu(_) => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::MemberActions)
            }
            ModalPopup::MessageUrlPicker(_) => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::MessageUrls)
            }
            ModalPopup::Options(popup) if popup.capturing_push_to_talk_shortcut => {
                ActivePopupPolicy::exclusive(kind)
            }
            ModalPopup::Options(_) => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::Options)
            }
            ModalPopup::UserProfile(popup) if popup.settings.status_picker.is_some() => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::UserProfileStatus)
            }
            ModalPopup::UserProfile(popup) if popup.settings.activity_picker.is_some() => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::UserProfileActivity)
            }
            ModalPopup::UserProfile(popup) if popup.settings.editing.is_some() => {
                ActivePopupPolicy::text_entry(
                    kind,
                    ActivePopupInteraction::EditingDocument(ScrollablePopupTarget::UserProfile),
                )
            }
            ModalPopup::UserProfile(_) => {
                ActivePopupPolicy::scrollable(kind, ScrollablePopupTarget::UserProfile)
            }
            ModalPopup::EmojiReactionPicker(popup) if popup.filter_editing => {
                ActivePopupPolicy::text_entry(
                    kind,
                    ActivePopupInteraction::SelectableList(SelectablePopupTarget::EmojiReactions),
                )
            }
            ModalPopup::EmojiReactionPicker(_) => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::EmojiReactions)
            }
            ModalPopup::PollVotePicker(_) => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::PollVotes)
            }
            ModalPopup::ReactionUsers(popup) if popup.viewing.is_some() => {
                ActivePopupPolicy::scrollable(kind, ScrollablePopupTarget::ReactionUsers)
            }
            ModalPopup::ReactionUsers(_) => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::ReactionList)
            }
            ModalPopup::KeymapHelp(_) => {
                ActivePopupPolicy::scrollable(kind, ScrollablePopupTarget::KeymapHelp)
            }
            ModalPopup::ChannelSwitcher(_) => ActivePopupPolicy::text_entry(
                kind,
                ActivePopupInteraction::SelectableList(SelectablePopupTarget::ChannelSwitcher),
            ),
            ModalPopup::NotificationInbox(_)
                if self.notification_inbox_is_confirming_mark_all() =>
            {
                ActivePopupPolicy::confirmation(kind)
            }
            ModalPopup::NotificationInbox(_) => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::NotificationInbox)
            }
            ModalPopup::Search(_) => ActivePopupPolicy::text_entry(
                kind,
                ActivePopupInteraction::Custom(CustomPopupTarget::Search),
            ),
            ModalPopup::ForumPostComposer(popup)
                if popup.editing == Some(ForumPostComposerFieldState::Tags) =>
            {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::ForumPostTags)
            }
            ModalPopup::ForumPostComposer(popup) if popup.editing.is_some() => {
                ActivePopupPolicy::text_entry(
                    kind,
                    ActivePopupInteraction::EditingDocument(
                        ScrollablePopupTarget::ForumPostComposer,
                    ),
                )
            }
            ModalPopup::ForumPostComposer(_) => {
                ActivePopupPolicy::scrollable(kind, ScrollablePopupTarget::ForumPostComposer)
            }
            ModalPopup::ThreadEdit(popup) if popup.editing_tags => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::ThreadEditTags)
            }
            ModalPopup::ThreadEdit(popup) if popup.editing_title => ActivePopupPolicy::text_entry(
                kind,
                ActivePopupInteraction::EditingDocument(ScrollablePopupTarget::ThreadEdit),
            ),
            ModalPopup::ThreadEdit(_) => {
                ActivePopupPolicy::scrollable(kind, ScrollablePopupTarget::ThreadEdit)
            }
            ModalPopup::ThreadActionMenu(_) => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::ThreadActions)
            }
            ModalPopup::MessageConfirmation(_)
            | ModalPopup::QuitConfirmation
            | ModalPopup::GuildLeaveConfirmation(_)
            | ModalPopup::ThreadDeleteConfirmation(_) => ActivePopupPolicy::confirmation(kind),
            ModalPopup::AttachmentViewer(_) | ModalPopup::DebugLog => {
                ActivePopupPolicy::routed(kind, ActivePopupInteraction::NoNavigation)
            }
            ModalPopup::VoiceParticipantAudio(_) => {
                ActivePopupPolicy::selectable(kind, SelectablePopupTarget::VoiceParticipantAudio)
            }
        };
        Some(policy)
    }

    fn active_popup_interaction(&self) -> Option<ActivePopupInteraction> {
        self.active_popup_policy().map(|policy| policy.interaction)
    }

    pub(in crate::tui) fn active_selectable_popup_snapshot(
        &self,
    ) -> Option<SelectablePopupSnapshot> {
        let target = match self.active_popup_interaction()? {
            ActivePopupInteraction::SelectableList(target) => target,
            ActivePopupInteraction::Custom(CustomPopupTarget::Search) => {
                self.popups.search_popup()?.active_selectable_target()
            }
            ActivePopupInteraction::ScrollableDocument(_)
            | ActivePopupInteraction::EditingDocument(_)
            | ActivePopupInteraction::Confirmation
            | ActivePopupInteraction::NoNavigation => return None,
        };
        let (selection, item_count) = self.selectable_popup_state(target)?;
        Some(SelectablePopupSnapshot {
            target,
            item_count,
            selected: selection.selected_for_len(item_count),
            scroll: selection.scroll(),
        })
    }

    pub(in crate::tui) fn popup_list_scroll(&self, target: SelectablePopupTarget) -> Option<usize> {
        self.selectable_popup_state(target)
            .map(|(selection, _)| selection.scroll())
    }

    fn selectable_popup_state(
        &self,
        target: SelectablePopupTarget,
    ) -> Option<(&SelectablePopupState, usize)> {
        Some(match target {
            SelectablePopupTarget::MessageActions => {
                let selection = &self.popups.message_action_menu()?.selection;
                (selection, self.selected_message_action_items().len())
            }
            SelectablePopupTarget::GuildActions => {
                let selection = match self.popups.guild_action_menu()? {
                    GuildActionMenuState::Actions { selection }
                    | GuildActionMenuState::MuteDuration { selection } => selection,
                };
                (selection, self.guild_action_row_count())
            }
            SelectablePopupTarget::ChannelActions => {
                let selection = match self.popups.channel_action_menu()? {
                    ChannelActionMenuState::Actions { selection, .. }
                    | ChannelActionMenuState::ParticipantActions { selection, .. }
                    | ChannelActionMenuState::MuteDuration { selection, .. }
                    | ChannelActionMenuState::StreamTargets { selection, .. } => selection,
                };
                (selection, self.channel_action_row_count())
            }
            SelectablePopupTarget::MemberActions => {
                let selection = &self.popups.member_action_menu()?.selection;
                (selection, self.selected_member_action_items().len())
            }
            SelectablePopupTarget::MessageUrls => {
                let picker = self.popups.message_url_picker()?;
                (&picker.selection, picker.items.len())
            }
            SelectablePopupTarget::Options => {
                let selection = &self.popups.options_popup()?.selection;
                (selection, self.options_popup_item_count())
            }
            SelectablePopupTarget::UserProfileStatus => {
                let selection = self
                    .popups
                    .user_profile_popup()?
                    .settings
                    .status_picker
                    .as_ref()?;
                (selection, PresenceStatus::user_selectable().len())
            }
            SelectablePopupTarget::UserProfileActivity => {
                let selection = self
                    .popups
                    .user_profile_popup()?
                    .settings
                    .activity_picker
                    .as_ref()?;
                (selection, self.detected_rich_presence().len() + 1)
            }
            SelectablePopupTarget::EmojiReactions => {
                let selection = &self.popups.emoji_reaction_picker()?.selection;
                (selection, self.filtered_emoji_reaction_items_slice()?.len())
            }
            SelectablePopupTarget::PollVotes => {
                let picker = self.popups.poll_vote_picker()?;
                (&picker.selection, picker.answers.len())
            }
            SelectablePopupTarget::ReactionList => {
                let popup = self.popups.reaction_users_popup()?;
                (&popup.list, popup.entries.len())
            }
            SelectablePopupTarget::ChannelSwitcher => {
                let popup = self.popups.channel_switcher()?;
                (popup.selection(), popup.visible_len())
            }
            SelectablePopupTarget::NotificationInbox => {
                let tab = self.notification_inbox_tab()?;
                let inbox = self.popups.notification_inbox()?;
                (inbox.selection(tab), inbox.active_len())
            }
            SelectablePopupTarget::ForumPostTags => {
                let popup = self.popups.forum_post_composer()?;
                (&popup.tag_selection, popup.tag_order.len())
            }
            SelectablePopupTarget::ThreadEditTags => {
                let popup = self.popups.thread_edit()?;
                (&popup.tag_selection, popup.tag_order.len())
            }
            SelectablePopupTarget::ThreadActions => {
                let selection = match self.popups.thread_action_menu()? {
                    ThreadActionMenuState::Actions { selection, .. }
                    | ThreadActionMenuState::MuteDuration { selection, .. }
                    | ThreadActionMenuState::NotificationSettings { selection, .. } => selection,
                };
                (selection, self.thread_action_row_count())
            }
            SelectablePopupTarget::VoiceParticipantAudio => {
                let popup = self.popups.voice_participant_audio()?;
                (&popup.selection, VOICE_PARTICIPANT_AUDIO_FIELD_COUNT)
            }
            SelectablePopupTarget::SearchResults | SelectablePopupTarget::SearchSuggestions => {
                self.popups.search_popup()?.selectable_state(target)?
            }
        })
    }

    pub(in crate::tui) fn set_active_popup_list_layout(
        &mut self,
        target: SelectablePopupTarget,
        scroll: usize,
        visible_items: usize,
    ) -> bool {
        if self
            .active_selectable_popup_snapshot()
            .map(|snapshot| snapshot.target)
            != Some(target)
        {
            return false;
        }
        self.update_selectable_popup(target, |selection, len| {
            selection.set_layout(scroll, visible_items, len);
        });
        true
    }

    pub(in crate::tui) fn select_active_popup_row(
        &mut self,
        target: SelectablePopupTarget,
        row: usize,
    ) -> bool {
        if self
            .active_selectable_popup_snapshot()
            .map(|snapshot| snapshot.target)
            != Some(target)
        {
            return false;
        }
        let mut selected = false;
        self.update_selectable_popup(target, |selection, len| {
            if row < len {
                selection.select(row);
                selected = true;
            }
        });
        if selected {
            self.after_selectable_popup_selection_changed(target);
        }
        selected
    }

    pub(in crate::tui) fn activate_active_popup_row(
        &mut self,
        target: SelectablePopupTarget,
    ) -> Option<AppCommand> {
        if self
            .active_selectable_popup_snapshot()
            .map(|snapshot| snapshot.target)
            != Some(target)
        {
            return None;
        }
        match target {
            SelectablePopupTarget::MessageActions => self.activate_selected_message_action(),
            SelectablePopupTarget::GuildActions => self.activate_selected_guild_action(),
            SelectablePopupTarget::ChannelActions => self.activate_selected_channel_action(),
            SelectablePopupTarget::MemberActions => self.activate_selected_member_action(),
            SelectablePopupTarget::MessageUrls => self.activate_selected_message_url(),
            SelectablePopupTarget::Options => {
                self.toggle_selected_display_option();
                None
            }
            SelectablePopupTarget::UserProfileStatus => self.activate_user_profile_status_picker(),
            SelectablePopupTarget::UserProfileActivity => {
                self.activate_user_profile_activity_picker()
            }
            SelectablePopupTarget::EmojiReactions => self.activate_selected_emoji_reaction(),
            SelectablePopupTarget::PollVotes => {
                self.toggle_selected_poll_vote_answer();
                None
            }
            SelectablePopupTarget::ReactionList => self.activate_reaction_users_popup(),
            SelectablePopupTarget::ChannelSwitcher => {
                self.activate_selected_channel_switcher_item()
            }
            SelectablePopupTarget::NotificationInbox => {
                self.activate_selected_notification_inbox_item()
            }
            SelectablePopupTarget::ForumPostTags => self.activate_forum_post_composer(),
            SelectablePopupTarget::ThreadEditTags => self.activate_thread_edit(),
            SelectablePopupTarget::ThreadActions => self.activate_selected_thread_action(),
            SelectablePopupTarget::VoiceParticipantAudio => {
                self.activate_voice_participant_audio_field()
            }
            SelectablePopupTarget::SearchResults | SelectablePopupTarget::SearchSuggestions => {
                self.activate_search_popup()
            }
        }
    }

    fn page_selectable_popup(&mut self, target: SelectablePopupTarget, action: SelectionAction) {
        self.update_selectable_popup(target, |selection, len| {
            selection.page(len, action);
        });
        self.after_selectable_popup_selection_changed(target);
    }

    pub(in crate::tui) fn jump_selectable_popup(
        &mut self,
        target: SelectablePopupTarget,
        action: UiAction,
    ) -> bool {
        if self
            .active_selectable_popup_snapshot()
            .map(|snapshot| snapshot.target)
            != Some(target)
        {
            return false;
        }
        let jump_bottom = match action {
            UiAction::JumpTop => false,
            UiAction::JumpBottom => true,
            _ => return false,
        };
        self.update_selectable_popup(target, |selection, len| {
            if jump_bottom {
                selection.jump_bottom(len);
            } else {
                selection.jump_top();
            }
        });
        self.after_selectable_popup_selection_changed(target);
        true
    }

    fn move_selectable_popup(&mut self, target: SelectablePopupTarget, action: SelectionAction) {
        self.update_selectable_popup(target, |selection, len| match action {
            SelectionAction::Next => selection.move_down(len),
            SelectionAction::Previous => selection.move_up(),
        });
        self.after_selectable_popup_selection_changed(target);
    }

    fn after_selectable_popup_selection_changed(&mut self, target: SelectablePopupTarget) {
        if target == SelectablePopupTarget::NotificationInbox {
            self.ensure_notification_inbox_requests();
        }
    }

    fn update_selectable_popup(
        &mut self,
        target: SelectablePopupTarget,
        update: impl FnOnce(&mut SelectablePopupState, usize),
    ) {
        match target {
            SelectablePopupTarget::MessageActions => {
                let len = self.selected_message_action_items().len();
                if let Some(menu) = self.popups.message_action_menu_mut() {
                    update(&mut menu.selection, len);
                }
            }
            SelectablePopupTarget::GuildActions => {
                let len = self.guild_action_row_count();
                if let Some(selection) = self.guild_action_selection_mut() {
                    update(selection, len);
                }
            }
            SelectablePopupTarget::ChannelActions => {
                let len = self.channel_action_row_count();
                if let Some(selection) = self.channel_action_selection_mut() {
                    update(selection, len);
                }
            }
            SelectablePopupTarget::MemberActions => {
                let len = self.selected_member_action_items().len();
                if let Some(menu) = self.popups.member_action_menu_mut() {
                    update(&mut menu.selection, len);
                }
            }
            SelectablePopupTarget::MessageUrls => {
                if let Some(picker) = self.popups.message_url_picker_mut() {
                    update(&mut picker.selection, picker.items.len());
                }
            }
            SelectablePopupTarget::Options => {
                let len = self.options_popup_item_count();
                if let Some(popup) = self.popups.options_popup_mut() {
                    update(&mut popup.selection, len);
                }
            }
            SelectablePopupTarget::UserProfileStatus => {
                let len = PresenceStatus::user_selectable().len();
                if let Some(selection) = self
                    .popups
                    .user_profile_popup_mut()
                    .and_then(|popup| popup.settings.status_picker.as_mut())
                {
                    update(selection, len);
                }
            }
            SelectablePopupTarget::UserProfileActivity => {
                let len = self.detected_rich_presence().len() + 1;
                if let Some(selection) = self
                    .popups
                    .user_profile_popup_mut()
                    .and_then(|popup| popup.settings.activity_picker.as_mut())
                {
                    update(selection, len);
                }
            }
            SelectablePopupTarget::EmojiReactions => {
                let len = self
                    .filtered_emoji_reaction_items_slice()
                    .map_or(0, <[EmojiReactionItem]>::len);
                if let Some(picker) = self.popups.emoji_reaction_picker_mut() {
                    update(&mut picker.selection, len);
                }
            }
            SelectablePopupTarget::PollVotes => {
                if let Some(picker) = self.popups.poll_vote_picker_mut() {
                    update(&mut picker.selection, picker.answers.len());
                }
            }
            SelectablePopupTarget::ReactionList => {
                if let Some(popup) = self.popups.reaction_users_popup_mut() {
                    update(&mut popup.list, popup.entries.len());
                }
            }
            SelectablePopupTarget::ChannelSwitcher => {
                if let Some(popup) = self.popups.channel_switcher_mut() {
                    let len = popup.visible_len();
                    update(popup.selection_mut(), len);
                }
            }
            SelectablePopupTarget::NotificationInbox => {
                let Some(tab) = self.notification_inbox_tab() else {
                    return;
                };
                if let Some(popup) = self.popups.notification_inbox_mut() {
                    let len = popup.active_len();
                    update(popup.selection_mut(tab), len);
                }
            }
            SelectablePopupTarget::ForumPostTags => {
                if let Some(popup) = self.popups.forum_post_composer_mut() {
                    update(&mut popup.tag_selection, popup.tag_order.len());
                }
            }
            SelectablePopupTarget::ThreadEditTags => {
                if let Some(popup) = self.popups.thread_edit_mut() {
                    update(&mut popup.tag_selection, popup.tag_order.len());
                }
            }
            SelectablePopupTarget::ThreadActions => {
                let len = self.thread_action_row_count();
                if let Some(selection) = self.thread_action_selection_mut() {
                    update(selection, len);
                }
            }
            SelectablePopupTarget::VoiceParticipantAudio => {
                if let Some(popup) = self.popups.voice_participant_audio_mut() {
                    update(&mut popup.selection, VOICE_PARTICIPANT_AUDIO_FIELD_COUNT);
                }
            }
            SelectablePopupTarget::SearchResults | SelectablePopupTarget::SearchSuggestions => {
                if let Some((selection, len)) = self
                    .popups
                    .search_popup_mut()
                    .and_then(|search| search.selectable_state_mut(target))
                {
                    update(selection, len);
                }
            }
        }
    }

    fn page_scrollable_popup(&mut self, target: ScrollablePopupTarget, action: SelectionAction) {
        if let Some(scroll) = self.scrollable_popup_state_mut(target) {
            scroll.page(action);
        }
    }

    fn scroll_popup_document(&mut self, target: ScrollablePopupTarget, action: SelectionAction) {
        if let Some(scroll) = self.scrollable_popup_state_mut(target) {
            match action {
                SelectionAction::Next => scroll.scroll_down(),
                SelectionAction::Previous => scroll.scroll_up(),
            }
        }
    }

    fn scrollable_popup_state_mut(
        &mut self,
        target: ScrollablePopupTarget,
    ) -> Option<&mut ScrollablePopupState> {
        match target {
            ScrollablePopupTarget::KeymapHelp => self
                .popups
                .keymap_popup_mut()
                .map(|popup| &mut popup.scroll),
            ScrollablePopupTarget::ReactionUsers => self
                .popups
                .reaction_users_popup_mut()
                .map(|popup| &mut popup.user_scroll),
            ScrollablePopupTarget::UserProfile => self
                .popups
                .user_profile_popup_mut()
                .map(|popup| &mut popup.scroll),
            ScrollablePopupTarget::ForumPostComposer => self
                .popups
                .forum_post_composer_mut()
                .map(|popup| &mut popup.scroll),
            ScrollablePopupTarget::ThreadEdit => {
                self.popups.thread_edit_mut().map(|popup| &mut popup.scroll)
            }
        }
    }

    pub(in crate::tui) fn message_action_shortcut_matches(&self, shortcut: KeyChord) -> bool {
        if self.popups.message_action_menu().is_none() {
            return false;
        }
        let actions = self.selected_message_action_items();
        action_shortcut_matches(
            self.key_bindings(),
            &actions,
            shortcut,
            |key_bindings, actions, index| key_bindings.message_action_shortcuts(actions, index),
            |action| action.is_enabled(),
        )
    }

    pub(in crate::tui) fn thread_action_shortcut_matches(&self, shortcut: KeyChord) -> bool {
        match self.popups.thread_action_menu() {
            Some(ThreadActionMenuState::Actions { .. }) => {
                let actions = self.selected_thread_action_items();
                action_shortcut_matches(
                    self.key_bindings(),
                    &actions,
                    shortcut,
                    |key_bindings, actions, index| {
                        key_bindings.thread_action_shortcuts(actions, index)
                    },
                    |action| action.is_enabled(),
                )
            }
            Some(ThreadActionMenuState::MuteDuration { .. }) => indexed_shortcut_matches(
                self.key_bindings(),
                shortcut,
                self.selected_thread_mute_duration_items().len(),
            ),
            Some(ThreadActionMenuState::NotificationSettings { .. }) => indexed_shortcut_matches(
                self.key_bindings(),
                shortcut,
                self.selected_thread_notification_items().len(),
            ),
            None => false,
        }
    }

    pub(in crate::tui) fn channel_action_shortcut_matches(&self, shortcut: KeyChord) -> bool {
        if !self.is_channel_action_menu_active() {
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
            |action| action.is_enabled(),
        )
    }

    pub(in crate::tui) fn guild_action_shortcut_matches(&self, shortcut: KeyChord) -> bool {
        if !self.is_guild_action_menu_active() {
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
            |action| action.is_enabled(),
        )
    }

    pub(in crate::tui) fn member_action_shortcut_matches(&self, shortcut: KeyChord) -> bool {
        if !self.is_member_action_menu_active() {
            return false;
        }
        let actions = self.selected_member_action_items();
        action_shortcut_matches(
            self.key_bindings(),
            &actions,
            shortcut,
            |key_bindings, actions, index| key_bindings.member_action_shortcuts(actions, index),
            |action| action.is_enabled(),
        )
    }

    pub(in crate::tui) fn message_url_shortcut_matches(&self, shortcut: KeyChord) -> bool {
        indexed_shortcut_matches(
            self.key_bindings(),
            shortcut,
            self.selected_message_url_items().len(),
        )
    }

    pub(in crate::tui) fn emoji_reaction_shortcut_matches(&self, shortcut: KeyChord) -> bool {
        self.filtered_emoji_reaction_items_slice()
            .is_some_and(|items| {
                indexed_shortcut_matches(self.key_bindings(), shortcut, items.len())
            })
    }

    pub(in crate::tui) fn poll_vote_shortcut_matches(&self, shortcut: KeyChord) -> bool {
        self.poll_vote_picker_items().is_some_and(|items| {
            indexed_shortcut_matches(self.key_bindings(), shortcut, items.len())
        })
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
    pub fn is_message_action_menu_active(&self) -> bool {
        self.popups.message_action_menu().is_some()
    }

    pub fn is_channel_action_menu_active(&self) -> bool {
        self.popups.channel_action_menu().is_some()
    }

    pub fn is_guild_action_menu_active(&self) -> bool {
        self.popups.guild_action_menu().is_some()
    }

    pub fn is_channel_action_mute_duration_phase(&self) -> bool {
        matches!(
            self.popups.channel_action_menu(),
            Some(ChannelActionMenuState::MuteDuration { .. })
        )
    }

    pub fn is_guild_action_mute_duration_phase(&self) -> bool {
        matches!(
            self.popups.guild_action_menu(),
            Some(GuildActionMenuState::MuteDuration { .. })
        )
    }
}
