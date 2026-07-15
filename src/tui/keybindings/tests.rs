use std::str::FromStr;

use super::*;
use crate::tui::state::ActionAvailability;

fn char_chords(values: &[char]) -> Vec<KeyChord> {
    values.iter().copied().map(char_chord).collect()
}

#[test]
fn key_chord_parses_bare_keys_and_labels() {
    let chord = KeyChord::from_str("k").expect("key should parse");

    assert_eq!(chord.code, KeyCode::Char('k'));
    assert_eq!(chord.modifiers, KeyModifiers::NONE);
    assert_eq!(chord.label(), "k");
}

#[test]
fn angle_key_parses_neovim_modifier_aliases() {
    let cases = [
        ("C-f", KeyCode::Char('f'), KeyModifiers::CONTROL, "Ctrl+f"),
        ("C-w", KeyCode::Char('w'), KeyModifiers::CONTROL, "Ctrl+w"),
        ("S-f", KeyCode::Char('f'), KeyModifiers::SHIFT, "Shift+f"),
        ("A-f", KeyCode::Char('f'), KeyModifiers::ALT, "Alt+f"),
        ("M-f", KeyCode::Char('f'), KeyModifiers::ALT, "Alt+f"),
        (
            "C-S-f",
            KeyCode::Char('f'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            "Ctrl+Shift+f",
        ),
    ];

    for (value, code, modifiers, label) in cases {
        let chord = parse_angle_key(value).expect("angle key should parse");
        assert_eq!(chord.code, code);
        assert_eq!(chord.modifiers, modifiers);
        assert_eq!(chord.label(), label);
    }
}

#[test]
fn angle_key_rejects_non_vim_modifier_spellings() {
    let cases = [
        "ctrl+w",
        "C+w",
        "ctrl-w",
        "control-w",
        "shift-f",
        "alt-f",
        "c-w",
    ];

    for value in cases {
        assert!(
            parse_angle_key(value).is_err(),
            "{value} should not parse as an angle key"
        );
    }
}

#[test]
fn key_chord_preserves_uppercase_letter_keys() {
    let chord = KeyChord::from_str("R").expect("uppercase key should parse");

    assert_eq!(chord.code, KeyCode::Char('R'));
    assert!(chord.matches(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT)));
    assert!(!chord.matches(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)));
}

#[test]
fn shifted_angle_letter_matches_shifted_terminal_event() {
    let chord = parse_angle_key("S-f").expect("shifted key should parse");

    assert!(chord.matches(KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT)));
    assert!(!chord.matches(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE)));
}

#[test]
fn ui_action_names_match_future_colon_command_names() {
    assert_eq!(
        UiAction::from_name("SelectNext"),
        Some(UiAction::SelectNext)
    );
    assert_eq!(
        UiAction::from_name("SelectPrevious"),
        Some(UiAction::SelectPrevious)
    );
    assert_eq!(
        UiAction::from_name("ClosePopup"),
        Some(UiAction::ClosePopup)
    );
    assert_eq!(
        UiAction::from_name("ScrollViewportDown"),
        Some(UiAction::ScrollViewportDown)
    );
    assert_eq!(
        UiAction::from_name("ScrollViewportUp"),
        Some(UiAction::ScrollViewportUp)
    );
    assert_eq!(
        UiAction::from_name("ToggleGuildPane"),
        Some(UiAction::ToggleGuildPane)
    );
    assert_eq!(UiAction::from_name("VoiceMute"), Some(UiAction::VoiceMute));
    assert_eq!(
        UiAction::from_name("VoiceLeave"),
        Some(UiAction::VoiceLeave)
    );
    assert_eq!(
        UiAction::from_name("ChannelSwitcher"),
        Some(UiAction::ChannelSwitcher)
    );
    assert_eq!(
        UiAction::from_name("OpenFocusedPaneAction"),
        Some(UiAction::OpenFocusedPaneAction)
    );
    assert_eq!(UiAction::from_name("Quit"), Some(UiAction::Quit));
    assert_eq!(UiAction::from_name("OpenVoiceActions"), None);
}

#[test]
fn all_ui_action_names_round_trip() {
    for action in UiAction::ALL {
        assert_eq!(UiAction::from_name(action.name()), Some(*action));
        assert!(!action.label().is_empty());
    }
}

#[test]
fn default_keymap_uses_leader_v_voice_group() {
    let key_bindings = KeyBindings::default();
    let mut prefix = key_bindings.leader_keymap_prefix();

    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &prefix,
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Pending)
    );
    prefix.push(KeyChord::from_str("v").expect("v should parse"));
    let children = key_bindings.leader_keymap_children(&prefix);

    assert!(
        children
            .iter()
            .any(|item| item.key == "m" && item.label == "mute voice")
    );
    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &prefix,
            KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Action(UiAction::VoiceMute))
    );
    assert!(
        key_bindings
            .leader_keymap_children(&key_bindings.leader_keymap_prefix())
            .iter()
            .any(|item| item.key == "v" && item.label == "Voice" && item.has_children)
    );
}

