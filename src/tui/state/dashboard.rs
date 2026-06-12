use crate::tui::message::syntax_highlight::SyntaxHighlightCache;

use super::{
    ComposerUiState, DiscordUiState, LayoutCacheState, MessageHistoryRefreshState,
    MessageViewportState, NavigationState, PopupUiState, RequestTrackingState, RuntimeUiState,
    SettingsState,
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
    pub(super) options: SettingsState,
    pub(super) requests: RequestTrackingState,
    pub(super) layout_cache: LayoutCacheState,
    pub(in crate::tui) syntax_highlight_cache: SyntaxHighlightCache,
}

impl DashboardState {
    pub fn new() -> Self {
        Self::default()
    }
}
