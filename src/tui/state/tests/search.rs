use super::*;
use crate::discord::{MessageSearchAuthorType, MessageSearchHas};
use crate::tui::state::SearchSuggestionItem;

#[test]
fn message_search_builds_query_and_jumps_to_selected_result() {
    let mut state = state_with_writable_channel();
    state.open_search_popup_for_focus(FocusPane::Messages);

    type_search_text(&mut state, "needle");

    let command = state
        .activate_search_popup()
        .expect("message search command");
    let AppCommand::SearchMessages { query } = command else {
        panic!("expected search command");
    };
    assert_eq!(query.guild_id, Some(Id::new(1)));
    assert_eq!(query.content.as_deref(), Some("needle"));
    assert_eq!(query.offset, 0);

    let mut result = message_info(Id::new(2), 42);
    result.content = Some("needle in a haystack".to_owned());
    state.push_event(AppEvent::MessageSearchLoaded {
        page: MessageSearchPage {
            query,
            messages: vec![result],
            total_results: Some(1),
            has_more: false,
        },
    });

    let view = state.search_popup_view().expect("search popup view");
    assert_eq!(view.results.len(), 1);
    match &view.results[0] {
        SearchResultItem::Message(item) => assert_eq!(item.content, "needle in a haystack"),
        SearchResultItem::Member(_) => panic!("expected message result"),
    }

    assert_eq!(
        state.activate_search_popup(),
        Some(AppCommand::LoadMessageHistoryAround {
            channel_id: Id::new(2),
            message_id: Id::new(42),
        })
    );
    assert_eq!(state.focus(), FocusPane::Messages);
}

#[test]
fn message_search_fields_use_requested_order_and_placeholders() {
    let mut state = state_with_writable_channel();
    state.open_search_popup_for_focus(FocusPane::Messages);

    let view = state.search_popup_view().expect("search popup view");
    let fields = view
        .fields
        .iter()
        .map(|field| (field.label.as_str(), field.placeholder.as_str()))
        .collect::<Vec<_>>();

    assert_eq!(
        fields,
        vec![
            ("contains", "text to search"),
            ("from", "user name"),
            ("in", "channel name"),
            ("has", "link, embed, file, video, image, sound, sticker"),
            ("mentions", "user name"),
            ("date", "gte:YYYY-MM-DD, lte:YYYY-MM-DD, equal:YYYY-MM-DD"),
            ("author type", "user, bot, webhook"),
            ("pinned", "y / n"),
        ]
    );
    assert!(view.fields[0].active);
}

#[test]
fn message_search_suggestions_show_names_and_use_selected_ids() {
    {
        let mut state = state_with_writable_channel_and_members();
        state.open_search_popup_for_focus(FocusPane::Messages);
        state.cycle_search_field_next();
        type_search_text(&mut state, "sal");

        let view = state.search_popup_view().expect("search popup view");
        match &view.suggestions[0] {
            SearchSuggestionItem::Member(member) => assert_eq!(member.user_id, Id::new(20)),
            SearchSuggestionItem::Channel(_) => panic!("expected member suggestion"),
        }

        assert_eq!(state.activate_search_popup(), None);
        let view = state.search_popup_view().expect("search popup view");
        assert_eq!(view.fields[1].value, "Sally");
        assert!(view.suggestions.is_empty());
        assert_eq!(state.search_popup_member_query(), None);

        let AppCommand::SearchMessages { query } = run_search(&mut state) else {
            panic!("expected search command");
        };
        assert_eq!(query.author_id, Some(Id::new(20)));
    }

    {
        let mut state = state_with_writable_channel_and_members();
        state.open_search_popup_for_focus(FocusPane::Messages);
        for _ in 0..4 {
            state.cycle_search_field_next();
        }
        type_search_text(&mut state, "sam");

        let view = state.search_popup_view().expect("search popup view");
        match &view.suggestions[0] {
            SearchSuggestionItem::Member(member) => assert_eq!(member.user_id, Id::new(21)),
            SearchSuggestionItem::Channel(_) => panic!("expected member suggestion"),
        }

        assert_eq!(state.activate_search_popup(), None);
        let view = state.search_popup_view().expect("search popup view");
        assert_eq!(view.fields[4].value, "Sammy");
        assert_eq!(state.search_popup_member_query(), None);

        let AppCommand::SearchMessages { query } = run_search(&mut state) else {
            panic!("expected search command");
        };
        assert_eq!(query.mentions_user_id, Some(Id::new(21)));
    }

    {
        let mut state = state_with_writable_channel();
        state.open_search_popup_for_focus(FocusPane::Messages);
        state.cycle_search_field_next();
        state.cycle_search_field_next();
        type_search_text(&mut state, "gen");

        let view = state.search_popup_view().expect("search popup view");
        match &view.suggestions[0] {
            SearchSuggestionItem::Channel(channel) => assert_eq!(channel.channel_id, Id::new(2)),
            SearchSuggestionItem::Member(_) => panic!("expected channel suggestion"),
        }

        assert_eq!(state.activate_search_popup(), None);
        let view = state.search_popup_view().expect("search popup view");
        assert_eq!(view.fields[2].value, "general");

        let AppCommand::SearchMessages { query } = run_search(&mut state) else {
            panic!("expected search command");
        };
        assert_eq!(query.channel_id, Some(Id::new(2)));
    }
}

