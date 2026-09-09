use super::*;
use crate::config::{AppOptions, KeymapBinding, KeymapOptions};
use crate::tui::input::{handle_key, handle_mouse};
use crate::tui::state::{DebugMediaSnapshot, FocusPane};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};

fn key(state: &mut DashboardState, code: KeyCode) {
    handle_key(state, KeyEvent::new(code, KeyModifiers::NONE));
}

fn entries(range: std::ops::Range<u64>, width: usize) -> Vec<DebugLogLine> {
    wrap_debug_entries(
        range.map(|id| (id, format!("12:34:56 [ERROR] test: entry {id:03}"))),
        width,
    )
}

fn render_panel(state: &DashboardState, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| render_debug_panel(frame, frame.area(), state))
        .expect("test draw");
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn media_overview_shows_cache_budgets_and_work_without_a_false_memory_total() {
    let mut state = DashboardState::new();
    state.open_debug_log_popup();
    let stats = MediaCacheStats {
        entries: 4,
        entry_limit: 16,
        ready: 2,
        loading: 1,
        failed: 1,
        decoded_bytes: 16 * 1024 * 1024,
        decoded_byte_budget: 64 * 1024 * 1024,
        render_protocol_bytes: 2 * 1024 * 1024,
        retryable: 1,
        ..Default::default()
    };
    let snapshot = DebugMediaSnapshot {
        previews: stats,
        avatars: MediaCacheStats {
            entry_limit: 32,
            ..stats
        },
        emojis: stats,
        shared: crate::tui::media::SharedMediaCacheStats {
            ready: 3,
            ready_limit: 32,
            ready_decoded_bytes: 20 * 1024 * 1024,
            decoded_byte_budget: 128 * 1024 * 1024,
            decoding: 2,
            pending_requests: 4,
            retry_pending: 1,
            retained_source_bytes: 1024 * 1024,
        },
        active_sources: 2,
        source_limit: 8,
        protocol: Some("Kitty".to_owned()),
    };
    assert!(state.set_debug_media_snapshot(snapshot.clone()));
    assert!(!state.set_debug_media_snapshot(snapshot));
    state.sync_debug_log_lines(entries(0..3, 100), 20);
    let dump = render_panel(&state, 120, 40);
    for text in [
        "Diagnostics",
        "MEDIA CACHE",
        "g g first",
        "G live",
        "Kitty",
        "sources 2/8",
        "Previews",
        "Avatars",
        "Emoji",
        "64.0",
        "3/32 ready",
        "20.0/128.0 MiB",
        "4 waiting consumers",
        "1 decode retries",
        "3 retryable",
        "not total process RAM",
        "Logs · 3 lines",
    ] {
        assert!(dump.contains(text), "missing {text}:\n{dump}");
    }
    assert_eq!(
        state.debug_media_snapshot().expect("snapshot").previews,
        stats
    );
    for (width, height) in [(120, 40), (80, 24), (64, 30), (48, 16)] {
        let layout = DebugPanelLayout::new(Rect::new(0, 0, width, height), false);
        assert_eq!(
            summary_lines(&state, layout.summary).len(),
            usize::from(layout.summary.height)
        );
    }
    for (width, height) in [(80, 24), (64, 30)] {
        let compact = render_panel(&state, width, height);
        for label in ["Previews", "Avatars", "Emoji", "Ready", "Wait", "Fail"] {
            assert!(
                compact.contains(label),
                "compact diagnostics retain cache state:\n{compact}"
            );
        }
    }
}

