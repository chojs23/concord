use super::*;

type DisplayOptionChange = fn(DisplayOptions) -> DisplayOptions;

#[test]
fn every_display_option_changes_only_its_own_setting() {
    let cases: [(&str, DisplayOptionChange); 10] = [
        ("disable image preview", |mut options| {
            options.disable_image_preview = !options.disable_image_preview;
            options
        }),
        ("show avatars", |mut options| {
            options.show_avatars = !options.show_avatars;
            options
        }),
        ("show images", |mut options| {
            options.show_images = !options.show_images;
            options
        }),
        ("image preview quality", |mut options| {
            options.image_preview_quality = options.image_preview_quality.next();
            options
        }),
        ("attachment viewer quality", |mut options| {
            options.attachment_viewer_quality = options.attachment_viewer_quality.next();
            options
        }),
        ("show custom emoji", |mut options| {
            options.show_custom_emoji = !options.show_custom_emoji;
            options
        }),
        ("circular avatars", |mut options| {
            options.circular_avatars = !options.circular_avatars;
            options
        }),
        ("media playback", |mut options| {
            options.media_playback = !options.media_playback;
            options
        }),
        ("24-hour time", |mut options| {
            options.hour_format_24 = !options.hour_format_24;
            options
        }),
        ("animate previews", |mut options| {
            options.animate_previews = options.animate_previews.next();
            options
        }),
    ];

    for (index, (label, change)) in cases.into_iter().enumerate() {
        let mut state = state_with_messages(1);
        let before = state.display_options();
        state.open_options_popup();
        for _ in 0..index {
            handle_key(&mut state, key(KeyCode::Down));
        }
        handle_key(&mut state, key(KeyCode::Enter));

        assert!(
            state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::Options),
            "{label}"
        );
        assert_eq!(state.display_options(), change(before), "{label}");
        assert_eq!(
            state.take_options_save_request(),
            Some(AppOptions {
                display: state.display_options(),
                composer: state.composer_options(),
                reactions: Default::default(),
                credentials: Default::default(),
                notifications: state.notification_options(),
                voice: state.voice_options(),
                presence: Default::default(),
            }),
            "{label}"
        );
    }
}

#[test]
fn options_popup_h_l_adjust_microphone_sensitivity_by_one_or_ten_db() {
    let mut state = state_with_messages(1);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));
    handle_key(&mut state, char_key('v'));
    for _ in 0..8 {
        handle_key(&mut state, key(KeyCode::Down));
    }

    handle_key(&mut state, char_key('h'));
    assert_eq!(
        state.voice_options().microphone_sensitivity,
        MicrophoneSensitivityDb::new(-31)
    );

    handle_key(&mut state, char_key('H'));
    assert_eq!(
        state.voice_options().microphone_sensitivity,
        MicrophoneSensitivityDb::new(-41)
    );

    handle_key(&mut state, char_key('l'));
    assert_eq!(
        state.voice_options().microphone_sensitivity,
        MicrophoneSensitivityDb::new(-40)
    );

    handle_key(&mut state, char_key('L'));
    assert_eq!(
        state.voice_options().microphone_sensitivity,
        MicrophoneSensitivityDb::new(-30)
    );

    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, char_key('H'));
    assert_eq!(
        state.voice_options().microphone_volume,
        VoiceVolumePercent::new(90)
    );
    handle_key(&mut state, char_key('l'));
    assert_eq!(
        state.voice_options().microphone_volume,
        VoiceVolumePercent::new(91)
    );

    assert_eq!(
        state.take_options_save_request(),
        Some(AppOptions {
            display: state.display_options(),
            composer: state.composer_options(),
            reactions: Default::default(),
            credentials: Default::default(),
            notifications: state.notification_options(),
            voice: state.voice_options(),
            presence: Default::default(),
        })
    );
}

#[test]
fn options_popup_uses_configured_close_popup_key() {
    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: std::collections::BTreeMap::new(),
        mappings: [("ClosePopup".to_owned(), KeymapBinding::one("x"))]
            .into_iter()
            .collect(),
        ..Default::default()
    });

    state.open_options_popup();
    handle_key(&mut state, char_key('q'));
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::Options));

    handle_key(&mut state, key(KeyCode::Esc));
    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::Options));

    state.open_options_popup();
    handle_key(&mut state, char_key('x'));
    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::Options));

    let mut state = state_with_keymap(KeymapOptions {
        leader: None,
        groups: std::collections::BTreeMap::new(),
        mappings: [("ClosePopup".to_owned(), KeymapBinding::one("pagedown"))]
            .into_iter()
            .collect(),
        ..Default::default()
    });
    state.open_options_popup();

    handle_key(&mut state, key(KeyCode::PageDown));

    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::Options));
}

#[test]
fn search_popup_still_accepts_printable_popup_navigation_keys_as_text() {
    let mut state = state_with_keymap(KeymapOptions {
        mappings: [
            ("HalfPageDown".to_owned(), KeymapBinding::one("x")),
            ("HalfPageUp".to_owned(), KeymapBinding::one("y")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    });
    state = state_with_messages_from_state(state, 1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, char_key('/'));
    handle_key(&mut state, char_key('q'));
    handle_key(&mut state, char_key('g'));
    handle_key(&mut state, char_key('g'));
    handle_key(&mut state, char_key('G'));
    handle_key(&mut state, char_key('x'));
    handle_key(&mut state, char_key('y'));

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::Search));
    let view = state
        .search_popup_view()
        .expect("search popup remains open");
    assert_eq!(view.fields[0].value, "qggGxy");
}

