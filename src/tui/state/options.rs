use crate::config::{
    AppOptions, DisplayOptions, ImagePreviewQualityPreset, KeymapOptions, NotificationOptions,
    UiStateOptions, VoiceOptions,
};
use crate::discord::AppCommand;
use crate::tui::keybindings::{KeyBindings, OptionsCategoryShortcut};

use super::{
    DashboardState, FocusPane, FolderKey,
    popups::{OptionsCategory, OptionsPopupState},
};

const DISPLAY_OPTION_COUNT: usize = 6;
const NOTIFICATION_OPTION_COUNT: usize = 1;
const VOICE_OPTION_COUNT: usize = 6;
const OPTION_CATEGORY_COUNT: usize = 3;
const MIN_PANE_WIDTH: u16 = 8;
const MAX_PANE_WIDTH: u16 = 80;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayOptionItem {
    pub label: &'static str,
    pub enabled: bool,
    pub value: Option<String>,
    pub gauge_percent: Option<u16>,
    pub effective: bool,
    pub description: &'static str,
}

#[derive(Debug, Default)]
pub(super) struct OptionsUiState {
    pub(super) display_options: DisplayOptions,
    pub(super) notification_options: NotificationOptions,
    pub(super) voice_options: VoiceOptions,
    pub(super) key_bindings: KeyBindings,
    pub(super) options_save_pending: bool,
}

impl OptionsUiState {
    pub(super) fn key_bindings(&self) -> &KeyBindings {
        &self.key_bindings
    }
}

impl DashboardState {
    pub fn new_with_options(
        display_options: DisplayOptions,
        notification_options: NotificationOptions,
        voice_options: VoiceOptions,
        keymap_options: KeymapOptions,
        ui_state_options: UiStateOptions,
    ) -> Self {
        let mut state = Self::new();
        state.options.display_options = display_options;
        state.options.notification_options = notification_options;
        state.options.voice_options = voice_options;
        state.options.key_bindings = KeyBindings::from_options(&keymap_options);
        state.apply_ui_state_options(ui_state_options);
        state
    }

    pub fn display_options(&self) -> DisplayOptions {
        self.options.display_options
    }

    #[cfg(test)]
    pub fn new_with_display_options(display_options: DisplayOptions) -> Self {
        Self::new_with_options(
            display_options,
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
            notification_options,
            VoiceOptions::default(),
            KeymapOptions::default(),
            UiStateOptions::default(),
        )
    }

    pub fn notification_options(&self) -> NotificationOptions {
        self.options.notification_options
    }

    pub fn voice_options(&self) -> VoiceOptions {
        self.options.voice_options
    }

    pub fn key_bindings(&self) -> &crate::tui::keybindings::KeyBindings {
        &self.options.key_bindings
    }

