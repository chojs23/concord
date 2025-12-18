mod format;
mod input;
mod login;
mod state;
mod ui;

use std::{collections::HashMap, io::stdout, time::Duration};

use crossterm::{
    event::{
        Event as TerminalEvent, EventStream, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
};
use futures::StreamExt;
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use tokio::sync::{broadcast, mpsc};
use twilight_model::id::{Id, marker::ChannelMarker};

use crate::{
    Result,
    discord::{AppCommand, AppEvent},
    logging,
};

use state::DashboardState;
use ui::ImagePreview;

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
    let mut tick = tokio::time::interval(Duration::from_millis(250));

    while !state.should_quit() {
        terminal.draw(|frame| {
            let image_preview_visible = image_previews.is_visible(&state);
            ui::sync_view_heights(frame.area(), &mut state, image_preview_visible);
            let image_preview = image_previews.render_state(&state);
            ui::render(frame, &state, image_preview);
        })?;

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
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => return Err(error.into()),
                    None => state.quit(),
                }
            }
            event = events.recv() => {
                match event {
                    Ok(event) => {
                        image_previews.record_event(&event);
                        record_history_event(&event, &mut history_requests);
                        state.push_event(event);
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => state.record_lag(skipped),
                    Err(broadcast::error::RecvError::Closed) => {
                        state.push_event(AppEvent::GatewayClosed);
                        state.quit();
                    }
                }
            }
            _ = tick.tick() => {}
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
        }

        if let Some(command) = image_previews.next_request(&state)
            && commands.send(command).await.is_err()
        {
            logging::error("tui", "command channel closed");
            state.push_event(AppEvent::GatewayError {
                message: "command channel closed".to_owned(),
            });
        }
    }

    Ok(())
}

struct ImagePreviewTarget {
    url: String,
    filename: String,
}

struct ImagePreviewCache {
    picker: Option<Picker>,
    entries: HashMap<String, ImagePreviewEntry>,
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

    fn is_visible(&self, state: &DashboardState) -> bool {
        selected_image_preview_target(state).is_some()
    }

    fn render_state(&mut self, state: &DashboardState) -> ImagePreview<'_> {
        let Some(target) = selected_image_preview_target(state) else {
            return ImagePreview::None;
        };

        match self.entries.get_mut(&target.url) {
            Some(ImagePreviewEntry::Loading { filename }) => ImagePreview::Loading { filename },
            Some(ImagePreviewEntry::Ready { filename, protocol }) => ImagePreview::Ready {
                filename,
                protocol: protocol.as_mut(),
            },
            Some(ImagePreviewEntry::Failed { filename, message }) => {
                ImagePreview::Failed { filename, message }
            }
            None => ImagePreview::Loading { filename: "image" },
        }
    }

    fn next_request(&mut self, state: &DashboardState) -> Option<AppCommand> {
        let target = selected_image_preview_target(state)?;
        if self.entries.contains_key(&target.url) {
            return None;
        }

        let url = target.url;
        self.entries.insert(
            url.clone(),
            ImagePreviewEntry::Loading {
                filename: target.filename,
            },
        );
        Some(AppCommand::LoadAttachmentPreview { url })
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
        let filename = self
            .entries
            .get(url)
            .map(ImagePreviewEntry::filename)
            .unwrap_or("image")
            .to_owned();

        let Some(picker) = &self.picker else {
            self.entries.insert(
                url.to_owned(),
                ImagePreviewEntry::Failed {
                    filename,
                    message: "inline preview unavailable in this terminal".to_owned(),
                },
            );
            return;
        };

        match image::load_from_memory(bytes) {
            Ok(image) => {
                self.entries.insert(
                    url.to_owned(),
                    ImagePreviewEntry::Ready {
                        filename,
                        protocol: Box::new(picker.new_resize_protocol(image)),
                    },
                );
            }
            Err(error) => {
                self.entries.insert(
                    url.to_owned(),
                    ImagePreviewEntry::Failed {
                        filename,
                        message: format!("decode failed: {error}"),
                    },
                );
            }
        }
    }

    fn store_failed(&mut self, url: &str, message: String) {
        let filename = self
            .entries
            .get(url)
            .map(ImagePreviewEntry::filename)
            .unwrap_or("image")
            .to_owned();
        self.entries.insert(
            url.to_owned(),
            ImagePreviewEntry::Failed { filename, message },
        );
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

fn selected_image_preview_target(state: &DashboardState) -> Option<ImagePreviewTarget> {
    let attachment = state.selected_message_image_attachment()?;
    let url = attachment.preferred_url()?.to_owned();
    Some(ImagePreviewTarget {
        url,
        filename: attachment.filename.clone(),
    })
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
}
