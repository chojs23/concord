use super::DashboardState;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct StreamInfoSection {
    pub(in crate::tui) paused: bool,
    pub(in crate::tui) broadcaster: String,
    pub(in crate::tui) viewers: Vec<String>,
}

impl DashboardState {
    pub(in crate::tui) fn stream_info_sections(&self) -> Vec<StreamInfoSection> {
        let mut sections = Vec::with_capacity(2);

        if let (Some(target), Some(current_user_id)) = (
            self.runtime.active_stream_broadcast.as_ref(),
            self.current_user_id(),
        ) {
            let stream =
                self.discord
                    .stream_participants(target.scope, target.channel_id, current_user_id);
            sections.push(StreamInfoSection {
                paused: stream.paused,
                broadcaster: stream.broadcaster,
                viewers: stream.viewers,
            });
        }

        if let Some(target) = self.runtime.active_stream_playback {
            let stream =
                self.discord
                    .stream_participants(target.scope, target.channel_id, target.user_id);
            sections.push(StreamInfoSection {
                paused: stream.paused,
                broadcaster: stream.broadcaster,
                viewers: stream.viewers,
            });
        }

        sections
    }
}
