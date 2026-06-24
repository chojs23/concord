use super::*;

impl KeyBindings {
    pub(in crate::tui) fn binding_summaries(&self) -> Vec<KeymapBindingSummary> {
        let mut summaries = self
            .keymap
            .specs
            .iter()
            .map(|(action, spec)| KeymapBindingSummary {
                scope: "keymap",
                action: action.name().to_owned(),
                keys: spec
                    .sequences
                    .iter()
                    .map(|sequence| keymap_sequence_label(sequence, Some(self.keymap.leader)))
                    .collect(),
            })
            .collect::<Vec<_>>();

        summaries.extend(self.action_shortcuts.binding_summaries());
        summaries.extend(self.composer.binding_summaries());
        summaries
    }

    fn binding_label(&self, action: UiAction) -> String {
        self.keymap.first_sequence_label(action)
    }

    pub(in crate::tui) fn leader_keymap_prefix(&self) -> Vec<KeyChord> {
        vec![self.keymap.leader]
    }

    pub(in crate::tui) fn is_leader_key(&self, key: KeyEvent) -> bool {
        self.keymap.leader.matches(key)
    }

    #[cfg(test)]
    pub(in crate::tui) fn keymap_lookup_direct_key(&self, key: KeyEvent) -> Option<UiAction> {
        let sequence = [self.keymap_chord_for_event(key)];
        match self.keymap.lookup(&sequence) {
            Some(KeyMapLookup::Action(action)) => Some(action),
            _ => None,
        }
    }

    pub(in crate::tui) fn keymap_lookup_root_key(&self, key: KeyEvent) -> Option<KeyMapLookup> {
        let sequence = [self.keymap_chord_for_event(key)];
        self.keymap.lookup(&sequence)
    }

    pub(in crate::tui) fn keymap_lookup_with_key(
        &self,
        prefix: &[KeyChord],
        key: KeyEvent,
    ) -> Option<KeyMapLookup> {
        let mut sequence = prefix.to_vec();
        sequence.push(
            KeyChord {
                code: key.code,
                modifiers: key.modifiers,
            }
            .canonical(),
        );
        self.keymap.lookup(&sequence)
    }

    pub(in crate::tui) fn keymap_chord_for_event(&self, key: KeyEvent) -> KeyChord {
        KeyChord {
            code: key.code,
            modifiers: key.modifiers,
        }
        .canonical()
    }

    pub(in crate::tui) fn keymap_prefix_title(&self, prefix: &[KeyChord]) -> String {
        if let Some((_, title)) = self
            .keymap
            .group_titles
            .iter()
            .find(|(sequence, _)| sequence.as_slice() == prefix)
        {
            return title.clone();
        }
        if prefix == self.leader_keymap_prefix() {
            return "Leader".to_owned();
        }
        prefix.iter().map(|chord| chord.title_label()).collect()
    }

    pub(in crate::tui) fn leader_keymap_children(
        &self,
        prefix: &[KeyChord],
    ) -> Vec<LeaderShortcutItem> {
        self.keymap.children(prefix)
    }