#[test]
fn log_snapshot_displays_all_levels_and_filters_physical_lines_by_text() {
    let area = Rect::new(0, 0, 120, 40);
    let mut state = DashboardState::new();
    state.open_debug_log_popup();
    assert!(render_panel(&state, 120, 40).contains("Empty"));
    let text = "2026-09-07 12:34:56 UTC [DEBUG] media: cache hit\n2026-09-07 12:34:57 UTC [ERROR] history: request failed\n\n  native backend detail\n";
    let entries = text
        .split_terminator('\n')
        .enumerate()
        .map(|(id, line)| crate::logging::LogLine {
            id: id as u64,
            text: line.to_owned(),
        })
        .collect::<Vec<_>>();
    assert!(state.store_debug_log_tail(entries.clone()));
    sync_debug_panel(area, &mut state);
    let dump = render_panel(&state, 120, 40);
    for expected in [
        "Logs · 4 lines",
        "[DEBUG] media: cache hit",
        "[ERROR] history: request failed",
        "  native backend detail",
    ] {
        assert!(dump.contains(expected), "log content is preserved:\n{dump}");
    }
    assert_eq!(state.debug_log_lines()[2].text, "");
    assert!(
        !state.store_debug_log_tail(entries),
        "unchanged tail does not redraw"
    );

    for (query, expected) in [
        ("CACHE", Some("[DEBUG] media: cache hit")),
        ("native", Some("  native backend detail")),
        ("없는 로그", None),
    ] {
        key(&mut state, KeyCode::Char('/'));
        for ch in query.chars() {
            key(&mut state, KeyCode::Char(ch));
        }
        sync_debug_panel(area, &mut state);
        assert_eq!(state.debug_log_filter_query(), Some(query));
        assert_eq!(
            state.debug_log_entries().len(),
            4,
            "filter keeps the raw tail"
        );
        let dump = render_panel(&state, 120, 40);
        if let Some(expected) = expected {
            assert_eq!(state.debug_log_lines().len(), 1);
            assert!(state.debug_log_lines()[0].text.contains(expected));
            assert!(dump.contains("1/4 lines"));
        } else {
            assert!(state.debug_log_lines().is_empty());
            assert!(dump.contains("Empty"));
        }
        key(&mut state, KeyCode::Enter);
        assert!(state.debug_log_filter_cursor().is_none());
        assert_eq!(state.debug_log_filter_query(), Some(query));
        key(&mut state, KeyCode::Esc);
        sync_debug_panel(area, &mut state);
        assert!(state.debug_log_filter_query().is_none());
        assert_eq!(state.debug_log_lines().len(), 4);
        assert!(state.is_active_modal_popup(ActiveModalPopupKind::DebugLog));
    }
    key(&mut state, KeyCode::Char('/'));
    key(&mut state, KeyCode::Enter);
    assert!(
        state.debug_log_filter_query().is_none(),
        "empty input clears the filter"
    );
}

#[test]
fn log_tail_preserves_paused_entries_and_remains_available_after_reopening() {
    let area = Rect::new(0, 0, 120, 40);
    let tail = |range: std::ops::Range<u64>| {
        range
            .map(|id| crate::logging::LogLine {
                id,
                text: format!("12:34:56 [DEBUG] media: line {id:03}"),
            })
            .collect::<Vec<_>>()
    };
    let mut state = DashboardState::new();
    assert!(
        !state.store_debug_log_tail(tail(0..100)),
        "closed panel ignores snapshots"
    );
    state.open_debug_log_popup();
    state.store_debug_log_tail(tail(0..100));
    key(&mut state, KeyCode::Char('/'));
    for ch in "line".chars() {
        key(&mut state, KeyCode::Char(ch));
    }
    key(&mut state, KeyCode::Enter);
    sync_debug_panel(area, &mut state);
    key(&mut state, KeyCode::Up);
    let entry_id = state.debug_log_lines()[state.debug_log_scroll()].entry_id;
    state.store_debug_log_tail(tail(5..105));
    sync_debug_panel(area, &mut state);
    assert_eq!(
        state.debug_log_lines()[state.debug_log_scroll()].entry_id,
        entry_id
    );
    assert!(!state.debug_log_following());
    assert_eq!(state.debug_log_filter_query(), Some("line"));
    key(&mut state, KeyCode::Char('g'));
    key(&mut state, KeyCode::Char('g'));
    assert_eq!(state.debug_log_scroll(), 0);
    assert!(!state.debug_log_following());
    key(&mut state, KeyCode::Char('G'));
    assert!(state.debug_log_following());
    let viewport = DebugPanelLayout::new(area, true).log_content().height as usize;
    assert_eq!(
        state.debug_log_scroll(),
        state.debug_log_lines().len() - viewport
    );
    key(&mut state, KeyCode::Up);
    assert!(
        state
            .debug_log_lines()
            .last()
            .expect("new matching line")
            .text
            .ends_with("104")
    );

    key(&mut state, KeyCode::Esc);
    key(&mut state, KeyCode::Esc);
    assert_eq!(state.active_modal_popup_kind(), None);
    state.open_debug_log_popup();
    state.store_debug_log_tail(tail(5..105));
    sync_debug_panel(area, &mut state);
    assert!(state.debug_log_following());
    assert_eq!(state.debug_log_entries().len(), 100);
    assert!(render_panel(&state, 120, 40).contains("[DEBUG] media: line 104"));
}

