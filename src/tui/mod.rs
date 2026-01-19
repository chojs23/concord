mod format;
mod input;
mod login;
mod state;
mod ui;

use std::{
    collections::{HashMap, HashSet},
    io::stdout,
};

use crossterm::{
    event::{
        Event as TerminalEvent, EventStream, KeyEventKind, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
};
use futures::StreamExt;
use image::{DynamicImage, ImageBuffer, Rgba, imageops::FilterType};
use ratatui::layout::Rect;
use ratatui_image::{Resize, picker::Picker, protocol::StatefulProtocol};
use tokio::sync::{broadcast, mpsc};
use twilight_model::id::marker::MessageMarker;
use twilight_model::id::{Id, marker::ChannelMarker};

use crate::{
    Result,
    discord::{AppCommand, AppEvent},
    logging,
};

use state::{DashboardState, message_base_line_count_for_width};
use ui::{ImagePreview, ImagePreviewLayout, ImagePreviewState};

const IMAGE_PREVIEW_SOURCE_PIXELS_PER_COLUMN: u64 = 10;

pub async fn prompt_login(notice: Option<String>) -> Result<String> {
    login::prompt_login(notice).await
}

pub async fn run(
    mut events: broadcast::Receiver<AppEvent>,
    commands: mpsc::Sender<AppCommand>,
) -> Result<()> {
    let mut terminal = ratatui::init();
    let _restore_guard = match TerminalRestoreGuard::new() {
        Ok(guard) => guard,
        Err(error) => {
            ratatui::restore();
            return Err(error);
        }
    };

    run_dashboard(&mut terminal, &mut events, commands).await
}

pub(super) struct TerminalRestoreGuard {
    keyboard_enhancement_enabled: bool,
}

impl TerminalRestoreGuard {
    pub(super) fn new() -> Result<Self> {
        execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        Ok(Self {
            keyboard_enhancement_enabled: true,
        })
    }
}

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
        if self.keyboard_enhancement_enabled {
            let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
        }
        ratatui::restore();
    }
}

async fn run_dashboard(
    terminal: &mut ratatui::DefaultTerminal,
    events: &mut broadcast::Receiver<AppEvent>,
    commands: mpsc::Sender<AppCommand>,
) -> Result<()> {
    let mut state = DashboardState::new();
    let mut image_previews = ImagePreviewCache::new();
    let mut terminal_events = EventStream::new();
    let mut history_requests = HashMap::new();
    let mut last_history_channel = None;
    let mut image_targets = Vec::new();
    let mut dirty = true;

    while !state.should_quit() {
        if dirty {
            terminal.draw(|frame| {
                ui::sync_view_heights(frame.area(), &mut state);
                let preview_layout = ui::image_preview_layout(frame.area(), &state);
                state.clamp_message_viewport_for_image_previews(
                    preview_layout.content_width,
                    preview_layout.preview_width,
                    preview_layout.max_preview_height,
                );
                image_targets = visible_image_preview_targets(&state, preview_layout);
                let image_previews = image_previews.render_state(&image_targets);
                ui::render(frame, &state, image_previews);
            })?;
            dirty = false;

            for command in image_previews.next_requests(&image_targets) {
                if commands.send(command).await.is_err() {
                    logging::error("tui", "command channel closed");
                    state.push_event(AppEvent::GatewayError {
                        message: "command channel closed".to_owned(),
                    });
                    dirty = true;
                    break;
                }
                dirty = true;
            }
        }

        tokio::select! {
            maybe_event = terminal_events.next() => {
                match maybe_event {
                    Some(Ok(TerminalEvent::Key(key))) => {
                        if let Some(command) = input::handle_key(&mut state, key)
                            && commands.send(command).await.is_err()
                        {
                            logging::error("tui", "command channel closed");
                            state.push_event(AppEvent::GatewayError {
                                message: "command channel closed".to_owned(),
                            });
                        }
                        if key.kind == KeyEventKind::Press {
                            dirty = true;
                        }
                    }
                    Some(Ok(TerminalEvent::Resize(_, _))) => dirty = true,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => return Err(error.into()),
                    None => {
                        state.quit();
                        dirty = true;
                    }
                }
            }
            event = events.recv() => {
                match event {
                    Ok(event) => {
                        image_previews.record_event(&event);
                        record_history_event(&event, &mut history_requests);
                        state.push_event(event);
                        dirty = true;
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        state.record_lag(skipped);
                        dirty = true;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        state.push_event(AppEvent::GatewayClosed);
                        state.quit();
                        dirty = true;
                    }
                }
            }
        }

        if let Some(channel_id) = next_history_request(
            state.selected_channel_id(),
            &mut history_requests,
            &mut last_history_channel,
        ) && commands
            .send(AppCommand::LoadMessageHistory {
                channel_id,
                before: None,
            })
            .await
            .is_err()
        {
            history_requests.insert(channel_id, HistoryRequestState::Failed);
            logging::error("tui", "command channel closed");
            state.push_event(AppEvent::GatewayError {
                message: "command channel closed".to_owned(),
            });
            dirty = true;
        }
    }

    Ok(())
}