    pub(in crate::tui) fn dashboard_action_for_ui_action(
        &self,
        action: UiAction,
        focus: FocusPane,
    ) -> Option<DashboardAction> {
        if focus == FocusPane::Messages
            && let Some(message_action) = action.message_action_kind()
        {
            return Some(DashboardAction::MessageShortcut(message_action));
        }

        match action {
            UiAction::StartComposer => Some(DashboardAction::StartComposer),
            UiAction::OpenPaneFilter => Some(DashboardAction::OpenFocusedPaneFilter),
            UiAction::FocusGuildPane => Some(DashboardAction::FocusPane(FocusPane::Guilds)),
            UiAction::FocusChannelPane => Some(DashboardAction::FocusPane(FocusPane::Channels)),
            UiAction::FocusMessagePane => Some(DashboardAction::FocusPane(FocusPane::Messages)),
            UiAction::FocusMemberPane => Some(DashboardAction::FocusPane(FocusPane::Members)),
            UiAction::SelectNext => Some(DashboardAction::Select(SelectionAction::Next)),
            UiAction::SelectPrevious => Some(DashboardAction::Select(SelectionAction::Previous)),
            UiAction::CycleFocusNext => Some(DashboardAction::CycleFocusForward),
            UiAction::CycleFocusPrevious => Some(DashboardAction::CycleFocusBackward),
            UiAction::HalfPageDown => Some(DashboardAction::HalfPageDown),
            UiAction::HalfPageUp => Some(DashboardAction::HalfPageUp),
            UiAction::ScrollViewportDown => Some(DashboardAction::ScrollViewportDown),
            UiAction::ScrollViewportUp => Some(DashboardAction::ScrollViewportUp),
            UiAction::JumpTop => Some(DashboardAction::JumpTop),
            UiAction::JumpBottom => Some(DashboardAction::JumpBottom),
            UiAction::ScrollHorizontalLeft => Some(DashboardAction::ScrollHorizontalLeft),
            UiAction::ScrollHorizontalRight => Some(DashboardAction::ScrollHorizontalRight),
            UiAction::ResizePaneLeft => Some(DashboardAction::ResizePaneLeft),
            UiAction::ResizePaneRight => Some(DashboardAction::ResizePaneRight),
            UiAction::Quit => Some(DashboardAction::Quit),
            _ => None,
        }
    }

    pub(in crate::tui) fn dashboard_action(&self, key: KeyEvent) -> Option<DashboardAction> {
        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            return Some(DashboardAction::Select(action));
        }

