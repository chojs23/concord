use crate::discord::AppCommand;
use crate::discord::{MicrophoneSensitivityDb, VoiceAudioSourceOptions, VoiceVolumePercent};
use crate::tui::keybindings::{OptionsCategoryShortcut, SelectionAction};

use super::super::{DashboardState, DisplayOptionGauge, DisplayOptionItem};
use super::{
    ActiveModalPopupKind, ModalPopup, OptionsCategory, OptionsPopupState, SelectablePopupTarget,
};

const DISPLAY_OPTION_COUNT: usize = 9;
const COMPOSER_OPTION_COUNT: usize = 1;
const NOTIFICATION_OPTION_COUNT: usize = 1;
const VOICE_OPTION_COUNT: usize = VoiceOption::ALL.len();
const OPTION_CATEGORY_COUNT: usize = 4;

/// A row of the voice options popup.
///
/// `ALL` is the single source of on-screen order: the row list is built by
/// mapping over it, and every selection index is resolved through it. Adding or
/// moving a row is therefore a change to `ALL` alone, with no match arms to
/// renumber and no row count to keep in sync by hand.
#[derive(Clone, Copy, Eq, PartialEq)]
enum VoiceOption {
    Muted,
    Deafened,
    InputSource,
    OutputSource,
    AllowMicrophoneTransmit,
    PushToTalk,
    PushToTalkShortcut,
    NoiseSuppression,
    MicrophoneSensitivity,
    MicrophoneVolume,
    VoiceVolume,
}

impl VoiceOption {
    const ALL: [Self; 11] = [
        Self::Muted,
        Self::Deafened,
        Self::InputSource,
        Self::OutputSource,
        Self::AllowMicrophoneTransmit,
        Self::PushToTalk,
        Self::PushToTalkShortcut,
        Self::NoiseSuppression,
        Self::MicrophoneSensitivity,
        Self::MicrophoneVolume,
        Self::VoiceVolume,
    ];

    fn at(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}

impl DashboardState {
    #[cfg(test)]
    pub fn open_options_popup(&mut self) {
        self.open_options_category(OptionsCategory::Display);
    }

    pub fn open_options_category_picker(&mut self) {
        self.popups
            .set_modal(ModalPopup::Options(OptionsPopupState::default()));
    }