struct ImagePreviewTarget {
    message_index: usize,
    preview_width: u16,
    preview_height: u16,
    visible_preview_height: u16,
    top_clip_rows: u16,
    message_id: Id<MessageMarker>,
    url: String,
    filename: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ImagePreviewKey {
    message_id: Id<MessageMarker>,
    url: String,
}

struct ImagePreviewCache {
    picker: Option<Picker>,
    entries: HashMap<ImagePreviewKey, ImagePreviewEntry>,
}

enum ImagePreviewEntry {
    Loading {
        filename: String,
    },
    Ready {
        filename: String,
        image: DynamicImage,
        protocol: Box<StatefulProtocol>,
    },
    Failed {
        filename: String,
        message: String,
    },
}

impl ImagePreviewCache {
    fn new() -> Self {
        let picker = match Picker::from_query_stdio() {
            Ok(picker) => Some(picker),
            Err(error) => {
                logging::error(
                    "preview",
                    format!("inline image picker unavailable: {error}"),
                );
                None
            }
        };

        Self {
            picker,
            entries: HashMap::new(),
        }
    }

    fn render_state(&mut self, targets: &[ImagePreviewTarget]) -> Vec<ImagePreview<'_>> {
        let picker = self.picker.clone();
        let target_by_key = targets
            .iter()
            .map(|target| (target.key(), target.preview_render_info()))
            .collect::<HashMap<_, _>>();
        let mut rendered_keys = HashSet::new();
        let mut previews = Vec::new();

        for (key, entry) in &mut self.entries {
            let Some(render_info) = target_by_key.get(key).copied() else {
                continue;
            };
            rendered_keys.insert(key.clone());
            let state = match entry {
                ImagePreviewEntry::Loading { filename } => ImagePreviewState::Loading {
                    filename: filename.clone(),
                },
                ImagePreviewEntry::Ready {
                    image, protocol, ..
                } => {
                    if render_info.needs_crop()
                        && let Some(protocol) = picker
                            .as_ref()
                            .and_then(|picker| clipped_preview_protocol(picker, image, render_info))
                    {
                        ImagePreviewState::ReadyCropped(protocol)
                    } else {
                        ImagePreviewState::Ready {
                            protocol: protocol.as_mut(),
                        }
                    }
                }
                ImagePreviewEntry::Failed { filename, message } => ImagePreviewState::Failed {
                    filename: filename.clone(),
                    message: message.clone(),
                },
            };
            previews.push(ImagePreview {
                message_index: render_info.message_index,
                preview_height: render_info.preview_height,
                state,
            });
        }

        for target in targets.iter() {
            if !rendered_keys.contains(&target.key()) {
                previews.push(ImagePreview {
                    message_index: target.message_index,
                    preview_height: target.preview_height,
                    state: ImagePreviewState::Loading {
                        filename: target.filename.clone(),
                    },
                });
            }
        }

        previews.sort_by_key(|preview| preview.message_index);
        previews
    }

    fn next_requests(&mut self, targets: &[ImagePreviewTarget]) -> Vec<AppCommand> {
        let mut commands = Vec::new();
        let mut requested_urls = HashSet::new();
        for target in targets {
            let key = target.key();
            if self.entries.contains_key(&key) {
                continue;
            }

            let url = target.url.clone();
            self.entries.insert(
                key,
                ImagePreviewEntry::Loading {
                    filename: target.filename.clone(),
                },
            );
            if requested_urls.insert(url.clone()) {
                commands.push(AppCommand::LoadAttachmentPreview { url });
            }
        }
        commands
    }

