use crate::config::{
    AppOptions, ComposerOptions, CredentialOptions, DisplayOptions, ImagePreviewQualityPreset,
    KeymapOptions, NotificationOptions, UiStateOptions, VoiceOptions,
};
use crate::discord::AppCommand;
use crate::tui::keybindings::KeyBindings;

use super::{DashboardState, FocusPane, FolderKey};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayOptionItem {
    pub label: &'static str,
    pub enabled: bool,
    pub value: Option<String>,
    pub gauge_percent: Option<u16>,
    pub effective: bool,
    pub description: &'static str,
}

#[cfg(test)]
#[allow(dead_code)]
impl DisplayOptionItem {
    pub(crate) fn test(label: &'static str) -> Self {
        Self {
            label,
            enabled: false,
            value: None,
            gauge_percent: None,
            effective: false,
            description: "",
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct SettingsState {
    pub(super) display_options: DisplayOptions,
    pub(super) composer_options: ComposerOptions,
    pub(super) credential_options: CredentialOptions,
    pub(super) notification_options: NotificationOptions,
    pub(super) voice_options: VoiceOptions,
    pub(super) key_bindings: KeyBindings,
    pub(super) config_save_pending: bool,
    pub(super) ui_state_save_pending: bool,
}

impl SettingsState {
    pub(super) fn key_bindings(&self) -> &KeyBindings {
        &self.key_bindings
    }
}

impl DashboardState {
    pub fn new_with_options(
        display_options: DisplayOptions,
        composer_options: ComposerOptions,
        credential_options: CredentialOptions,
        notification_options: NotificationOptions,
        voice_options: VoiceOptions,
        keymap_options: KeymapOptions,
        ui_state_options: UiStateOptions,
    ) -> Self {
        let mut state = Self::new();
        state.options.display_options = display_options;
        state.options.composer_options = composer_options;
        state.options.credential_options = credential_options;
        state.options.notification_options = notification_options;
        state.options.voice_options = voice_options;
        state.options.key_bindings = KeyBindings::from_options(&keymap_options);
        state.apply_ui_state_options(ui_state_options);
        state
    }

    #[cfg(test)]
    pub fn display_options(&self) -> DisplayOptions {
        self.options.display_options
    }

    #[cfg(test)]
    pub fn composer_options(&self) -> ComposerOptions {
        self.options.composer_options
    }

    #[cfg(test)]
    pub fn new_with_display_options(display_options: DisplayOptions) -> Self {
        Self::new_with_options(
            display_options,
            ComposerOptions::default(),
            CredentialOptions::default(),
            NotificationOptions::default(),
            VoiceOptions::default(),
            KeymapOptions::default(),
            UiStateOptions::default(),
        )
    }

    #[cfg(test)]
    pub fn new_with_voice_options(voice_options: VoiceOptions) -> Self {
        Self::new_with_options(
            DisplayOptions::default(),
            ComposerOptions::default(),
            CredentialOptions::default(),
            NotificationOptions::default(),
            voice_options,
            KeymapOptions::default(),
            UiStateOptions::default(),
        )
    }

    #[cfg(test)]
    pub fn new_with_notification_options(notification_options: NotificationOptions) -> Self {
        Self::new_with_options(
            DisplayOptions::default(),
            ComposerOptions::default(),
            CredentialOptions::default(),
            notification_options,
            VoiceOptions::default(),
            KeymapOptions::default(),
            UiStateOptions::default(),
        )
    }

    pub fn notification_options(&self) -> NotificationOptions {
        self.options.notification_options.clone()
    }

    #[cfg(test)]
    pub fn voice_options(&self) -> VoiceOptions {
        self.options.voice_options
    }

    pub fn key_bindings(&self) -> &crate::tui::keybindings::KeyBindings {
        &self.options.key_bindings
    }

    fn apply_ui_state_options(&mut self, options: UiStateOptions) {
        self.navigation.guilds.visible = options.guild_pane_visible;
        self.navigation.channels.visible = options.channel_pane_visible;
        self.navigation.members.visible = options.member_pane_visible;
        self.navigation.guilds.width = options.server_width;
        self.navigation.channels.width = options.channel_list_width;
        self.navigation.members.width = options.member_list_width;
        if !self.is_pane_visible(self.navigation.focus) {
            self.navigation.focus = FocusPane::Messages;
        }
        self.navigation.channels.collapsed_channel_categories =
            options.collapsed_channel_categories.into_iter().collect();
        self.navigation.guilds.collapsed_folders = options
            .collapsed_server_folder_ids
            .into_iter()
            .map(FolderKey::Id)
            .chain(
                options
                    .collapsed_server_folder_guilds
                    .into_iter()
                    .map(FolderKey::Guilds),
            )
            .collect();
    }

    fn ui_state_options(&self) -> UiStateOptions {
        let mut collapsed_channel_categories: Vec<_> = self
            .navigation
            .channels
            .collapsed_channel_categories
            .iter()
            .copied()
            .collect();
        collapsed_channel_categories.sort_by_key(|id| id.get());

        let mut collapsed_server_folder_ids = Vec::new();
        let mut collapsed_server_folder_guilds = Vec::new();
        for folder in &self.navigation.guilds.collapsed_folders {
            match folder {
                FolderKey::Id(id) => collapsed_server_folder_ids.push(*id),
                FolderKey::Guilds(guilds) => collapsed_server_folder_guilds.push(guilds.clone()),
            }
        }
        collapsed_server_folder_ids.sort_unstable();
        collapsed_server_folder_guilds.sort_by(|left, right| {
            left.iter()
                .map(|id| id.get())
                .cmp(right.iter().map(|id| id.get()))
        });

        UiStateOptions {
            guild_pane_visible: self.navigation.guilds.visible,
            channel_pane_visible: self.navigation.channels.visible,
            member_pane_visible: self.navigation.members.visible,
            server_width: self.navigation.guilds.width,
            channel_list_width: self.navigation.channels.width,
            member_list_width: self.navigation.members.width,
            collapsed_channel_categories,
            collapsed_server_folder_ids,
            collapsed_server_folder_guilds,
        }
    }

    pub fn show_avatars(&self) -> bool {
        self.options.display_options.avatars_visible()
    }

    pub fn circular_avatars(&self) -> bool {
        self.options.display_options.circular_avatars
    }

    pub fn show_images(&self) -> bool {
        self.options.display_options.images_visible()
    }

    pub fn image_preview_quality(&self) -> ImagePreviewQualityPreset {
        self.options.display_options.image_preview_quality
    }

    pub fn show_custom_emoji(&self) -> bool {
        self.options.display_options.custom_emoji_visible()
    }

    pub fn desktop_notifications_enabled(&self) -> bool {
        self.options.notification_options.desktop_notifications
    }

    pub fn desktop_notification_icon(&self) -> Option<String> {
        self.options.notification_options.notification_icon.clone()
    }

    pub(in crate::tui::state) fn queue_current_voice_state_update(&mut self) {
        let Some(voice) = self.runtime.voice_connection else {
            return;
        };
        let Some(channel_id) = voice.channel_id else {
            return;
        };

        self.enqueue_pending_command(AppCommand::UpdateVoiceState {
            scope: voice.scope,
            channel_id,
            self_mute: self.options.voice_options.self_mute,
            self_deaf: self.options.voice_options.self_deaf,
        });
    }

    pub(in crate::tui) fn take_options_save_request(&mut self) -> Option<AppOptions> {
        if !self.options.config_save_pending {
            return None;
        }
        self.options.config_save_pending = false;
        Some(AppOptions {
            display: self.options.display_options,
            composer: self.options.composer_options,
            credentials: self.options.credential_options,
            notifications: self.options.notification_options.clone(),
            voice: self.options.voice_options,
        })
    }

    pub(in crate::tui) fn take_ui_state_save_request(&mut self) -> Option<UiStateOptions> {
        if !self.options.ui_state_save_pending {
            return None;
        }
        self.options.ui_state_save_pending = false;
        Some(self.ui_state_options())
    }
}
