mod format;
mod input;
mod login;
mod state;
mod ui;

use std::{collections::HashMap, io::stdout, time::Duration};

use crossterm::{
    event::{
        Event as TerminalEvent, EventStream, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
};
use futures::StreamExt;
use tokio::sync::{broadcast, mpsc};
use twilight_model::id::{Id, marker::ChannelMarker};

use crate::{
    Result,
    discord::{AppCommand, AppEvent},
    logging,
};

use state::DashboardState;

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
    let mut terminal_events = EventStream::new();
    let mut history_requests = HashMap::new();
    let mut last_history_channel = None;
    let mut tick = tokio::time::interval(Duration::from_millis(250));

    while !state.should_quit() {
        terminal.draw(|frame| {
            ui::sync_view_heights(frame.area(), &mut state);
            ui::render(frame, &state);
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

        if let Some(channel_id) =
            next_history_request(
                state.selected_channel_id(),
                &mut history_requests,
                &mut last_history_channel,
            )
            && commands
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
    }

    Ok(())
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
        Some(HistoryRequestState::Requested | HistoryRequestState::Loaded | HistoryRequestState::Failed) => None,
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

        assert_eq!(next_history_request(None, &mut requests, &mut last_channel), None);
        assert_eq!(
            next_history_request(Some(first), &mut requests, &mut last_channel),
            Some(first)
        );
        assert_eq!(next_history_request(Some(first), &mut requests, &mut last_channel), None);
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
        assert_eq!(next_history_request(Some(first), &mut requests, &mut last_channel), None);
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