    fn record_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::AttachmentPreviewLoaded { url, bytes } => self.store_loaded(url, bytes),
            AppEvent::AttachmentPreviewLoadFailed { url, message } => {
                self.store_failed(url, message.clone())
            }
            _ => {}
        }
    }

    fn store_loaded(&mut self, url: &str, bytes: &[u8]) {
        let keys = self.keys_for_url(url);
        if keys.is_empty() {
            return;
        }

        let Some(picker) = &self.picker else {
            for key in keys {
                let filename = self.filename_for_key(&key);
                self.entries.insert(
                    key,
                    ImagePreviewEntry::Failed {
                        filename,
                        message: "inline preview unavailable in this terminal".to_owned(),
                    },
                );
            }
            return;
        };

        for key in keys {
            let filename = self.filename_for_key(&key);
            match image::load_from_memory(bytes) {
                Ok(image) => {
                    self.entries.insert(
                        key,
                        ImagePreviewEntry::Ready {
                            filename,
                            image: image.clone(),
                            protocol: Box::new(picker.new_resize_protocol(image)),
                        },
                    );
                }
                Err(error) => {
                    self.entries.insert(
                        key,
                        ImagePreviewEntry::Failed {
                            filename,
                            message: format!("decode failed: {error}"),
                        },
                    );
                }
            }
        }
    }

    fn store_failed(&mut self, url: &str, message: String) {
        for key in self.keys_for_url(url) {
            let filename = self.filename_for_key(&key);
            self.entries.insert(
                key,
                ImagePreviewEntry::Failed {
                    filename,
                    message: message.clone(),
                },
            );
        }
    }

    fn keys_for_url(&self, url: &str) -> Vec<ImagePreviewKey> {
        self.entries
            .keys()
            .filter(|key| key.url == url)
            .cloned()
            .collect()
    }

    fn filename_for_key(&self, key: &ImagePreviewKey) -> String {
        self.entries
            .get(key)
            .map(ImagePreviewEntry::filename)
            .unwrap_or("image")
            .to_owned()
    }
}

impl ImagePreviewTarget {
    fn key(&self) -> ImagePreviewKey {
        ImagePreviewKey {
            message_id: self.message_id,
            url: self.url.clone(),
        }
    }

    fn preview_render_info(&self) -> ImagePreviewRenderInfo {
        ImagePreviewRenderInfo {
            message_index: self.message_index,
            preview_width: self.preview_width,
            preview_height: self.preview_height,
            visible_preview_height: self.visible_preview_height,
            top_clip_rows: self.top_clip_rows,
        }
    }
}

#[derive(Clone, Copy)]
struct ImagePreviewRenderInfo {
    message_index: usize,
    preview_width: u16,
    preview_height: u16,
    visible_preview_height: u16,
    top_clip_rows: u16,
}

impl ImagePreviewRenderInfo {
    fn needs_crop(self) -> bool {
        self.top_clip_rows > 0 || self.visible_preview_height < self.preview_height
    }
}

impl ImagePreviewEntry {
    fn filename(&self) -> &str {
        match self {
            Self::Loading { filename }
            | Self::Ready { filename, .. }
            | Self::Failed { filename, .. } => filename,
        }
    }
}

fn clipped_preview_protocol(
    picker: &Picker,
    image: &DynamicImage,
    render_info: ImagePreviewRenderInfo,
) -> Option<ratatui_image::protocol::Protocol> {
    if render_info.preview_width == 0
        || render_info.preview_height == 0
        || render_info.visible_preview_height == 0
    {
        return None;
    }

    let (font_width, font_height) = picker.font_size();
    let full_width = u32::from(render_info.preview_width).checked_mul(u32::from(font_width))?;
    let full_height = u32::from(render_info.preview_height).checked_mul(u32::from(font_height))?;
    let crop_top = u32::from(render_info.top_clip_rows).checked_mul(u32::from(font_height))?;
    let crop_height = u32::from(render_info.visible_preview_height)
        .checked_mul(u32::from(font_height))?
        .min(full_height.saturating_sub(crop_top));
    if full_width == 0 || crop_height == 0 {
        return None;
    }

    let fitted = fit_image_to_canvas(image, full_width, full_height);
    let cropped = fitted.crop_imm(0, crop_top, full_width, crop_height);
    picker
        .new_protocol(
            cropped,
            Rect::new(
                0,
                0,
                render_info.preview_width,
                render_info.visible_preview_height,
            ),
            Resize::Fit(None),
        )
        .ok()
}

