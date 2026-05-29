use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, UserMarker},
};
use crate::discord::{
    AppCommand, AppEvent, MessageInfo, MessageSearchAuthorType, MessageSearchHas,
    MessageSearchPage, MessageSearchQuery,
};
use crate::tui::fuzzy::{best_fuzzy_name_match_score, fuzzy_text_score};
use crate::tui::keybindings::SelectionAction;
use crate::tui::state::popups::{ActiveModalPopupKind, ModalPopup, SelectablePopupState};
use crate::tui::text_cursor::{clamp_cursor_index, next_char_boundary, previous_char_boundary};
use chrono::NaiveDate;

use super::super::{
    ActiveGuildScope, ChannelSearchSuggestionItem, DashboardState, FocusPane,
    MemberSearchResultItem, MessageSearchResultItem, SearchFieldView, SearchPopupMode,
    SearchPopupView, SearchResultItem, SearchSuggestionItem,
};

const MESSAGE_SEARCH_PAGE_SIZE: usize = 25;
const SEARCH_SUGGESTION_LIMIT: usize = 8;

const MESSAGE_SEARCH_FIELDS: [(&str, &str); 8] = [
    ("contains", "text to search"),
    ("from", "user name"),
    ("in", "channel name"),
    ("has", "link, embed, file, video, image, sound, sticker"),
    ("mentions", "user name"),
    ("date", "gte:YYYY-MM-DD, lte:YYYY-MM-DD, equal:YYYY-MM-DD"),
    ("author type", "user, bot, webhook"),
    ("pinned", "y / n"),
];

