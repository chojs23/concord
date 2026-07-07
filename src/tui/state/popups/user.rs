use crate::discord::ids::{
    Id,
    marker::{GuildMarker, UserMarker},
};
use crate::discord::{
    ActivityInfo, ActivityKind, AppCommand, GlobalUserProfileUpdate, GuildUserProfileUpdate,
    MessageAttachmentUpload, PresenceStatus, ProfileAvatarUpload, UserProfileInfo,
    UserProfileUpdate,
};
use crate::tui::keybindings::KeyChord;
use crate::tui::text_input::TextInputState;

use super::super::model::{FocusPane, MemberActionItem, MemberActionKind};
use super::super::{ActiveGuildScope, DashboardState};
use super::{
    ActiveModalPopupKind, LeaderActionState, LeaderMode, LeaderPopupState, MemberLeaderActionState,
    ModalPopup, SelectablePopupState, UserProfilePopupState, UserProfileSettingsField,
    UserProfileSettingsState, UserProfileSettingsTab,
};

impl DashboardState {
    pub fn is_member_leader_action_active(&self) -> bool {
        self.popups.member_leader_action().is_some()
    }

    /// Direct shortcut from the member pane: open the profile popup for the
    /// currently selected member without going through Leader Actions.
    pub fn show_selected_member_profile(&mut self) -> Option<AppCommand> {
        if self.navigation.focus != FocusPane::Members {
            return None;
        }
        let entries = self.flattened_members();
        let entry = entries.get(self.selected_member())?;
        let user_id = entry.user_id();
        let guild_id = match self.navigation.guilds.active {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => None,
        };
        self.open_user_profile_popup(user_id, guild_id)
    }

    pub fn open_current_user_profile_popup(&mut self) -> Option<AppCommand> {
        let user_id = self.current_user_id()?;
        let guild_id = match self.navigation.guilds.active {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => None,
        };
        self.open_user_profile_popup(user_id, guild_id)
    }

    pub fn open_selected_member_actions(&mut self) {
        if let Some(action) = self.selected_member_action_context() {
            self.popups.modal = Some(ModalPopup::Leader(LeaderPopupState {
                mode: LeaderMode::Actions,
                keymap_prefix: Vec::new(),
                action: Some(LeaderActionState::Member(action)),
            }));
        }
    }

    pub(super) fn selected_member_action_context(&self) -> Option<MemberLeaderActionState> {
        if self.navigation.focus != FocusPane::Members {
            return None;
        }
        let entries = self.flattened_members();
        let entry = entries.get(self.selected_member())?;
        let user_id = entry.user_id();
        // For DM/group-DM panes there is no guild context. Pass it through so
        // the profile fetch can omit `guild_id` and skip the guild_member
        // section gracefully.
        let guild_id = match self.navigation.guilds.active {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => None,
        };
        Some(MemberLeaderActionState {
            user_id,
            guild_id,
            selection: Default::default(),
        })
    }

    pub fn close_member_leader_action(&mut self) {
        if self.is_member_leader_action_active() {
            self.popups.clear_modal();
        }
    }

    pub fn selected_member_action_items(&self) -> Vec<MemberActionItem> {
        if self.popups.member_leader_action().is_none() {
            return Vec::new();
        }
        vec![MemberActionItem::new(
            MemberActionKind::ShowProfile,
            "Show profile",
            true,
        )]
    }

    pub fn select_member_action_row(&mut self, row: usize) -> bool {
        if row >= self.selected_member_action_items().len() {
            return false;
        }
        if let Some(action) = self.popups.member_leader_action_mut() {
            action.selection.select(row);
            return true;
        }
        false
    }

    pub fn activate_selected_member_action(&mut self) -> Option<AppCommand> {
        let action = self.popups.member_leader_action().cloned()?;
        let items = self.selected_member_action_items();
        let item = items
            .get(action.selection.selected_for_len(items.len()))?
            .clone();
        if !item.enabled {
            return None;
        }
        match item.kind {
            MemberActionKind::ShowProfile => {
                self.close_member_leader_action();
                self.open_user_profile_popup(action.user_id, action.guild_id)
            }
        }
    }