#[test]
fn logs_follow_the_tail_until_scrolled_and_resume_at_the_bottom() {
    let mut state = DashboardState::new();
    state.open_debug_log_popup();
    state.sync_debug_log_lines(entries(0..10, 80), 4);
    assert_eq!(state.debug_log_scroll(), 6);
    state.sync_debug_log_lines(entries(0..11, 80), 4);
    assert_eq!(state.debug_log_scroll(), 7);

    key(&mut state, KeyCode::Up);
    assert!(!state.debug_log_following());
    assert_eq!(state.debug_log_scroll(), 6);
    state.sync_debug_log_lines(entries(0..12, 80), 4);
    assert_eq!(
        state.debug_log_scroll(),
        6,
        "new lines do not move a reader"
    );
    assert!(render_panel(&state, 100, 25).contains("PAUSED"));
    key(&mut state, KeyCode::Down);
    assert!(!state.debug_log_following());
    key(&mut state, KeyCode::Down);
    assert!(state.debug_log_following());
    state.sync_debug_log_lines(entries(0..13, 80), 4);
    assert_eq!(state.debug_log_scroll(), 9);

    key(&mut state, KeyCode::Char('g'));
    key(&mut state, KeyCode::Char('g'));
    assert_eq!(state.debug_log_scroll(), 0);
    assert!(!state.debug_log_following());
    key(&mut state, KeyCode::Char('G'));
    assert_eq!(state.debug_log_scroll(), 9);
    assert!(state.debug_log_following());
}

#[test]
fn paused_history_keeps_its_entry_when_the_tail_advances_or_width_changes() {
    let mut state = DashboardState::new();
    state.open_debug_log_popup();
    state.sync_debug_log_lines(entries(0..200, 80), 4);
    key(&mut state, KeyCode::PageUp);
    key(&mut state, KeyCode::PageUp);
    let anchor = state.debug_log_lines()[state.debug_log_scroll()].entry_id;
    for (range, width, height) in [(5..205, 80, 4), (5..205, 20, 6), (5..205, 100, 6)] {
        state.sync_debug_log_lines(entries(range, width), height);
        assert_eq!(
            state.debug_log_lines()[state.debug_log_scroll()].entry_id,
            anchor
        );
        assert!(!state.debug_log_following());
    }
    let long_entry = "12:34:56 [ERROR] resize: ".to_owned() + &"이미지 data  ".repeat(60);
    state.sync_debug_log_lines(wrap_debug_entries([(500, long_entry.clone())], 30), 4);
    key(&mut state, KeyCode::Char('G'));
    key(&mut state, KeyCode::PageUp);
    key(&mut state, KeyCode::PageUp);
    let source_start = state.debug_log_lines()[state.debug_log_scroll()].source_start;
    for width in [20, 40] {
        state.sync_debug_log_lines(wrap_debug_entries([(500, long_entry.clone())], width), 4);
        let visible = &state.debug_log_lines()[state.debug_log_scroll()];
        assert!(visible.source_start <= source_start);
        assert!(
            visible.source_start + visible.text.len() >= source_start,
            "resizing retains the source text being read"
        );
    }
    state.sync_debug_log_lines(entries(600..800, 80), 4);
    assert_eq!(
        state.debug_log_scroll(),
        0,
        "evicted anchor falls back to oldest retained entry"
    );
    assert!(!state.debug_log_following());
}

