use super::{DashboardState, popups::ModalPopup};
use crate::config::KlipyOptions;
use crate::klipy::{Gif, GifPage, KlipyClient};
use crate::tui::text_input::{TextEditAction, TextInputState};

#[derive(Debug, Default)]
pub(super) struct KlipyState {
    pub options: KlipyOptions,
    generation: u64,
    selected: Vec<(String, String, String)>,
    shares: Vec<(String, String)>,
}

#[derive(Debug)]
pub(in crate::tui) struct GifPickerState {
    pub query: TextInputState,
    pub page: u32,
    pub results: Vec<Gif>,
    pub selected: usize,
    pub has_next: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub generation: u64,
}

impl DashboardState {
    pub(in crate::tui) fn apply_klipy_options(&mut self, options: KlipyOptions) {
        self.klipy.options = options;
    }

    pub(in crate::tui) fn open_gif_picker(&mut self) {
        if !self.is_composing() || self.composer.edit_target_message.is_some() {
            return;
        }
        if let Err(error) = KlipyClient::new(&self.klipy.options) {
            self.show_error_toast(error, std::time::Instant::now());
            return;
        }
        self.cancel_active_composer_picker();
        self.klipy.generation = self.klipy.generation.wrapping_add(1);
        self.popups.set_modal(ModalPopup::GifPicker(GifPickerState {
            query: TextInputState::default(),
            page: 1,
            results: Vec::new(),
            selected: 0,
            has_next: false,
            loading: true,
            error: None,
            generation: self.klipy.generation,
        }));
    }

    pub(in crate::tui) fn gif_picker(&self) -> Option<&GifPickerState> {
        match self.popups.modal.as_ref()? {
            ModalPopup::GifPicker(picker) => Some(picker),
            _ => None,
        }
    }

    fn gif_picker_mut(&mut self) -> Option<&mut GifPickerState> {
        match self.popups.modal.as_mut()? {
            ModalPopup::GifPicker(picker) => Some(picker),
            _ => None,
        }
    }

    fn refresh_gif_query(&mut self, page: u32) {
        self.klipy.generation = self.klipy.generation.wrapping_add(1);
        let generation = self.klipy.generation;
        if let Some(picker) = self.gif_picker_mut() {
            picker.page = page;
            picker.results.clear();
            picker.selected = 0;
            picker.has_next = false;
            picker.loading = true;
            picker.error = None;
            picker.generation = generation;
        }
    }

    pub(in crate::tui) fn insert_gif_query(&mut self, text: &str) {
        if let Some(picker) = self.gif_picker_mut() {
            let text: String = text
                .chars()
                .filter(|c| !c.is_control())
                .take(256usize.saturating_sub(picker.query.value().chars().count()))
                .collect();
            if text.is_empty() {
                return;
            }
            picker.query.insert_str(&text);
            self.refresh_gif_query(1);
        }
    }

    pub(in crate::tui) fn edit_gif_query(&mut self, action: TextEditAction) {
        if self
            .gif_picker_mut()
            .is_some_and(|p| p.query.apply_edit_action(action))
        {
            self.refresh_gif_query(1);
        }
    }

    pub(in crate::tui) fn clear_gif_query(&mut self) {
        if let Some(picker) = self.gif_picker_mut() {
            picker.query.clear();
            self.refresh_gif_query(1);
        }
    }

    pub(in crate::tui) fn move_gif_selection(&mut self, delta: isize) {
        if let Some(picker) = self.gif_picker_mut() {
            picker.selected = picker
                .selected
                .saturating_add_signed(delta)
                .min(picker.results.len().saturating_sub(1));
        }
    }

    pub(in crate::tui) fn change_gif_page(&mut self, next: bool) {
        if let Some(picker) = self.gif_picker() {
            if picker.loading {
                return;
            }
            let page = if next && picker.has_next {
                picker.page.saturating_add(1)
            } else if !next && picker.page > 1 {
                picker.page - 1
            } else {
                return;
            };
            self.refresh_gif_query(page);
        }
    }

    pub(in crate::tui) fn store_gif_results(
        &mut self,
        generation: u64,
        result: Result<GifPage, String>,
    ) -> bool {
        let Some(picker) = self.gif_picker_mut().filter(|p| p.generation == generation) else {
            return false;
        };
        picker.loading = false;
        match result {
            Ok(page) => {
                picker.results = page.data;
                picker.has_next = page.has_next;
            }
            Err(error) => picker.error = Some(error),
        }
        true
    }

    pub(in crate::tui) fn confirm_gif_selection(&mut self) {
        if !self.is_composing() || !self.can_send_in_selected_channel() {
            self.popups.clear_modal();
            return;
        }
        let Some(picker) = self.gif_picker() else {
            return;
        };
        if picker.error.is_some() {
            self.refresh_gif_query(picker.page);
            return;
        }
        let Some(gif) = picker.results.get(picker.selected) else {
            return;
        };
        let Some(url) = gif.media_url(false).map(str::to_owned) else {
            return;
        };
        let selection = (
            url.clone(),
            gif.slug.clone(),
            picker.query.value().to_owned(),
        );
        self.popups.clear_modal();
        // Append on its own line so links survive mid-word cursors and markdown drafts.
        self.move_composer_cursor_end();
        if !self.composer_input().is_empty() && !self.composer_input().ends_with('\n') {
            self.insert_composer_text_at_cursor("\n");
        }
        self.insert_composer_text_at_cursor(&url);
        self.klipy.selected.retain(|(selected, _, _)| {
            selected != &url && self.composer.composer_input.value().contains(selected)
        });
        self.klipy.selected.push(selection);
    }

    pub(in crate::tui::state) fn clear_klipy_selections(&mut self) {
        self.klipy.selected.clear();
    }

    pub(in crate::tui::state) fn queue_klipy_shares(&mut self, content: &str) {
        for (url, slug, query) in self.klipy.selected.drain(..) {
            if content.split_whitespace().any(|word| word == url) {
                self.klipy.shares.push((slug, query));
            }
        }
    }

    pub(in crate::tui) fn take_klipy_shares(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.klipy.shares)
    }
}