    pub fn activate_member_action_shortcut(&mut self, shortcut: KeyChord) -> Option<AppCommand> {
        let actions = self.selected_member_action_items();
        let index = self.options.key_bindings().matching_action_shortcut_index(
            &actions,
            shortcut,
            |key_bindings, actions, index| key_bindings.member_action_shortcuts(actions, index),
            |action| action.enabled,
        )?;
        self.select_member_action_row(index);
        self.activate_selected_member_action()
    }

    /// Opens the profile popup for `user_id`. The returned command is a profile
    /// open intent. Backend request lifecycle decides whether profile or note
    /// data is already cached or in flight.
    pub fn open_user_profile_popup(
        &mut self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Option<AppCommand> {
        self.popups.modal = Some(ModalPopup::UserProfile(UserProfilePopupState {
            user_id,
            guild_id,
            load_error: None,
            settings: UserProfileSettingsState::default(),
            scroll: Default::default(),
        }));
        Some(AppCommand::LoadUserProfile { user_id, guild_id })
    }

    pub fn close_user_profile_popup(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::UserProfile) {
            self.popups.clear_modal();
        }
    }

    pub fn close_or_cancel_user_profile_popup(&mut self) {
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.settings.status_picker.take().is_some()
        {
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.settings.activity_picker.take().is_some()
        {
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.settings.editing.take().is_some()
        {
            popup.settings.edit_input.clear();
            return;
        }
        self.close_user_profile_popup();
    }

    pub fn is_user_profile_popup_editing(&self) -> bool {
        self.popups
            .user_profile_popup()
            .and_then(|popup| popup.settings.editing)
            .is_some()
    }

    pub fn is_current_user_profile_popup(&self) -> bool {
        self.popups
            .user_profile_popup()
            .is_some_and(|popup| self.current_user_id() == Some(popup.user_id))
    }

    pub(in crate::tui) fn user_profile_settings_tab(&self) -> UserProfileSettingsTab {
        self.popups
            .user_profile_popup()
            .map(|popup| popup.settings.tab)
            .unwrap_or_default()
    }

    pub(in crate::tui) fn user_profile_settings_active_field(
        &self,
    ) -> Option<UserProfileSettingsField> {
        self.popups
            .user_profile_popup()
            .map(|popup| popup.settings.active_field())
    }

    pub(in crate::tui) fn user_profile_settings_editing_field(
        &self,
    ) -> Option<UserProfileSettingsField> {
        self.popups
            .user_profile_popup()
            .and_then(|popup| popup.settings.editing)
    }

    pub(in crate::tui) fn user_profile_settings_edit_cursor_byte_index(&self) -> usize {
        self.popups
            .user_profile_popup()
            .map(|popup| popup.settings.edit_input.cursor_byte_index())
            .unwrap_or(0)
    }

    pub(in crate::tui) fn user_profile_settings_status(&self) -> Option<&str> {
        self.popups
            .user_profile_popup()
            .and_then(|popup| popup.settings.status.as_deref())
    }

    pub(in crate::tui) fn user_profile_settings_presence_status(&self) -> PresenceStatus {
        self.popups
            .user_profile_popup()
            .and_then(|popup| popup.settings.presence_status)
            .unwrap_or_else(|| self.user_profile_popup_status())
    }

    fn user_profile_settings_manual_activities(&self) -> Vec<ActivityInfo> {
        let value = self
            .popups
            .user_profile_popup()
            .and_then(|popup| popup.settings.manual_activity.clone())
            .or_else(|| self.current_user_activity_name());
        value
            .as_deref()
            .map(manual_activity_from_text)
            .unwrap_or_default()
    }

    fn current_user_activity_name(&self) -> Option<String> {
        self.current_user_id().and_then(|user_id| {
            self.discord
                .cache
                .user_activities(user_id)
                .iter()
                .find(|activity| {
                    activity.kind == ActivityKind::Playing && !activity.name.trim().is_empty()
                })
                .map(|activity| activity.name.clone())
        })
    }

    pub(in crate::tui) fn user_profile_settings_dirty_count(&self) -> usize {
        let Some(popup) = self.popups.user_profile_popup() else {
            return 0;
        };
        profile_settings_changed_field_count(
            popup.user_id,
            &popup.settings,
            self.user_profile_popup_data(),
            popup.guild_id,
        )
    }

    pub(in crate::tui) fn user_profile_settings_saving(&self) -> bool {
        self.popups
            .user_profile_popup()
            .map(|popup| popup.settings.saving)
            .unwrap_or(false)
    }

    pub(in crate::tui) fn user_profile_popup_guild_id(&self) -> Option<Id<GuildMarker>> {
        self.popups
            .user_profile_popup()
            .and_then(|popup| popup.guild_id)
    }

    pub(in crate::tui) fn user_profile_settings_field_value(
        &self,
        field: UserProfileSettingsField,
    ) -> String {
        let Some(popup) = self.popups.user_profile_popup() else {
            return String::new();
        };
        if popup.settings.editing == Some(field) {
            return popup.settings.edit_input.value().to_owned();
        }
        let profile = self.user_profile_popup_data();
        match field {
            UserProfileSettingsField::GlobalDisplayName => popup
                .settings
                .global_display_name
                .clone()
                .or_else(|| profile.and_then(|profile| profile.global_name.clone()))
                .unwrap_or_default(),
            UserProfileSettingsField::GlobalPronouns => popup
                .settings
                .global_pronouns
                .clone()
                .or_else(|| profile.and_then(|profile| profile.pronouns.clone()))
                .unwrap_or_default(),
            UserProfileSettingsField::GlobalAvatarPath => popup
                .settings
                .global_avatar_path
                .clone()
                .or_else(|| {
                    popup
                        .settings
                        .global_avatar_upload
                        .as_ref()
                        .map(|upload| upload.filename.clone())
                })
                .unwrap_or_default(),
            UserProfileSettingsField::CurrentStatus => self
                .user_profile_settings_presence_status()
                .label()
                .to_owned(),
            UserProfileSettingsField::ManualActivity => popup
                .settings
                .manual_activity
                .clone()
                .unwrap_or_else(|| self.current_user_activity_name().unwrap_or_default()),
            UserProfileSettingsField::GuildNickname => popup
                .settings
                .guild_nickname
                .clone()
                .or_else(|| profile.and_then(|profile| profile.guild_nick.clone()))
                .unwrap_or_default(),
            UserProfileSettingsField::GuildPronouns => popup
                .settings
                .guild_pronouns
                .clone()
                .or_else(|| profile.and_then(|profile| profile.guild_pronouns.clone()))
                .unwrap_or_default(),
            UserProfileSettingsField::Save
            | UserProfileSettingsField::Cancel
            | UserProfileSettingsField::SignOut => String::new(),
        }
    }

    pub fn next_user_profile_settings_field(&mut self) {
        if !self.is_current_user_profile_popup() {
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.settings.editing.is_none()
        {
            popup.settings.next_field();
        }
    }

    pub fn previous_user_profile_settings_field(&mut self) {
        if !self.is_current_user_profile_popup() {
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.settings.editing.is_none()
        {
            popup.settings.previous_field();
        }
    }

    pub fn switch_user_profile_settings_to_global(&mut self) {
        if !self.is_current_user_profile_popup() {
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.settings.editing.is_none()
        {
            popup.settings.tab = UserProfileSettingsTab::Global;
        }
    }

    pub fn switch_user_profile_settings_to_guild(&mut self) {
        if !self.is_current_user_profile_popup() {
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.settings.editing.is_none()
        {
            popup.settings.tab = UserProfileSettingsTab::Guild;
        }
    }

    pub fn start_or_commit_user_profile_edit(&mut self) -> Option<AppCommand> {
        if !self.is_current_user_profile_popup() {
            return None;
        }
        let field = self.user_profile_settings_active_field()?;
        if self
            .popups
            .user_profile_popup()
            .and_then(|popup| popup.settings.editing)
            == Some(field)
        {
            if let Some(popup) = self.popups.user_profile_popup_mut() {
                let value = popup.settings.edit_input.value().to_owned();
                popup.settings.set_field_value(field, value);
                popup.settings.editing = None;
                popup.settings.edit_input.clear();
                popup.settings.status = None;
            }
            if field == UserProfileSettingsField::ManualActivity {
                let status = self.user_profile_settings_presence_status();
                let activities = self.user_profile_settings_manual_activities();
                // track_client_id None: a manually-typed activity is not tracked,
                // so RPC updates must not override it.
                return Some(AppCommand::UpdateCurrentUserActivity {
                    status,
                    activities,
                    track_client_id: None,
                });
            }
            return None;
        }
        if field == UserProfileSettingsField::CurrentStatus {
            self.open_user_profile_status_picker();
            return None;
        }
        if field == UserProfileSettingsField::ManualActivity {
            self.open_user_profile_activity_picker();
            return None;
        }
        if field == UserProfileSettingsField::Save {
            return self.save_user_profile_settings_command();
        }
        if field == UserProfileSettingsField::Cancel {
            self.close_or_cancel_user_profile_popup();
            return None;
        }
        if field == UserProfileSettingsField::SignOut {
            return self.sign_out_command();
        }
        let value = self.user_profile_settings_field_value(field);
        if let Some(popup) = self.popups.user_profile_popup_mut() {
            popup.settings.editing = Some(field);
            popup.settings.edit_input.set_value(value);
        }
        None
    }

    pub fn is_user_profile_status_picker_open(&self) -> bool {
        self.popups
            .user_profile_popup()
            .is_some_and(|popup| popup.settings.status_picker.is_some())
    }

    pub fn open_user_profile_status_picker(&mut self) {
        if !self.is_current_user_profile_popup() {
            return;
        }
        let current_status = self.user_profile_popup_status();
        if let Some(popup) = self.popups.user_profile_popup_mut() {
            let mut picker = SelectablePopupState::default();
            let selected = PresenceStatus::user_selectable()
                .iter()
                .position(|status| *status == current_status)
                .unwrap_or(0);
            picker.select(selected);
            popup.settings.editing = None;
            popup.settings.edit_input.clear();
            popup.settings.status_picker = Some(picker);
        }
    }

    pub fn close_user_profile_status_picker(&mut self) {
        if let Some(popup) = self.popups.user_profile_popup_mut() {
            popup.settings.status_picker = None;
        }
    }

    pub fn move_user_profile_status_picker_down(&mut self) {
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && let Some(picker) = popup.settings.status_picker.as_mut()
        {
            picker.move_down(PresenceStatus::user_selectable().len());
        }
    }

    pub fn move_user_profile_status_picker_up(&mut self) {
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && let Some(picker) = popup.settings.status_picker.as_mut()
        {
            picker.move_up();
        }
    }

    pub fn activate_user_profile_status_picker(&mut self) -> Option<AppCommand> {
        if !self.is_current_user_profile_popup() {
            return None;
        }
        let popup = self.popups.user_profile_popup_mut()?;
        let picker = popup.settings.status_picker.take()?;
        let statuses = PresenceStatus::user_selectable();
        let status = statuses[picker.selected_for_len(statuses.len())];
        popup.settings.presence_status = Some(status);
        Some(AppCommand::UpdateCurrentUserStatus { status })
    }

    pub(in crate::tui) fn user_profile_status_picker_rows(&self) -> Vec<(PresenceStatus, bool)> {
        let Some(popup) = self.popups.user_profile_popup() else {
            return Vec::new();
        };
        let Some(picker) = popup.settings.status_picker.as_ref() else {
            return Vec::new();
        };
        let statuses = PresenceStatus::user_selectable();
        let selected = picker.selected_for_len(statuses.len());
        statuses
            .into_iter()
            .enumerate()
            .map(|(index, status)| (status, index == selected))
            .collect()
    }

    fn user_profile_activity_picker_len(&self) -> usize {
        self.detected_rich_presence().len() + 1
    }

    pub fn open_user_profile_activity_picker(&mut self) {
        if !self.is_current_user_profile_popup() {
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut() {
            let mut picker = SelectablePopupState::default();
            picker.select(0);
            popup.settings.editing = None;
            popup.settings.edit_input.clear();
            popup.settings.activity_picker = Some(picker);
        }
    }

    pub fn close_user_profile_activity_picker(&mut self) {
        if let Some(popup) = self.popups.user_profile_popup_mut() {
            popup.settings.activity_picker = None;
        }
    }

    pub fn move_user_profile_activity_picker_down(&mut self) {
        let len = self.user_profile_activity_picker_len();
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && let Some(picker) = popup.settings.activity_picker.as_mut()
        {
            picker.move_down(len);
        }
    }

    pub fn move_user_profile_activity_picker_up(&mut self) {
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && let Some(picker) = popup.settings.activity_picker.as_mut()
        {
            picker.move_up();
        }
    }

    pub fn is_user_profile_activity_picker_open(&self) -> bool {
        self.popups
            .user_profile_popup()
            .is_some_and(|popup| popup.settings.activity_picker.is_some())
    }

    pub fn activate_user_profile_activity_picker(&mut self) -> Option<AppCommand> {
        if !self.is_current_user_profile_popup() {
            return None;
        }
        let detected = self.detected_rich_presence().to_vec();
        let len = detected.len() + 1;
        let selected = {
            let popup = self.popups.user_profile_popup()?;
            let picker = popup.settings.activity_picker.as_ref()?;
            picker.selected_for_len(len)
        };

        if let Some(activity) = detected.get(selected).cloned() {
            let status = self.user_profile_settings_presence_status();
            let track_client_id = activity.application_id.clone();
            if let Some(popup) = self.popups.user_profile_popup_mut() {
                popup.settings.activity_picker = None;
                popup.settings.manual_activity = Some(activity.name.clone());
            }
            return Some(AppCommand::UpdateCurrentUserActivity {
                status,
                activities: vec![activity],
                track_client_id,
            });
        }

        let value =
            self.user_profile_settings_field_value(UserProfileSettingsField::ManualActivity);
        if let Some(popup) = self.popups.user_profile_popup_mut() {
            popup.settings.activity_picker = None;
            popup.settings.editing = Some(UserProfileSettingsField::ManualActivity);
            popup.settings.edit_input.set_value(value);
        }
        None
    }

    pub(in crate::tui) fn user_profile_activity_picker_rows(&self) -> Vec<(String, bool)> {
        let Some(popup) = self.popups.user_profile_popup() else {
            return Vec::new();
        };
        let Some(picker) = popup.settings.activity_picker.as_ref() else {
            return Vec::new();
        };
        let detected = self.detected_rich_presence();
        let len = detected.len() + 1;
        let selected = picker.selected_for_len(len);
        let mut rows: Vec<(String, bool)> = detected
            .iter()
            .enumerate()
            .map(|(index, activity)| (activity_picker_label(activity), index == selected))
            .collect();
        rows.push((
            MANUAL_ACTIVITY_PICKER_LABEL.to_owned(),
            selected == detected.len(),
        ));
        rows
    }

    pub fn push_user_profile_edit_char(&mut self, value: char) {
        self.insert_user_profile_edit_text(&value.to_string());
    }

    pub fn insert_user_profile_edit_text(&mut self, value: &str) {
        if !self.is_current_user_profile_popup() {
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.settings.editing.is_some()
        {
            popup.settings.edit_input.insert_str(value);
        }
    }

    pub fn pop_user_profile_edit_char(&mut self) {
        if !self.is_current_user_profile_popup() {
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.settings.editing.is_some()
        {
            popup.settings.edit_input.delete_previous_grapheme();
        }
    }

    pub fn delete_previous_user_profile_edit_word(&mut self) {
        if !self.is_current_user_profile_popup() {
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.settings.editing.is_some()
        {
            popup.settings.edit_input.delete_previous_word();
        }
    }

    pub fn delete_user_profile_edit_to_line_start(&mut self) {
        if !self.is_current_user_profile_popup() {
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.settings.editing.is_some()
        {
            popup.settings.edit_input.delete_to_line_start();
        }
    }

    pub fn delete_user_profile_edit_to_line_end(&mut self) {
        if !self.is_current_user_profile_popup() {
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.settings.editing.is_some()
        {
            popup.settings.edit_input.delete_to_line_end();
        }
    }

    pub fn move_user_profile_edit_cursor_left(&mut self) {
        self.move_user_profile_edit_cursor_with(|input| input.move_left());
    }

    pub fn move_user_profile_edit_cursor_right(&mut self) {
        self.move_user_profile_edit_cursor_with(|input| input.move_right());
    }

    pub fn move_user_profile_edit_cursor_word_left(&mut self) {
        self.move_user_profile_edit_cursor_with(|input| input.move_word_left());
    }

    pub fn move_user_profile_edit_cursor_word_right(&mut self) {
        self.move_user_profile_edit_cursor_with(|input| input.move_word_right());
    }

    pub fn move_user_profile_edit_cursor_home(&mut self) {
        self.move_user_profile_edit_cursor_with(|input| input.move_home());
    }

    pub fn move_user_profile_edit_cursor_end(&mut self) {
        self.move_user_profile_edit_cursor_with(|input| input.move_end());
    }

    fn move_user_profile_edit_cursor_with(&mut self, update: impl FnOnce(&mut TextInputState)) {
        if !self.is_current_user_profile_popup() {
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.settings.editing.is_some()
        {
            update(&mut popup.settings.edit_input);
        }
    }

    pub fn accepts_user_profile_avatar_paste(&self) -> bool {
        self.is_current_user_profile_popup()
            && self.user_profile_settings_active_field()
                == Some(UserProfileSettingsField::GlobalAvatarPath)
    }

    pub fn request_user_profile_avatar_clipboard_paste(&mut self) {
        if !self.accepts_user_profile_avatar_paste() {
            if let Some(popup) = self.popups.user_profile_popup_mut() {
                popup.settings.status = Some("Select the avatar image field first".to_owned());
            }
            return;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut() {
            popup.settings.editing = None;
            popup.settings.edit_input.clear();
            popup.settings.status = Some("Reading clipboard image...".to_owned());
        }
        self.request_paste_clipboard();
    }

    pub fn set_user_profile_avatar_from_attachment(
        &mut self,
        upload: MessageAttachmentUpload,
    ) -> bool {
        if !self.accepts_user_profile_avatar_paste() {
            return false;
        }
        let upload = ProfileAvatarUpload::from_message_attachment(upload);
        if let Some(popup) = self.popups.user_profile_popup_mut() {
            popup.settings.set_avatar_upload(upload);
            popup.settings.editing = None;
            popup.settings.edit_input.clear();
            popup.settings.status = None;
            return true;
        }
        false
    }

    pub fn save_user_profile_settings_command(&mut self) -> Option<AppCommand> {
        let current_user_id = self.current_user_id();
        let profile = self.user_profile_popup_data().cloned();
        let popup = self.popups.user_profile_popup_mut()?;
        if current_user_id != Some(popup.user_id) {
            popup.settings.status = Some("Only your own profile can be edited".to_owned());
            return None;
        }
        if popup.settings.editing.is_some() {
            popup.settings.status = Some("Press Enter to finish editing first".to_owned());
            return None;
        }
        if popup.guild_id.is_none()
            && (popup.settings.guild_nickname.is_some() || popup.settings.guild_pronouns.is_some())
        {
            popup.settings.status = Some("Server profile needs an active server".to_owned());
            return None;
        }
        let update = pending_user_profile_update(
            popup.user_id,
            popup.guild_id,
            &popup.settings,
            profile.as_ref(),
        );
        if update.is_empty() {
            popup.settings.status = Some("No profile changes to save".to_owned());
            return None;
        }
        popup.settings.saving = true;
        popup.settings.status = Some("Saving profile changes...".to_owned());
        Some(AppCommand::UpdateUserProfile { update })
    }

    pub fn sign_out_command(&mut self) -> Option<AppCommand> {
        if !self.is_current_user_profile_popup() {
            return None;
        }
        if let Some(popup) = self.popups.user_profile_popup_mut() {
            popup.settings.editing = None;
            popup.settings.edit_input.clear();
            popup.settings.status_picker = None;
            popup.settings.activity_picker = None;
            popup.settings.status = Some("Signing out...".to_owned());
        }
        Some(AppCommand::SignOut)
    }

    pub(in crate::tui) fn record_user_profile_update_succeeded(
        &mut self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
    ) {
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.user_id == user_id
            && popup.guild_id == guild_id
            && popup.settings.saving
        {
            popup.settings.clear_after_save();
        }
    }

    pub(in crate::tui) fn record_user_profile_update_failed(
        &mut self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
        message: &str,
    ) {
        if let Some(popup) = self.popups.user_profile_popup_mut()
            && popup.user_id == user_id
            && popup.guild_id == guild_id
        {
            popup.settings.saving = false;
            popup.settings.status = Some(format!("Save failed: {message}"));
        }
    }

    pub fn user_profile_popup_data(&self) -> Option<&UserProfileInfo> {
        let popup = self.popups.user_profile_popup()?;
        self.discord
            .cache
            .user_profile(popup.user_id, popup.guild_id)
    }

    pub fn user_profile_popup_load_error(&self) -> Option<&str> {
        self.popups
            .user_profile_popup()
            .and_then(|popup| popup.load_error.as_deref())
    }

    pub fn user_profile_popup_status(&self) -> PresenceStatus {
        let Some(popup) = self.popups.user_profile_popup() else {
            return PresenceStatus::Unknown;
        };

        if let Some(guild_id) = popup.guild_id
            && let Some(status) = self
                .discord
                .members_for_guild(guild_id)
                .into_iter()
                .find(|member| member.user_id == popup.user_id)
                .map(|member| member.status)
        {
            return status;
        }

        if let Some(guild_id) = popup.guild_id
            && let Some(status) = self
                .discord
                .user_presence_for_guild(Some(guild_id), popup.user_id)
        {
            return status;
        }

        let recipient_status = self
            .discord
            .channels_for_guild(None)
            .into_iter()
            .flat_map(|channel| channel.recipients.iter())
            .find(|recipient| recipient.user_id == popup.user_id)
            .map(|recipient| recipient.status);

        recipient_status
            .filter(|status| *status != PresenceStatus::Unknown)
            .or_else(|| self.discord.cache.user_presence(popup.user_id))
            .unwrap_or(PresenceStatus::Unknown)
    }

    /// URL of the avatar image to render into the open profile popup. None
    /// when the popup is closed, the profile has not loaded yet, or the user
    /// has no avatar attachment.
    pub fn user_profile_popup_avatar_url(&self) -> Option<&str> {
        self.user_profile_popup_data()?.avatar_url.as_deref()
    }

    pub fn user_profile_popup_has_avatar_preview(&self) -> bool {
        self.user_profile_popup_pending_avatar_preview_key()
            .is_some()
            || self.user_profile_popup_avatar_url().is_some()
    }

    pub fn user_profile_popup_pending_avatar_preview_key(&self) -> Option<&str> {
        let popup = self.popups.user_profile_popup()?;
        popup.settings.pending_global_avatar_preview_key()
    }

    pub fn user_profile_popup_pending_avatar_upload(&self) -> Option<ProfileAvatarUpload> {
        let popup = self.popups.user_profile_popup()?;
        popup.settings.pending_global_avatar_upload()
    }

    pub fn user_profile_popup_activities(&self) -> &[ActivityInfo] {
        let Some(popup) = self.popups.user_profile_popup() else {
            return &[];
        };
        self.discord
            .cache
            .user_activities_for_guild(popup.guild_id, popup.user_id)
    }

    /// Top-of-viewport row for the popup body. Used by the renderer.
    pub fn user_profile_popup_scroll(&self) -> usize {
        self.popups
            .user_profile_popup()
            .map(|popup| popup.scroll.scroll())
            .unwrap_or(0)
    }

    /// Renderer hook: passes the latest viewport height back so scroll
    /// methods can clamp without snapping past the last visible row.
    pub fn set_user_profile_popup_view_height(&mut self, height: usize) {
        if let Some(popup) = self.popups.user_profile_popup_mut() {
            popup.scroll.set_view_height(height);
        }
    }

    /// Renderer hook: stash the laid-out content height so scroll
    /// clamping is a constant-time check instead of recomputing layout.
    pub fn set_user_profile_popup_total_lines(&mut self, total_lines: usize) {
        if let Some(popup) = self.popups.user_profile_popup_mut() {
            popup.scroll.set_total_lines(total_lines);
        }
    }

    pub fn scroll_user_profile_popup_down(&mut self) {
        if let Some(popup) = self.popups.user_profile_popup_mut() {
            popup.scroll.scroll_down();
        }
    }

    pub fn scroll_user_profile_popup_up(&mut self) {
        if let Some(popup) = self.popups.user_profile_popup_mut() {
            popup.scroll.scroll_up();
        }
    }
}

const MANUAL_ACTIVITY_PICKER_LABEL: &str = "Set manually…";

fn manual_activity_from_text(value: &str) -> Vec<ActivityInfo> {
    let value = value.trim();
    if value.is_empty() {
        Vec::new()
    } else {
        vec![ActivityInfo::playing(value)]
    }
}

/// Appends details/state so two instances of the same app are distinguishable.
fn activity_picker_label(activity: &ActivityInfo) -> String {
    let name = if activity.name.trim().is_empty() {
        "Unknown app"
    } else {
        activity.name.trim()
    };
    match activity.details.as_deref().or(activity.state.as_deref()) {
        Some(detail) if !detail.trim().is_empty() => format!("{name} — {}", detail.trim()),
        _ => name.to_owned(),
    }
}

fn changed_text(dirty: Option<&String>, current: Option<&str>) -> Option<String> {
    let dirty = dirty?;
    let current = current.unwrap_or_default();
    (dirty != current).then(|| dirty.clone())
}

fn pending_user_profile_update(
    user_id: Id<UserMarker>,
    guild_id: Option<Id<GuildMarker>>,
    settings: &UserProfileSettingsState,
    profile: Option<&UserProfileInfo>,
) -> UserProfileUpdate {
    let guild_update = guild_id.map(|guild_id| GuildUserProfileUpdate {
        guild_id,
        nickname: changed_text(
            settings.guild_nickname.as_ref(),
            profile.and_then(|profile| profile.guild_nick.as_deref()),
        ),
        pronouns: changed_text(
            settings.guild_pronouns.as_ref(),
            profile.and_then(|profile| profile.guild_pronouns.as_deref()),
        ),
    });

    UserProfileUpdate {
        user_id,
        guild_id,
        global: GlobalUserProfileUpdate {
            display_name: changed_text(
                settings.global_display_name.as_ref(),
                profile.and_then(|profile| profile.global_name.as_deref()),
            ),
            pronouns: changed_text(
                settings.global_pronouns.as_ref(),
                profile.and_then(|profile| profile.pronouns.as_deref()),
            ),
            avatar: settings.pending_global_avatar_upload(),
        },
        guild: guild_update.filter(|update| !update.is_empty()),
    }
}

fn user_profile_update_changed_field_count(update: &UserProfileUpdate) -> usize {
    let mut count = 0;
    count += usize::from(update.global.display_name.is_some());
    count += usize::from(update.global.pronouns.is_some());
    count += usize::from(update.global.avatar.is_some());
    if let Some(guild) = update.guild.as_ref() {
        count += usize::from(guild.nickname.is_some());
        count += usize::from(guild.pronouns.is_some());
    }
    count
}

fn profile_settings_changed_field_count(
    user_id: Id<UserMarker>,
    settings: &UserProfileSettingsState,
    profile: Option<&UserProfileInfo>,
    guild_id: Option<Id<GuildMarker>>,
) -> usize {
    let update = pending_user_profile_update(user_id, guild_id, settings, profile);
    user_profile_update_changed_field_count(&update)
}
