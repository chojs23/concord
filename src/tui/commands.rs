use tokio::sync::mpsc;

use crate::{
    discord::{AppCommand, AppEvent},
    logging,
};

use super::state::DashboardState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CommandSendOutcome {
    Sent,
    ChannelClosed,
}

impl CommandSendOutcome {
    pub(super) fn is_channel_closed(self) -> bool {
        matches!(self, Self::ChannelClosed)
    }
}

pub(super) fn record_command_channel_closed(state: &mut DashboardState) {
    logging::error("tui", "command channel closed");
    state.push_effect(AppEvent::GatewayError {
        message: "command channel closed".to_owned(),
    });
}

pub(super) async fn send_or_record_closed(
    state: &mut DashboardState,
    commands: &mpsc::Sender<AppCommand>,
    command: AppCommand,
) -> CommandSendOutcome {
    let pending_message = match &command {
        AppCommand::SendMessage {
            channel_id, nonce, ..
        }
        | AppCommand::SendTtsMessage {
            channel_id, nonce, ..
        } => Some((*channel_id, *nonce)),
        _ => None,
    };
    if commands.send(command).await.is_ok() {
        return CommandSendOutcome::Sent;
    }
    if let Some((channel_id, nonce)) = pending_message {
        state.remove_pending_message(channel_id, nonce);
    }
    record_command_channel_closed(state);
    CommandSendOutcome::ChannelClosed
}

#[cfg(test)]
mod tests {
    use crate::discord::ids::Id;

    use super::*;

    #[tokio::test]
    async fn send_or_record_closed_sends_without_error() {
        let mut state = DashboardState::new();
        let (commands, mut receiver) = mpsc::channel(1);

        let outcome = send_or_record_closed(
            &mut state,
            &commands,
            AppCommand::SetSelectedGuild {
                guild_id: Some(Id::new(1)),
            },
        )
        .await;

        assert_eq!(outcome, CommandSendOutcome::Sent);
        assert!(state.gateway_error().is_none());
        assert_eq!(
            receiver.try_recv(),
            Ok(AppCommand::SetSelectedGuild {
                guild_id: Some(Id::new(1)),
            })
        );
    }

    #[tokio::test]
    async fn send_or_record_closed_records_closed_channel() {
        let mut state = DashboardState::new();
        let (commands, receiver) = mpsc::channel(1);
        drop(receiver);

        let outcome = send_or_record_closed(
            &mut state,
            &commands,
            AppCommand::SetSelectedGuild {
                guild_id: Some(Id::new(1)),
            },
        )
        .await;

        assert_eq!(outcome, CommandSendOutcome::ChannelClosed);
        assert_eq!(state.gateway_error(), Some("command channel closed"));
        assert_eq!(
            state.toast_message().map(|toast| toast.text),
            Some("command channel closed")
        );
    }
}
