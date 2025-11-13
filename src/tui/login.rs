use crossterm::event::{Event as TerminalEvent, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::{AppError, Result};

use super::TerminalRestoreGuard;

pub async fn prompt_token(notice: Option<String>) -> Result<String> {
    let mut terminal = ratatui::init();
    let _restore_guard = TerminalRestoreGuard;
    let mut state = LoginState::new(notice);
    let mut events = EventStream::new();

    loop {
        terminal.draw(|frame| render_login(frame, &state))?;

        if let Some(event) = events.next().await {
            match event {
                Ok(TerminalEvent::Key(key)) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Enter => match state.token() {
                        Some(token) => return Ok(token),
                        None => state.error = Some("Token cannot be empty".to_owned()),
                    },
                    KeyCode::Esc => return Err(AppError::LoginCancelled),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Err(AppError::LoginCancelled);
                    }
                    KeyCode::Backspace => {
                        state.input.pop();
                        state.error = None;
                    }
                    KeyCode::Char(value) => {
                        state.input.push(value);
                        state.error = None;
                    }
                    _ => {}
                },
                Ok(_) => {}
                Err(error) => return Err(error.into()),
            }
        } else {
            return Err(AppError::LoginCancelled);
        }
    }
}

struct LoginState {
    input: String,
    notice: Option<String>,
    error: Option<String>,
}

impl LoginState {
    fn new(notice: Option<String>) -> Self {
        Self {
            input: String::new(),
            notice,
            error: None,
        }
    }

    fn token(&self) -> Option<String> {
        let token = self.input.trim();
        if token.is_empty() {
            None
        } else {
            Some(token.to_owned())
        }
    }
}

fn render_login(frame: &mut Frame, state: &LoginState) {
    let area = centered_rect(72, 14, frame.area());
    let masked = "•".repeat(state.input.chars().count());
    let hint_style = Style::default().fg(Color::DarkGray);
    let accent = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let error_line = state
        .error
        .as_deref()
        .map(|error| {
            Line::from(Span::styled(
                error.to_owned(),
                Style::default().fg(Color::Red),
            ))
        })
        .unwrap_or_else(|| Line::from(""));

    let persistence_text = if state.notice.is_some() {
        "Paste your bot token below. It will be used for this session."
    } else {
        "Paste your bot token below. It will be saved to ~/.discord-rs/credential."
    };

    let mut lines = vec![
        Line::from(Span::styled("Discord login", accent)),
        Line::from(""),
        Line::from("No saved Discord token was found."),
        Line::from(persistence_text),
        Line::from(""),
        Line::from(vec![
            Span::styled("Token  ", hint_style),
            Span::styled(masked, Style::default().fg(Color::Green)),
        ]),
        error_line,
        Line::from(""),
        Line::from(Span::styled(
            "Enter save | Esc cancel | Ctrl-C quit",
            hint_style,
        )),
    ];

    if let Some(notice) = &state.notice {
        lines.insert(
            4,
            Line::from(Span::styled(
                notice.clone(),
                Style::default().fg(Color::Yellow),
            )),
        );
    }

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .title(" Login ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Plain)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title_style(
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
            ),
        area,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let [vertical] = Layout::vertical([Constraint::Length(height)])
        .flex(ratatui::layout::Flex::Center)
        .areas(area);

    let [horizontal] = Layout::horizontal([Constraint::Length(width)])
        .flex(ratatui::layout::Flex::Center)
        .areas(vertical);

    horizontal
}

#[cfg(test)]
mod tests {
    use super::LoginState;

    #[test]
    fn trims_entered_token() {
        let state = LoginState {
            input: "  token  ".to_owned(),
            notice: None,
            error: None,
        };

        assert_eq!(state.token().as_deref(), Some("token"));
    }

    #[test]
    fn rejects_blank_token() {
        let state = LoginState {
            input: "   ".to_owned(),
            notice: None,
            error: None,
        };

        assert!(state.token().is_none());
    }
}