#[derive(Clone, Debug, Eq, PartialEq)]
enum SearchFieldSelection {
    User(Id<UserMarker>),
    Channel(Id<ChannelMarker>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SearchTextField {
    label: &'static str,
    placeholder: &'static str,
    value: String,
    cursor_byte_index: usize,
    selection: Option<SearchFieldSelection>,
}

impl SearchTextField {
    fn new(label: &'static str, placeholder: &'static str) -> Self {
        Self {
            label,
            placeholder,
            value: String::new(),
            cursor_byte_index: 0,
            selection: None,
        }
    }

    fn cursor(&self) -> usize {
        clamp_cursor_index(&self.value, self.cursor_byte_index)
    }

    fn push_char(&mut self, value: char) {
        let cursor = self.cursor();
        self.value.insert(cursor, value);
        self.cursor_byte_index = cursor + value.len_utf8();
        self.selection = None;
    }

    fn pop_char(&mut self) {
        let cursor = self.cursor();
        if cursor == 0 {
            return;
        }
        let start = previous_char_boundary(&self.value, cursor);
        self.value.replace_range(start..cursor, "");
        self.cursor_byte_index = start;
        self.selection = None;
    }

    fn cursor_left(&mut self) {
        let cursor = self.cursor();
        self.cursor_byte_index = previous_char_boundary(&self.value, cursor);
    }

    fn cursor_right(&mut self) {
        let cursor = self.cursor();
        self.cursor_byte_index = next_char_boundary(&self.value, cursor);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui::state) struct SearchPopupState {
    mode: SearchPopupMode,
    fields: Vec<SearchTextField>,
    active_field: usize,
    selection: SelectablePopupState,
    suggestion_selection: SelectablePopupState,
    suggestions: Vec<SearchSuggestionItem>,
    results: Vec<SearchResultItem>,
    loading: bool,
    error: Option<String>,
    total_results: Option<usize>,
    has_more: bool,
    dirty: bool,
    last_query: Option<MessageSearchQuery>,
}

impl SearchPopupState {
    fn message() -> Self {
        Self {
            mode: SearchPopupMode::Message,
            fields: MESSAGE_SEARCH_FIELDS
                .into_iter()
                .map(|(label, placeholder)| SearchTextField::new(label, placeholder))
                .collect(),
            active_field: 0,
            selection: Default::default(),
            suggestion_selection: Default::default(),
            suggestions: Vec::new(),
            results: Vec::new(),
            loading: false,
            error: None,
            total_results: None,
            has_more: false,
            dirty: true,
            last_query: None,
        }
    }

    fn member(results: Vec<SearchResultItem>) -> Self {
        Self {
            mode: SearchPopupMode::Member,
            fields: vec![SearchTextField::new("member", "search members")],
            active_field: 0,
            selection: Default::default(),
            suggestion_selection: Default::default(),
            suggestions: Vec::new(),
            results,
            loading: false,
            error: None,
            total_results: None,
            has_more: false,
            dirty: false,
            last_query: None,
        }
    }

    fn active_field_mut(&mut self) -> Option<&mut SearchTextField> {
        self.fields.get_mut(self.active_field)
    }

    fn field_value(&self, label: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| field.value.trim())
            .filter(|value| !value.is_empty())
    }

    fn selected_user_id(&self, label: &str) -> Option<Id<UserMarker>> {
        self.fields
            .iter()
            .find(|field| field.label == label)
            .and_then(|field| match field.selection {
                Some(SearchFieldSelection::User(user_id)) => Some(user_id),
                Some(SearchFieldSelection::Channel(_)) | None => None,
            })
    }

    fn selected_channel_id(&self, label: &str) -> Option<Id<ChannelMarker>> {
        self.fields
            .iter()
            .find(|field| field.label == label)
            .and_then(|field| match field.selection {
                Some(SearchFieldSelection::Channel(channel_id)) => Some(channel_id),
                Some(SearchFieldSelection::User(_)) | None => None,
            })
    }

    fn selected(&self) -> usize {
        self.selection.selected_for_len(self.results.len())
    }

    fn selected_suggestion(&self) -> usize {
        self.suggestion_selection
            .selected_for_len(self.suggestions.len())
    }

    fn view(&self) -> SearchPopupView {
        SearchPopupView {
            mode: self.mode,
            fields: self
                .fields
                .iter()
                .enumerate()
                .map(|(index, field)| SearchFieldView {
                    label: field.label.to_owned(),
                    value: field.value.clone(),
                    placeholder: field.placeholder.to_owned(),
                    active: index == self.active_field,
                    cursor: field.cursor(),
                })
                .collect(),
            suggestions: self.suggestions.clone(),
            selected_suggestion: self.selected_suggestion(),
            results: self.results.clone(),
            selected: self.selected(),
            loading: self.loading,
            error: self.error.clone(),
            total_results: self.total_results,
            has_more: self.has_more,
        }
    }
}

impl DashboardState {
    pub fn open_search_popup_for_focus(&mut self, focus: FocusPane) {
        match focus {
            FocusPane::Messages => self.open_message_search_popup(),
            FocusPane::Members => self.open_member_search_popup(),
            FocusPane::Guilds | FocusPane::Channels => self.open_pane_filter(focus),
        }
    }

    pub fn open_message_search_popup(&mut self) {
        self.popups.modal = Some(ModalPopup::Search(SearchPopupState::message()));
    }

    pub fn open_member_search_popup(&mut self) {
        let results = self.member_search_results_for_query("");
        self.popups.modal = Some(ModalPopup::Search(SearchPopupState::member(results)));
    }

    pub fn close_search_popup(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::Search) {
            self.popups.clear_modal();
        }
    }

    pub fn search_popup_view(&self) -> Option<SearchPopupView> {
        self.popups.search_popup().map(SearchPopupState::view)
    }

    pub fn cycle_search_field_next(&mut self) {
        if let Some(search) = self.popups.search_popup_mut() {
            if !search.fields.is_empty() {
                search.active_field = (search.active_field + 1) % search.fields.len();
            }
        }
        self.refresh_message_search_suggestions();
    }

    pub fn cycle_search_field_previous(&mut self) {
        if let Some(search) = self.popups.search_popup_mut() {
            search.active_field = if search.active_field == 0 {
                search.fields.len().saturating_sub(1)
            } else {
                search.active_field.saturating_sub(1)
            };
        }
        self.refresh_message_search_suggestions();
    }

    pub fn move_search_result_down(&mut self) -> Option<AppCommand> {
        if let Some(search) = self.popups.search_popup_mut()
            && !search.suggestions.is_empty()
        {
            search
                .suggestion_selection
                .move_down(search.suggestions.len());
            return None;
        }
        let (at_bottom, has_more) = self
            .popups
            .search_popup()
            .map(|search| {
                (
                    search.selected().saturating_add(1) >= search.results.len(),
                    search.has_more,
                )
            })
            .unwrap_or_default();
        if at_bottom && has_more {
            return self.load_next_message_search_page();
        }
        if let Some(search) = self.popups.search_popup_mut() {
            search.selection.move_down(search.results.len());
        }
        None
    }

    pub fn move_search_result_up(&mut self) {
        if let Some(search) = self.popups.search_popup_mut()
            && !search.suggestions.is_empty()
        {
            search.suggestion_selection.move_up();
            return;
        }
        if let Some(search) = self.popups.search_popup_mut() {
            search.selection.move_up();
        }
    }

    pub fn page_search_result_down(&mut self) -> Option<AppCommand> {
        if let Some(search) = self.popups.search_popup_mut()
            && !search.suggestions.is_empty()
        {
            search
                .suggestion_selection
                .page(search.suggestions.len(), SelectionAction::Next);
            return None;
        }
        let (at_last_page, has_more) = self
            .popups
            .search_popup()
            .map(|search| {
                (
                    search.selected().saturating_add(MESSAGE_SEARCH_PAGE_SIZE)
                        >= search.results.len().saturating_sub(1),
                    search.has_more,
                )
            })
            .unwrap_or_default();
        if at_last_page && has_more {
            return self.load_next_message_search_page();
        }
        if let Some(search) = self.popups.search_popup_mut() {
            search
                .selection
                .page(search.results.len(), SelectionAction::Next);
        }
        None
    }

    pub fn page_search_result_up(&mut self) {
        if let Some(search) = self.popups.search_popup_mut()
            && !search.suggestions.is_empty()
        {
            search
                .suggestion_selection
                .page(search.suggestions.len(), SelectionAction::Previous);
            return;
        }
        if let Some(search) = self.popups.search_popup_mut() {
            search
                .selection
                .page(search.results.len(), SelectionAction::Previous);
        }
    }

    pub fn push_search_char(&mut self, value: char) {
        let mode = self.popups.search_popup().map(|search| search.mode);
        if let Some(search) = self.popups.search_popup_mut()
            && let Some(field) = search.active_field_mut()
        {
            field.push_char(value);
            search.dirty = true;
            search.error = None;
            search.selection.select(0);
        }
        if mode == Some(SearchPopupMode::Member) {
            self.refresh_member_search_results();
        } else {
            self.refresh_message_search_suggestions();
        }
    }

    pub fn pop_search_char(&mut self) {
        let mode = self.popups.search_popup().map(|search| search.mode);
        if let Some(search) = self.popups.search_popup_mut()
            && let Some(field) = search.active_field_mut()
        {
            field.pop_char();
            search.dirty = true;
            search.error = None;
            search.selection.select(0);
        }
        if mode == Some(SearchPopupMode::Member) {
            self.refresh_member_search_results();
        } else {
            self.refresh_message_search_suggestions();
        }
    }

    pub fn move_search_cursor_left(&mut self) {
        if let Some(search) = self.popups.search_popup_mut()
            && let Some(field) = search.active_field_mut()
        {
            field.cursor_left();
        }
    }

    pub fn move_search_cursor_right(&mut self) {
        if let Some(search) = self.popups.search_popup_mut()
            && let Some(field) = search.active_field_mut()
        {
            field.cursor_right();
        }
    }

    pub fn activate_search_popup(&mut self) -> Option<AppCommand> {
        match self.popups.search_popup().map(|search| search.mode) {
            Some(SearchPopupMode::Message) => self.activate_message_search_popup(),
            Some(SearchPopupMode::Member) => self.activate_member_search_popup(),
            None => None,
        }
    }

    pub fn record_search_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::MessageSearchLoaded { page } => self.record_message_search_page(page),
            AppEvent::MessageSearchLoadFailed { query, message } => {
                self.record_message_search_error(query, message)
            }
            _ => {}
        }
    }

    pub fn search_popup_member_query(&self) -> Option<&str> {
        let search = self.popups.search_popup()?;
        match search.mode {
            SearchPopupMode::Member => search.field_value("member"),
            SearchPopupMode::Message => {
                let field = search.fields.get(search.active_field)?;
                let value = field.value.trim();
                (matches!(field.label, "from" | "mentions")
                    && !value.is_empty()
                    && field.selection.is_none()
                    && parse_search_id(value).is_none())
                .then_some(value)
            }
        }
    }

    pub(in crate::tui::state) fn refresh_search_popup_after_member_cache_update(&mut self) {
        match self.popups.search_popup().map(|search| search.mode) {
            Some(SearchPopupMode::Member) => self.refresh_member_search_results(),
            Some(SearchPopupMode::Message) => self.refresh_message_search_suggestions(),
            None => {}
        }
    }

    fn activate_message_search_popup(&mut self) -> Option<AppCommand> {
        if self.apply_active_message_search_suggestion() {
            return None;
        }
        let dirty = self
            .popups
            .search_popup()
            .is_some_and(|search| search.dirty || search.results.is_empty());
        if dirty {
            return self.run_message_search(0);
        }

        let result = self
            .popups
            .search_popup()
            .and_then(|search| search.results.get(search.selected()).cloned());
        let Some(SearchResultItem::Message(result)) = result else {
            return None;
        };
        self.close_search_popup();
        if let Some(channel) = self.discord.cache.channel(result.channel_id) {
            match channel.guild_id {
                Some(guild_id) => self.activate_guild(ActiveGuildScope::Guild(guild_id)),
                None => self.activate_guild(ActiveGuildScope::DirectMessages),
            }
        }
        self.restore_channel_cursor(Some(result.channel_id));
        self.activate_channel(result.channel_id);
        self.focus_pane(FocusPane::Messages);
        Some(AppCommand::LoadMessageHistoryAround {
            channel_id: result.channel_id,
            message_id: result.message_id,
        })
    }

    fn activate_member_search_popup(&mut self) -> Option<AppCommand> {
        let result = self
            .popups
            .search_popup()
            .and_then(|search| search.results.get(search.selected()).cloned());
        let Some(SearchResultItem::Member(result)) = result else {
            return None;
        };
        self.open_user_profile_popup(result.user_id, result.guild_id)
    }

    fn apply_active_message_search_suggestion(&mut self) -> bool {
        let suggestion = self.popups.search_popup().and_then(|search| {
            search
                .suggestions
                .get(search.selected_suggestion())
                .cloned()
        });
        let Some(suggestion) = suggestion else {
            return false;
        };

        let (replacement, selection) = match suggestion {
            SearchSuggestionItem::Member(member) => (
                member.display_name,
                SearchFieldSelection::User(member.user_id),
            ),
            SearchSuggestionItem::Channel(channel) => (
                channel.channel_label,
                SearchFieldSelection::Channel(channel.channel_id),
            ),
        };
        if let Some(search) = self.popups.search_popup_mut()
            && let Some(field) = search.active_field_mut()
        {
            field.value = replacement;
            field.cursor_byte_index = field.value.len();
            field.selection = Some(selection);
            search.suggestions.clear();
            search.suggestion_selection.select(0);
            search.dirty = true;
            search.error = None;
            return true;
        }
        false
    }

    fn run_message_search(&mut self, offset: usize) -> Option<AppCommand> {
        if let Some(error) = self.message_search_validation_error() {
            if let Some(search) = self.popups.search_popup_mut() {
                search.error = Some(error);
                search.loading = false;
            }
            return None;
        }
        let query = self.message_search_query(offset)?;
        if query.is_empty() {
            if let Some(search) = self.popups.search_popup_mut() {
                search.error = Some("Enter at least one search filter".to_owned());
                search.loading = false;
            }
            return None;
        }
        if let Some(search) = self.popups.search_popup_mut() {
            if offset == 0 {
                search.results.clear();
                search.selection.select(0);
                search.total_results = None;
            }
            search.loading = true;
            search.error = None;
            search.has_more = false;
            search.dirty = false;
            search.last_query = Some(query.clone());
        }
        Some(AppCommand::SearchMessages { query })
    }

    fn load_next_message_search_page(&mut self) -> Option<AppCommand> {
        let mut query = self.popups.search_popup()?.last_query.clone()?;
        query.offset = query.offset.saturating_add(MESSAGE_SEARCH_PAGE_SIZE);
        self.run_message_search(query.offset)
    }

    fn message_search_query(&self, offset: usize) -> Option<MessageSearchQuery> {
        let search = self.popups.search_popup()?;
        let guild_id = self.selected_guild_id();
        let channel_id = search
            .selected_channel_id("in")
            .or_else(|| {
                search
                    .field_value("in")
                    .and_then(|value| self.resolve_channel_search_value(value))
            })
            .or_else(|| {
                guild_id
                    .is_none()
                    .then(|| self.selected_channel_id())
                    .flatten()
            });
        Some(MessageSearchQuery {
            guild_id,
            channel_id,
            author_id: search.selected_user_id("from").or_else(|| {
                search
                    .field_value("from")
                    .and_then(|value| self.resolve_user_search_value(value))
            }),
            mentions_user_id: search.selected_user_id("mentions").or_else(|| {
                search
                    .field_value("mentions")
                    .and_then(|value| self.resolve_user_search_value(value))
            }),
            content: search.field_value("contains").map(str::to_owned),
            has: search
                .field_value("has")
                .and_then(|value| parse_search_values(value, MessageSearchHas::from_input))
                .unwrap_or_default(),
            date: search.field_value("date").map(str::to_owned),
            author_type: search
                .field_value("author type")
                .and_then(|value| parse_search_values(value, MessageSearchAuthorType::from_input))
                .unwrap_or_default(),
            pinned: search.field_value("pinned").and_then(parse_search_bool),
            offset,
        })
    }

    fn message_search_validation_error(&self) -> Option<String> {
        let search = self.popups.search_popup()?;
        if !search
            .fields
            .iter()
            .any(|field| !field.value.trim().is_empty())
        {
            return Some("Enter at least one search filter".to_owned());
        }
        if let Some(value) = search.field_value("date")
            && !valid_search_date(value)
        {
            return Some(
                "Use date as gte:YYYY-MM-DD, lte:YYYY-MM-DD, or equal:YYYY-MM-DD".to_owned(),
            );
        }
        if let Some(value) = search.field_value("has")
            && parse_search_values(value, MessageSearchHas::from_input).is_none()
        {
            return Some("Use has: link, embed, file, video, image, sound, or sticker".to_owned());
        }
        if let Some(value) = search.field_value("author type")
            && parse_search_values(value, MessageSearchAuthorType::from_input).is_none()
        {
            return Some("Use author type: user, bot, or webhook".to_owned());
        }
        if let Some(value) = search.field_value("pinned")
            && parse_search_bool(value).is_none()
        {
            return Some("Use pinned: y / n".to_owned());
        }
        if let Some(value) = search.field_value("from")
            && search.selected_user_id("from").is_none()
            && self.resolve_user_search_value(value).is_none()
        {
            return Some("No matching sender found".to_owned());
        }
        if let Some(value) = search.field_value("mentions")
            && search.selected_user_id("mentions").is_none()
            && self.resolve_user_search_value(value).is_none()
        {
            return Some("No matching mentioned user found".to_owned());
        }
        if let Some(value) = search.field_value("in")
            && search.selected_channel_id("in").is_none()
            && self.resolve_channel_search_value(value).is_none()
        {
            return Some("No matching channel found".to_owned());
        }
        None
    }

    fn record_message_search_page(&mut self, page: &MessageSearchPage) {
        let mut items = page
            .messages
            .iter()
            .map(|message| SearchResultItem::Message(self.message_search_result_item(message)))
            .collect::<Vec<_>>();
        if let Some(search) = self.popups.search_popup_mut()
            && search.mode == SearchPopupMode::Message
        {
            if page.query.offset == 0 {
                search.results = items;
                search.selection.select(0);
            } else {
                search.results.append(&mut items);
            }
            search.loading = false;
            search.error = None;
            search.total_results = page.total_results;
            search.has_more = page.has_more;
            search.last_query = Some(page.query.clone());
        }
    }

    fn record_message_search_error(&mut self, _query: &MessageSearchQuery, message: &str) {
        if let Some(search) = self.popups.search_popup_mut()
            && search.mode == SearchPopupMode::Message
        {
            search.loading = false;
            search.error = Some(message.to_owned());
            search.dirty = true;
        }
        self.show_error_toast(message, std::time::Instant::now());
    }

    fn refresh_message_search_suggestions(&mut self) {
        let suggestion_query = self.popups.search_popup().and_then(|search| {
            if search.mode != SearchPopupMode::Message {
                return None;
            }
            let field = search.fields.get(search.active_field)?;
            let value = field.value.trim();
            if value.is_empty() || field.selection.is_some() {
                return None;
            }
            Some((field.label, value.to_owned()))
        });
        let suggestions = match suggestion_query {
            Some(("from" | "mentions", query)) => self.member_suggestions_for_query(&query),
            Some(("in", query)) => self.channel_suggestions_for_query(&query),
            _ => Vec::new(),
        };
        if let Some(search) = self.popups.search_popup_mut()
            && search.mode == SearchPopupMode::Message
        {
            search.suggestions = suggestions;
            search.suggestion_selection.select(0);
        }
    }

    fn message_search_result_item(&self, message: &MessageInfo) -> MessageSearchResultItem {
        let channel_label = self
            .discord
            .cache
            .channel(message.channel_id)
            .map(|channel| channel.name.clone())
            .unwrap_or_else(|| format!("channel-{}", message.channel_id.get()));
        MessageSearchResultItem {
            channel_id: message.channel_id,
            message_id: message.message_id,
            channel_label,
            author: message.author.clone(),
            content: message_search_content_label(message),
        }
    }

    fn refresh_member_search_results(&mut self) {
        let query = self
            .popups
            .search_popup()
            .and_then(|search| search.field_value("member"))
            .unwrap_or_default()
            .to_owned();
        let results = self.member_search_results_for_query(&query);
        if let Some(search) = self.popups.search_popup_mut()
            && search.mode == SearchPopupMode::Member
        {
            search.results = results;
            search.selection.select(0);
            search.dirty = false;
        }
    }

    fn member_search_results_for_query(&self, query: &str) -> Vec<SearchResultItem> {
        let guild_id = match self.navigation.active_guild {
            ActiveGuildScope::Guild(guild_id) => Some(guild_id),
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => None,
        };
        let mut scored = self
            .flattened_members()
            .into_iter()
            .filter_map(|member| {
                let display_name = self.member_display_name(member);
                let username = member.username();
                let score = if query.trim().is_empty() {
                    Some(0)
                } else {
                    let mut candidates = vec![display_name.as_str()];
                    if let Some(username) = username.as_deref() {
                        candidates.push(username);
                    }
                    best_fuzzy_name_match_score(&candidates, query).map(|(_, score)| score.0)
                }?;
                Some((
                    score,
                    display_name.to_ascii_lowercase(),
                    SearchResultItem::Member(MemberSearchResultItem {
                        user_id: member.user_id(),
                        guild_id,
                        display_name,
                        username,
                        status: member.status(),
                        is_bot: member.is_bot(),
                    }),
                ))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, _, item)| item).collect()
    }

    fn member_suggestions_for_query(&self, query: &str) -> Vec<SearchSuggestionItem> {
        self.member_search_results_for_query(query)
            .into_iter()
            .filter_map(|item| match item {
                SearchResultItem::Member(member) => Some(SearchSuggestionItem::Member(member)),
                SearchResultItem::Message(_) => None,
            })
            .take(SEARCH_SUGGESTION_LIMIT)
            .collect()
    }

    fn channel_suggestions_for_query(&self, query: &str) -> Vec<SearchSuggestionItem> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        let mut scored = self
            .channels()
            .into_iter()
            .filter_map(|channel| {
                let score = fuzzy_text_score(&channel.name, query)?.0;
                let guild_label = channel
                    .guild_id
                    .and_then(|guild_id| self.discord.cache.guild(guild_id))
                    .map(|guild| guild.name.clone());
                Some((
                    score,
                    channel.name.to_ascii_lowercase(),
                    SearchSuggestionItem::Channel(ChannelSearchSuggestionItem {
                        channel_id: channel.id,
                        channel_label: channel.name.clone(),
                        guild_label,
                    }),
                ))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        scored
            .into_iter()
            .map(|(_, _, item)| item)
            .take(SEARCH_SUGGESTION_LIMIT)
            .collect()
    }

    fn resolve_user_search_value(&self, value: &str) -> Option<Id<UserMarker>> {
        parse_search_id(value)
            .and_then(Id::new_checked)
            .or_else(|| {
                self.member_search_results_for_query(value)
                    .into_iter()
                    .find_map(|item| match item {
                        SearchResultItem::Member(member) => Some(member.user_id),
                        SearchResultItem::Message(_) => None,
                    })
            })
    }

    fn resolve_channel_search_value(&self, value: &str) -> Option<Id<ChannelMarker>> {
        if let Some(id) = parse_search_id(value).and_then(Id::new_checked) {
            return Some(id);
        }
        self.channels()
            .into_iter()
            .filter_map(|channel| {
                fuzzy_text_score(&channel.name, value)
                    .map(|score| (score.0, channel.name.to_ascii_lowercase(), channel.id))
            })
            .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)))
            .map(|(_, _, id)| id)
    }
}

