mod format;
mod input;
mod login;
mod state;
mod ui;

use std::{collections::HashSet, time::Duration};

use crossterm::event::{Event as TerminalEvent, EventStream};
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
    let _restore_guard = TerminalRestoreGuard;

    run_dashboard(&mut terminal, &mut events, commands).await
}

struct TerminalRestoreGuard;

impl Drop for TerminalRestoreGuard {
    fn drop(&mut self) {
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
    let mut history_requested = HashSet::new();
    let mut tick = tokio::time::interval(Duration::from_millis(250));

    while !state.should_quit() {
        terminal.draw(|frame| ui::render(frame, &mut state))?;

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
                    Ok(event) => state.push_event(event),
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
            next_history_request(state.selected_channel_id(), &mut history_requested)
            && commands
                .send(AppCommand::LoadMessageHistory { channel_id })
                .await
                .is_err()
        {
            logging::error("tui", "command channel closed");
            state.push_event(AppEvent::GatewayError {
                message: "command channel closed".to_owned(),
            });
        }
    }

    Ok(())
}

fn next_history_request(
    channel_id: Option<Id<ChannelMarker>>,
    requested: &mut HashSet<Id<ChannelMarker>>,
) -> Option<Id<ChannelMarker>> {
    let channel_id = channel_id?;
    requested.insert(channel_id).then_some(channel_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_request_is_sent_once_per_channel() {
        let mut requested = HashSet::new();
        let first = Id::new(1);
        let second = Id::new(2);

        assert_eq!(next_history_request(None, &mut requested), None);
        assert_eq!(
            next_history_request(Some(first), &mut requested),
            Some(first)
        );
        assert_eq!(next_history_request(Some(first), &mut requested), None);
        assert_eq!(
            next_history_request(Some(second), &mut requested),
            Some(second)
        );
    }
}
