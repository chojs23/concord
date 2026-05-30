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
            notifications: state.notification_options(),
            voice: state.voice_options(),
            ui_state: Default::default(),
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
            notifications: state.notification_options(),
            voice: state.voice_options(),
            ui_state: Default::default(),
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
            notifications: state.notification_options(),
            voice: state.voice_options(),
            ui_state: Default::default(),
        })
    );
}

#[test]
fn options_popup_esc_closes_popup() {
    let mut state = state_with_messages(1);

    state.open_options_popup();
    handle_key(&mut state, key(KeyCode::Esc));

    assert!(!state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::Options));
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
    assert_eq!(state.selected_option_index(), Some(6));

    handle_key(&mut state, ctrl_key('u'));
    assert_eq!(state.selected_option_index(), Some(0));
}