fn parse_search_id(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        return value.parse().ok();
    }
    value
        .strip_prefix("<@")
        .and_then(|inner| inner.strip_suffix('>'))
        .or_else(|| {
            value
                .strip_prefix("<#")
                .and_then(|inner| inner.strip_suffix('>'))
        })
        .map(|inner| inner.trim_start_matches('!'))
        .filter(|inner| inner.chars().all(|ch| ch.is_ascii_digit()))
        .and_then(|inner| inner.parse().ok())
}

fn valid_search_date(value: &str) -> bool {
    value.split(',').all(|part| {
        let part = part.trim();
        if part.is_empty() {
            return false;
        }
        let date = part
            .strip_prefix("gte:")
            .or_else(|| part.strip_prefix("lte:"))
            .or_else(|| part.strip_prefix("equal:"))
            .unwrap_or(part);
        NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").is_ok()
    })
}

fn parse_search_values<T>(value: &str, parse: impl Fn(&str) -> Option<T>) -> Option<Vec<T>> {
    let mut parsed = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return None;
        }
        parsed.push(parse(part)?);
    }
    (!parsed.is_empty()).then_some(parsed)
}

fn parse_search_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "y" => Some(true),
        "n" => Some(false),
        _ => None,
    }
}

fn message_search_content_label(message: &MessageInfo) -> String {
    let content = message.content.as_deref().unwrap_or_default().trim();
    if !content.is_empty() {
        return content.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    if !message.attachments.is_empty() {
        return format!("{} attachment(s)", message.attachments.len());
    }
    if !message.sticker_names.is_empty() {
        return format!("{} sticker(s)", message.sticker_names.len());
    }
    if !message.embeds.is_empty() {
        return format!("{} embed(s)", message.embeds.len());
    }
    message.message_kind.label().to_owned()
}