    pub fn open_options_category(&mut self, category: OptionsCategory) {
        if category == OptionsCategory::Voice {
            self.options.next_voice_audio_sources_request_id = self
                .options
                .next_voice_audio_sources_request_id
                .wrapping_add(1)
                .max(1);
            let request_id = self.options.next_voice_audio_sources_request_id;
            self.options.voice_audio_sources_request_id = Some(request_id);
            self.enqueue_pending_command(AppCommand::LoadVoiceAudioSources { request_id });
        }
        self.popups
            .set_modal(ModalPopup::Options(OptionsPopupState {
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
        self.move_selectable_popup(SelectablePopupTarget::Options, SelectionAction::Next);
    }

    pub fn move_option_up(&mut self) {
        self.move_selectable_popup(SelectablePopupTarget::Options, SelectionAction::Previous);
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

    pub(in crate::tui) fn is_capturing_push_to_talk_shortcut(&self) -> bool {
        self.popups
            .options_popup()
            .is_some_and(|popup| popup.capturing_push_to_talk_shortcut)
    }

    pub(in crate::tui) fn cancel_push_to_talk_shortcut_capture(&mut self) {
        if let Some(popup) = self.popups.options_popup_mut() {
            popup.capturing_push_to_talk_shortcut = false;
        }
    }

    pub(in crate::tui) fn capture_push_to_talk_shortcut(&mut self, shortcut: String) {
        self.cancel_push_to_talk_shortcut_capture();
        if self.options.voice_options.push_to_talk_shortcut == shortcut {
            return;
        }
        self.options.voice_options.push_to_talk_shortcut = shortcut;
        self.mark_options_changed();
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
        vec![
            DisplayOptionItem {
                label: "Display",
                enabled: true,
                value: Some(OptionsCategoryShortcut::Display.key().to_string()),
                gauge: None,
                effective: true,
                description: "Image, custom emoji, and pane display settings.",
            },
            DisplayOptionItem {
                label: "Composer",
                enabled: true,
                value: Some(OptionsCategoryShortcut::Composer.key().to_string()),
                gauge: None,
                effective: true,
                description: "Message input and send-format settings.",
            },
            DisplayOptionItem {
                label: "Notifications",
                enabled: true,
                value: Some(OptionsCategoryShortcut::Notifications.key().to_string()),
                gauge: None,
                effective: true,
                description: "Desktop notification settings.",
            },
            DisplayOptionItem {
                label: "Voice",
                enabled: true,
                value: Some(OptionsCategoryShortcut::Voice.key().to_string()),
                gauge: None,
                effective: true,
                description: "Mute, deaf, push-to-talk, microphone processing, and volume settings.",
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
                gauge: None,
                effective: options.disable_image_preview,
                description: "Master switch for avatars, images, and custom emoji images.",
            },
            DisplayOptionItem {
                label: "Show avatars",
                enabled: options.show_avatars,
                value: None,
                gauge: None,
                effective: options.avatars_visible(),
                description: "Message and profile avatars.",
            },
            DisplayOptionItem {
                label: "Show images",
                enabled: options.show_images,
                value: None,
                gauge: None,
                effective: options.images_visible(),
                description: "Attachment, embed, and attachment viewer previews.",
            },
            DisplayOptionItem {
                label: "Image preview quality",
                enabled: true,
                value: Some(options.image_preview_quality.label().to_owned()),
                gauge: None,
                effective: options.images_visible(),
                description: "Quality preset for attachment and embed.",
            },
            DisplayOptionItem {
                label: "Attachment viewer quality",
                enabled: true,
                value: Some(options.attachment_viewer_quality.label().to_owned()),
                gauge: None,
                effective: options.images_visible(),
                description: "Quality preset for attachment viewer previews.",
            },
            DisplayOptionItem {
                label: "Show custom emoji images",
                enabled: options.show_custom_emoji,
                value: None,
                gauge: None,
                effective: options.custom_emoji_visible(),
                description: "When off, custom emoji are shown as their emoji id.",
            },
            DisplayOptionItem {
                label: "Circular avatars",
                enabled: options.circular_avatars,
                value: None,
                gauge: None,
                effective: options.avatars_visible() && options.circular_avatars,
                description: "Mask message and profile avatars into a circle.",
            },
            DisplayOptionItem {
                label: "Media playback",
                enabled: options.media_playback,
                value: None,
                gauge: None,
                effective: options.media_playback_enabled(),
                description: "Allow videos to open in the external media player.",
            },
            DisplayOptionItem {
                label: "24-hour time",
                enabled: options.hour_format_24,
                value: None,
                gauge: None,
                effective: options.hour_format_24,
                description: "Use 24-hour time for message timestamps.",
            },
        ]
    }

    fn display_option_items_for_composer(&self) -> Vec<DisplayOptionItem> {
        let options = self.options.composer_options;
        vec![DisplayOptionItem {
            label: "Emojis as links",
            enabled: options.emojis_as_links,
            value: None,
            gauge: None,
            effective: options.emojis_as_links,
            description: "Sends unavailable emojis as a link instead.",
        }]
    }

    fn display_option_items_for_notifications(&self) -> Vec<DisplayOptionItem> {
        vec![DisplayOptionItem {
            label: "Desktop notifications",
            enabled: self.options.notification_options.desktop_notifications,
            value: None,
            gauge: None,
            effective: self.options.notification_options.desktop_notifications,
            description: "Show OS notifications for Discord messages that pass notification settings.",
        }]
    }

    fn display_option_items_for_voice(&self) -> Vec<DisplayOptionItem> {
        VoiceOption::ALL
            .iter()
            .map(|option| self.voice_option_item(*option))
            .collect()
    }

    fn voice_option_item(&self, option: VoiceOption) -> DisplayOptionItem {
        let voice = &self.options.voice_options;
        match option {
            VoiceOption::Muted => DisplayOptionItem {
                label: "Voice muted",
                enabled: voice.self_mute,
                value: None,
                gauge: None,
                effective: true,
                description: "",
            },
            VoiceOption::Deafened => DisplayOptionItem {
                label: "Voice deafened",
                enabled: voice.self_deaf,
                value: None,
                gauge: None,
                effective: true,
                description: "",
            },
            VoiceOption::InputSource => DisplayOptionItem {
                label: "Input source",
                enabled: true,
                value: Some(self.voice_audio_source_value(|sources| {
                    sources.input_label(voice.input_source.as_deref())
                })),
                gauge: None,
                effective: voice.allow_microphone_transmit,
                description: "Enter or ←/→ to change.",
            },
            VoiceOption::OutputSource => DisplayOptionItem {
                label: "Output source",
                enabled: true,
                value: Some(self.voice_audio_source_value(|sources| {
                    sources.output_label(voice.output_source.as_deref())
                })),
                gauge: None,
                effective: !voice.self_deaf,
                description: "Enter or ←/→ to change.",
            },
            VoiceOption::AllowMicrophoneTransmit => DisplayOptionItem {
                label: "Allow microphone transmit",
                enabled: voice.allow_microphone_transmit,
                value: None,
                gauge: None,
                effective: true,
                description: "",
            },
            VoiceOption::PushToTalk => DisplayOptionItem {
                label: "Push to talk",
                enabled: voice.push_to_talk,
                value: None,
                gauge: None,
                effective: voice.allow_microphone_transmit,
                description: "Hold the shortcut to transmit.",
            },
            VoiceOption::PushToTalkShortcut => DisplayOptionItem {
                label: "Push-to-talk shortcut",
                enabled: true,
                value: Some(if self.is_capturing_push_to_talk_shortcut() {
                    "Press shortcut (Esc cancels)".to_owned()
                } else {
                    voice.push_to_talk_shortcut.clone()
                }),
                gauge: None,
                effective: voice.push_to_talk,
                description: if self.is_capturing_push_to_talk_shortcut() {
                    ""
                } else {
                    "Enter to record."
                },
            },
            VoiceOption::NoiseSuppression => DisplayOptionItem {
                label: "Noise suppression",
                enabled: voice.noise_suppression,
                value: None,
                gauge: None,
                effective: voice.allow_microphone_transmit,
                description: "",
            },
            VoiceOption::MicrophoneSensitivity => DisplayOptionItem {
                label: "Microphone sensitivity",
                enabled: true,
                value: Some(voice.microphone_sensitivity.label()),
                gauge: Some(DisplayOptionGauge::new(
                    microphone_sensitivity_percent(voice.microphone_sensitivity),
                    100,
                )),
                effective: voice.allow_microphone_transmit && !voice.push_to_talk,
                description: "Lower dB detects quieter input.",
            },
            VoiceOption::MicrophoneVolume => DisplayOptionItem {
                label: "Microphone volume",
                enabled: true,
                value: Some(voice.microphone_volume.label()),
                gauge: Some(DisplayOptionGauge::new(
                    u16::from(voice.microphone_volume.value()),
                    u16::from(VoiceVolumePercent::maximum()),
                )),
                effective: voice.allow_microphone_transmit,
                description: "",
            },
            VoiceOption::VoiceVolume => DisplayOptionItem {
                label: "Voice volume",
                enabled: true,
                value: Some(voice.voice_output_volume.label()),
                gauge: Some(DisplayOptionGauge::new(
                    u16::from(voice.voice_output_volume.value()),
                    u16::from(VoiceVolumePercent::maximum()),
                )),
                effective: !voice.self_deaf,
                description: "",
            },
        }
    }

    fn voice_audio_source_value(
        &self,
        label: impl FnOnce(&VoiceAudioSourceOptions) -> String,
    ) -> String {
        if self.options.voice_audio_sources_request_id.is_some() {
            return "Loading sources...".to_owned();
        }
        label(&self.options.voice_audio_source_options)
    }

    pub fn toggle_selected_display_option(&mut self) {
        let Some(selected) = self.selected_option_index() else {
            return;
        };
        let Some(category) = self.popups.options_popup().and_then(|popup| popup.category) else {
            self.open_selected_options_category();
            return;
        };

        if category == OptionsCategory::Voice {
            if let Some(option) = VoiceOption::at(selected) {
                self.toggle_voice_option(option);
            }
            return;
        }

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
                self.options.display_options.attachment_viewer_quality = self
                    .options
                    .display_options
                    .attachment_viewer_quality
                    .next()
            }
            (OptionsCategory::Display, 5) => {
                self.options.display_options.show_custom_emoji =
                    !self.options.display_options.show_custom_emoji
            }
            (OptionsCategory::Display, 6) => {
                self.options.display_options.circular_avatars =
                    !self.options.display_options.circular_avatars
            }
            (OptionsCategory::Display, 7) => {
                self.options.display_options.media_playback =
                    !self.options.display_options.media_playback
            }
            (OptionsCategory::Display, 8) => {
                self.options.display_options.hour_format_24 =
                    !self.options.display_options.hour_format_24
            }
            (OptionsCategory::Composer, 0) => {
                self.options.composer_options.emojis_as_links =
                    !self.options.composer_options.emojis_as_links
            }
            (OptionsCategory::Notifications, 0) => {
                self.options.notification_options.desktop_notifications =
                    !self.options.notification_options.desktop_notifications
            }
            _ => return,
        }
        if images_visible_before != self.show_images() {
            self.refresh_composer_attachment_previews();
        }
        self.mark_options_changed();
    }

    /// Enter on a voice row. Toggles boolean rows and steps the cycled source
    /// rows forward. The gauge rows respond to left and right only.
    fn toggle_voice_option(&mut self, option: VoiceOption) {
        match option {
            VoiceOption::Muted => {
                self.options.voice_options.self_mute = !self.options.voice_options.self_mute;
                self.mark_options_changed();
                self.queue_current_voice_state_update();
            }
            VoiceOption::Deafened => {
                self.options.voice_options.self_deaf = !self.options.voice_options.self_deaf;
                self.mark_options_changed();
                self.queue_current_voice_state_update();
            }
            VoiceOption::InputSource | VoiceOption::OutputSource => {
                self.adjust_voice_option(option, 1);
            }
            VoiceOption::AllowMicrophoneTransmit => {
                self.options.voice_options.allow_microphone_transmit =
                    !self.options.voice_options.allow_microphone_transmit;
                self.mark_options_changed();
                self.queue_current_voice_capture_permission_update();
            }
            VoiceOption::PushToTalk => {
                self.options.voice_options.push_to_talk = !self.options.voice_options.push_to_talk;
                self.mark_options_changed();
                self.queue_current_voice_capture_permission_update();
            }
            VoiceOption::PushToTalkShortcut => {
                if let Some(popup) = self.popups.options_popup_mut() {
                    popup.capturing_push_to_talk_shortcut = true;
                }
            }
            VoiceOption::NoiseSuppression => {
                self.options.voice_options.noise_suppression =
                    !self.options.voice_options.noise_suppression;
                self.mark_options_changed();
                self.queue_current_voice_capture_permission_update();
            }
            VoiceOption::MicrophoneSensitivity
            | VoiceOption::MicrophoneVolume
            | VoiceOption::VoiceVolume => {}
        }
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
        if let Some(option) = VoiceOption::at(selected) {
            self.adjust_voice_option(option, delta);
        }
    }

    fn adjust_voice_option(&mut self, option: VoiceOption, delta: i8) {
        match option {
            VoiceOption::InputSource | VoiceOption::OutputSource => {
                // Cycling against a list that is still loading would pick from
                // the previous popup's devices.
                if self.options.voice_audio_sources_request_id.is_some() {
                    return;
                }
                let changed = if option == VoiceOption::InputSource {
                    self.options
                        .voice_audio_source_options
                        .adjust_input(&mut self.options.voice_options.input_source, delta)
                } else {
                    self.options
                        .voice_audio_source_options
                        .adjust_output(&mut self.options.voice_options.output_source, delta)
                };
                if changed {
                    self.mark_options_changed();
                    self.queue_current_voice_audio_sources_update();
                }
            }
            VoiceOption::MicrophoneSensitivity => {
                let previous = self.options.voice_options.microphone_sensitivity;
                self.options.voice_options.microphone_sensitivity = previous.adjust(delta);
                if self.options.voice_options.microphone_sensitivity != previous {
                    self.mark_options_changed();
                    self.queue_current_voice_capture_permission_update();
                }
            }
            VoiceOption::MicrophoneVolume => {
                let previous = self.options.voice_options.microphone_volume;
                self.options.voice_options.microphone_volume = previous.adjust(delta);
                if self.options.voice_options.microphone_volume != previous {
                    self.mark_options_changed();
                    self.queue_current_voice_capture_permission_update();
                }
            }
            VoiceOption::VoiceVolume => {
                let previous = self.options.voice_options.voice_output_volume;
                self.options.voice_options.voice_output_volume = previous.adjust(delta);
                if self.options.voice_options.voice_output_volume != previous {
                    self.mark_options_changed();
                    self.queue_current_voice_capture_permission_update();
                }
            }
            VoiceOption::Muted
            | VoiceOption::Deafened
            | VoiceOption::AllowMicrophoneTransmit
            | VoiceOption::PushToTalk
            | VoiceOption::PushToTalkShortcut
            | VoiceOption::NoiseSuppression => {}
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

    fn mark_options_changed(&mut self) {
        self.clear_message_row_content_metrics_cache();
        self.options.config_save_pending = true;
    }

    pub(in crate::tui) fn queue_current_voice_audio_sources_update(&mut self) {
        self.enqueue_pending_command(AppCommand::UpdateVoiceAudioSources {
            input_source: self.options.voice_options.input_source.clone(),
            output_source: self.options.voice_options.output_source.clone(),
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
            scope: voice.scope,
            channel_id,
            allow_microphone_transmit: self.options.voice_options.allow_microphone_transmit,
            noise_suppression: self.options.voice_options.noise_suppression,
            microphone_sensitivity: self.options.voice_options.microphone_sensitivity,
            microphone_volume: self.options.voice_options.microphone_volume,
            voice_output_volume: self.options.voice_options.voice_output_volume,
        });
    }
}

fn microphone_sensitivity_percent(sensitivity: MicrophoneSensitivityDb) -> u16 {
    (i16::from(sensitivity.value()) + 100) as u16
}