        match key.code {
            KeyCode::Esc => Some(DashboardAction::Back),
            KeyCode::Enter => Some(DashboardAction::ActivateFocused),
            _ => None,
        }
    }

    pub(in crate::tui) fn global_action(&self, key: KeyEvent) -> Option<GlobalAction> {
        match key.code {
            KeyCode::Char('`') => Some(GlobalAction::ToggleDebugLog),
            _ => None,
        }
    }

    pub(in crate::tui) fn channel_switcher_action(
        &self,
        key: KeyEvent,
    ) -> Option<ChannelSwitcherAction> {
        if self.is_text_entry_popup_close_key(key) {
            return Some(ChannelSwitcherAction::Close);
        }
        if let Some(action) = self.selection_action(key, SelectionKeySet::TextSafe) {
            return Some(ChannelSwitcherAction::Select(action));
        }

        match key.code {
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

    pub(in crate::tui) fn notification_inbox_action(
        &self,
        key: KeyEvent,
    ) -> Option<NotificationInboxAction> {
        if self.is_popup_close_key(key) {
            return Some(NotificationInboxAction::Close);
        }
        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            return Some(NotificationInboxAction::Select(action));
        }
        match key.code {
            KeyCode::Enter => Some(NotificationInboxAction::ActivateSelected),
            // Left/Right (and h/l) flip between the Unreads and Mentions tabs.
            KeyCode::Left => Some(NotificationInboxAction::SwitchTab(
                SelectionAction::Previous,
            )),
            KeyCode::Right => Some(NotificationInboxAction::SwitchTab(SelectionAction::Next)),
            KeyCode::Tab => Some(NotificationInboxAction::SwitchTab(SelectionAction::Next)),
            KeyCode::BackTab => Some(NotificationInboxAction::SwitchTab(
                SelectionAction::Previous,
            )),
            KeyCode::Char('h') | KeyCode::Char('H') if is_shortcut_key(key) => Some(
                NotificationInboxAction::SwitchTab(SelectionAction::Previous),
            ),
            KeyCode::Char('l') | KeyCode::Char('L') if is_shortcut_key(key) => {
                Some(NotificationInboxAction::SwitchTab(SelectionAction::Next))
            }
            KeyCode::Char('r') | KeyCode::Char('R') if is_shortcut_key(key) => {
                Some(NotificationInboxAction::MarkSelectedRead)
            }
            KeyCode::Char('a') | KeyCode::Char('A') if is_shortcut_key(key) => {
                Some(NotificationInboxAction::MarkAllRead)
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn search_popup_action(&self, key: KeyEvent) -> Option<SearchPopupAction> {
        if self.is_text_entry_popup_close_key(key) {
            return Some(SearchPopupAction::Close);
        }
        if let Some(action) = self.selection_action(key, SelectionKeySet::TextSafe) {
            return Some(SearchPopupAction::Select(action));
        }
        if let Some(action) = self.popup_page_action(key) {
            return Some(SearchPopupAction::Page(action));
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(SearchPopupAction::Close)
            }
            KeyCode::Enter => Some(SearchPopupAction::ActivateSelected),
            KeyCode::Tab => Some(SearchPopupAction::NextField),
            KeyCode::BackTab => Some(SearchPopupAction::PreviousField),
            KeyCode::Left => Some(SearchPopupAction::MoveCursorLeft),
            KeyCode::Right => Some(SearchPopupAction::MoveCursorRight),
            KeyCode::Backspace => Some(SearchPopupAction::DeleteChar),
            KeyCode::Char(value) if is_shortcut_key(key) => {
                Some(SearchPopupAction::InsertChar(value))
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
            KeyCode::Char(_) => {
                LeaderActionMenuAction::ActivateShortcut(self.keymap_chord_for_event(key))
            }
            code if is_left_key(code) => LeaderActionMenuAction::BackOrClose,
            _ => LeaderActionMenuAction::UnknownClose,
        }
    }

    pub(in crate::tui) fn popup_list_action(&self, key: KeyEvent) -> Option<PopupListAction> {
        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            if self.is_popup_close_key(key) {
                // Character keys may also be popup action shortcuts, so let the
                // handler try the shortcut first and fall back to close there.
                return match key.code {
                    KeyCode::Char(_) => Some(PopupListAction::ActivateShortcut(
                        self.keymap_chord_for_event(key),
                    )),
                    _ => Some(PopupListAction::Close),
                };
            }
            return Some(PopupListAction::Select(action));
        }

        match key.code {
            code if is_confirm_key(code) => Some(PopupListAction::ActivateSelected),
            KeyCode::Char(_) => Some(PopupListAction::ActivateShortcut(
                self.keymap_chord_for_event(key),
            )),
            _ if self.is_popup_close_key(key) => Some(PopupListAction::Close),
            _ => None,
        }
    }

    pub(in crate::tui) fn message_confirmation_action(
        &self,
        key: KeyEvent,
    ) -> Option<MessageConfirmationAction> {
        if self.is_popup_close_key(key) {
            return Some(MessageConfirmationAction::Cancel);
        }
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') if is_shortcut_key(key) => {
                Some(MessageConfirmationAction::Confirm)
            }
            KeyCode::Char('n') if is_shortcut_key(key) => Some(MessageConfirmationAction::Cancel),
            _ => None,
        }
    }

    pub(in crate::tui) fn attachment_viewer_action(
        &self,
        key: KeyEvent,
    ) -> Option<AttachmentViewerAction> {
        if self.is_popup_close_key(key) {
            return Some(AttachmentViewerAction::Close);
        }
        match key.code {
            code if is_left_key(code) => Some(AttachmentViewerAction::Previous),
            code if is_right_key(code) => Some(AttachmentViewerAction::Next),
            KeyCode::Char('x') if is_shortcut_key(key) => {
                Some(AttachmentViewerAction::PlaySelected)
            }
            KeyCode::Char('d') if is_shortcut_key(key) => {
                Some(AttachmentViewerAction::DownloadSelected)
            }
            KeyCode::Char('z') if is_shortcut_key(key) => Some(AttachmentViewerAction::ToggleZoom),
            KeyCode::Char('+') | KeyCode::Char('=') if is_shortcut_key(key) => {
                Some(AttachmentViewerAction::ZoomIn)
            }
            KeyCode::Char('-') | KeyCode::Char('_') if is_shortcut_key(key) => {
                Some(AttachmentViewerAction::ZoomOut)
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn profile_popup_action(
        &self,
        key: KeyEvent,
        editing: bool,
    ) -> Option<ProfilePopupAction> {
        if editing {
            return Self::profile_edit_action_from_composer_action(self.composer_action(key));
        }

        if self.is_popup_close_key(key) {
            return Some(ProfilePopupAction::Close);
        }
        match key.code {
            KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(ProfilePopupAction::PasteClipboard)
            }
            KeyCode::Enter => Some(ProfilePopupAction::StartOrCommitEdit),
            KeyCode::Char('g') if is_shortcut_key(key) => {
                Some(ProfilePopupAction::SwitchTab(ProfilePopupTabAction::Global))
            }
            KeyCode::Char('v') if is_shortcut_key(key) => {
                Some(ProfilePopupAction::SwitchTab(ProfilePopupTabAction::Guild))
            }
            KeyCode::Char('s') if is_shortcut_key(key) => Some(ProfilePopupAction::Save),
            KeyCode::Char('o') if is_shortcut_key(key) => Some(ProfilePopupAction::SignOut),
            _ => self
                .selection_action(key, SelectionKeySet::Navigation)
                .map(|action| match action {
                    SelectionAction::Next => ProfilePopupAction::NextField,
                    SelectionAction::Previous => ProfilePopupAction::PreviousField,
                })
                .or_else(|| self.scroll_action(key).map(ProfilePopupAction::Scroll)),
        }
    }

    fn profile_edit_action_from_composer_action(
        action: ComposerAction,
    ) -> Option<ProfilePopupAction> {
        match action {
            ComposerAction::PasteClipboard => Some(ProfilePopupAction::PasteClipboard),
            ComposerAction::Submit => Some(ProfilePopupAction::StartOrCommitEdit),
            ComposerAction::Close => Some(ProfilePopupAction::Close),
            ComposerAction::DeletePreviousChar => Some(ProfilePopupAction::DeleteChar),
            ComposerAction::DeletePreviousWord => Some(ProfilePopupAction::DeletePreviousWord),
            ComposerAction::MoveCursorWordLeft => Some(ProfilePopupAction::MoveCursorWordLeft),
            ComposerAction::MoveCursorLeft => Some(ProfilePopupAction::MoveCursorLeft),
            ComposerAction::MoveCursorWordRight => Some(ProfilePopupAction::MoveCursorWordRight),
            ComposerAction::MoveCursorRight => Some(ProfilePopupAction::MoveCursorRight),
            ComposerAction::MoveCursorHome => Some(ProfilePopupAction::MoveCursorHome),
            ComposerAction::MoveCursorEnd => Some(ProfilePopupAction::MoveCursorEnd),
            ComposerAction::InsertChar(value) => Some(ProfilePopupAction::InsertChar(value)),
            ComposerAction::OpenInEditor
            | ComposerAction::InsertNewline
            | ComposerAction::ClearInput
            | ComposerAction::RemoveLastAttachment
            | ComposerAction::MoveCursorUp
            | ComposerAction::MoveCursorDown
            | ComposerAction::Ignore => None,
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
        filter_editing: bool,
    ) -> Option<EmojiReactionPickerAction> {
        let key_set = if filter_editing {
            SelectionKeySet::TextSafe
        } else {
            SelectionKeySet::Navigation
        };
        if let Some(action) = self.selection_action(key, key_set) {
            return Some(EmojiReactionPickerAction::Select(action));
        }

        let close_key = if filter_editing {
            self.is_text_entry_popup_close_key(key)
        } else {
            self.is_popup_close_key(key)
        };
        if close_key {
            return Some(EmojiReactionPickerAction::Close);
        }

        match key.code {
            KeyCode::Enter if filter_editing => Some(EmojiReactionPickerAction::CommitFilter),
            KeyCode::Backspace if filter_editing => {
                Some(EmojiReactionPickerAction::DeleteFilterChar)
            }
            KeyCode::Char('/') if is_shortcut_key(key) && !filter_editing => {
                Some(EmojiReactionPickerAction::StartFilter)
            }
            KeyCode::Char(value) if is_shortcut_key(key) && filter_editing => {
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
        if self.is_popup_close_key(key) {
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
        if self.is_popup_close_key(key) {
            return Some(ReactionUsersPopupAction::Close);
        }
        if let Some(action) = self.scroll_action(key) {
            return Some(ReactionUsersPopupAction::Scroll(action));
        }

        None
    }

    pub(in crate::tui) fn debug_log_popup_action(
        &self,
        key: KeyEvent,
    ) -> Option<DebugLogPopupAction> {
        if self.is_popup_close_key(key) || key.code == KeyCode::Char('`') {
            Some(DebugLogPopupAction::Close)
        } else {
            None
        }
    }

    pub(in crate::tui) fn options_popup_action(
        &self,
        key: KeyEvent,
        category_picker_open: bool,
    ) -> Option<OptionsPopupAction> {
        if self.is_popup_close_key(key) || key.code == KeyCode::Char('o') {
            return Some(OptionsPopupAction::Close);
        }
        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            return Some(OptionsPopupAction::Select(action));
        }
        match key.code {
            KeyCode::Char(shortcut @ ('d' | 'D' | 'c' | 'C' | 'n' | 'N' | 'v' | 'V'))
                if is_shortcut_key(key) && category_picker_open =>
            {
                self.options_category_shortcut(shortcut)
                    .map(OptionsPopupAction::OpenCategory)
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
        if let Some(action) = self.composer.action_for_key(key) {
            return action;
        }

        match key.code {
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
            _ if is_composer_newline_key(key) => ComposerCompletionAction::FallThrough,
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
            _ if key_set == SelectionKeySet::Navigation => self.keymap_selection_action(key),
            _ => None,
        }
    }

    pub(in crate::tui) fn is_popup_close_key(&self, key: KeyEvent) -> bool {
        if key.code == KeyCode::Esc && key.modifiers.is_empty() {
            return true;
        }

        self.keymap_single_key_shortcuts(UiAction::ClosePopup)
            .iter()
            .any(|shortcut| shortcut.matches(key))
    }

    fn is_text_entry_popup_close_key(&self, key: KeyEvent) -> bool {
        self.is_popup_close_key(key)
            && !matches!(key.code, KeyCode::Char(_) if is_shortcut_key(key))
    }

    pub(in crate::tui) fn popup_page_action(&self, key: KeyEvent) -> Option<SelectionAction> {
        match key.code {
            KeyCode::PageDown => return Some(SelectionAction::Next),
            KeyCode::PageUp => return Some(SelectionAction::Previous),
            _ => {}
        }

        self.keymap_single_key_shortcuts(UiAction::HalfPageDown)
            .iter()
            .any(|shortcut| shortcut.matches(key))
            .then_some(SelectionAction::Next)
            .or_else(|| {
                self.keymap_single_key_shortcuts(UiAction::HalfPageUp)
                    .iter()
                    .any(|shortcut| shortcut.matches(key))
                    .then_some(SelectionAction::Previous)
            })
    }

    fn keymap_selection_action(&self, key: KeyEvent) -> Option<SelectionAction> {
        self.keymap_single_key_shortcuts(UiAction::SelectNext)
            .iter()
            .any(|shortcut| shortcut.matches(key))
            .then_some(SelectionAction::Next)
            .or_else(|| {
                self.keymap_single_key_shortcuts(UiAction::SelectPrevious)
                    .iter()
                    .any(|shortcut| shortcut.matches(key))
                    .then_some(SelectionAction::Previous)
            })
    }

    pub(in crate::tui) fn scroll_action(&self, key: KeyEvent) -> Option<ScrollAction> {
        match key.code {
            KeyCode::Down => Some(ScrollAction::Down),
            KeyCode::Up => Some(ScrollAction::Up),
            _ => self
                .keymap_single_key_shortcuts(UiAction::ScrollViewportDown)
                .iter()
                .any(|shortcut| shortcut.matches(key))
                .then_some(ScrollAction::Down)
                .or_else(|| {
                    self.keymap_single_key_shortcuts(UiAction::ScrollViewportUp)
                        .iter()
                        .any(|shortcut| shortcut.matches(key))
                        .then_some(ScrollAction::Up)
                }),
        }
    }

    pub fn unread_mark_as_read_hint(&self) -> &'static str {
        "channel action (a) to mark as read "
    }

    pub fn start_composer_key_label(&self) -> String {
        self.binding_label(UiAction::StartComposer)
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
            'c' | 'C' => Some(OptionsCategoryShortcut::Composer),
            'n' | 'N' => Some(OptionsCategoryShortcut::Notifications),
            'v' | 'V' => Some(OptionsCategoryShortcut::Voice),
            _ => None,
        }
    }

    pub fn options_category_shortcut_label(&self, category: OptionsCategoryShortcut) -> String {
        let action = match category {
            OptionsCategoryShortcut::Display => UiAction::OpenDisplayOptions,
            OptionsCategoryShortcut::Composer => UiAction::OpenComposerOptions,
            OptionsCategoryShortcut::Notifications => UiAction::OpenNotificationOptions,
            OptionsCategoryShortcut::Voice => UiAction::OpenVoiceOptions,
        };
        let label = self.binding_label(action);
        if label.is_empty() {
            match category {
                OptionsCategoryShortcut::Display => "d",
                OptionsCategoryShortcut::Composer => "c",
                OptionsCategoryShortcut::Notifications => "n",
                OptionsCategoryShortcut::Voice => "v",
            }
            .to_owned()
        } else {
            label
        }
    }

    pub fn channel_action_shortcuts(
        &self,
        actions: &[ChannelActionItem],
        index: usize,
    ) -> Vec<KeyChord> {
        scoped_action_shortcuts(
            index,
            actions.iter().map(|item| item.kind),
            &self.action_shortcuts.channel,
            |kind| self.default_channel_action_shortcut(kind),
        )
    }

    pub fn channel_action_label(&self, action: &ChannelActionItem) -> String {
        action_label(&self.action_shortcuts.channel, action.kind, &action.label)
    }

    fn default_channel_action_shortcut(&self, kind: ChannelActionKind) -> Vec<KeyChord> {
        vec![char_chord(match kind {
            ChannelActionKind::JoinVoice => 'j',
            ChannelActionKind::LeaveVoice => 'l',
            ChannelActionKind::LoadPinnedMessages => 'p',
            ChannelActionKind::ShowThreads => 't',
            ChannelActionKind::MarkAsRead => 'm',
            ChannelActionKind::ToggleMute => 'u',
        })]
    }

    pub fn guild_action_shortcuts(
        &self,
        actions: &[GuildActionItem],
        index: usize,
    ) -> Vec<KeyChord> {
        scoped_action_shortcuts(
            index,
            actions.iter().map(|item| item.kind),
            &self.action_shortcuts.guild,
            |kind| self.default_guild_action_shortcut(kind),
        )
    }

    pub fn guild_action_label(&self, action: &GuildActionItem) -> String {
        action_label(&self.action_shortcuts.guild, action.kind, &action.label)
    }

    fn default_guild_action_shortcut(&self, kind: GuildActionKind) -> Vec<KeyChord> {
        match kind {
            GuildActionKind::MarkAsRead => vec![char_chord('m')],
            GuildActionKind::ToggleMute => vec![char_chord('u')],
            GuildActionKind::LeaveServer => vec![char_chord('l')],
            GuildActionKind::FolderSettings => vec![char_chord('r')],
            GuildActionKind::NoActionsYet => Vec::new(),
        }
    }

    pub fn member_action_shortcuts(
        &self,
        actions: &[MemberActionItem],
        index: usize,
    ) -> Vec<KeyChord> {
        scoped_action_shortcuts(
            index,
            actions.iter().map(|item| item.kind),
            &self.action_shortcuts.member,
            |kind| self.default_member_action_shortcut(kind),
        )
    }

    pub fn member_action_label(&self, action: &MemberActionItem) -> String {
        action_label(&self.action_shortcuts.member, action.kind, &action.label)
    }

    fn default_member_action_shortcut(&self, kind: MemberActionKind) -> Vec<KeyChord> {
        vec![char_chord(match kind {
            MemberActionKind::ShowProfile => 'p',
        })]
    }

    pub fn thread_action_shortcuts(
        &self,
        actions: &[ThreadActionItem],
        index: usize,
    ) -> Vec<KeyChord> {
        scoped_action_shortcuts(
            index,
            actions.iter().map(|item| item.kind),
            &self.action_shortcuts.thread,
            |kind| self.default_thread_action_shortcut(kind),
        )
    }

    pub fn thread_action_label(&self, action: &ThreadActionItem) -> String {
        action_label(&self.action_shortcuts.thread, action.kind, &action.label)
    }

    fn default_thread_action_shortcut(&self, kind: ThreadActionKind) -> Vec<KeyChord> {
        vec![char_chord(match kind {
            ThreadActionKind::MarkAsRead => 'm',
            ThreadActionKind::ToggleFollow => 'f',
            ThreadActionKind::Close => 'c',
            ThreadActionKind::Lock => 'l',
            ThreadActionKind::Edit => 'e',
            ThreadActionKind::CopyLink => 'y',
            ThreadActionKind::ToggleMute => 'u',
            ThreadActionKind::NotificationSettings => 'n',
            ThreadActionKind::Pin => 'P',
            ThreadActionKind::Delete => 'd',
            ThreadActionKind::CopyId => 'i',
        })]
    }

    pub fn thread_action_shortcut_label(
        &self,
        actions: &[ThreadActionItem],
        index: usize,
    ) -> String {
        let activation_shortcuts = self.thread_action_shortcuts(actions, index);
        if !activation_shortcuts.is_empty() {
            return key_chord_list_label(&activation_shortcuts);
        }
        String::new()
    }

    pub fn message_action_shortcuts(
        &self,
        actions: &[MessageActionItem],
        index: usize,
    ) -> Vec<KeyChord> {
        scoped_action_shortcuts(
            index,
            actions.iter().map(|item| item.kind),
            &self.action_shortcuts.message,
            |kind| self.default_message_action_shortcut(kind),
        )
    }

    pub fn message_action_label(&self, action: &MessageActionItem) -> String {
        action_label(&self.action_shortcuts.message, action.kind, &action.label)
    }

    fn default_message_action_shortcut(&self, kind: MessageActionKind) -> Vec<KeyChord> {
        vec![char_chord(match kind {
            MessageActionKind::CopyContent => 'y',
            MessageActionKind::OpenReactionPicker => 'r',
            MessageActionKind::Reply => 'R',
            MessageActionKind::OpenDeleteConfirmation => 'd',
            MessageActionKind::Edit => 'e',
            MessageActionKind::OpenUrl => 'o',
            MessageActionKind::RemoveEmbeds => 'D',
            MessageActionKind::PlayMedia => 'x',
            MessageActionKind::ViewAttachment => 'v',
            MessageActionKind::GoToReferencedMessage => 'g',
            MessageActionKind::ShowProfile => 'p',
            MessageActionKind::OpenPinConfirmation => 'P',
            MessageActionKind::OpenThread => 't',
            MessageActionKind::ShowReactionUsers => 'u',
            MessageActionKind::OpenPollVotePicker => 'c',
        })]
    }

    pub fn message_action_shortcut_label(
        &self,
        actions: &[MessageActionItem],
        index: usize,
    ) -> String {
        let activation_shortcuts = self.message_action_shortcuts(actions, index);
        if !activation_shortcuts.is_empty() {
            return key_chord_list_label(&activation_shortcuts);
        }
        String::new()
    }

    fn keymap_single_key_shortcuts(&self, action: UiAction) -> Vec<KeyChord> {
        self.keymap
            .specs
            .get(&action)
            .map(|spec| {
                spec.sequences
                    .iter()
                    .filter_map(|sequence| match sequence.as_slice() {
                        [shortcut] => Some(*shortcut),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(in crate::tui) fn matching_action_shortcut_index<A>(
        &self,
        actions: &[A],
        shortcut: KeyChord,
        shortcuts: impl Fn(&Self, &[A], usize) -> Vec<KeyChord>,
        is_enabled: impl Fn(&A) -> bool,
    ) -> Option<usize> {
        actions.iter().enumerate().position(|(index, action)| {
            is_enabled(action)
                && shortcuts(self, actions, index)
                    .iter()
                    .any(|candidate| candidate.matches_chord(shortcut))
        })
    }

    pub fn indexed_shortcut(&self, index: usize) -> Option<char> {
        indexed_shortcut(index)
    }

    pub(in crate::tui) fn indexed_shortcut_matches(
        &self,
        shortcut: KeyChord,
        index: usize,
    ) -> bool {
        self.indexed_shortcut(index)
            .is_some_and(|candidate| shortcut.matches_char(candidate))
    }

    pub(in crate::tui) fn matching_indexed_shortcut_index(
        &self,
        shortcut: KeyChord,
        len: usize,
    ) -> Option<usize> {
        (0..len).find(|index| self.indexed_shortcut_matches(shortcut, *index))
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

fn indexed_shortcut(index: usize) -> Option<char> {
    match index {
        0..=8 => char::from_digit(u32::try_from(index + 1).ok()?, 10),
        9 => Some('0'),
        _ => None,
    }
}

fn action_label<K>(bindings: &[ActionShortcutBinding<K>], kind: K, fallback: &str) -> String
where
    K: Copy + Eq,
{
    bindings
        .iter()
        .find(|binding| binding.kind == kind)
        .and_then(|binding| binding.description.clone())
        .unwrap_or_else(|| fallback.to_owned())
}

fn scoped_action_shortcuts<K>(
    index: usize,
    kinds: impl IntoIterator<Item = K>,
    bindings: &[ActionShortcutBinding<K>],
    default_shortcuts: impl Fn(K) -> Vec<KeyChord>,
) -> Vec<KeyChord>
where
    K: Copy + Eq,
{
    let shortcut_sets = kinds
        .into_iter()
        .map(|kind| action_shortcut_candidates(bindings, kind, &default_shortcuts))
        .collect::<Vec<_>>();
    if index >= shortcut_sets.len() {
        return Vec::new();
    }
    action_shortcuts(index, shortcut_sets)
}

fn action_shortcut_candidates<K>(
    bindings: &[ActionShortcutBinding<K>],
    kind: K,
    default_shortcuts: &impl Fn(K) -> Vec<KeyChord>,
) -> Vec<KeyChord>
where
    K: Copy + Eq,
{
    bindings
        .iter()
        .find(|binding| binding.kind == kind)
        .map(|binding| binding.shortcuts.clone())
        .unwrap_or_else(|| default_shortcuts(kind))
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

fn is_composer_newline_key(key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Enter => key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT),
        _ => false,
    }
}

fn action_shortcuts(
    index: usize,
    shortcut_sets: impl IntoIterator<Item = Vec<KeyChord>>,
) -> Vec<KeyChord> {
    let shortcut_sets = shortcut_sets.into_iter().collect::<Vec<_>>();
    let Some(preferred) = shortcut_sets.get(index) else {
        return Vec::new();
    };
    let shortcuts = unique_action_shortcuts(preferred, shortcut_sets.clone());
    if !shortcuts.is_empty() {
        return shortcuts;
    }

    let mut used = shortcut_sets.iter().flatten().copied().collect::<Vec<_>>();
    for fallback_index in 0..=index {
        let Some(preferred) = shortcut_sets.get(fallback_index) else {
            return Vec::new();
        };
        if !unique_action_shortcuts(preferred, shortcut_sets.clone()).is_empty() {
            continue;
        }
        let Some(fallback) = first_unused_indexed_shortcut(&used) else {
            return Vec::new();
        };
        if fallback_index == index {
            return vec![fallback];
        }
        used.push(fallback);
    }
    Vec::new()
}

fn first_unused_indexed_shortcut(used: &[KeyChord]) -> Option<KeyChord> {
    (0..10)
        .filter_map(indexed_shortcut)
        .map(char_chord)
        .find(|shortcut| {
            !used
                .iter()
                .any(|used| key_chords_match_same_event(*used, *shortcut))
        })
}

fn unique_action_shortcuts(
    preferred: &[KeyChord],
    shortcut_sets: impl IntoIterator<Item = Vec<KeyChord>>,
) -> Vec<KeyChord> {
    let shortcut_sets = shortcut_sets.into_iter().collect::<Vec<_>>();
    let mut unique = Vec::new();
    for candidate in preferred.iter().copied() {
        if unique
            .iter()
            .any(|unique| key_chords_match_same_event(*unique, candidate))
        {
            continue;
        }
        let matches = shortcut_sets
            .iter()
            .filter(|shortcuts| {
                shortcuts
                    .iter()
                    .any(|shortcut| key_chords_match_same_event(*shortcut, candidate))
            })
            .count();
        if matches == 1 {
            unique.push(candidate);
        }
    }
    unique
}

fn key_chord_list_label(shortcuts: &[KeyChord]) -> String {
    shortcuts
        .iter()
        .map(|key| key.label())
        .collect::<Vec<_>>()
        .join("/")
}
