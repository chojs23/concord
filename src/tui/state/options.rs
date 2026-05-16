use crate::config::{DisplayOptions, ImagePreviewQualityPreset};

use super::{DashboardState, FocusPane, popups::OptionsPopupState};

const OPTION_COUNT: usize = 6;
const MIN_PANE_WIDTH: u16 = 8;
const MAX_PANE_WIDTH: u16 = 80;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayOptionItem {
    pub label: &'static str,
    pub enabled: bool,
    pub value: Option<&'static str>,
    pub effective: bool,
    pub description: &'static str,
}

impl DashboardState {
    pub fn new_with_display_options(display_options: DisplayOptions) -> Self {
        Self {
            display_options,
            ..Self::new()
        }
    }

    pub fn display_options(&self) -> DisplayOptions {
        self.display_options
    }

    pub fn show_avatars(&self) -> bool {
        self.display_options.avatars_visible()
    }

    pub fn show_images(&self) -> bool {
        self.display_options.images_visible()
    }

    pub fn image_preview_quality(&self) -> ImagePreviewQualityPreset {
        self.display_options.image_preview_quality
    }

    pub fn show_custom_emoji(&self) -> bool {
        self.display_options.custom_emoji_visible()
    }

    pub fn desktop_notifications_enabled(&self) -> bool {
        self.display_options.desktop_notifications
    }

    pub fn pane_width(&self, pane: FocusPane) -> u16 {
        match pane {
            FocusPane::Guilds => self.display_options.server_width,
            FocusPane::Channels => self.display_options.channel_list_width,
            FocusPane::Members => self.display_options.member_list_width,
            FocusPane::Messages => 0,
        }
    }

    pub fn adjust_focused_pane_width(&mut self, delta: i16) {
        let width = match self.focus {
            FocusPane::Guilds => &mut self.display_options.server_width,
            FocusPane::Channels => &mut self.display_options.channel_list_width,
            FocusPane::Members => &mut self.display_options.member_list_width,
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
            self.display_options_save_pending = true;
        }
    }

    pub fn is_options_popup_open(&self) -> bool {
        self.options_popup.is_some()
    }

    pub fn open_options_popup(&mut self) {
        self.options_popup = Some(OptionsPopupState { selected: 0 });
    }

    pub fn close_options_popup(&mut self) {
        self.options_popup = None;
    }

    pub fn move_option_down(&mut self) {
        if let Some(popup) = &mut self.options_popup {
            popup.selected = popup.selected.saturating_add(1).min(OPTION_COUNT - 1);
        }
    }

    pub fn move_option_up(&mut self) {
        if let Some(popup) = &mut self.options_popup {
            popup.selected = popup.selected.saturating_sub(1);
        }
    }

    pub fn selected_option_index(&self) -> Option<usize> {
        self.options_popup
            .as_ref()
            .map(|popup| popup.selected.min(OPTION_COUNT - 1))
    }

    pub fn display_option_items(&self) -> Vec<DisplayOptionItem> {
        let options = self.display_options;
        vec![
            DisplayOptionItem {
                label: "Disable all image previews",
                enabled: options.disable_image_preview,
                value: None,
                effective: options.disable_image_preview,
                description: "Master switch for avatars, images, and custom emoji images.",
            },
            DisplayOptionItem {
                label: "Show avatars",
                enabled: options.show_avatars,
                value: None,
                effective: options.avatars_visible(),
                description: "Message and profile avatars.",
            },
            DisplayOptionItem {
                label: "Show images",
                enabled: options.show_images,
                value: None,
                effective: options.images_visible(),
                description: "Attachment, embed, and image viewer previews.",
            },
            DisplayOptionItem {
                label: "Image preview quality",
                enabled: true,
                value: Some(options.image_preview_quality.label()),
                effective: options.images_visible(),
                description: "Quality preset for attachment, embed, and viewer previews.",
            },
            DisplayOptionItem {
                label: "Show custom emoji images",
                enabled: options.show_custom_emoji,
                value: None,
                effective: options.custom_emoji_visible(),
                description: "When off, custom emoji are shown as their emoji id.",
            },
            DisplayOptionItem {
                label: "Desktop notifications",
                enabled: options.desktop_notifications,
                value: None,
                effective: options.desktop_notifications,
                description: "Show OS notifications for Discord messages that pass notification settings.",
            },
        ]
    }

    pub fn toggle_selected_display_option(&mut self) {
        let Some(selected) = self.selected_option_index() else {
            return;
        };

        match selected {
            0 => {
                self.display_options.disable_image_preview =
                    !self.display_options.disable_image_preview
            }
            1 => self.display_options.show_avatars = !self.display_options.show_avatars,
            2 => self.display_options.show_images = !self.display_options.show_images,
            3 => {
                self.display_options.image_preview_quality =
                    self.display_options.image_preview_quality.next()
            }
            4 => self.display_options.show_custom_emoji = !self.display_options.show_custom_emoji,
            5 => {
                self.display_options.desktop_notifications =
                    !self.display_options.desktop_notifications
            }
            _ => return,
        }
        if !self.show_images() {
            self.close_image_viewer();
        }
        self.clear_message_row_content_metrics_cache();
        self.display_options_save_pending = true;
    }

    pub(in crate::tui) fn take_display_options_save_request(&mut self) -> Option<DisplayOptions> {
        if !self.display_options_save_pending {
            return None;
        }
        self.display_options_save_pending = false;
        Some(self.display_options)
    }
}
