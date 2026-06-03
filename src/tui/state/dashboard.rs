use super::{
    ComposerUiState, DiscordUiState, LayoutCacheState, MessageHistoryRefreshState,
    MessageViewportState, NavigationState, OptionsUiState, PopupUiState, RequestTrackingState,
    RuntimeUiState,
};

#[derive(Debug, Default)]
pub struct DashboardState {
    pub(super) discord: DiscordUiState,
    pub(super) navigation: NavigationState,
    pub(super) message_history_refresh: MessageHistoryRefreshState,
    pub(super) messages: MessageViewportState,
    pub(super) composer: ComposerUiState,
    pub(super) popups: PopupUiState,
    pub(super) runtime: RuntimeUiState,
    pub(super) options: OptionsUiState,
    pub(super) requests: RequestTrackingState,
    pub(super) layout_cache: LayoutCacheState,
}

impl DashboardState {
    pub fn new() -> Self {
        Self::default()
    }
}
