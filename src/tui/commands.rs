use crate::{discord::AppEvent, logging};

use super::state::DashboardState;

pub(super) fn record_command_channel_closed(state: &mut DashboardState) {
    logging::error("tui", "command channel closed");
    state.push_effect(AppEvent::GatewayError {
        message: "command channel closed".to_owned(),
    });
}