#[test]
fn options_popup_selection_aliases_move_selection() {
    let mut state = state_with_messages(1);
    state.open_options_popup();
    crate::tui::ui::sync_view_heights(dashboard_area(), &mut state);

    handle_key(&mut state, ctrl_key('n'));
    assert_eq!(state.selected_option_index(), Some(1));

    handle_key(&mut state, ctrl_key('p'));
    assert_eq!(state.selected_option_index(), Some(0));

    handle_key(&mut state, char_key('j'));
    assert_eq!(state.selected_option_index(), Some(1));

    handle_key(&mut state, char_key('k'));
    assert_eq!(state.selected_option_index(), Some(0));

    handle_key(&mut state, key(KeyCode::Down));
    assert_eq!(state.selected_option_index(), Some(1));

    handle_key(&mut state, key(KeyCode::Up));
    assert_eq!(state.selected_option_index(), Some(0));

    handle_key(&mut state, ctrl_key('d'));
    assert!(state.selected_option_index().is_some_and(|index| index > 1));

    handle_key(&mut state, ctrl_key('u'));
    assert_eq!(state.selected_option_index(), Some(0));

    let last = state.display_option_items().len().saturating_sub(1);
    handle_key(&mut state, char_key('G'));
    assert_eq!(state.selected_option_index(), Some(last));

    handle_key(&mut state, char_key('g'));
    assert_eq!(state.selected_option_index(), Some(last));
    handle_key(&mut state, char_key('g'));
    assert_eq!(state.selected_option_index(), Some(0));
}

#[test]
fn options_popup_sequences_own_continuations_then_restore_fixed_shortcuts() {
    let mut state = state_with_keymap(KeymapOptions {
        mappings: [
            (
                "OpenComposerOptions".to_owned(),
                KeymapBinding::one("<leader>o x"),
            ),
            ("HalfPageDown".to_owned(), KeymapBinding::one("z c")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    });

    state.open_options_category_picker();
    assert_eq!(
        state
            .display_option_items()
            .into_iter()
            .map(|item| item.value.expect("category has a shortcut"))
            .collect::<Vec<_>>(),
        ["d", "c", "n", "v"]
    );

    handle_key(&mut state, char_key('z'));
    assert!(state.is_key_sequence_active());
    assert_eq!(state.key_sequence_title(), "z");
    assert_eq!(
        state
            .key_sequence_shortcuts()
            .into_iter()
            .map(|item| (item.key, item.label))
            .collect::<Vec<_>>(),
        [("c".to_owned(), "half page down".to_owned())]
    );
    handle_key(&mut state, char_key('c'));

    assert!(!state.is_key_sequence_active());
    assert!(state.is_options_category_picker_open());
    assert!(state.selected_option_index().is_some_and(|index| index > 0));

    handle_key(&mut state, char_key('c'));
    assert_eq!(state.display_option_items()[0].label, "Emojis as links");
    handle_key(&mut state, key(KeyCode::Enter));

    assert!(state.composer_options().emojis_as_links);
    assert_eq!(
        state.take_options_save_request(),
        Some(AppOptions {
            display: state.display_options(),
            composer: state.composer_options(),
            reactions: Default::default(),
            credentials: Default::default(),
            notifications: state.notification_options(),
            voice: state.voice_options(),
            presence: Default::default(),
        })
    );
}

#[test]
fn voice_options_enable_push_to_talk_and_apply_shortcut_without_restart() {
    let mut state = state_with_keymap(KeymapOptions {
        mappings: [("ClosePopup".to_owned(), KeymapBinding::one("x"))]
            .into_iter()
            .collect(),
        ..Default::default()
    });
    state = state_with_messages_from_state(state, 1);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));
    handle_key(&mut state, char_key('v'));
    for _ in 0..5 {
        handle_key(&mut state, key(KeyCode::Down));
    }
    handle_key(&mut state, key(KeyCode::Enter));

    assert!(state.voice_options().push_to_talk);

    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));
    assert!(state.is_capturing_push_to_talk_shortcut());
    assert_eq!(
        state.display_option_items()[6].value.as_deref(),
        Some("Press shortcut (Esc cancels)")
    );

    handle_key(&mut state, char_key('x'));

    assert!(!state.is_capturing_push_to_talk_shortcut());
    assert_eq!(state.voice_options().push_to_talk_shortcut, "X");
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::Options));
    assert!(state.take_options_save_request().is_some());
}

#[test]
fn push_to_talk_shortcut_capture_handles_invalid_cancel_and_modified_escape() {
    let mut state = state_with_messages(1);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));
    handle_key(&mut state, char_key('v'));
    for _ in 0..6 {
        handle_key(&mut state, key(KeyCode::Down));
    }

    handle_key(&mut state, key(KeyCode::Enter));
    handle_key(&mut state, key(KeyCode::Null));
    assert!(state.is_capturing_push_to_talk_shortcut());
    assert_eq!(state.voice_options().push_to_talk_shortcut, "F8");

    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_capturing_push_to_talk_shortcut());
    assert_eq!(state.voice_options().push_to_talk_shortcut, "F8");
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::Options));

    handle_key(&mut state, key(KeyCode::Enter));
    assert!(state.is_capturing_push_to_talk_shortcut());

    handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::CONTROL),
    );

    assert!(!state.is_capturing_push_to_talk_shortcut());
    assert_eq!(state.voice_options().push_to_talk_shortcut, "Control+Esc");
}