#[test]
fn search_popup_member_query_uses_member_and_message_user_fields() {
    let mut state = state_with_writable_channel_and_members();
    state.focus_pane(FocusPane::Members);
    state.open_search_popup_for_focus(FocusPane::Members);
    type_search_text(&mut state, "alice");
    assert_eq!(state.search_popup_member_query(), Some("alice"));

    state.open_search_popup_for_focus(FocusPane::Messages);
    type_search_text(&mut state, "hello");
    assert_eq!(state.search_popup_member_query(), None);

    state.cycle_search_field_next();
    type_search_text(&mut state, "sally");
    assert_eq!(state.search_popup_member_query(), Some("sally"));
}

#[test]
fn member_search_filters_loaded_members_and_opens_profile() {
    let guild_id = Id::new(1);
    let alice = Id::new(10);
    let bob = Id::new(20);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: Some(2),
        owner_id: None,
        channels: vec![text_channel_info(guild_id, Id::new(2), "general")],
        members: vec![
            member_with_username(alice, "Alice A", "alice"),
            member_with_username(bob, "Bob B", "bob"),
        ],
        presences: vec![
            (alice, PresenceStatus::Online),
            (bob, PresenceStatus::Offline),
        ],
        roles: Vec::new(),
        emojis: Vec::new(),
    });
    state.activate_guild(ActiveGuildScope::Guild(guild_id));
    state.focus_pane(FocusPane::Members);

    state.open_search_popup_for_focus(FocusPane::Members);
    type_search_text(&mut state, "ali");

    let view = state.search_popup_view().expect("member search view");
    assert_eq!(view.results.len(), 1);
    match &view.results[0] {
        SearchResultItem::Member(item) => assert_eq!(item.display_name, "Alice A"),
        SearchResultItem::Message(_) => panic!("expected member result"),
    }

    assert_eq!(
        state.activate_search_popup(),
        Some(AppCommand::LoadUserProfile {
            user_id: alice,
            guild_id: Some(guild_id),
        })
    );
}

#[test]
fn blank_message_search_does_not_run_for_current_dm() {
    let channel_id = Id::new(20);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::ChannelUpsert(dm_channel_info(
        channel_id, "alice",
    )));
    state.activate_guild(ActiveGuildScope::DirectMessages);
    state.activate_channel(channel_id);

    state.open_search_popup_for_focus(FocusPane::Messages);

    assert_eq!(state.activate_search_popup(), None);
    assert_eq!(
        state.search_popup_view().and_then(|view| view.error),
        Some("Enter at least one search filter".to_owned())
    );
}

#[test]
fn message_search_builds_advanced_filter_query() {
    let mut state = state_with_writable_channel();
    state.open_search_popup_for_focus(FocusPane::Messages);
    cycle_search_field(&mut state, 3);
    type_search_text(&mut state, "link,image");
    cycle_search_field(&mut state, 2);
    type_search_text(&mut state, "gte:2026-05-01,lte:2026-05-30,equal:2026-05-10");
    cycle_search_field(&mut state, 1);
    type_search_text(&mut state, "user,bot");
    cycle_search_field(&mut state, 1);
    type_search_text(&mut state, "y");

    let AppCommand::SearchMessages { query } = run_search(&mut state) else {
        panic!("expected search command");
    };
    assert_eq!(
        query.has,
        vec![MessageSearchHas::Link, MessageSearchHas::Image]
    );
    assert_eq!(
        query.author_type,
        vec![MessageSearchAuthorType::User, MessageSearchAuthorType::Bot]
    );
    assert_eq!(
        query.date.as_deref(),
        Some("gte:2026-05-01,lte:2026-05-30,equal:2026-05-10")
    );
    assert_eq!(query.pinned, Some(true));
}

#[test]
fn message_search_rejects_invalid_filters_before_backend_command() {
    let cases = [
        (
            5,
            "2026-99-99",
            "Use date as gte:YYYY-MM-DD, lte:YYYY-MM-DD, or equal:YYYY-MM-DD",
        ),
        (
            3,
            "link,",
            "Use has: link, embed, file, video, image, sound, or sticker",
        ),
    ];

    for (field_hops, value, expected_error) in cases {
        let mut state = state_with_writable_channel();
        state.open_search_popup_for_focus(FocusPane::Messages);
        cycle_search_field(&mut state, field_hops);
        type_search_text(&mut state, value);

        assert_eq!(state.activate_search_popup(), None, "input {value:?}");
        assert_eq!(
            state.search_popup_view().and_then(|view| view.error),
            Some(expected_error.to_owned()),
            "input {value:?}"
        );
    }
}

#[test]
fn message_search_does_not_parse_digits_inside_names_as_ids() {
    let mut state = state_with_writable_channel();
    state.open_search_popup_for_focus(FocusPane::Messages);
    state.cycle_search_field_next();
    type_search_text(&mut state, "alice123");

    assert_eq!(state.activate_search_popup(), None);
    assert_eq!(
        state.search_popup_view().and_then(|view| view.error),
        Some("No matching sender found".to_owned())
    );
}

fn type_search_text(state: &mut DashboardState, value: &str) {
    for ch in value.chars() {
        state.push_search_char(ch);
    }
}

fn cycle_search_field(state: &mut DashboardState, count: usize) {
    for _ in 0..count {
        state.cycle_search_field_next();
    }
}

fn run_search(state: &mut DashboardState) -> AppCommand {
    state
        .activate_search_popup()
        .expect("message search command")
}