fn fit_image_to_canvas(image: &DynamicImage, width: u32, height: u32) -> DynamicImage {
    let resized = image.resize(width, height, FilterType::Nearest);
    if resized.width() == width && resized.height() == height {
        return resized;
    }

    let mut canvas =
        DynamicImage::ImageRgba8(ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0])));
    image::imageops::overlay(&mut canvas, &resized, 0, 0);
    canvas
}

fn visible_image_preview_targets(
    state: &DashboardState,
    layout: ImagePreviewLayout,
) -> Vec<ImagePreviewTarget> {
    let mut rendered_rows = 0usize;
    let mut targets = Vec::new();

    for (message_index, message) in state.visible_messages().into_iter().enumerate() {
        if rendered_rows >= layout.list_height {
            break;
        }

        let line_offset = usize::from(message_index == 0) * state.message_line_scroll();
        let base_rows = message_base_line_count_for_width(message, layout.content_width);

        let Some((attachment, url)) = message
            .attachments_in_display_order()
            .find_map(|attachment| attachment.inline_preview_url().map(|url| (attachment, url)))
        else {
            rendered_rows = rendered_rows.saturating_add(base_rows.saturating_sub(line_offset));
            continue;
        };

        let preview_height = image_preview_height_for_dimensions(
            layout.preview_width,
            layout.max_preview_height,
            attachment.width,
            attachment.height,
        );
        let preview_top = rendered_rows as isize + base_rows as isize - line_offset as isize;
        let preview_bottom = preview_top.saturating_add(preview_height as isize);
        let visible_top = preview_top.max(0);
        let visible_bottom = preview_bottom.min(layout.list_height as isize);
        if preview_height > 0 && visible_top < visible_bottom {
            targets.push(ImagePreviewTarget {
                message_index,
                preview_width: layout.preview_width,
                preview_height,
                visible_preview_height: u16::try_from(visible_bottom - visible_top)
                    .unwrap_or(u16::MAX),
                top_clip_rows: u16::try_from(visible_top - preview_top).unwrap_or(u16::MAX),
                message_id: message.id,
                url: url.to_owned(),
                filename: attachment.filename.clone(),
            });
        }

        rendered_rows = rendered_rows.saturating_add(
            base_rows
                .saturating_add(preview_height as usize)
                .saturating_sub(line_offset),
        );
    }

    targets
}

