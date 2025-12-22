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
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use tokio::sync::{broadcast, mpsc};
use twilight_model::id::marker::MessageMarker;
use twilight_model::id::{Id, marker::ChannelMarker};

use crate::{
    Result,
    discord::{AppCommand, AppEvent},
    logging,
};

use state::DashboardState;
use ui::{ImagePreview, ImagePreviewLayout, ImagePreviewState};

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
                state.clamp_message_viewport_for_image_previews(usize::from(
                    preview_layout.preview_height,
                ));
                image_targets = visible_image_preview_targets(&state, preview_layout);
                let image_previews = image_previews.render_state(&image_targets);
                ui::render(frame, &state, image_previews);
            })?;
            dirty = false;
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
            .send(AppCommand::LoadMessageHistory { channel_id })
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

        if dirty {
            continue;
        }

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

    Ok(())
}

struct ImagePreviewTarget {
    message_index: usize,
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
        let target_by_key = targets
            .iter()
            .map(|target| (target.key(), target.message_index))
            .collect::<HashMap<_, _>>();
        let mut rendered_keys = HashSet::new();
        let mut previews = Vec::new();

        for (key, entry) in &mut self.entries {
            let Some(message_index) = target_by_key.get(key).copied() else {
                continue;
            };
            rendered_keys.insert(key.clone());
            let state = match entry {
                ImagePreviewEntry::Loading { filename } => ImagePreviewState::Loading {
                    filename: filename.clone(),
                },
                ImagePreviewEntry::Ready { protocol, .. } => ImagePreviewState::Ready {
                    protocol: protocol.as_mut(),
                },
                ImagePreviewEntry::Failed { filename, message } => ImagePreviewState::Failed {
                    filename: filename.clone(),
                    message: message.clone(),
                },
            };
            previews.push(ImagePreview {
                message_index,
                state,
            });
        }

        for target in targets.iter() {
            if !rendered_keys.contains(&target.key()) {
                previews.push(ImagePreview {
                    message_index: target.message_index,
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

fn visible_image_preview_targets(
    state: &DashboardState,
    layout: ImagePreviewLayout,
) -> Vec<ImagePreviewTarget> {
    let mut rendered_rows = 0usize;
    let mut targets = Vec::new();
    let preview_height = usize::from(layout.preview_height);

    for (message_index, message) in state.visible_messages().into_iter().enumerate() {
        if rendered_rows >= layout.list_height {
            break;
        }

        rendered_rows = rendered_rows.saturating_add(1);

        let Some(attachment) = message
            .attachments
            .iter()
            .find(|attachment| attachment.is_image())
        else {
            continue;
        };
        if preview_height == 0 || rendered_rows >= layout.list_height {
            continue;
        }

        if let Some(url) = attachment.preferred_url() {
            targets.push(ImagePreviewTarget {
                message_index,
                message_id: message.id,
                url: url.to_owned(),
                filename: attachment.filename.clone(),
            });
            rendered_rows = rendered_rows.saturating_add(preview_height);
        }
    }

    targets
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
    use crate::discord::{AttachmentInfo, ChannelInfo};

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
                preview_height: 3,
            },
        );

        assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    }

    #[test]
    fn image_preview_targets_follow_the_scrolled_message_window() {
        let mut state = state_with_image_messages(8, &[1, 6]);
        state.set_message_view_height(6);

        let targets = visible_image_preview_targets(
            &state,
            ImagePreviewLayout {
                list_height: 6,
                preview_height: 3,
            },
        );

        assert_eq!(target_message_ids(&targets), vec![Id::new(6)]);
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
                content: Some(format!("msg {id}")),
                attachments: image_message_ids
                    .contains(&id)
                    .then(|| image_attachment(id))
                    .into_iter()
                    .collect(),
            });
        }

        state
    }

    fn target_message_ids(targets: &[ImagePreviewTarget]) -> Vec<Id<MessageMarker>> {
        targets.iter().map(|target| target.message_id).collect()
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
}
