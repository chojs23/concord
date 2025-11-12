mod format;
mod input;
mod login;
mod state;
mod ui;

use std::time::Duration;

use crossterm::event::{Event as TerminalEvent, EventStream};
use futures::StreamExt;
use tokio::sync::broadcast;

use crate::{Result, discord::AppEvent};

use state::DashboardState;

pub async fn prompt_token(notice: Option<String>) -> Result<String> {
    login::prompt_token(notice).await
}

pub async fn run(mut events: broadcast::Receiver<AppEvent>) -> Result<()> {
    let mut terminal = ratatui::init();
    let _restore_guard = TerminalRestoreGuard;

    run_dashboard(&mut terminal, &mut events).await
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
) -> Result<()> {
    let mut state = DashboardState::new(500);
    let mut terminal_events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(250));

    while !state.should_quit() {
        terminal.draw(|frame| ui::render(frame, &state))?;

        tokio::select! {
            maybe_event = terminal_events.next() => {
                match maybe_event {
                    Some(Ok(TerminalEvent::Key(key))) => input::handle_key(&mut state, key),
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
    }

    Ok(())
}