#[test]
fn panel_routes_configured_navigation_pages_and_wheel_without_moving_background_focus() {
    let defaults = AppOptions::default();
    let mut state = DashboardState::new_with_options(
        defaults.display,
        defaults.composer,
        defaults.credentials,
        defaults.notifications,
        defaults.voice,
        KeymapOptions {
            mappings: [
                ("OpenDebugPanel".to_owned(), KeymapBinding::one("z d")),
                ("JumpTop".to_owned(), KeymapBinding::one("t t")),
                ("JumpBottom".to_owned(), KeymapBinding::one("b")),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        },
        Default::default(),
    );
    state.focus_pane(FocusPane::Messages);
    key(&mut state, KeyCode::Char('`'));
    assert!(!state.is_active_modal_popup(ActiveModalPopupKind::DebugLog));
    key(&mut state, KeyCode::Char('z'));
    key(&mut state, KeyCode::Char('d'));
    assert!(state.is_active_modal_popup(ActiveModalPopupKind::DebugLog));
    state.sync_debug_log_lines(entries(0..100, 80), 10);
    for (code, expected) in [(KeyCode::PageUp, 85), (KeyCode::PageDown, 90)] {
        key(&mut state, code);
        assert_eq!(state.debug_log_scroll(), expected);
    }
    key(&mut state, KeyCode::Char('t'));
    assert!(state.is_key_sequence_active());
    assert_eq!(
        state.debug_log_scroll(),
        90,
        "a jump waits for the full sequence"
    );
    key(&mut state, KeyCode::Char('t'));
    assert_eq!(state.debug_log_scroll(), 0);
    assert!(!state.debug_log_following());
    let dump = render_panel(&state, 120, 40);
    assert!(dump.contains("t t first"));
    assert!(dump.contains("b live"));
    key(&mut state, KeyCode::Char('b'));
    assert_eq!(state.debug_log_scroll(), 90);
    assert!(state.debug_log_following());
    key(&mut state, KeyCode::Char('t'));
    key(&mut state, KeyCode::Char('t'));
    let area = Rect::new(0, 0, 120, 40);
    let content = DebugPanelLayout::new(area, false).log_content();
    for (kind, expected) in [
        (MouseEventKind::ScrollDown, 1),
        (MouseEventKind::ScrollUp, 0),
    ] {
        assert!(handle_mouse(
            &mut state,
            MouseEvent {
                kind,
                column: content.x,
                row: content.y,
                modifiers: KeyModifiers::NONE
            },
            area
        ));
        assert_eq!(state.debug_log_scroll(), expected);
    }
    key(&mut state, KeyCode::Esc);
    assert_eq!(state.active_modal_popup_kind(), None);
    assert_eq!(state.focus(), FocusPane::Messages);
    state.open_debug_log_popup();
    state.sync_debug_log_lines(entries(0..100, 80), 10);
    assert!(state.debug_log_following(), "a new panel opens live");
    key(&mut state, KeyCode::Char('q'));
    assert_eq!(state.active_modal_popup_kind(), None);

    for (binding, keys) in [
        ("/", vec![KeyCode::Char('/')]),
        ("f", vec![KeyCode::Char('f')]),
        ("z f", vec![KeyCode::Char('z'), KeyCode::Char('f')]),
    ] {
        let defaults = AppOptions::default();
        let mut state = DashboardState::new_with_options(
            defaults.display,
            defaults.composer,
            defaults.credentials,
            defaults.notifications,
            defaults.voice,
            KeymapOptions {
                mappings: [
                    ("OpenPaneFilter".to_owned(), KeymapBinding::one(binding)),
                    (
                        "SelectNext".to_owned(),
                        KeymapBinding {
                            keys: vec!["j".to_owned(), "<C-j>".to_owned()],
                            description: None,
                        },
                    ),
                    (
                        "SelectPrevious".to_owned(),
                        KeymapBinding {
                            keys: vec!["k".to_owned(), "<C-k>".to_owned()],
                            description: None,
                        },
                    ),
                ]
                .into_iter()
                .collect(),
                ..Default::default()
            },
            Default::default(),
        );
        state.open_debug_log_popup();
        state.sync_debug_log_lines(entries(0..100, 80), 10);
        let scroll = state.debug_log_scroll();
        for code in &keys {
            key(&mut state, *code);
        }
        assert_eq!(state.debug_log_filter_cursor(), Some(0), "{binding}");
        for ch in "jggGqk/이미지".chars() {
            key(&mut state, KeyCode::Char(ch));
        }
        assert_eq!(state.debug_log_filter_query(), Some("jggGqk/이미지"));
        assert_eq!(state.debug_log_scroll(), scroll, "typing does not scroll");
        for (ch, expected) in [('k', scroll - 1), ('j', scroll)] {
            handle_key(
                &mut state,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL),
            );
            assert_eq!(state.debug_log_scroll(), expected);
            assert_eq!(state.debug_log_filter_query(), Some("jggGqk/이미지"));
        }
        for (code, cursor) in [(KeyCode::Home, 0), (KeyCode::End, "jggGqk/이미지".len())] {
            key(&mut state, code);
            assert_eq!(state.debug_log_filter_cursor(), Some(cursor));
            assert_eq!(
                state.debug_log_scroll(),
                scroll,
                "cursor keys do not scroll"
            );
        }
        key(&mut state, KeyCode::Left);
        key(&mut state, KeyCode::Backspace);
        key(&mut state, KeyCode::Right);
        assert_eq!(state.debug_log_filter_query(), Some("jggGqk/이지"));
        assert!(crate::tui::input::handle_paste(&mut state, " cache\r\n"));
        assert_eq!(state.debug_log_filter_query(), Some("jggGqk/이지 cache"));
        key(&mut state, KeyCode::Enter);
        assert!(state.debug_log_filter_cursor().is_none());
        key(&mut state, KeyCode::Char('k'));
        assert_eq!(state.debug_log_scroll(), scroll - 1);
        key(&mut state, KeyCode::Char('j'));
        assert_eq!(state.debug_log_scroll(), scroll);
        for code in keys {
            key(&mut state, code);
        }
        assert_eq!(
            state.debug_log_filter_query(),
            Some("jggGqk/이지 cache"),
            "reopen edits existing query"
        );
        key(&mut state, KeyCode::Esc);
        assert!(state.debug_log_filter_query().is_none());
        assert!(state.is_active_modal_popup(ActiveModalPopupKind::DebugLog));
        assert!(!crate::tui::input::handle_paste(&mut state, "ignored"));
        key(&mut state, KeyCode::Esc);
        assert_eq!(state.active_modal_popup_kind(), None);
    }
}