fn image_preview_height_for_dimensions(
    preview_width: u16,
    max_preview_height: u16,
    image_width: Option<u64>,
    image_height: Option<u64>,
) -> u16 {
    if preview_width == 0 || max_preview_height == 0 {
        return 0;
    }

    let (Some(image_width), Some(image_height)) = (image_width, image_height) else {
        return max_preview_height;
    };
    if image_width == 0 || image_height == 0 {
        return max_preview_height;
    }

    let source_width_columns = image_width.div_ceil(IMAGE_PREVIEW_SOURCE_PIXELS_PER_COLUMN);
    let preview_width = preview_width.min(u16::try_from(source_width_columns).unwrap_or(u16::MAX));

    let rows = (u128::from(preview_width) * u128::from(image_height))
        .div_ceil(u128::from(image_width) * 3);
    let rows = u16::try_from(rows).unwrap_or(u16::MAX);

    rows.clamp(3.min(max_preview_height), max_preview_height)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HistoryRequestState {
    Requested,
    Loaded,
    Failed,
}

fn record_history_event(
    event: &AppEvent,
    requests: &mut HashMap<Id<ChannelMarker>, HistoryRequestState>,
) {
    match event {
        AppEvent::MessageHistoryLoaded { channel_id, .. } => {
            requests.insert(*channel_id, HistoryRequestState::Loaded);
        }
        AppEvent::MessageHistoryLoadFailed { channel_id, .. } => {
            requests.insert(*channel_id, HistoryRequestState::Failed);
        }
        _ => {}
    }
}

fn next_history_request(
    channel_id: Option<Id<ChannelMarker>>,
    requests: &mut HashMap<Id<ChannelMarker>, HistoryRequestState>,
    last_channel: &mut Option<Id<ChannelMarker>>,
) -> Option<Id<ChannelMarker>> {
    let Some(channel_id) = channel_id else {
        *last_channel = None;
        return None;
    };
    let channel_changed = *last_channel != Some(channel_id);
    *last_channel = Some(channel_id);

    match requests.get(&channel_id).copied() {
        None => {
            requests.insert(channel_id, HistoryRequestState::Requested);
            Some(channel_id)
        }
        Some(HistoryRequestState::Failed) if channel_changed => {
            requests.insert(channel_id, HistoryRequestState::Requested);
            Some(channel_id)
        }
        Some(
            HistoryRequestState::Requested
            | HistoryRequestState::Loaded
            | HistoryRequestState::Failed,
        ) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::discord::{AttachmentInfo, ChannelInfo, MessageSnapshotInfo};

    use super::*;

    #[test]
    fn history_request_is_sent_once_per_channel() {
        let mut requests = HashMap::new();
        let mut last_channel = None;
        let first = Id::new(1);
        let second = Id::new(2);

        assert_eq!(
            next_history_request(None, &mut requests, &mut last_channel),
            None
        );
        assert_eq!(
            next_history_request(Some(first), &mut requests, &mut last_channel),
            Some(first)
        );
        assert_eq!(
            next_history_request(Some(first), &mut requests, &mut last_channel),
            None
        );
        assert_eq!(
            next_history_request(Some(second), &mut requests, &mut last_channel),
            Some(second)
        );
    }

    #[test]
    fn history_request_retries_failed_channel_after_reselect() {
        let mut requests = HashMap::new();
        let mut last_channel = None;
        let first = Id::new(1);
        let second = Id::new(2);

        assert_eq!(
            next_history_request(Some(first), &mut requests, &mut last_channel),
            Some(first)
        );
        record_history_event(
            &AppEvent::MessageHistoryLoadFailed {
                channel_id: first,
                message: "temporary failure".to_owned(),
            },
            &mut requests,
        );
        assert_eq!(
            next_history_request(Some(first), &mut requests, &mut last_channel),
            None
        );
        assert_eq!(
            next_history_request(Some(second), &mut requests, &mut last_channel),
            Some(second)
        );
        assert_eq!(
            next_history_request(Some(first), &mut requests, &mut last_channel),
            Some(first)
        );
    }

    #[test]
    fn image_preview_targets_stop_at_rendered_row_budget() {
        let mut state = state_with_image_messages(6, &[1, 3, 6]);
        state.set_message_view_height(6);

        let targets = visible_image_preview_targets(
            &state,
            ImagePreviewLayout {
                list_height: 6,
                content_width: 200,
                preview_width: 16,
                max_preview_height: 3,
            },
        );

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    }

    #[test]
    fn image_preview_targets_include_preview_that_would_be_clipped() {
        let mut state = state_with_image_messages(2, &[1, 2]);
        state.set_message_view_height(6);

        let targets = visible_image_preview_targets(
            &state,
            ImagePreviewLayout {
                list_height: 6,
                content_width: 200,
                preview_width: 16,
                max_preview_height: 3,
            },
        );

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    }

    #[test]
    fn image_preview_targets_account_for_first_message_line_offset() {
        let mut state = state_with_image_messages(1, &[1]);
        focus_messages(&mut state);
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(200, 16, 3);

        let targets = visible_image_preview_targets(
            &state,
            ImagePreviewLayout {
                list_height: 2,
                content_width: 200,
                preview_width: 16,
                max_preview_height: 3,
            },
        );

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    }

    #[test]
    fn image_preview_targets_include_top_clipped_preview_rows() {
        let mut state = state_with_image_messages(1, &[1]);
        focus_messages(&mut state);
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
        for _ in 0..3 {
            state.scroll_message_viewport_down();
            state.clamp_message_viewport_for_image_previews(200, 16, 3);
        }

        let targets = visible_image_preview_targets(
            &state,
            ImagePreviewLayout {
                list_height: 2,
                content_width: 200,
                preview_width: 16,
                max_preview_height: 3,
            },
        );

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
        assert_eq!(targets[0].visible_preview_height, 2);
        assert_eq!(targets[0].top_clip_rows, 1);
    }

    #[test]
    fn image_preview_targets_skip_preview_when_no_preview_row_is_visible() {
        let mut state = state_with_image_messages(2, &[1, 2]);
        state.set_message_view_height(5);

        let targets = visible_image_preview_targets(
            &state,
            ImagePreviewLayout {
                list_height: 5,
                content_width: 200,
                preview_width: 16,
                max_preview_height: 3,
            },
        );

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    }

    #[test]
    fn image_preview_request_is_created_for_clipped_draw_target() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
        };
        let mut state = state_with_image_messages(2, &[1, 2]);
        state.set_message_view_height(6);
        let targets = visible_image_preview_targets(
            &state,
            ImagePreviewLayout {
                list_height: 6,
                content_width: 200,
                preview_width: 16,
                max_preview_height: 3,
            },
        );

        let requests = cache.next_requests(&targets);

        assert_eq!(requests.len(), 1);
        assert_eq!(cache.entries.len(), 1);
        assert!(requests.contains(&AppCommand::LoadAttachmentPreview {
            url: "https://cdn.discordapp.com/image-1.png".to_owned(),
        }));
    }

    #[test]
    fn video_attachment_does_not_request_original_as_image_preview() {
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            poll: None,
            content: Some("clip".to_owned()),
            attachments: vec![video_attachment(2)],
            forwarded_snapshots: Vec::new(),
        });

        let targets = visible_image_preview_targets(
            &state,
            ImagePreviewLayout {
                list_height: 6,
                content_width: 200,
                preview_width: 16,
                max_preview_height: 3,
            },
        );

        assert!(targets.is_empty());
    }

    #[test]
    fn image_preview_targets_include_forwarded_image_attachments() {
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::MessageCreate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(2),
            author_id: Id::new(99),
            author: "neo".to_owned(),
            message_kind: crate::discord::MessageKind::regular(),
            reply: None,
            poll: None,
            content: Some(String::new()),
            attachments: Vec::new(),
            forwarded_snapshots: vec![forwarded_snapshot(2)],
        });

        let targets = visible_image_preview_targets(
            &state,
            ImagePreviewLayout {
                list_height: 6,
                content_width: 200,
                preview_width: 16,
                max_preview_height: 3,
            },
        );

        assert_eq!(target_message_ids(&targets), vec![Id::new(2)]);
        assert_eq!(targets[0].url, "https://cdn.discordapp.com/image-2.png");
    }

    #[test]
    fn image_preview_targets_follow_the_scrolled_message_window() {
        let mut state = state_with_image_messages(8, &[1, 6]);
        state.set_message_view_height(6);

        let targets = visible_image_preview_targets(
            &state,
            ImagePreviewLayout {
                list_height: 7,
                content_width: 200,
                preview_width: 16,
                max_preview_height: 3,
            },
        );

        assert_eq!(target_message_ids(&targets), vec![Id::new(6)]);
    }

    #[test]
    fn image_preview_targets_include_image_messages_in_scrolloff_context() {
        let mut state = state_with_image_messages(8, &[5, 6, 7]);
        focus_messages(&mut state);
        state.set_message_view_height(14);
        while state.selected_message() > 3 {
            state.move_up();
        }
        state.clamp_message_viewport_for_image_previews(200, 16, 3);

        let targets = visible_image_preview_targets(
            &state,
            ImagePreviewLayout {
                list_height: 14,
                content_width: 200,
                preview_width: 16,
                max_preview_height: 3,
            },
        );

        assert_eq!(target_message_ids(&targets), vec![Id::new(5), Id::new(6)]);
    }

    #[test]
    fn image_preview_request_is_created_for_draw_target() {
        let mut cache = ImagePreviewCache {
            picker: None,
            entries: HashMap::new(),
        };
        let target = image_preview_target(1);

        assert!(cache.entries.is_empty());
        assert_eq!(cache.render_state(std::slice::from_ref(&target)).len(), 1);
        assert!(cache.entries.is_empty());

        let requests = cache.next_requests(std::slice::from_ref(&target));

        assert_eq!(
            requests,
            vec![AppCommand::LoadAttachmentPreview {
                url: target.url.clone()
            }]
        );
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn wide_image_preview_height_is_shorter_than_square_image() {
        let wide = image_preview_height_for_dimensions(60, 10, Some(2400), Some(600));
        let square = image_preview_height_for_dimensions(60, 10, Some(800), Some(800));

        assert!(wide < square);
        assert_eq!(wide, 5);
        assert_eq!(square, 10);
    }

    #[test]
    fn screenshot_like_wide_image_uses_compact_preview_height() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(481), Some(160)),
            6
        );
    }

    #[test]
    fn small_image_preview_height_does_not_upscale_to_full_width() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(100), Some(100)),
            4
        );
    }

    #[test]
    fn tiny_image_preview_height_stays_compact_but_visible() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(32), Some(32)),
            3
        );
    }

    #[test]
    fn small_wide_image_preview_height_stays_compact() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(100), Some(40)),
            3
        );
    }

    #[test]
    fn medium_small_square_image_preview_height_stays_below_max() {
        assert_eq!(
            image_preview_height_for_dimensions(72, 10, Some(128), Some(128)),
            5
        );
    }

    #[test]
    fn image_preview_height_falls_back_to_max_without_dimensions() {
        assert_eq!(image_preview_height_for_dimensions(60, 10, None, None), 10);
    }

    #[test]
    fn image_preview_height_falls_back_to_max_with_zero_dimensions() {
        assert_eq!(
            image_preview_height_for_dimensions(60, 10, Some(0), Some(100)),
            10
        );
    }

    fn state_with_image_messages(count: u64, image_message_ids: &[u64]) -> DashboardState {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let mut state = DashboardState::new();

        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                channel_id,
                parent_id: None,
                position: None,
                last_message_id: None,
                name: "general".to_owned(),
                kind: "GuildText".to_owned(),
            }],
            members: Vec::new(),
            presences: Vec::new(),
        });
        state.confirm_selected_guild();
        state.confirm_selected_channel();

        for id in 1..=count {
            state.push_event(AppEvent::MessageCreate {
                guild_id: Some(guild_id),
                channel_id,
                message_id: Id::new(id),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                message_kind: crate::discord::MessageKind::regular(),
                reply: None,
                poll: None,
                content: Some(format!("msg {id}")),
                attachments: image_message_ids
                    .contains(&id)
                    .then(|| image_attachment(id))
                    .into_iter()
                    .collect(),
                forwarded_snapshots: Vec::new(),
            });
        }

        state
    }

    fn target_message_ids(targets: &[ImagePreviewTarget]) -> Vec<Id<MessageMarker>> {
        targets.iter().map(|target| target.message_id).collect()
    }

    fn image_preview_target(id: u64) -> ImagePreviewTarget {
        ImagePreviewTarget {
            message_index: 0,
            preview_width: 16,
            preview_height: 3,
            visible_preview_height: 3,
            top_clip_rows: 0,
            message_id: Id::new(id),
            url: format!("https://cdn.discordapp.com/image-{id}.png"),
            filename: format!("image-{id}.png"),
        }
    }

    fn image_attachment(id: u64) -> AttachmentInfo {
        AttachmentInfo {
            id: Id::new(id),
            filename: format!("image-{id}.png"),
            url: format!("https://cdn.discordapp.com/image-{id}.png"),
            proxy_url: format!("https://media.discordapp.net/image-{id}.png"),
            content_type: Some("image/png".to_owned()),
            size: 2048,
            width: Some(640),
            height: Some(480),
            description: None,
        }
    }

    fn video_attachment(id: u64) -> AttachmentInfo {
        AttachmentInfo {
            id: Id::new(id),
            filename: format!("clip-{id}.mp4"),
            url: format!("https://cdn.discordapp.com/clip-{id}.mp4"),
            proxy_url: format!("https://media.discordapp.net/clip-{id}.mp4"),
            content_type: Some("video/mp4".to_owned()),
            size: 78_364_758,
            width: Some(1920),
            height: Some(1080),
            description: None,
        }
    }

    fn forwarded_snapshot(id: u64) -> MessageSnapshotInfo {
        MessageSnapshotInfo {
            content: Some(format!("forwarded {id}")),
            attachments: vec![image_attachment(id)],
            source_channel_id: None,
            timestamp: None,
        }
    }

    fn focus_messages(state: &mut DashboardState) {
        while state.focus() != super::state::FocusPane::Messages {
            state.cycle_focus();
        }
    }
}
