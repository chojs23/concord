use super::*;

#[test]
fn options_popup_toggles_selected_setting() {
    let mut state = state_with_messages(1);

    state.open_options_popup();
    handle_key(&mut state, key(KeyCode::Down));
    handle_key(&mut state, key(KeyCode::Enter));

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::Options));
    assert!(!state.display_options().show_avatars);
    assert_eq!(
        state.take_options_save_request(),
        Some(AppOptions {
            display: state.display_options(),
            composer: state.composer_options(),
            credentials: Default::default(),
            notifications: state.notification_options(),
            voice: state.voice_options(),
        })
    );
}

#[test]
fn options_popup_cycles_image_preview_quality() {
    let mut state = state_with_messages(1);

    state.open_options_popup();
    for _ in 0..3 {
        handle_key(&mut state, key(KeyCode::Down));
    }
    handle_key(&mut state, key(KeyCode::Enter));

    assert_eq!(
        state.display_options().image_preview_quality,
        ImagePreviewQualityPreset::High
    );
    assert_eq!(
        state.take_options_save_request(),
        Some(AppOptions {
            display: state.display_options(),
            composer: state.composer_options(),
            credentials: Default::default(),
            notifications: state.notification_options(),
            voice: state.voice_options(),
        })
    );
}

#[test]
fn options_popup_h_l_adjust_microphone_sensitivity_by_one_or_ten_db() {
    let mut state = state_with_messages(1);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));
    handle_key(&mut state, char_key('v'));
    for _ in 0..3 {
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
            credentials: Default::default(),
            notifications: state.notification_options(),
            voice: state.voice_options(),
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
fn search_popup_still_accepts_printable_default_close_key_as_text() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    handle_key(&mut state, char_key('/'));
    handle_key(&mut state, char_key('q'));

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::Search));
    let view = state
        .search_popup_view()
        .expect("search popup remains open");
    assert_eq!(view.fields[0].value, "q");
}

#[test]
fn options_popup_selection_aliases_move_selection() {
    let mut state = state_with_messages(1);
    state.open_options_popup();

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
    assert_eq!(state.selected_option_index(), Some(5));

    handle_key(&mut state, ctrl_key('u'));
    assert_eq!(state.selected_option_index(), Some(0));
}

#[test]
fn options_popup_toggles_composer_emoji_links() {
    let mut state = state_with_messages(1);

    handle_key(&mut state, char_key(' '));
    handle_key(&mut state, char_key('o'));
    handle_key(&mut state, char_key('c'));
    handle_key(&mut state, key(KeyCode::Enter));

    assert!(state.composer_options().emojis_as_links);
    assert_eq!(
        state.take_options_save_request(),
        Some(AppOptions {
            display: state.display_options(),
            composer: state.composer_options(),
            credentials: Default::default(),
            notifications: state.notification_options(),
            voice: state.voice_options(),
        })
    );
}
