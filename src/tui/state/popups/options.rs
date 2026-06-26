use crate::config::MicrophoneSensitivityDb;
use crate::discord::AppCommand;
use crate::tui::keybindings::OptionsCategoryShortcut;

use super::super::{DashboardState, DisplayOptionItem};
use super::{ActiveModalPopupKind, ModalPopup, OptionsCategory, OptionsPopupState};

const DISPLAY_OPTION_COUNT: usize = 6;
const COMPOSER_OPTION_COUNT: usize = 1;
const NOTIFICATION_OPTION_COUNT: usize = 1;
const VOICE_OPTION_COUNT: usize = 6;
const OPTION_CATEGORY_COUNT: usize = 4;

impl DashboardState {
    #[cfg(test)]
    pub fn open_options_popup(&mut self) {
        self.open_options_category(OptionsCategory::Display);
    }

    pub fn open_options_category_picker(&mut self) {
        self.popups.modal = Some(ModalPopup::Options(OptionsPopupState::default()));
    }

    pub fn open_options_category(&mut self, category: OptionsCategory) {
        self.popups.modal = Some(ModalPopup::Options(OptionsPopupState {
            category: Some(category),
            ..OptionsPopupState::default()
        }));
    }

    pub fn close_options_popup(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::Options) {
            self.popups.clear_modal();
        }
    }

    pub fn move_option_down(&mut self) {
        let item_count = self.options_popup_item_count();
        if let Some(popup) = self.popups.options_popup_mut() {
            popup.selection.move_down(item_count);
        }
    }

    pub fn move_option_up(&mut self) {
        if let Some(popup) = self.popups.options_popup_mut() {
            popup.selection.move_up();
        }
    }

    pub fn selected_option_index(&self) -> Option<usize> {
        self.popups.options_popup().map(|popup| {
            popup
                .selection
                .selected_for_len(self.options_popup_item_count())
        })
    }

    pub fn options_popup_title(&self) -> &'static str {
        match self.popups.options_popup().and_then(|popup| popup.category) {
            None => "Options",
            Some(OptionsCategory::Display) => "Display Options",
            Some(OptionsCategory::Composer) => "Composer Options",
            Some(OptionsCategory::Notifications) => "Notification Options",
            Some(OptionsCategory::Voice) => "Voice Options",
        }
    }

    pub fn is_options_category_picker_open(&self) -> bool {
        self.popups
            .options_popup()
            .is_some_and(|popup| popup.category.is_none())
    }

    pub(super) fn options_popup_item_count(&self) -> usize {
        match self.popups.options_popup().and_then(|popup| popup.category) {
            None => OPTION_CATEGORY_COUNT,
            Some(OptionsCategory::Display) => DISPLAY_OPTION_COUNT,
            Some(OptionsCategory::Composer) => COMPOSER_OPTION_COUNT,
            Some(OptionsCategory::Notifications) => NOTIFICATION_OPTION_COUNT,
            Some(OptionsCategory::Voice) => VOICE_OPTION_COUNT,
        }
    }

    pub fn display_option_items(&self) -> Vec<DisplayOptionItem> {
        match self.popups.options_popup().and_then(|popup| popup.category) {
            None if self.is_active_modal_popup(ActiveModalPopupKind::Options) => {
                return self.option_category_items();
            }
            Some(OptionsCategory::Display) => return self.display_option_items_for_display(),
            Some(OptionsCategory::Composer) => return self.display_option_items_for_composer(),
            Some(OptionsCategory::Notifications) => {
                return self.display_option_items_for_notifications();
            }
            Some(OptionsCategory::Voice) => return self.display_option_items_for_voice(),
            None => {}
        }

        let mut items = self.display_option_items_for_display();
        items.extend(self.display_option_items_for_composer());
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
                description: "Image, custom emoji, and pane display settings.",
            },
            DisplayOptionItem {
                label: "Composer",
                enabled: true,
                value: Some(
                    key_bindings
                        .options_category_shortcut_label(OptionsCategoryShortcut::Composer)
                        .to_owned(),
                ),
                gauge_percent: None,
                effective: true,
                description: "Message input and send-format settings.",
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
                description: "Attachment, embed, and attachment viewer previews.",
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

    fn display_option_items_for_composer(&self) -> Vec<DisplayOptionItem> {
        let options = self.options.composer_options;
        vec![DisplayOptionItem {
            label: "Emojis as links",
            enabled: options.emojis_as_links,
            value: None,
            gauge_percent: None,
            effective: options.emojis_as_links,
            description: "Sends unavailable emojis as a link instead.",
        }]
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
        let Some(category) = self.popups.options_popup().and_then(|popup| popup.category) else {
            self.open_selected_options_category();
            return;
        };

        let mut update_current_voice_state = false;
        let mut update_current_voice_capture_permission = false;
        let images_visible_before = self.show_images();

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
            (OptionsCategory::Composer, 0) => {
                self.options.composer_options.emojis_as_links =
                    !self.options.composer_options.emojis_as_links
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
        if images_visible_before != self.show_images() {
            self.refresh_composer_attachment_previews();
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
        if self.popups.options_popup().and_then(|popup| popup.category)
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
            OptionsCategoryShortcut::Composer => {
                self.open_options_category(OptionsCategory::Composer)
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
            Some(1) => self.open_options_category(OptionsCategory::Composer),
            Some(2) => self.open_options_category(OptionsCategory::Notifications),
            Some(3) => self.open_options_category(OptionsCategory::Voice),
            _ => {}
        }
    }

    fn after_display_option_changed(
        &mut self,
        update_current_voice_state: bool,
        update_current_voice_capture_permission: bool,
    ) {
        self.clear_message_row_content_metrics_cache();
        self.options.config_save_pending = true;
        if update_current_voice_state {
            self.queue_current_voice_state_update();
        }
        if update_current_voice_capture_permission {
            self.queue_current_voice_capture_permission_update();
        }
    }

    fn queue_current_voice_capture_permission_update(&mut self) {
        let Some(voice) = self.runtime.voice_connection else {
            return;
        };
        let Some(channel_id) = voice.channel_id else {
            return;
        };

        self.enqueue_pending_command(AppCommand::UpdateVoiceCapturePermission {
            scope: voice.scope,
            channel_id,
            allow_microphone_transmit: self.options.voice_options.allow_microphone_transmit,
            microphone_sensitivity: self.options.voice_options.microphone_sensitivity,
            microphone_volume: self.options.voice_options.microphone_volume,
            voice_output_volume: self.options.voice_options.voice_output_volume,
        });
    }
}

fn microphone_sensitivity_percent(sensitivity: MicrophoneSensitivityDb) -> u16 {
    (i16::from(sensitivity.value()) + 100) as u16
}