#[test]
fn close_popup_defaults_to_esc_and_q_and_can_be_remapped() {
    let key_bindings = KeyBindings::default();

    assert!(key_bindings.is_popup_close_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert!(key_bindings.is_popup_close_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)));

    let documented_keymap = KeymapOptions {
        mappings: [("ClosePopup".to_owned(), KeymapBinding::one("q"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let documented_key_bindings = KeyBindings::try_from_options(&documented_keymap)
        .expect("documented close popup keymap parses");
    assert!(
        documented_key_bindings.is_popup_close_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
    );
    assert!(
        documented_key_bindings
            .is_popup_close_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
    );

    let keymap = KeymapOptions {
        mappings: [(
            "ClosePopup".to_owned(),
            KeymapBinding {
                keys: vec!["x".to_owned(), "<C-g>".to_owned()],
                description: None,
            },
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("close popup keymap parses");

    assert!(key_bindings.is_popup_close_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert!(
        !key_bindings.is_popup_close_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
    );
    assert!(key_bindings.is_popup_close_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)));
    assert!(
        key_bindings.is_popup_close_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL))
    );
}

#[test]
fn default_keymap_uses_g_prefix() {
    let key_bindings = KeyBindings::default();
    let prefix = [KeyChord::from_str("g").expect("g should parse")];

    assert_eq!(
        key_bindings.keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
        Some(KeyMapLookup::Pending)
    );
    assert_eq!(key_bindings.keymap_prefix_title(&prefix), "g");
    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &prefix,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Action(UiAction::JumpTop))
    );

    let children = key_bindings.leader_keymap_children(&prefix);
    assert!(
        children
            .iter()
            .any(|item| item.key == "g" && item.label == "jump top")
    );

    for menu_only_key in ['p', 't', 'u', 'c', 'P'] {
        assert_eq!(
            key_bindings.keymap_lookup_direct_key(KeyEvent::new(
                KeyCode::Char(menu_only_key),
                KeyModifiers::NONE
            )),
            None,
            "{menu_only_key} should not be a default direct message action binding"
        );
    }
}

#[test]
fn scoped_action_keymaps_override_pane_action_shortcuts_and_labels() {
    let keymap = KeymapOptions {
        guild_actions: [(
            "ToggleMute".to_owned(),
            KeymapBinding {
                keys: vec!["x".to_owned()],
                description: Some("mute server".to_owned()),
            },
        )]
        .into_iter()
        .collect(),
        channel_actions: [("ToggleMute".to_owned(), KeymapBinding::one("x"))]
            .into_iter()
            .collect(),
        message_actions: [("GoToReferencedMessage".to_owned(), KeymapBinding::one("g"))]
            .into_iter()
            .collect(),
        member_actions: [("ShowProfile".to_owned(), KeymapBinding::one("s"))]
            .into_iter()
            .collect(),
        thread_actions: [(
            "Close".to_owned(),
            KeymapBinding {
                keys: vec!["x".to_owned()],
                description: Some("close post".to_owned()),
            },
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings =
        KeyBindings::try_from_options(&keymap).expect("scoped action keymaps should parse");

    let guild_actions = [GuildActionItem::new(
        GuildActionKind::ToggleMute,
        "Mute server",
        ActionAvailability::Enabled,
    )];
    assert_eq!(
        key_bindings.guild_action_shortcuts(&guild_actions, 0),
        char_chords(&['x'])
    );
    assert_eq!(
        key_bindings.guild_action_label(&guild_actions[0]),
        "mute server"
    );

    let channel_actions = [ChannelActionItem::new(
        ChannelActionKind::ToggleMute,
        "Mute channel",
        ActionAvailability::Enabled,
    )];
    assert_eq!(
        key_bindings.channel_action_shortcuts(&channel_actions, 0),
        char_chords(&['x'])
    );

    let message_actions = [MessageActionItem::new(
        MessageActionKind::GoToReferencedMessage,
        "Go to referenced message",
        ActionAvailability::Enabled,
    )];
    assert_eq!(
        key_bindings.message_action_shortcuts(&message_actions, 0),
        char_chords(&['g'])
    );

    let member_actions = [MemberActionItem::new(
        MemberActionKind::ShowProfile,
        "Show profile",
        ActionAvailability::Enabled,
    )];
    assert_eq!(
        key_bindings.member_action_shortcuts(&member_actions, 0),
        char_chords(&['s'])
    );

    let thread_actions = [ThreadActionItem::new(
        ThreadActionKind::Close,
        "Close post",
        ActionAvailability::Enabled,
    )];
    assert_eq!(
        key_bindings.thread_action_shortcuts(&thread_actions, 0),
        char_chords(&['x'])
    );
    assert_eq!(
        key_bindings.thread_action_label(&thread_actions[0]),
        "close post"
    );
}

#[test]
fn thread_action_shortcuts_default_to_mnemonic_keys() {
    let key_bindings = KeyBindings::default();
    let actions = [
        ThreadActionItem::new(
            ThreadActionKind::MarkAsRead,
            "Mark as read",
            ActionAvailability::Enabled,
        ),
        ThreadActionItem::new(
            ThreadActionKind::Delete,
            "Delete post",
            ActionAvailability::Enabled,
        ),
    ];

    assert_eq!(key_bindings.thread_action_shortcut_label(&actions, 0), "m");
    assert_eq!(key_bindings.thread_action_shortcut_label(&actions, 1), "d");
}

#[test]
fn message_action_menu_shortcuts_follow_message_action_scope() {
    let keymap = KeymapOptions {
        mappings: [
            ("ReplyMessage".to_owned(), KeymapBinding::one("n")),
            ("OpenThread".to_owned(), KeymapBinding::one("gt")),
        ]
        .into_iter()
        .collect(),
        message_actions: [
            (
                "ReplyMessage".to_owned(),
                KeymapBinding {
                    keys: vec!["m".to_owned()],
                    description: Some("reply from menu".to_owned()),
                },
            ),
            ("OpenThread".to_owned(), KeymapBinding::one("T")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("message keymap should parse");
    let actions = [MessageActionItem::new(
        MessageActionKind::Reply,
        "reply",
        ActionAvailability::Enabled,
    )];

    assert_eq!(key_bindings.message_action_shortcut_label(&actions, 0), "m");
    assert_eq!(
        key_bindings.message_action_label(&actions[0]),
        "reply from menu"
    );
    assert_eq!(
        key_bindings
            .keymap_lookup_direct_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        Some(UiAction::ReplyMessage)
    );
    let thread_actions = [MessageActionItem::new(
        MessageActionKind::OpenThread,
        "open thread",
        ActionAvailability::Enabled,
    )];
    assert_eq!(
        key_bindings.message_action_shortcuts(&thread_actions, 0),
        char_chords(&['T'])
    );
    let direct_thread_prefix = [KeyChord::from_str("g").expect("g should parse")];
    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &direct_thread_prefix,
            KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Action(UiAction::OpenThread))
    );
    assert_eq!(
        key_bindings.dashboard_action_for_ui_action(UiAction::OpenThread, FocusPane::Messages),
        Some(DashboardAction::MessageShortcut(
            MessageActionKind::OpenThread
        ))
    );
    assert_eq!(
        key_bindings.message_action_label(&thread_actions[0]),
        "open thread"
    );
}

#[test]
fn disabled_keymap_binding_removes_default_direct_shortcut() {
    let keymap = KeymapOptions {
        mappings: [("PlayMedia".to_owned(), KeymapBinding::disabled())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let key_bindings =
        KeyBindings::try_from_options(&keymap).expect("disabled keymap should parse");

    assert_eq!(
        key_bindings
            .keymap_lookup_direct_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        None
    );
}

#[test]
fn disabled_message_action_binding_removes_default_action_shortcut() {
    let keymap = KeymapOptions {
        message_actions: [("PlayMedia".to_owned(), KeymapBinding::disabled())]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let key_bindings =
        KeyBindings::try_from_options(&keymap).expect("disabled action keymap should parse");
    let actions = [MessageActionItem::new(
        MessageActionKind::PlayMedia,
        "play media",
        ActionAvailability::Enabled,
    )];

    assert!(
        key_bindings
            .message_action_shortcuts(&actions, 0)
            .is_empty()
    );
    assert_eq!(key_bindings.message_action_shortcut_label(&actions, 0), "");
}

#[test]
fn scoped_action_keymaps_reject_actions_outside_their_scope() {
    let keymap = KeymapOptions {
        guild_actions: [("ShowPinnedMessages".to_owned(), KeymapBinding::one("x"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };

    assert!(KeyBindings::try_from_options(&keymap).is_err());
}

#[test]
fn scoped_action_keymaps_try_later_keys_when_first_key_conflicts() {
    let keymap = KeymapOptions {
        channel_actions: [
            ("ShowPinnedMessages".to_owned(), KeymapBinding::one("x")),
            (
                "ToggleMute".to_owned(),
                KeymapBinding {
                    keys: vec!["x".to_owned(), "z".to_owned()],
                    description: None,
                },
            ),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings =
        KeyBindings::try_from_options(&keymap).expect("scoped action keymaps should parse");
    let actions = [
        ChannelActionItem::new(
            ChannelActionKind::ShowPinnedMessages,
            "Show pinned messages",
            ActionAvailability::Enabled,
        ),
        ChannelActionItem::new(
            ChannelActionKind::ToggleMute,
            "Mute channel",
            ActionAvailability::Enabled,
        ),
    ];

    assert_eq!(
        key_bindings.channel_action_shortcuts(&actions, 0),
        char_chords(&['1'])
    );
    assert_eq!(
        key_bindings.channel_action_shortcuts(&actions, 1),
        char_chords(&['z'])
    );
}

#[test]
fn scoped_action_keymaps_keep_multiple_unique_aliases() {
    let keymap = KeymapOptions {
        channel_actions: [(
            "ToggleMute".to_owned(),
            KeymapBinding {
                keys: vec!["x".to_owned(), "u".to_owned()],
                description: None,
            },
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings =
        KeyBindings::try_from_options(&keymap).expect("scoped action keymaps should parse");
    let actions = [ChannelActionItem::new(
        ChannelActionKind::ToggleMute,
        "Mute channel",
        ActionAvailability::Enabled,
    )];

    assert_eq!(
        key_bindings.channel_action_shortcuts(&actions, 0),
        char_chords(&['x', 'u'])
    );
}

#[test]
fn scoped_action_keymaps_keep_modified_shortcuts_distinct() {
    let keymap = KeymapOptions {
        channel_actions: [(
            "ToggleMute".to_owned(),
            KeymapBinding {
                keys: vec!["u".to_owned(), "<C-u>".to_owned()],
                description: None,
            },
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings =
        KeyBindings::try_from_options(&keymap).expect("scoped action keymaps should parse");
    let actions = [ChannelActionItem::new(
        ChannelActionKind::ToggleMute,
        "Mute channel",
        ActionAvailability::Enabled,
    )];

    assert_eq!(
        key_bindings.channel_action_shortcuts(&actions, 0),
        vec![
            KeyChord::from_str("u").expect("u should parse"),
            parse_angle_key("C-u").expect("C-u should parse"),
        ]
    );
}

#[test]
fn scoped_action_keymaps_do_not_reuse_conflicting_numeric_keys_as_fallbacks() {
    let keymap = KeymapOptions {
        channel_actions: [
            ("ShowPinnedMessages".to_owned(), KeymapBinding::one("1")),
            ("ToggleMute".to_owned(), KeymapBinding::one("1")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings =
        KeyBindings::try_from_options(&keymap).expect("scoped action keymaps should parse");
    let actions = [
        ChannelActionItem::new(
            ChannelActionKind::ShowPinnedMessages,
            "Show pinned messages",
            ActionAvailability::Enabled,
        ),
        ChannelActionItem::new(
            ChannelActionKind::ToggleMute,
            "Mute channel",
            ActionAvailability::Enabled,
        ),
    ];

    assert_eq!(
        key_bindings.channel_action_shortcuts(&actions, 0),
        char_chords(&['2'])
    );
    assert_eq!(
        key_bindings.channel_action_shortcuts(&actions, 1),
        char_chords(&['3'])
    );
}

#[test]
fn composer_keymaps_override_default_composer_shortcuts() {
    let keymap = KeymapOptions {
        composer: [
            ("OpenEditor".to_owned(), KeymapBinding::one("<C-o>")),
            (
                "DeletePreviousWord".to_owned(),
                KeymapBinding {
                    keys: vec!["<A-backspace>".to_owned()],
                    description: None,
                },
            ),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("composer keymap parses");

    assert_eq!(
        key_bindings.composer_action(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)),
        ComposerAction::OpenInEditor
    );
    assert_eq!(
        key_bindings.composer_action(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL)),
        ComposerAction::Ignore
    );
    assert_eq!(
        key_bindings.composer_action(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT)),
        ComposerAction::EditText(crate::tui::text_input::TextEditAction::DeletePreviousWord)
    );
    assert_eq!(
        key_bindings.composer_action(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
        ComposerAction::Ignore
    );
}

#[test]
fn profile_popup_editing_uses_configured_composer_text_keys() {
    let keymap = KeymapOptions {
        composer: [
            ("PasteClipboard".to_owned(), KeymapBinding::one("<C-y>")),
            ("Submit".to_owned(), KeymapBinding::one("<C-s>")),
            ("Close".to_owned(), KeymapBinding::one("<C-q>")),
            (
                "DeletePreviousWord".to_owned(),
                KeymapBinding::one("<A-backspace>"),
            ),
            ("MoveCursorLeft".to_owned(), KeymapBinding::one("<A-left>")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("composer keymap parses");

    assert_eq!(
        key_bindings.profile_popup_action(
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
            true,
        ),
        Some(ProfilePopupAction::PasteClipboard)
    );
    assert_eq!(
        key_bindings.profile_popup_action(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            true,
        ),
        Some(ProfilePopupAction::StartOrCommitEdit)
    );
    assert_eq!(
        key_bindings.profile_popup_action(
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL),
            true,
        ),
        Some(ProfilePopupAction::Close)
    );
    assert_eq!(
        key_bindings
            .profile_popup_action(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT), true),
        Some(ProfilePopupAction::EditText(
            crate::tui::text_input::TextEditAction::DeletePreviousWord,
        ))
    );
    assert_eq!(
        key_bindings.profile_popup_action(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT), true),
        Some(ProfilePopupAction::EditText(
            crate::tui::text_input::TextEditAction::MoveCursorLeft,
        ))
    );
}

#[test]
fn composer_keymaps_reject_unknown_actions_and_conflicts() {
    let unknown = KeymapOptions {
        composer: [("ToggleMute".to_owned(), KeymapBinding::one("<C-m>"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    assert!(KeyBindings::try_from_options(&unknown).is_err());

    let conflicting = KeymapOptions {
        composer: [
            ("OpenEditor".to_owned(), KeymapBinding::one("<C-o>")),
            ("ClearInput".to_owned(), KeymapBinding::one("<C-o>")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    assert!(KeyBindings::try_from_options(&conflicting).is_err());

    let shifted_printable_conflict = KeymapOptions {
        composer: [
            ("OpenEditor".to_owned(), KeymapBinding::one("A")),
            ("ClearInput".to_owned(), KeymapBinding::one("<S-a>")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    assert!(KeyBindings::try_from_options(&shifted_printable_conflict).is_err());
}

#[test]
fn options_category_shortcut_labels_keep_contextual_defaults() {
    let key_bindings = KeyBindings::default();

    assert_eq!(
        key_bindings.options_category_shortcut_label(OptionsCategoryShortcut::Display),
        "d"
    );
    assert_eq!(
        key_bindings.options_category_shortcut_label(OptionsCategoryShortcut::Composer),
        "c"
    );
    assert_eq!(
        key_bindings.options_category_shortcut_label(OptionsCategoryShortcut::Notifications),
        "n"
    );
    assert_eq!(
        key_bindings.options_category_shortcut_label(OptionsCategoryShortcut::Voice),
        "v"
    );
}

#[test]
fn keymap_parses_leader_start_composer_sequence() {
    let keymap = KeymapOptions {
        mappings: [("StartComposer".to_owned(), KeymapBinding::one("<leader>e"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("keymap should parse");
    let leader_prefix = key_bindings.leader_keymap_prefix();

    assert!(
        key_bindings
            .leader_keymap_children(&leader_prefix)
            .iter()
            .any(|item| item.key == "e" && item.label == "start composer")
    );
    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &leader_prefix,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Action(UiAction::StartComposer))
    );
}

#[test]
fn keymap_parses_nested_leader_reply_sequence() {
    let keymap = KeymapOptions {
        mappings: [("ReplyMessage".to_owned(), KeymapBinding::one("<leader>m r"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("keymap should parse");
    let mut prefix = key_bindings.leader_keymap_prefix();

    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &prefix,
            KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Pending)
    );
    prefix.push(KeyChord::from_str("m").expect("m should parse"));
    let children = key_bindings.leader_keymap_children(&prefix);
    assert_eq!(children[0].key, "r");
    assert_eq!(children[0].label, "reply");
    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &prefix,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Action(UiAction::ReplyMessage))
    );
}

#[test]
fn keymap_allows_navigation_keys_after_leader_prefix() {
    let keymap = KeymapOptions {
        mappings: [("StartComposer".to_owned(), KeymapBinding::one("<leader>j"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("leader j should parse");
    let leader_prefix = key_bindings.leader_keymap_prefix();

    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &leader_prefix,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Action(UiAction::StartComposer))
    );
}

#[test]
fn keymap_parses_adjacent_angle_key_after_leader() {
    let keymap = KeymapOptions {
        mappings: [(
            "ChannelSwitcher".to_owned(),
            KeymapBinding::one("<leader><space>"),
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("leader space should parse");
    let leader_prefix = key_bindings.leader_keymap_prefix();

    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &leader_prefix,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Action(UiAction::ChannelSwitcher))
    );
}

#[test]
fn keymap_parses_adjacent_control_key_after_leader() {
    let keymap = KeymapOptions {
        mappings: [(
            "ChannelSwitcher".to_owned(),
            KeymapBinding::one("<leader><C-w>"),
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("leader C-w should parse");
    let leader_prefix = key_bindings.leader_keymap_prefix();

    assert!(
        key_bindings
            .leader_keymap_children(&leader_prefix)
            .iter()
            .any(|item| item.key == "Ctrl+w" && item.label == "Switch channels")
    );
    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &leader_prefix,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)
        ),
        Some(KeyMapLookup::Action(UiAction::ChannelSwitcher))
    );
}

#[test]
fn keymap_parses_direct_sequence() {
    let keymap = KeymapOptions {
        mappings: [("ChannelSwitcher".to_owned(), KeymapBinding::one("<C-w>"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("direct key should parse");

    assert_eq!(
        key_bindings
            .keymap_lookup_direct_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
        Some(UiAction::ChannelSwitcher)
    );
    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &key_bindings.leader_keymap_prefix(),
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)
        ),
        None
    );
}

#[test]
fn default_leader_p_opens_current_user_profile() {
    let key_bindings = KeyBindings::default();
    let leader_prefix = key_bindings.leader_keymap_prefix();

    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &leader_prefix,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
        ),
        Some(KeyMapLookup::Action(UiAction::OpenCurrentUserProfile))
    );
    assert!(
        key_bindings
            .leader_keymap_children(&leader_prefix)
            .iter()
            .any(|item| item.key == "p" && item.label == "My profile")
    );
}

#[test]
fn keymap_parses_compact_non_leader_prefix_sequence() {
    let keymap = KeymapOptions {
        mappings: [("ChannelSwitcher".to_owned(), KeymapBinding::one("<C-w>f"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("prefix should parse");
    let prefix = [KeyChord {
        code: KeyCode::Char('w'),
        modifiers: KeyModifiers::CONTROL,
    }];

    assert_eq!(
        key_bindings.keymap.lookup(&prefix),
        Some(KeyMapLookup::Pending)
    );
    assert_eq!(key_bindings.keymap_prefix_title(&prefix), "<C-w>");
    assert_eq!(key_bindings.leader_keymap_children(&prefix)[0].key, "f");
    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &prefix,
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Action(UiAction::ChannelSwitcher))
    );
}

#[test]
fn keymap_parses_plain_compact_prefix_sequence() {
    let keymap = KeymapOptions {
        mappings: [("VoiceDeafen".to_owned(), KeymapBinding::one("fd"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("prefix should parse");
    let prefix = [KeyChord::from_str("f").expect("f should parse")];

    assert_eq!(
        key_bindings.keymap.lookup(&prefix),
        Some(KeyMapLookup::Pending)
    );
    assert_eq!(key_bindings.keymap_prefix_title(&prefix), "f");
    assert_eq!(key_bindings.leader_keymap_children(&prefix)[0].key, "d");
    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &prefix,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Action(UiAction::VoiceDeafen))
    );
}

#[test]
fn keymap_configured_prefix_disables_conflicting_default_shortcut() {
    let keymap = KeymapOptions {
        mappings: [("VoiceDeafen".to_owned(), KeymapBinding::one("dd"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("prefix should parse");
    let prefix = [KeyChord::from_str("d").expect("d should parse")];

    assert_eq!(
        key_bindings.keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)),
        Some(KeyMapLookup::Pending)
    );
    assert_eq!(key_bindings.leader_keymap_children(&prefix)[0].key, "d");
    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &prefix,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Action(UiAction::VoiceDeafen))
    );
}

#[test]
fn keymap_configured_mapping_removes_canonical_default_alias_conflicts() {
    let keymap = KeymapOptions {
        mappings: [("VoiceDeafen".to_owned(), KeymapBinding::one("<S-tab> d"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("prefix should parse");
    let prefix = [parse_angle_key("S-tab").expect("S-tab should parse")];

    assert_eq!(
        key_bindings.keymap_lookup_root_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT)),
        Some(KeyMapLookup::Pending)
    );
    assert_eq!(key_bindings.leader_keymap_children(&prefix)[0].key, "d");
    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &prefix,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Action(UiAction::VoiceDeafen))
    );
}

#[test]
fn keymap_can_remap_quit_action() {
    let keymap = KeymapOptions {
        mappings: [("Quit".to_owned(), KeymapBinding::one("x"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("quit should parse");

    assert_eq!(
        key_bindings.keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        Some(KeyMapLookup::Action(UiAction::Quit))
    );
    assert_eq!(
        key_bindings.keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        None
    );
}

#[test]
fn keymap_can_remap_navigation_selection_actions() {
    let keymap = KeymapOptions {
        mappings: [
            ("SelectNext".to_owned(), KeymapBinding::one("n")),
            ("SelectPrevious".to_owned(), KeymapBinding::one("p")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("selection keys should parse");

    assert_eq!(
        key_bindings.keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        Some(KeyMapLookup::Action(UiAction::SelectNext))
    );
    assert_eq!(
        key_bindings.keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        Some(KeyMapLookup::Action(UiAction::SelectPrevious))
    );
    assert_eq!(
        key_bindings.keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
        None
    );
    assert_eq!(
        key_bindings.selection_action(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
            SelectionKeySet::Navigation,
        ),
        Some(SelectionAction::Next)
    );
    assert_eq!(
        key_bindings.selection_action(
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
            SelectionKeySet::Navigation,
        ),
        Some(SelectionAction::Previous)
    );
    assert_eq!(
        key_bindings.selection_action(
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            SelectionKeySet::Navigation,
        ),
        None
    );
    assert_eq!(
        key_bindings.selection_action(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
            SelectionKeySet::TextSafe,
        ),
        None
    );
    assert_eq!(
        key_bindings.selection_action(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
            SelectionKeySet::Navigation,
        ),
        Some(SelectionAction::Next)
    );
    assert_eq!(
        key_bindings.selection_action(
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
            SelectionKeySet::Navigation,
        ),
        Some(SelectionAction::Previous)
    );
}

#[test]
fn keymap_can_remap_viewport_scroll_actions() {
    let keymap = KeymapOptions {
        mappings: [
            ("ScrollViewportDown".to_owned(), KeymapBinding::one("N")),
            ("ScrollViewportUp".to_owned(), KeymapBinding::one("P")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings =
        KeyBindings::try_from_options(&keymap).expect("viewport scroll keys should parse");

    assert_eq!(
        key_bindings.keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::NONE)),
        Some(KeyMapLookup::Action(UiAction::ScrollViewportDown))
    );
    assert_eq!(
        key_bindings.keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE)),
        Some(KeyMapLookup::Action(UiAction::ScrollViewportUp))
    );
    assert_eq!(
        key_bindings.keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE)),
        None
    );
    assert_eq!(
        key_bindings
            .dashboard_action_for_ui_action(UiAction::ScrollViewportDown, FocusPane::Messages,),
        Some(DashboardAction::ScrollViewportDown)
    );
    assert_eq!(
        key_bindings
            .dashboard_action_for_ui_action(UiAction::ScrollViewportUp, FocusPane::Messages),
        Some(DashboardAction::ScrollViewportUp)
    );
    assert_eq!(
        key_bindings
            .dashboard_action_for_ui_action(UiAction::ScrollViewportDown, FocusPane::Channels,),
        Some(DashboardAction::ScrollViewportDown)
    );
}

#[test]
fn keymap_maps_message_shortcuts_to_message_actions() {
    let key_bindings = KeyBindings::default();

    for binding in MessageActionKind::KEYMAP_BINDINGS {
        assert_eq!(
            key_bindings.dashboard_action_for_ui_action(binding.ui_action, FocusPane::Messages),
            Some(DashboardAction::MessageShortcut(binding.message_action))
        );
        assert_eq!(
            key_bindings.dashboard_action_for_ui_action(binding.ui_action, FocusPane::Channels),
            None
        );
        assert_eq!(
            MessageActionKind::from_keymap_name(binding.keymap_name),
            Some(binding.message_action)
        );
        assert_eq!(binding.message_action.name(), binding.keymap_name);
    }
}

#[test]
fn close_popup_rejects_multi_key_sequences() {
    let keymap = KeymapOptions {
        mappings: [("ClosePopup".to_owned(), KeymapBinding::one("zz"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };

    assert!(KeyBindings::try_from_options(&keymap).is_err());
}

#[test]
fn keymap_rejects_fixed_control_selection_keys() {
    for key in ["<C-n>", "<C-p>", "<C-N>", "<C-P>"] {
        let keymap = KeymapOptions {
            mappings: [("StartComposer".to_owned(), KeymapBinding::one(key))]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        assert!(
            KeyBindings::try_from_options(&keymap).is_err(),
            "{key} should stay reserved for row movement"
        );
    }
}

#[test]
fn default_keymap_maps_resize_shortcuts_to_dashboard_actions() {
    let key_bindings = KeyBindings::default();

    let cases = [
        (
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT),
            UiAction::ResizePaneLeft,
            DashboardAction::ResizePaneLeft,
        ),
        (
            KeyEvent::new(KeyCode::Left, KeyModifiers::ALT),
            UiAction::ResizePaneLeft,
            DashboardAction::ResizePaneLeft,
        ),
        (
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::ALT),
            UiAction::ResizePaneRight,
            DashboardAction::ResizePaneRight,
        ),
        (
            KeyEvent::new(KeyCode::Right, KeyModifiers::ALT),
            UiAction::ResizePaneRight,
            DashboardAction::ResizePaneRight,
        ),
    ];

    for (key, ui_action, dashboard_action) in cases {
        assert_eq!(key_bindings.keymap_lookup_direct_key(key), Some(ui_action));
        assert_eq!(
            key_bindings.dashboard_action_for_ui_action(ui_action, FocusPane::Messages),
            Some(dashboard_action)
        );
    }
}

#[test]
fn keymap_can_remap_resize_actions() {
    let keymap = KeymapOptions {
        mappings: [
            ("ResizePaneLeft".to_owned(), KeymapBinding::one("<C-h>")),
            ("ResizePaneRight".to_owned(), KeymapBinding::one("<C-l>")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("resize keys should parse");

    assert_eq!(
        key_bindings
            .keymap_lookup_direct_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL)),
        Some(UiAction::ResizePaneLeft)
    );
    assert_eq!(
        key_bindings
            .keymap_lookup_direct_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
        Some(UiAction::ResizePaneRight)
    );
    assert_eq!(
        key_bindings.keymap_lookup_direct_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)),
        None
    );
    assert_eq!(
        key_bindings.keymap_lookup_direct_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::ALT)),
        None
    );
}

#[test]
fn keymap_uses_configured_description_for_shortcut_label() {
    let keymap = KeymapOptions {
        mappings: [(
            "ChannelSwitcher".to_owned(),
            KeymapBinding {
                keys: vec!["<C-w>f".to_owned()],
                description: Some("find channel".to_owned()),
            },
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("description should parse");
    let prefix = [KeyChord {
        code: KeyCode::Char('w'),
        modifiers: KeyModifiers::CONTROL,
    }];

    assert_eq!(
        key_bindings.leader_keymap_children(&prefix)[0].label,
        "find channel"
    );
}

#[test]
fn keymap_uses_configured_group_title() {
    let keymap = KeymapOptions {
        groups: [("<C-w>".to_owned(), "Window".to_owned())]
            .into_iter()
            .collect(),
        mappings: [("ChannelSwitcher".to_owned(), KeymapBinding::one("<C-w>f"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("group should parse");
    let prefix = [KeyChord {
        code: KeyCode::Char('w'),
        modifiers: KeyModifiers::CONTROL,
    }];

    assert_eq!(key_bindings.keymap_prefix_title(&prefix), "Window");
}

#[test]
fn keymap_uses_default_group_title() {
    let key_bindings = KeyBindings::default();
    let prefix = [key_bindings.keymap.leader, char_chord('v')];

    assert_eq!(key_bindings.keymap_prefix_title(&prefix), "Voice");
}

#[test]
fn lossy_keymap_keeps_valid_mapping_when_another_mapping_is_invalid() {
    let keymap = KeymapOptions {
        mappings: [
            ("StartComposer".to_owned(), KeymapBinding::one("<leader>e")),
            ("ReplyMessage".to_owned(), KeymapBinding::one("Enter")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::from_options(&keymap);
    let leader_prefix = key_bindings.leader_keymap_prefix();

    assert_eq!(
        key_bindings.keymap_lookup_with_key(
            &leader_prefix,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)
        ),
        Some(KeyMapLookup::Action(UiAction::StartComposer))
    );
}

#[test]
fn keymap_summaries_include_active_direct_and_composer_bindings() {
    let key_bindings = KeyBindings::default();
    let summaries = key_bindings.binding_summaries();

    assert!(summaries.iter().any(|summary| {
        summary.scope == "keymap"
            && summary.action == "StartComposer"
            && summary.keys.iter().any(|key| key == "i")
    }));
    assert!(summaries.iter().any(|summary| {
        summary.scope == "keymap.composer"
            && summary.action == "Submit"
            && summary.keys.iter().any(|key| key == "<Enter>")
    }));
    assert!(summaries.iter().any(|summary| {
        summary.scope == "keymap"
            && summary.action == "ToggleGuildPane"
            && summary.keys.iter().any(|key| key == "<leader> 1")
    }));
}

#[test]
fn keymap_uses_custom_leader_key() {
    let keymap = KeymapOptions {
        leader: Some("<C-k>".to_owned()),
        mappings: [("StartComposer".to_owned(), KeymapBinding::one("<leader>e"))]
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("custom leader should parse");

    assert!(key_bindings.is_leader_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)));
}

#[test]
fn profile_popup_uses_configured_selection_and_scroll_keys() {
    let keymap = KeymapOptions {
        mappings: [
            ("SelectNext".to_owned(), KeymapBinding::one("n")),
            ("SelectPrevious".to_owned(), KeymapBinding::one("p")),
            ("ScrollViewportDown".to_owned(), KeymapBinding::one("N")),
            ("ScrollViewportUp".to_owned(), KeymapBinding::one("P")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let key_bindings = KeyBindings::try_from_options(&keymap).expect("keymap should parse");

    assert_eq!(
        key_bindings
            .profile_popup_action(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE), false,),
        Some(ProfilePopupAction::NextField)
    );
    assert_eq!(
        key_bindings.profile_popup_action(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), false),
        Some(ProfilePopupAction::StartOrCommitEdit)
    );
    assert_eq!(
        key_bindings
            .profile_popup_action(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE), false,),
        Some(ProfilePopupAction::SignOut)
    );
    assert_eq!(
        key_bindings
            .profile_popup_action(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE), false,),
        Some(ProfilePopupAction::PreviousField)
    );
    assert_eq!(
        key_bindings.profile_popup_action(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), false),
        None
    );
    assert_eq!(
        key_bindings
            .profile_popup_action(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::NONE), false,),
        Some(ProfilePopupAction::Scroll(ScrollAction::Down))
    );
    assert_eq!(
        key_bindings
            .profile_popup_action(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE), false,),
        Some(ProfilePopupAction::Scroll(ScrollAction::Up))
    );
    assert_eq!(
        key_bindings
            .profile_popup_action(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE), false,),
        None
    );
}

#[test]
fn keymap_rejects_overlong_sequences() {
    let long_sequence = std::iter::once("<leader>".to_owned())
        .chain((0..MAX_KEYMAP_SEQUENCE_CHORDS).map(|_| "x".to_owned()))
        .collect::<Vec<_>>()
        .join(" ");
    let keymap = KeymapOptions {
        mappings: [(
            "StartComposer".to_owned(),
            KeymapBinding::one(long_sequence),
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };

    assert!(KeyBindings::try_from_options(&keymap).is_err());
}

#[test]
fn keymap_rejects_ambiguous_leaf_and_prefix_mappings() {
    let keymap = KeymapOptions {
        mappings: [
            ("StartComposer".to_owned(), KeymapBinding::one("<leader>m")),
            ("ReplyMessage".to_owned(), KeymapBinding::one("<leader>m r")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };

    assert!(KeyBindings::try_from_options(&keymap).is_err());
}