    fn apply_ui_state_options(&mut self, options: UiStateOptions) {
        self.navigation.collapsed_channel_categories =
            options.collapsed_channel_categories.into_iter().collect();
        self.navigation.collapsed_folders = options
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
            .collapsed_channel_categories
            .iter()
            .copied()
            .collect();
        collapsed_channel_categories.sort_by_key(|id| id.get());

        let mut collapsed_server_folder_ids = Vec::new();
        let mut collapsed_server_folder_guilds = Vec::new();
        for folder in &self.navigation.collapsed_folders {
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

    pub fn pane_width(&self, pane: FocusPane) -> u16 {
        match pane {
            FocusPane::Guilds => self.options.display_options.server_width,
            FocusPane::Channels => self.options.display_options.channel_list_width,
            FocusPane::Members => self.options.display_options.member_list_width,
            FocusPane::Messages => 0,
        }
    }

    pub fn adjust_focused_pane_width(&mut self, delta: i16) {
        let width = match self.navigation.focus {
            FocusPane::Guilds => &mut self.options.display_options.server_width,
            FocusPane::Channels => &mut self.options.display_options.channel_list_width,
            FocusPane::Members => &mut self.options.display_options.member_list_width,
            FocusPane::Messages => return,
        };

        let adjusted = if delta.is_negative() {
            width.saturating_sub(delta.unsigned_abs())
        } else {
            width.saturating_add(delta as u16)
        };
        let adjusted = adjusted.clamp(MIN_PANE_WIDTH, MAX_PANE_WIDTH);
        if adjusted != *width {
            *width = adjusted;
            self.options.options_save_pending = true;
        }
    }

    pub fn is_options_popup_open(&self) -> bool {
        self.popups.options_popup.is_some()
    }

    #[cfg(test)]
    pub fn open_options_popup(&mut self) {
        self.open_options_category(OptionsCategory::Display);
    }

    pub fn open_options_category_picker(&mut self) {
        self.popups.options_popup = Some(OptionsPopupState::default());
    }

    pub fn open_options_category(&mut self, category: OptionsCategory) {
        self.popups.options_popup = Some(OptionsPopupState {
            category: Some(category),
            ..OptionsPopupState::default()
        });
    }

    pub fn close_options_popup(&mut self) {
        self.popups.options_popup = None;
    }

    pub fn move_option_down(&mut self) {
        let max_selected = self.options_popup_item_count().saturating_sub(1);
        if let Some(popup) = &mut self.popups.options_popup {
            popup.selected = popup.selected.saturating_add(1).min(max_selected);
        }
    }

    pub fn move_option_up(&mut self) {
        if let Some(popup) = &mut self.popups.options_popup {
            popup.selected = popup.selected.saturating_sub(1);
        }
    }

    pub fn selected_option_index(&self) -> Option<usize> {
        self.popups.options_popup.as_ref().map(|popup| {
            popup
                .selected
                .min(self.options_popup_item_count().saturating_sub(1))
        })
    }

    pub fn options_popup_title(&self) -> &'static str {
        match self
            .popups
            .options_popup
            .as_ref()
            .and_then(|popup| popup.category)
        {
            None => "Options",
            Some(OptionsCategory::Display) => "Display Options",
            Some(OptionsCategory::Notifications) => "Notification Options",
            Some(OptionsCategory::Voice) => "Voice Options",
        }
    }

    pub fn is_options_category_picker_open(&self) -> bool {
        self.popups
            .options_popup
            .as_ref()
            .is_some_and(|popup| popup.category.is_none())
    }

    fn options_popup_item_count(&self) -> usize {
        match self
            .popups
            .options_popup
            .as_ref()
            .and_then(|popup| popup.category)
        {
            None => OPTION_CATEGORY_COUNT,
            Some(OptionsCategory::Display) => DISPLAY_OPTION_COUNT,
            Some(OptionsCategory::Notifications) => NOTIFICATION_OPTION_COUNT,
            Some(OptionsCategory::Voice) => VOICE_OPTION_COUNT,
        }
    }

    pub fn display_option_items(&self) -> Vec<DisplayOptionItem> {
        match self
            .popups
            .options_popup
            .as_ref()
            .and_then(|popup| popup.category)
        {
            None if self.is_options_popup_open() => return self.option_category_items(),
            Some(OptionsCategory::Display) => return self.display_option_items_for_display(),
            Some(OptionsCategory::Notifications) => {
                return self.display_option_items_for_notifications();
            }
            Some(OptionsCategory::Voice) => return self.display_option_items_for_voice(),
            None => {}
        }

        let mut items = self.display_option_items_for_display();
        items.extend(self.display_option_items_for_notifications());
        items.extend(self.display_option_items_for_voice());
        items
    }

    fn option_category_items(&self) -> Vec<DisplayOptionItem> {
        let key_bindings = self.options.key_bindings();
        vec![
            DisplayOptionItem {
                label: "Display",
                enabled: true,
                value: Some(
                    key_bindings
                        .options_category_shortcut_label(OptionsCategoryShortcut::Display)
                        .to_owned(),
                ),
                gauge_percent: None,
                effective: true,
                description: "Image, emoji, and pane display settings.",
            },
            DisplayOptionItem {
                label: "Notifications",
                enabled: true,
                value: Some(
                    key_bindings
                        .options_category_shortcut_label(OptionsCategoryShortcut::Notifications)
                        .to_owned(),
                ),
                gauge_percent: None,
                effective: true,
                description: "Desktop notification settings.",
            },
            DisplayOptionItem {
                label: "Voice",
                enabled: true,
                value: Some(
                    key_bindings
                        .options_category_shortcut_label(OptionsCategoryShortcut::Voice)
                        .to_owned(),
                ),
                gauge_percent: None,
                effective: true,
                description: "Mute, deaf, microphone transmit, sensitivity, and volume settings.",
            },
        ]
    }

    fn display_option_items_for_display(&self) -> Vec<DisplayOptionItem> {
        let options = self.options.display_options;
        vec![
            DisplayOptionItem {
                label: "Disable all image previews",
                enabled: options.disable_image_preview,
                value: None,
                gauge_percent: None,
                effective: options.disable_image_preview,
                description: "Master switch for avatars, images, and custom emoji images.",
            },
            DisplayOptionItem {
                label: "Show avatars",
                enabled: options.show_avatars,
                value: None,
                gauge_percent: None,
                effective: options.avatars_visible(),
                description: "Message and profile avatars.",
            },
            DisplayOptionItem {
                label: "Show images",
                enabled: options.show_images,
                value: None,
                gauge_percent: None,
                effective: options.images_visible(),
                description: "Attachment, embed, and image viewer previews.",
            },
            DisplayOptionItem {
                label: "Image preview quality",
                enabled: true,
                value: Some(options.image_preview_quality.label().to_owned()),
                gauge_percent: None,
                effective: options.images_visible(),
                description: "Quality preset for attachment, embed, and viewer previews.",
            },
            DisplayOptionItem {
                label: "Show custom emoji images",
                enabled: options.show_custom_emoji,
                value: None,
                gauge_percent: None,
                effective: options.custom_emoji_visible(),
                description: "When off, custom emoji are shown as their emoji id.",
            },
            DisplayOptionItem {
                label: "Circular avatars",
                enabled: options.circular_avatars,
                value: None,
                gauge_percent: None,
                effective: options.avatars_visible() && options.circular_avatars,
                description: "Mask message and profile avatars into a circle.",
            },
        ]
    }

    fn display_option_items_for_notifications(&self) -> Vec<DisplayOptionItem> {
        vec![DisplayOptionItem {
            label: "Desktop notifications",
            enabled: self.options.notification_options.desktop_notifications,
            value: None,
            gauge_percent: None,
            effective: self.options.notification_options.desktop_notifications,
            description: "Show OS notifications for Discord messages that pass notification settings.",
        }]
    }

    fn display_option_items_for_voice(&self) -> Vec<DisplayOptionItem> {
        vec![
            DisplayOptionItem {
                label: "Voice muted",
                enabled: self.options.voice_options.self_mute,
                value: None,
                gauge_percent: None,
                effective: true,
                description: "Set your Discord voice microphone mute state.",
            },
            DisplayOptionItem {
                label: "Voice deafened",
                enabled: self.options.voice_options.self_deaf,
                value: None,
                gauge_percent: None,
                effective: true,
                description: "Set your Discord voice playback deaf state.",
            },
            DisplayOptionItem {
                label: "Allow microphone transmit",
                enabled: self.options.voice_options.allow_microphone_transmit,
                value: None,
                gauge_percent: None,
                effective: true,
                description: "Permit microphone transmit while joined and not muted.",
            },
            DisplayOptionItem {
                label: "Microphone sensitivity",
                enabled: true,
                value: Some(self.options.voice_options.microphone_sensitivity.label()),
                gauge_percent: Some(microphone_sensitivity_percent(
                    self.options.voice_options.microphone_sensitivity,
                )),
                effective: self.options.voice_options.allow_microphone_transmit,
                description: "Lower dB values transmit quieter microphone input.",
            },
            DisplayOptionItem {
                label: "Microphone volume",
                enabled: true,
                value: Some(self.options.voice_options.microphone_volume.label()),
                gauge_percent: Some(u16::from(
                    self.options.voice_options.microphone_volume.value(),
                )),
                effective: self.options.voice_options.allow_microphone_transmit,
                description: "Adjust outgoing microphone audio level.",
            },
            DisplayOptionItem {
                label: "Voice volume",
                enabled: true,
                value: Some(self.options.voice_options.voice_output_volume.label()),
                gauge_percent: Some(u16::from(
                    self.options.voice_options.voice_output_volume.value(),
                )),
                effective: !self.options.voice_options.self_deaf,
                description: "Adjust received voice playback level.",
            },
        ]
    }

    pub fn toggle_selected_display_option(&mut self) {
        let Some(selected) = self.selected_option_index() else {
            return;
        };
        let Some(category) = self
            .popups
            .options_popup
            .as_ref()
            .and_then(|popup| popup.category)
        else {
            self.open_selected_options_category();
            return;
        };

        let mut update_current_voice_state = false;
        let mut update_current_voice_capture_permission = false;

        match (category, selected) {
            (OptionsCategory::Display, 0) => {
                self.options.display_options.disable_image_preview =
                    !self.options.display_options.disable_image_preview
            }
            (OptionsCategory::Display, 1) => {
                self.options.display_options.show_avatars =
                    !self.options.display_options.show_avatars
            }
            (OptionsCategory::Display, 2) => {
                self.options.display_options.show_images = !self.options.display_options.show_images
            }
            (OptionsCategory::Display, 3) => {
                self.options.display_options.image_preview_quality =
                    self.options.display_options.image_preview_quality.next()
            }
            (OptionsCategory::Display, 4) => {
                self.options.display_options.show_custom_emoji =
                    !self.options.display_options.show_custom_emoji
            }
            (OptionsCategory::Display, 5) => {
                self.options.display_options.circular_avatars =
                    !self.options.display_options.circular_avatars
            }
            (OptionsCategory::Notifications, 0) => {
                self.options.notification_options.desktop_notifications =
                    !self.options.notification_options.desktop_notifications
            }
            (OptionsCategory::Voice, 0) => {
                self.options.voice_options.self_mute = !self.options.voice_options.self_mute;
                update_current_voice_state = true;
            }
            (OptionsCategory::Voice, 1) => {
                self.options.voice_options.self_deaf = !self.options.voice_options.self_deaf;
                update_current_voice_state = true;
            }
            (OptionsCategory::Voice, 2) => {
                self.options.voice_options.allow_microphone_transmit =
                    !self.options.voice_options.allow_microphone_transmit;
                update_current_voice_capture_permission = true;
            }
            _ => return,
        }
        self.after_display_option_changed(
            update_current_voice_state,
            update_current_voice_capture_permission,
        );
    }

    pub fn adjust_selected_display_option(&mut self, delta: i8) {
        let Some(selected) = self.selected_option_index() else {
            return;
        };
        if self
            .popups
            .options_popup
            .as_ref()
            .and_then(|popup| popup.category)
            != Some(OptionsCategory::Voice)
        {
            return;
        }
        let changed = match selected {
            3 => {
                let previous = self.options.voice_options.microphone_sensitivity;
                self.options.voice_options.microphone_sensitivity = previous.adjust(delta);
                self.options.voice_options.microphone_sensitivity != previous
            }
            4 => {
                let previous = self.options.voice_options.microphone_volume;
                self.options.voice_options.microphone_volume = previous.adjust(delta);
                self.options.voice_options.microphone_volume != previous
            }
            5 => {
                let previous = self.options.voice_options.voice_output_volume;
                self.options.voice_options.voice_output_volume = previous.adjust(delta);
                self.options.voice_options.voice_output_volume != previous
            }
            _ => false,
        };
        if changed {
            self.after_display_option_changed(false, true);
        }
    }

    pub fn open_options_category_from_shortcut(&mut self, shortcut: OptionsCategoryShortcut) {
        match shortcut {
            OptionsCategoryShortcut::Display => {
                self.open_options_category(OptionsCategory::Display)
            }
            OptionsCategoryShortcut::Notifications => {
                self.open_options_category(OptionsCategory::Notifications)
            }
            OptionsCategoryShortcut::Voice => self.open_options_category(OptionsCategory::Voice),
        }
    }

    fn open_selected_options_category(&mut self) {
        match self.selected_option_index() {
            Some(0) => self.open_options_category(OptionsCategory::Display),
            Some(1) => self.open_options_category(OptionsCategory::Notifications),
            Some(2) => self.open_options_category(OptionsCategory::Voice),
            _ => {}
        }
    }

    fn after_display_option_changed(
        &mut self,
        update_current_voice_state: bool,
        update_current_voice_capture_permission: bool,
    ) {
        if !self.show_images() {
            self.close_image_viewer();
        }
        self.clear_message_row_content_metrics_cache();
        self.options.options_save_pending = true;
        if update_current_voice_state {
            self.queue_current_voice_state_update();
        }
        if update_current_voice_capture_permission {
            self.queue_current_voice_capture_permission_update();
        }
    }

    pub(in crate::tui::state) fn queue_current_voice_state_update(&mut self) {
        let Some(voice) = self.runtime.voice_connection else {
            return;
        };
        let Some(channel_id) = voice.channel_id else {
            return;
        };

        self.enqueue_pending_command(AppCommand::UpdateVoiceState {
            guild_id: voice.guild_id,
            channel_id,
            self_mute: self.options.voice_options.self_mute,
            self_deaf: self.options.voice_options.self_deaf,
        });
    }

    fn queue_current_voice_capture_permission_update(&mut self) {
        let Some(voice) = self.runtime.voice_connection else {
            return;
        };
        let Some(channel_id) = voice.channel_id else {
            return;
        };

        self.enqueue_pending_command(AppCommand::UpdateVoiceCapturePermission {
            guild_id: voice.guild_id,
            channel_id,
            allow_microphone_transmit: self.options.voice_options.allow_microphone_transmit,
            microphone_sensitivity: self.options.voice_options.microphone_sensitivity,
            microphone_volume: self.options.voice_options.microphone_volume,
            voice_output_volume: self.options.voice_options.voice_output_volume,
        });
    }

    pub(in crate::tui) fn take_options_save_request(&mut self) -> Option<AppOptions> {
        if !self.options.options_save_pending {
            return None;
        }
        self.options.options_save_pending = false;
        Some(AppOptions {
            display: self.options.display_options,
            notifications: self.options.notification_options,
            voice: self.options.voice_options,
            ui_state: self.ui_state_options(),
        })
    }
}

fn microphone_sensitivity_percent(sensitivity: crate::config::MicrophoneSensitivityDb) -> u16 {
    (i16::from(sensitivity.value()) + 100) as u16
}