#[test]
fn panel_layout_preserves_logs_on_small_screens_and_wraps_unicode_details() {
    let detail = "12:34:56 [ERROR] image: 이미지 처리 실패. detail=Discord HTTP 403 Missing Access";
    for (width, height) in [
        (120, 40),
        (80, 24),
        (48, 16),
        (30, 10),
        (10, 5),
        (2, 2),
        (0, 0),
    ] {
        let area = Rect::new(0, 0, width, height);
        let layout = DebugPanelLayout::new(area, false);
        assert_eq!(layout.popup.intersection(area), layout.popup);
        let content = layout.log_content();
        if height >= 10 {
            assert!(content.height >= 3, "log space survives compact summaries");
        }
        let mut state = DashboardState::new();
        state.open_debug_log_popup();
        let lines = wrap_debug_entries(
            [(0, detail.to_owned())],
            usize::from(content.width.saturating_sub(1)),
        );
        for line in &lines {
            assert!(
                unicode_width::UnicodeWidthStr::width(line.text.as_str())
                    <= usize::from(content.width.saturating_sub(1)).max(2)
            );
        }
        assert!(
            lines
                .iter()
                .flat_map(|line| line.text.chars())
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>()
                .contains("MissingAccess")
        );
        state.sync_debug_log_lines(lines, usize::from(content.height));
        if width > 0 && height > 0 {
            let dump = render_panel(&state, width, height);
            if width >= 48 {
                assert!(dump.contains("ccess"), "tail stays visible:\n{dump}");
            }
            assert_eq!(
                state.debug_log_scroll(),
                state
                    .debug_log_lines()
                    .len()
                    .saturating_sub(usize::from(content.height))
            );
        }
    }
    for (width, height) in [(120, 40), (48, 16), (10, 8), (3, 8)] {
        let area = Rect::new(0, 0, width, height);
        let mut state = DashboardState::new();
        state.open_debug_log_popup();
        key(&mut state, KeyCode::Char('/'));
        for ch in "긴 검색어 이미지 cache history".repeat(4).chars() {
            key(&mut state, KeyCode::Char(ch));
        }
        sync_debug_panel(area, &mut state);
        let filter = DebugPanelLayout::new(area, true).filter;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| render_debug_panel(frame, area, &state))
            .expect("filtered draw");
        if let Some(filter) = filter {
            let cursor = terminal.get_cursor_position().expect("filter cursor");
            assert!(
                filter.contains(cursor),
                "cursor stays inside the filter row"
            );
        } else {
            assert!(width < 10, "usable panels show the filter row");
        }
    }
    let mut state = DashboardState::new();
    state.open_debug_log_popup();
    let dump = render_panel(&state, 120, 40);
    let content = DebugPanelLayout::new(Rect::new(0, 0, 120, 40), false).log_content();
    let visible_lines = dump
        .lines()
        .skip(usize::from(content.y))
        .take(usize::from(content.height))
        .map(|row| {
            row.chars()
                .skip(usize::from(content.x))
                .take(usize::from(content.width))
                .collect::<String>()
        })
        .map(|row| row.trim().to_owned())
        .filter(|row| !row.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(visible_lines, ["Empty"]);
    assert!(dump.contains("Logs · 0 lines"));
    assert!(dump.contains("diagnostics unavailable"));
}
