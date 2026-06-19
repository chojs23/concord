use std::{cell::RefCell, collections::HashMap};

use crate::discord::AppEvent;

use super::DashboardState;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct MessageRowContentMetricsCacheKey {
    pub(super) message_id: u64,
    pub(super) content_width: usize,
    pub(super) preview_width: u16,
    pub(super) max_preview_height: u16,
    pub(super) show_custom_emoji: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MessageRowContentMetrics {
    pub(super) content_rows: usize,
    pub(super) reaction_rows: usize,
    pub(super) preview_rows: usize,
}

#[derive(Debug, Default)]
pub(super) struct LayoutCacheState {
    pub(super) message_row_content_metrics_cache:
        RefCell<HashMap<MessageRowContentMetricsCacheKey, MessageRowContentMetrics>>,
}

impl DashboardState {
    pub(super) fn clear_message_row_content_metrics_cache(&mut self) {
        self.layout_cache
            .message_row_content_metrics_cache
            .get_mut()
            .clear();
    }

    pub(super) fn event_affects_message_row_content_metrics(event: &AppEvent) -> bool {
        !matches!(
            event,
            AppEvent::TypingStart { .. }
                | AppEvent::PresenceUpdate { .. }
                | AppEvent::GuildMemberListUpdate { .. }
                | AppEvent::UserSettingsUpdate { .. }
                | AppEvent::UserNoteLoaded { .. }
                | AppEvent::UserGuildSettingsInit { .. }
                | AppEvent::UserGuildSettingsUpdate { .. }
                | AppEvent::RelationshipsLoaded { .. }
                | AppEvent::RelationshipUpsert { .. }
                | AppEvent::RelationshipRemove { .. }
                | AppEvent::ReadStateInit { .. }
                | AppEvent::MessageAck { .. }
                | AppEvent::VoiceServerUpdate { .. }
                | AppEvent::VoiceConnectionStatusChanged { .. }
        )
    }

    #[cfg(test)]
    pub(super) fn message_row_content_metrics_cache_len(&self) -> usize {
        self.layout_cache
            .message_row_content_metrics_cache
            .borrow()
            .len()
    }
}
