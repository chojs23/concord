use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::{
    ChannelActionItem, ChannelActionKind, EmojiReactionItem, FocusPane, GuildActionItem,
    GuildActionKind, MemberActionItem, MemberActionKind, MessageActionItem, MessageActionKind,
    VoiceActionItem, VoiceActionKind,
};
use crate::discord::{ReactionEmoji, password_auth::MfaMethod};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KeyBindings;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum SelectionAction {
    Next,
    Previous,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum SelectionKeySet {
    TextSafe,
    Navigation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ScrollAction {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum GlobalAction {
    ToggleDebugLog,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum DashboardAction {
    Select(SelectionAction),
    MessageShortcut(MessageShortcutAction),
    Back,
    Quit,
    StartComposer,
    OpenLeader,
    FocusPane(FocusPane),
    CycleFocusForward,
    CycleFocusBackward,
    OpenFocusedPaneFilter,
    ResizePaneLeft,
    ResizePaneRight,
    HalfPageDown,
    HalfPageUp,
    JumpTop,
    JumpBottom,
    ScrollMessageViewportTop,
    ScrollMessageViewportBottom,
    ScrollMessageViewportDown,
    ScrollMessageViewportUp,
    ScrollHorizontalLeft,
    ScrollHorizontalRight,
    ActivateFocused,
    OpenTreeNode,
    CloseTreeNode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MessageShortcutAction {
    CopyContent,
    OpenReactionPicker,
    Reply,
    OpenDeleteConfirmation,
    Edit,
    OpenUrl,
    ViewImage,
    ShowProfile,
    OpenPinConfirmation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LeaderAction {
    TogglePane(FocusPane),
    OpenActions,
    OpenOptions,
    OpenVoiceActions,
    OpenChannelSwitcher,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ChannelSwitcherAction {
    Select(SelectionAction),
    Close,
    ActivateSelected,
    MoveQueryCursorLeft,
    MoveQueryCursorRight,
    DeleteQueryChar,
    InsertQueryChar(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LeaderActionMenuAction {
    BackOrClose,
    Close,
    ActivateShortcut(char),
    UnknownClose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MessageActionMenuAction {
    Close,
    Select(SelectionAction),
    ActivateSelected,
    ActivateShortcut(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MessageConfirmationAction {
    Confirm,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ImageViewerAction {
    Close,
    Previous,
    Next,
    DownloadSelected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ProfilePopupAction {
    Close,
    Scroll(ScrollAction),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum PaneFilterAction {
    Select(SelectionAction),
    Close,
    Confirm,
    DeleteChar,
    MoveCursorLeft,
    MoveCursorRight,
    Ignore,
    InsertChar(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum EmojiReactionPickerAction {
    Select(SelectionAction),
    Close,
    StartFilter,
    DeleteFilterChar,
    InsertFilterChar(char),
    ActivateSelected,
    ActivateShortcut(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum PollVotePickerAction {
    Close,
    Select(SelectionAction),
    ToggleSelected,
    Submit,
    ToggleShortcut(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ReactionUsersPopupAction {
    Close,
    Scroll(ScrollAction),
    PageDown,
    PageUp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum DebugLogPopupAction {
    Close,
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum OptionsPopupAction {
    Close,
    OpenCategory(char),
    Select(SelectionAction),
    ToggleSelected,
    AdjustSelected(i8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ComposerAction {
    OpenInEditor,
    InsertNewline,
    Submit,
    Close,
    ClearInput,
    RemoveLastAttachment,
    DeletePreviousChar,
    DeleteNextChar,
    MoveCursorUp,
    MoveCursorDown,
    MoveCursorWordLeft,
    MoveCursorLeft,
    MoveCursorWordRight,
    MoveCursorRight,
    MoveCursorHome,
    MoveCursorEnd,
    InsertChar(char),
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ComposerCompletionAction {
    Select(SelectionAction),
    Confirm,
    Cancel,
    FallThrough,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LoginGlobalAction {
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LoginModeSelectAction {
    StartToken,
    StartPassword,
    StartQr,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LoginTextInputAction {
    Submit,
    Back,
    DeletePreviousChar,
    InsertChar(char),
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LoginPasswordInputAction {
    Submit,
    SwitchField,
    Back,
    DeletePreviousChar,
    InsertChar(char),
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LoginMfaSelectAction {
    Choose(MfaMethod),
    Back,
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LoginBusyAction {
    Cancel,
    Ignore,
}

impl KeyBindings {
    pub(in crate::tui) fn dashboard_action(
        &self,
        key: KeyEvent,
        focus: FocusPane,
    ) -> Option<DashboardAction> {
        if focus == FocusPane::Messages
            && let Some(action) = self.message_shortcut_action(key)
        {
            return Some(DashboardAction::MessageShortcut(action));
        }

        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            return Some(DashboardAction::Select(action));
        }

        match key.code {
            KeyCode::Esc => Some(DashboardAction::Back),
            KeyCode::Char('q') => Some(DashboardAction::Quit),
            KeyCode::Char('i') => Some(DashboardAction::StartComposer),
            KeyCode::Char(' ') if is_shortcut_key(key) => Some(DashboardAction::OpenLeader),
            KeyCode::Char('1') => Some(DashboardAction::FocusPane(FocusPane::Guilds)),
            KeyCode::Char('2') => Some(DashboardAction::FocusPane(FocusPane::Channels)),
            KeyCode::Char('3') => Some(DashboardAction::FocusPane(FocusPane::Messages)),
            KeyCode::Char('4') => Some(DashboardAction::FocusPane(FocusPane::Members)),
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                Some(DashboardAction::CycleFocusBackward)
            }
            KeyCode::BackTab => Some(DashboardAction::CycleFocusBackward),
            KeyCode::Tab => Some(DashboardAction::CycleFocusForward),
            KeyCode::Char('/') if is_shortcut_key(key) => {
                Some(DashboardAction::OpenFocusedPaneFilter)
            }
            KeyCode::Char('h') | KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(DashboardAction::ResizePaneLeft)
            }
            KeyCode::Char('l') | KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(DashboardAction::ResizePaneRight)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(DashboardAction::HalfPageDown)
            }
            KeyCode::PageDown => Some(DashboardAction::HalfPageDown),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(DashboardAction::HalfPageUp)
            }
            KeyCode::PageUp => Some(DashboardAction::HalfPageUp),
            KeyCode::Home if focus == FocusPane::Messages => {
                Some(DashboardAction::ScrollMessageViewportTop)
            }
            KeyCode::Char('g') | KeyCode::Home => Some(DashboardAction::JumpTop),
            KeyCode::End if focus == FocusPane::Messages => {
                Some(DashboardAction::ScrollMessageViewportBottom)
            }
            KeyCode::Char('G') | KeyCode::End => Some(DashboardAction::JumpBottom),
            KeyCode::Char('J') if focus == FocusPane::Messages => {
                Some(DashboardAction::ScrollMessageViewportDown)
            }
            KeyCode::Char('K') if focus == FocusPane::Messages => {
                Some(DashboardAction::ScrollMessageViewportUp)
            }
            KeyCode::Char('H') => Some(DashboardAction::ScrollHorizontalLeft),
            KeyCode::Char('L') => Some(DashboardAction::ScrollHorizontalRight),
            KeyCode::Enter => Some(DashboardAction::ActivateFocused),
            KeyCode::Char('l') | KeyCode::Right => Some(DashboardAction::OpenTreeNode),
            KeyCode::Char('h') | KeyCode::Left => Some(DashboardAction::CloseTreeNode),
            _ => None,
        }
    }

    pub(in crate::tui) fn message_shortcut_action(
        &self,
        key: KeyEvent,
    ) -> Option<MessageShortcutAction> {
        if !is_shortcut_key(key) {
            return None;
        }

        match key.code {
            KeyCode::Char('y') => Some(MessageShortcutAction::CopyContent),
            KeyCode::Char('r') => Some(MessageShortcutAction::OpenReactionPicker),
            KeyCode::Char('R') => Some(MessageShortcutAction::Reply),
            KeyCode::Char('d') => Some(MessageShortcutAction::OpenDeleteConfirmation),
            KeyCode::Char('e') => Some(MessageShortcutAction::Edit),
            KeyCode::Char('o') => Some(MessageShortcutAction::OpenUrl),
            KeyCode::Char('v') => Some(MessageShortcutAction::ViewImage),
            KeyCode::Char('p') => Some(MessageShortcutAction::ShowProfile),
            KeyCode::Char('P') => Some(MessageShortcutAction::OpenPinConfirmation),
            _ => None,
        }
    }

    pub(in crate::tui) fn global_action(&self, key: KeyEvent) -> Option<GlobalAction> {
        match key.code {
            KeyCode::Char('`') => Some(GlobalAction::ToggleDebugLog),
            _ => None,
        }
    }

    pub(in crate::tui) fn leader_action(&self, key: KeyEvent) -> LeaderAction {
        match key.code {
            KeyCode::Char('1') if is_shortcut_key(key) => {
                LeaderAction::TogglePane(FocusPane::Guilds)
            }
            KeyCode::Char('2') if is_shortcut_key(key) => {
                LeaderAction::TogglePane(FocusPane::Channels)
            }
            KeyCode::Char('4') if is_shortcut_key(key) => {
                LeaderAction::TogglePane(FocusPane::Members)
            }
            KeyCode::Char('a') if is_shortcut_key(key) => LeaderAction::OpenActions,
            KeyCode::Char('o') if is_shortcut_key(key) => LeaderAction::OpenOptions,
            KeyCode::Char('v') if is_shortcut_key(key) => LeaderAction::OpenVoiceActions,
            KeyCode::Char(' ') if is_shortcut_key(key) => LeaderAction::OpenChannelSwitcher,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                LeaderAction::Close
            }
            KeyCode::Esc => LeaderAction::Close,
            _ => LeaderAction::Close,
        }
    }

    pub(in crate::tui) fn channel_switcher_action(
        &self,
        key: KeyEvent,
    ) -> Option<ChannelSwitcherAction> {
        if let Some(action) = self.selection_action(key, SelectionKeySet::TextSafe) {
            return Some(ChannelSwitcherAction::Select(action));
        }

        match key.code {
            KeyCode::Esc => Some(ChannelSwitcherAction::Close),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(ChannelSwitcherAction::Close)
            }
            KeyCode::Enter => Some(ChannelSwitcherAction::ActivateSelected),
            KeyCode::Left => Some(ChannelSwitcherAction::MoveQueryCursorLeft),
            KeyCode::Right => Some(ChannelSwitcherAction::MoveQueryCursorRight),
            KeyCode::Backspace => Some(ChannelSwitcherAction::DeleteQueryChar),
            KeyCode::Char(value) if is_shortcut_key(key) => {
                Some(ChannelSwitcherAction::InsertQueryChar(value))
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn leader_action_menu_action(
        &self,
        key: KeyEvent,
    ) -> LeaderActionMenuAction {
        match key.code {
            KeyCode::Esc => LeaderActionMenuAction::BackOrClose,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                LeaderActionMenuAction::Close
            }
            KeyCode::Char(shortcut) if is_shortcut_key(key) => {
                LeaderActionMenuAction::ActivateShortcut(shortcut)
            }
            code if is_left_key(code) => LeaderActionMenuAction::BackOrClose,
            _ => LeaderActionMenuAction::UnknownClose,
        }
    }

    pub(in crate::tui) fn message_action_menu_action(
        &self,
        key: KeyEvent,
    ) -> Option<MessageActionMenuAction> {
        if key.code == KeyCode::Esc {
            return Some(MessageActionMenuAction::Close);
        }
        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            return Some(MessageActionMenuAction::Select(action));
        }

        match key.code {
            code if is_confirm_key(code) => Some(MessageActionMenuAction::ActivateSelected),
            KeyCode::Char(shortcut) if is_shortcut_key(key) => {
                Some(MessageActionMenuAction::ActivateShortcut(shortcut))
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn message_confirmation_action(
        &self,
        key: KeyEvent,
    ) -> Option<MessageConfirmationAction> {
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') if is_shortcut_key(key) => {
                Some(MessageConfirmationAction::Confirm)
            }
            KeyCode::Esc | KeyCode::Char('n') if is_shortcut_key(key) => {
                Some(MessageConfirmationAction::Cancel)
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn image_viewer_action(&self, key: KeyEvent) -> Option<ImageViewerAction> {
        match key.code {
            KeyCode::Esc => Some(ImageViewerAction::Close),
            code if is_left_key(code) => Some(ImageViewerAction::Previous),
            code if is_right_key(code) => Some(ImageViewerAction::Next),
            KeyCode::Char('d') if is_shortcut_key(key) => Some(ImageViewerAction::DownloadSelected),
            _ => None,
        }
    }

    pub(in crate::tui) fn profile_popup_action(&self, key: KeyEvent) -> Option<ProfilePopupAction> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Some(ProfilePopupAction::Close),
            _ => self.scroll_action(key).map(ProfilePopupAction::Scroll),
        }
    }

    pub(in crate::tui) fn pane_filter_action(&self, key: KeyEvent) -> Option<PaneFilterAction> {
        if let Some(action) = self.selection_action(key, SelectionKeySet::TextSafe) {
            return Some(PaneFilterAction::Select(action));
        }

        match key.code {
            KeyCode::Esc => Some(PaneFilterAction::Close),
            KeyCode::Enter => Some(PaneFilterAction::Confirm),
            KeyCode::Backspace => Some(PaneFilterAction::DeleteChar),
            KeyCode::Left => Some(PaneFilterAction::MoveCursorLeft),
            KeyCode::Right => Some(PaneFilterAction::MoveCursorRight),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(PaneFilterAction::Ignore)
            }
            KeyCode::Char(value) if is_shortcut_key(key) => {
                Some(PaneFilterAction::InsertChar(value))
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn emoji_reaction_picker_action(
        &self,
        key: KeyEvent,
        filtering: bool,
    ) -> Option<EmojiReactionPickerAction> {
        let key_set = if filtering {
            SelectionKeySet::TextSafe
        } else {
            SelectionKeySet::Navigation
        };
        if let Some(action) = self.selection_action(key, key_set) {
            return Some(EmojiReactionPickerAction::Select(action));
        }

        match key.code {
            KeyCode::Esc => Some(EmojiReactionPickerAction::Close),
            KeyCode::Backspace if filtering => Some(EmojiReactionPickerAction::DeleteFilterChar),
            KeyCode::Char('/') if is_shortcut_key(key) && !filtering => {
                Some(EmojiReactionPickerAction::StartFilter)
            }
            KeyCode::Char(value) if is_shortcut_key(key) && filtering => {
                Some(EmojiReactionPickerAction::InsertFilterChar(value))
            }
            code if is_confirm_key(code) => Some(EmojiReactionPickerAction::ActivateSelected),
            KeyCode::Char(shortcut) if is_shortcut_key(key) => {
                Some(EmojiReactionPickerAction::ActivateShortcut(shortcut))
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn poll_vote_picker_action(
        &self,
        key: KeyEvent,
    ) -> Option<PollVotePickerAction> {
        if key.code == KeyCode::Esc {
            return Some(PollVotePickerAction::Close);
        }
        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            return Some(PollVotePickerAction::Select(action));
        }

        match key.code {
            KeyCode::Char(' ') => Some(PollVotePickerAction::ToggleSelected),
            KeyCode::Enter => Some(PollVotePickerAction::Submit),
            KeyCode::Char(shortcut) if is_shortcut_key(key) => {
                Some(PollVotePickerAction::ToggleShortcut(shortcut))
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn reaction_users_popup_action(
        &self,
        key: KeyEvent,
    ) -> Option<ReactionUsersPopupAction> {
        if key.code == KeyCode::Esc {
            return Some(ReactionUsersPopupAction::Close);
        }
        if let Some(action) = self.scroll_action(key) {
            return Some(ReactionUsersPopupAction::Scroll(action));
        }

        match key.code {
            KeyCode::PageDown => Some(ReactionUsersPopupAction::PageDown),
            KeyCode::PageUp => Some(ReactionUsersPopupAction::PageUp),
            _ => None,
        }
    }

    pub(in crate::tui) fn debug_log_popup_action(&self, key: KeyEvent) -> DebugLogPopupAction {
        match key.code {
            KeyCode::Esc | KeyCode::Char('`') => DebugLogPopupAction::Close,
            _ => DebugLogPopupAction::Ignore,
        }
    }

    pub(in crate::tui) fn options_popup_action(
        &self,
        key: KeyEvent,
        category_picker_open: bool,
    ) -> Option<OptionsPopupAction> {
        if matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('o')
        ) {
            return Some(OptionsPopupAction::Close);
        }
        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            return Some(OptionsPopupAction::Select(action));
        }

        match key.code {
            KeyCode::Char(shortcut @ ('d' | 'D' | 'n' | 'N' | 'v' | 'V'))
                if is_shortcut_key(key) && category_picker_open =>
            {
                Some(OptionsPopupAction::OpenCategory(shortcut))
            }
            KeyCode::Char('h') | KeyCode::Char('H') if is_shortcut_key(key) => Some(
                OptionsPopupAction::AdjustSelected(if key.code == KeyCode::Char('H') {
                    -10
                } else {
                    -1
                }),
            ),
            KeyCode::Char('l') | KeyCode::Char('L') if is_shortcut_key(key) => Some(
                OptionsPopupAction::AdjustSelected(if key.code == KeyCode::Char('L') {
                    10
                } else {
                    1
                }),
            ),
            code if is_confirm_key(code) => Some(OptionsPopupAction::ToggleSelected),
            _ => None,
        }
    }

    pub(in crate::tui) fn composer_action(&self, key: KeyEvent) -> ComposerAction {
        match key.code {
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ComposerAction::OpenInEditor
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                ComposerAction::InsertNewline
            }
            KeyCode::Enter => ComposerAction::Submit,
            KeyCode::Esc => ComposerAction::Close,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ComposerAction::ClearInput
            }
            KeyCode::Backspace if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ComposerAction::RemoveLastAttachment
            }
            KeyCode::Backspace => ComposerAction::DeletePreviousChar,
            KeyCode::Delete => ComposerAction::DeleteNextChar,
            KeyCode::Up => ComposerAction::MoveCursorUp,
            KeyCode::Down => ComposerAction::MoveCursorDown,
            KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ComposerAction::MoveCursorWordLeft
            }
            KeyCode::Left => ComposerAction::MoveCursorLeft,
            KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ComposerAction::MoveCursorWordRight
            }
            KeyCode::Right => ComposerAction::MoveCursorRight,
            KeyCode::Home => ComposerAction::MoveCursorHome,
            KeyCode::End => ComposerAction::MoveCursorEnd,
            KeyCode::Char(value) if is_shortcut_key(key) => ComposerAction::InsertChar(value),
            _ => ComposerAction::Ignore,
        }
    }

    pub(in crate::tui) fn composer_completion_action(
        &self,
        key: KeyEvent,
    ) -> ComposerCompletionAction {
        if let Some(action) = self.selection_action(key, SelectionKeySet::TextSafe) {
            return ComposerCompletionAction::Select(action);
        }

        match key.code {
            KeyCode::Tab | KeyCode::Enter => ComposerCompletionAction::Confirm,
            KeyCode::Esc => ComposerCompletionAction::Cancel,
            _ => ComposerCompletionAction::FallThrough,
        }
    }

    pub(in crate::tui) fn login_global_action(&self, key: KeyEvent) -> Option<LoginGlobalAction> {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(LoginGlobalAction::Cancel)
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn login_mode_select_action(
        &self,
        key: KeyEvent,
    ) -> Option<LoginModeSelectAction> {
        match key.code {
            KeyCode::Char(value) if value.eq_ignore_ascii_case(&'t') => {
                Some(LoginModeSelectAction::StartToken)
            }
            KeyCode::Char(value) if value.eq_ignore_ascii_case(&'e') => {
                Some(LoginModeSelectAction::StartPassword)
            }
            KeyCode::Char(value) if value.eq_ignore_ascii_case(&'q') => {
                Some(LoginModeSelectAction::StartQr)
            }
            KeyCode::Esc => Some(LoginModeSelectAction::Cancel),
            _ => None,
        }
    }

    pub(in crate::tui) fn login_text_input_action(&self, key: KeyEvent) -> LoginTextInputAction {
        match key.code {
            KeyCode::Enter => LoginTextInputAction::Submit,
            KeyCode::Esc => LoginTextInputAction::Back,
            KeyCode::Backspace => LoginTextInputAction::DeletePreviousChar,
            KeyCode::Char(value) => LoginTextInputAction::InsertChar(value),
            _ => LoginTextInputAction::Ignore,
        }
    }

    pub(in crate::tui) fn login_password_input_action(
        &self,
        key: KeyEvent,
    ) -> LoginPasswordInputAction {
        match key.code {
            KeyCode::Enter => LoginPasswordInputAction::Submit,
            KeyCode::Tab | KeyCode::Down | KeyCode::Up => LoginPasswordInputAction::SwitchField,
            KeyCode::Esc => LoginPasswordInputAction::Back,
            KeyCode::Backspace => LoginPasswordInputAction::DeletePreviousChar,
            KeyCode::Char(value) => LoginPasswordInputAction::InsertChar(value),
            _ => LoginPasswordInputAction::Ignore,
        }
    }

    pub(in crate::tui) fn login_mfa_select_action(&self, key: KeyEvent) -> LoginMfaSelectAction {
        match key.code {
            KeyCode::Char(value) if value.eq_ignore_ascii_case(&'t') => {
                LoginMfaSelectAction::Choose(MfaMethod::Totp)
            }
            KeyCode::Char(value) if value.eq_ignore_ascii_case(&'s') => {
                LoginMfaSelectAction::Choose(MfaMethod::Sms)
            }
            KeyCode::Esc => LoginMfaSelectAction::Back,
            _ => LoginMfaSelectAction::Ignore,
        }
    }

    pub(in crate::tui) fn login_busy_action(&self, key: KeyEvent) -> LoginBusyAction {
        match key.code {
            KeyCode::Esc => LoginBusyAction::Cancel,
            _ => LoginBusyAction::Ignore,
        }
    }

    pub(in crate::tui) fn selection_action(
        &self,
        key: KeyEvent,
        key_set: SelectionKeySet,
    ) -> Option<SelectionAction> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Down => Some(SelectionAction::Next),
            KeyCode::Up => Some(SelectionAction::Previous),
            KeyCode::Char('n') if ctrl => Some(SelectionAction::Next),
            KeyCode::Char('p') if ctrl => Some(SelectionAction::Previous),
            KeyCode::Char('j')
                if key_set == SelectionKeySet::Navigation && is_shortcut_key(key) =>
            {
                Some(SelectionAction::Next)
            }
            KeyCode::Char('k')
                if key_set == SelectionKeySet::Navigation && is_shortcut_key(key) =>
            {
                Some(SelectionAction::Previous)
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn scroll_action(&self, key: KeyEvent) -> Option<ScrollAction> {
        match key.code {
            KeyCode::Char('j') if is_shortcut_key(key) => Some(ScrollAction::Down),
            KeyCode::Char('k') if is_shortcut_key(key) => Some(ScrollAction::Up),
            KeyCode::Down => Some(ScrollAction::Down),
            KeyCode::Up => Some(ScrollAction::Up),
            _ => None,
        }
    }

    pub fn leader_root_shortcuts(&self) -> [(&'static str, &'static str); 7] {
        [
            ("1", "toggle Servers"),
            ("2", "toggle Channels"),
            ("4", "toggle Members"),
            ("a", "Actions"),
            ("o", "Options"),
            ("v", "Voice"),
            ("Space", "Switch channels"),
        ]
    }

    pub fn message_confirmation_confirm_label(&self) -> &'static str {
        "Enter/y"
    }

    pub fn message_confirmation_cancel_label(&self) -> &'static str {
        "Esc/n"
    }

    pub fn image_viewer_download_hint(&self) -> &'static str {
        "[d] download image"
    }

    pub fn unread_mark_as_read_hint(&self) -> &'static str {
        "channel action (a) to mark as read "
    }

    pub fn start_composer_key_label(&self) -> &'static str {
        "i"
    }

    pub fn emoji_reaction_filter_prefix(&self) -> &'static str {
        "/"
    }

    pub fn login_token_choice_prefix(&self) -> &'static str {
        "[t] "
    }

    pub fn login_password_choice_prefix(&self) -> &'static str {
        "[e] "
    }

    pub fn login_qr_choice_prefix(&self) -> &'static str {
        "[q] "
    }

    pub fn login_totp_choice_prefix(&self) -> &'static str {
        "[t] "
    }

    pub fn login_sms_choice_prefix(&self) -> &'static str {
        "[s] "
    }

    pub fn login_cancel_quit_label(&self) -> &'static str {
        "Esc cancel | Ctrl-C quit"
    }

    pub fn login_token_input_label(&self) -> &'static str {
        "Enter save | Esc back | Ctrl-C quit"
    }

    pub fn login_password_input_label(&self) -> &'static str {
        "Tab switch field | Enter login | Esc back | Ctrl-C quit"
    }

    pub fn login_back_quit_label(&self) -> &'static str {
        "Esc back | Ctrl-C quit"
    }

    pub fn login_mfa_code_label(&self) -> &'static str {
        "Enter verify | Esc choose method | Ctrl-C quit"
    }

    pub fn options_category_shortcut(&self, shortcut: char) -> Option<OptionsCategoryShortcut> {
        match shortcut {
            'd' | 'D' => Some(OptionsCategoryShortcut::Display),
            'n' | 'N' => Some(OptionsCategoryShortcut::Notifications),
            'v' | 'V' => Some(OptionsCategoryShortcut::Voice),
            _ => None,
        }
    }

    pub fn options_category_shortcut_label(
        &self,
        category: OptionsCategoryShortcut,
    ) -> &'static str {
        match category {
            OptionsCategoryShortcut::Display => "d",
            OptionsCategoryShortcut::Notifications => "n",
            OptionsCategoryShortcut::Voice => "v",
        }
    }

    pub fn message_action_shortcut(
        &self,
        actions: &[MessageActionItem],
        index: usize,
    ) -> Option<char> {
        let action = actions.get(index)?;
        unique_preferred_shortcut(
            self.message_action_preferred_shortcut(action.kind),
            actions
                .iter()
                .map(|item| self.message_action_preferred_shortcut(item.kind)),
        )
        .or_else(|| self.indexed_shortcut(index))
    }

    fn message_action_preferred_shortcut(&self, kind: MessageActionKind) -> Option<char> {
        match kind {
            MessageActionKind::Reply => Some('R'),
            MessageActionKind::Edit => Some('e'),
            MessageActionKind::Delete => Some('d'),
            MessageActionKind::OpenThread => Some('t'),
            MessageActionKind::ViewImage => Some('v'),
            MessageActionKind::OpenUrl => Some('o'),
            MessageActionKind::DownloadAttachment(_) => Some('f'),
            MessageActionKind::AddReaction => Some('r'),
            MessageActionKind::RemoveReaction(_) => Some('x'),
            MessageActionKind::ShowReactionUsers => Some('u'),
            MessageActionKind::ShowProfile => Some('p'),
            MessageActionKind::SetPinned(_) => Some('P'),
            MessageActionKind::VotePollAnswer(_) => None,
            MessageActionKind::OpenPollVotePicker => Some('c'),
        }
    }

    pub fn channel_action_shortcut(
        &self,
        actions: &[ChannelActionItem],
        index: usize,
    ) -> Option<char> {
        let action = actions.get(index)?;
        unique_preferred_shortcut(
            Some(self.channel_action_preferred_shortcut(action.kind)),
            actions
                .iter()
                .map(|item| Some(self.channel_action_preferred_shortcut(item.kind))),
        )
        .or_else(|| self.indexed_shortcut(index))
    }

    fn channel_action_preferred_shortcut(&self, kind: ChannelActionKind) -> char {
        match kind {
            ChannelActionKind::JoinVoice => 'j',
            ChannelActionKind::LeaveVoice => 'l',
            ChannelActionKind::LoadPinnedMessages => 'p',
            ChannelActionKind::ShowThreads => 't',
            ChannelActionKind::MarkAsRead => 'm',
            ChannelActionKind::ToggleMute => 'u',
        }
    }

    pub fn voice_action_shortcut(&self, actions: &[VoiceActionItem], index: usize) -> Option<char> {
        let action = actions.get(index)?;
        unique_preferred_shortcut(
            Some(self.voice_action_preferred_shortcut(action.kind)),
            actions
                .iter()
                .map(|item| Some(self.voice_action_preferred_shortcut(item.kind))),
        )
        .or_else(|| self.indexed_shortcut(index))
    }

    fn voice_action_preferred_shortcut(&self, kind: VoiceActionKind) -> char {
        match kind {
            VoiceActionKind::QuickDeafen => 'd',
            VoiceActionKind::QuickMute => 'm',
            VoiceActionKind::QuickLeave => 'l',
        }
    }

    pub fn guild_action_shortcut(&self, actions: &[GuildActionItem], index: usize) -> Option<char> {
        let action = actions.get(index)?;
        let preferred = self.guild_action_preferred_shortcut(action.kind)?;
        unique_preferred_shortcut(
            Some(preferred),
            actions
                .iter()
                .map(|item| self.guild_action_preferred_shortcut(item.kind)),
        )
        .or_else(|| self.indexed_shortcut(index))
    }

    fn guild_action_preferred_shortcut(&self, kind: GuildActionKind) -> Option<char> {
        match kind {
            GuildActionKind::MarkAsRead => Some('m'),
            GuildActionKind::ToggleMute => Some('u'),
            GuildActionKind::NoActionsYet => None,
        }
    }

    pub fn member_action_shortcut(
        &self,
        actions: &[MemberActionItem],
        index: usize,
    ) -> Option<char> {
        let action = actions.get(index)?;
        unique_preferred_shortcut(
            Some(self.member_action_preferred_shortcut(action.kind)),
            actions
                .iter()
                .map(|item| Some(self.member_action_preferred_shortcut(item.kind))),
        )
        .or_else(|| self.indexed_shortcut(index))
    }

    fn member_action_preferred_shortcut(&self, kind: MemberActionKind) -> char {
        match kind {
            MemberActionKind::ShowProfile => 'p',
        }
    }

    pub fn indexed_shortcut(&self, index: usize) -> Option<char> {
        match index {
            0..=8 => char::from_digit(u32::try_from(index + 1).ok()?, 10),
            9 => Some('0'),
            _ => None,
        }
    }

    pub fn emoji_reaction_shortcut(
        &self,
        reactions: &[EmojiReactionItem],
        existing_reactions: &[ReactionEmoji],
        index: usize,
    ) -> Option<char> {
        let reaction = reactions.get(index)?;
        if let Some(existing_index) = existing_reactions
            .iter()
            .position(|existing| existing == &reaction.emoji)
        {
            return self.qwerty_shortcut(existing_index);
        }

        let regular_index = reactions[..index]
            .iter()
            .filter(|item| !existing_reactions.contains(&item.emoji))
            .count();
        self.indexed_shortcut(regular_index)
    }

    fn qwerty_shortcut(&self, index: usize) -> Option<char> {
        const SHORTCUTS: &[u8] = b"qwertyuiop";
        SHORTCUTS.get(index).map(|shortcut| char::from(*shortcut))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionsCategoryShortcut {
    Display,
    Notifications,
    Voice,
}

fn is_left_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('h') | KeyCode::Left)
}

fn is_right_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('l') | KeyCode::Right)
}

fn is_confirm_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Enter | KeyCode::Char(' '))
}

fn is_shortcut_key(key: KeyEvent) -> bool {
    key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT
}

fn unique_preferred_shortcut(
    preferred: Option<char>,
    shortcuts: impl IntoIterator<Item = Option<char>>,
) -> Option<char> {
    let preferred = preferred?;
    let matches = shortcuts
        .into_iter()
        .filter(|shortcut| shortcut.is_some_and(|shortcut| shortcut == preferred))
        .count();
    (matches == 1).then_some(preferred)
}
