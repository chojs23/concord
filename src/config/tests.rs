use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    AnimatePreviews, AppOptions, BorderShape, BorderSurface, ComposerOptions, CredentialOptions,
    CredentialStoreMode, DisplayOptions, HighlightGroup, ImagePreviewQualityPreset,
    ImageProtocolPreference, KeymapBinding, KeymapFileOptions, KeymapOptions, NotificationOptions,
    PresenceOptions, ThemeOptions, VoiceOptions, load_keymap_options_from_path,
    load_options_from_path, parse_app_options, parse_theme_options, save_options_to_path,
};
use crate::discord::{MicrophoneSensitivityDb, VoiceParticipantVolumePercent, VoiceVolumePercent};

#[test]
fn display_options_default_to_all_media_enabled() {
    let options = DisplayOptions::default();

    assert!(options.avatars_visible());
    assert!(options.images_visible());
    assert!(!options.media_playback_enabled());
    assert!(options.custom_emoji_visible());
    assert!(options.hour_format_24);
    assert_eq!(
        options.image_preview_quality,
        ImagePreviewQualityPreset::Balanced
    );
    assert_eq!(options.image_protocol, ImageProtocolPreference::Auto);
    assert_eq!(options.animate_previews, AnimatePreviews::Always);
}

#[test]
fn global_disable_overrides_individual_toggles() {
    let options = DisplayOptions {
        disable_image_preview: true,
        show_avatars: true,
        show_images: true,
        media_playback: false,
        image_preview_quality: ImagePreviewQualityPreset::Balanced,
        attachment_viewer_quality: ImagePreviewQualityPreset::Original,
        image_protocol: ImageProtocolPreference::Auto,
        animate_previews: AnimatePreviews::Selected,
        show_custom_emoji: true,
        circular_avatars: false,
        hour_format_24: true,
    };

    assert!(!options.avatars_visible());
    assert!(!options.images_visible());
    assert!(!options.custom_emoji_visible());
}

#[test]
fn app_config_parses_partial_toml_with_defaults() {
    let cases = [
        (
            "[display]\ndisable_image_preview = true\n",
            true,
            ImagePreviewQualityPreset::Balanced,
            false,
            false,
            false,
            false,
            MicrophoneSensitivityDb::default(),
        ),
        (
            "[display]\nimage_preview_quality = \"original\"\n",
            false,
            ImagePreviewQualityPreset::Original,
            false,
            false,
            false,
            false,
            MicrophoneSensitivityDb::default(),
        ),
        (
            "[display]\nmedia_playback = true\n",
            false,
            ImagePreviewQualityPreset::Balanced,
            false,
            false,
            true,
            false,
            MicrophoneSensitivityDb::default(),
        ),
        (
            "[voice]\nself_mute = true\n",
            false,
            ImagePreviewQualityPreset::Balanced,
            true,
            false,
            false,
            false,
            MicrophoneSensitivityDb::default(),
        ),
        (
            "[voice]\nallow_microphone_transmit = true\n",
            false,
            ImagePreviewQualityPreset::Balanced,
            false,
            false,
            false,
            true,
            MicrophoneSensitivityDb::default(),
        ),
        (
            "[voice]\nmicrophone_sensitivity = -70\n",
            false,
            ImagePreviewQualityPreset::Balanced,
            false,
            false,
            false,
            false,
            MicrophoneSensitivityDb::new(-70),
        ),
        (
            "[voice]\nmicrophone_sensitivity = 10\n",
            false,
            ImagePreviewQualityPreset::Balanced,
            false,
            false,
            false,
            false,
            MicrophoneSensitivityDb::new(0),
        ),
        (
            "[voice]\nmicrophone_sensitivity = -500\n",
            false,
            ImagePreviewQualityPreset::Balanced,
            false,
            false,
            false,
            false,
            MicrophoneSensitivityDb::new(-100),
        ),
        (
            "[notifications]\ndesktop_notifications = false\n",
            false,
            ImagePreviewQualityPreset::Balanced,
            false,
            false,
            false,
            false,
            MicrophoneSensitivityDb::default(),
        ),
        (
            "[notifications]\nvoice_join_sound = \"/tmp/join.wav\"\nvoice_leave_sound = \"/tmp/leave.wav\"\n",
            false,
            ImagePreviewQualityPreset::Balanced,
            false,
            false,
            false,
            false,
            MicrophoneSensitivityDb::default(),
        ),
        (
            "[notifications]\nnotification_sound = \"/tmp/message.wav\"\n",
            false,
            ImagePreviewQualityPreset::Balanced,
            false,
            false,
            false,
            false,
            MicrophoneSensitivityDb::default(),
        ),
        (
            "[composer]\nemojis_as_links = true\n",
            false,
            ImagePreviewQualityPreset::Balanced,
            false,
            false,
            false,
            false,
            MicrophoneSensitivityDb::default(),
        ),
        (
            "[credentials]\nstore = \"plain\"\n",
            false,
            ImagePreviewQualityPreset::Balanced,
            false,
            false,
            false,
            false,
            MicrophoneSensitivityDb::default(),
        ),
    ];

    for (
        toml,
        disable_image_preview,
        image_preview_quality,
        self_mute,
        self_deaf,
        media_playback,
        allow_microphone_transmit,
        microphone_sensitivity,
    ) in cases
    {
        let config: AppOptions = toml::from_str(toml).expect("partial config should parse");
        assert_eq!(config.display.disable_image_preview, disable_image_preview);
        assert!(config.display.show_avatars);
        assert!(config.display.show_images);
        assert_eq!(config.display.media_playback, media_playback);
        assert_eq!(config.display.image_preview_quality, image_preview_quality);
        assert_eq!(config.display.image_protocol, ImageProtocolPreference::Auto);
        assert!(config.display.show_custom_emoji);
        assert!(!config.display.circular_avatars);
        let expected_desktop_notifications =
            !toml.contains("[notifications]\ndesktop_notifications = false");
        assert_eq!(
            config.notifications.desktop_notifications,
            expected_desktop_notifications
        );
        if toml.contains("notification_sound") {
            assert_eq!(
                config.notifications.notification_sound.as_deref(),
                Some(std::path::Path::new("/tmp/message.wav"))
            );
        } else {
            assert!(config.notifications.notification_sound.is_none());
        }
        if toml.contains("voice_join_sound") {
            assert_eq!(
                config.notifications.voice_join_sound.as_deref(),
                Some(std::path::Path::new("/tmp/join.wav"))
            );
            assert_eq!(
                config.notifications.voice_leave_sound.as_deref(),
                Some(std::path::Path::new("/tmp/leave.wav"))
            );
        } else {
            assert!(config.notifications.voice_join_sound.is_none());
            assert!(config.notifications.voice_leave_sound.is_none());
        }
        assert_eq!(config.voice.self_mute, self_mute);
        assert_eq!(config.voice.self_deaf, self_deaf);
        assert_eq!(
            config.voice.allow_microphone_transmit,
            allow_microphone_transmit
        );
        assert!(config.voice.noise_suppression);
        assert_eq!(config.voice.microphone_sensitivity, microphone_sensitivity);
        assert_eq!(
            config.voice.microphone_volume,
            VoiceVolumePercent::default()
        );
        assert_eq!(
            config.voice.voice_output_volume,
            VoiceVolumePercent::default()
        );
        assert_eq!(
            config.composer.emojis_as_links,
            toml.contains("emojis_as_links")
        );
        let expected_credential_store = if toml.contains("store = \"plain\"") {
            CredentialStoreMode::Plain
        } else {
            CredentialStoreMode::Auto
        };
        assert_eq!(config.credentials.store, expected_credential_store);
    }
}

#[test]
fn hour_format_24_defaults_to_enabled_and_can_be_disabled() {
    let defaults: AppOptions = toml::from_str("[display]\n").expect("display config should parse");
    let twelve_hour: AppOptions =
        toml::from_str("[display]\nhour_format_24 = false\n").expect("display config should parse");

    assert!(defaults.display.hour_format_24);
    assert!(!twelve_hour.display.hour_format_24);
}

#[test]
fn display_image_protocol_parses_supported_values() {
    let cases = [
        ("auto", ImageProtocolPreference::Auto),
        ("iterm2", ImageProtocolPreference::Iterm2),
        ("kitty", ImageProtocolPreference::Kitty),
        ("sixel", ImageProtocolPreference::Sixel),
        ("halfblocks", ImageProtocolPreference::Halfblocks),
    ];

    for (value, expected) in cases {
        let config: AppOptions =
            toml::from_str(&format!("[display]\nimage_protocol = \"{value}\"\n"))
                .expect("image protocol config should parse");

        assert_eq!(config.display.image_protocol, expected);
    }
}

#[test]
fn invalid_value_is_skipped_without_discarding_the_rest() {
    let (options, warnings) = parse_app_options(
            "[display]\nshow_avatars = false\nimage_protocol = \"bogus\"\nshow_images = \"yes\"\n\n[voice]\nself_mute = true\n",
        )
        .expect("syntactically valid config should parse");

    assert!(!options.display.show_avatars, "valid sibling value applies");
    assert!(
        options.voice.self_mute,
        "valid value in other section applies"
    );
    assert_eq!(
        options.display.image_protocol,
        ImageProtocolPreference::default(),
        "invalid value falls back to default"
    );
    assert!(
        options.display.show_images,
        "wrong-typed value falls back to default"
    );
    assert_eq!(warnings.len(), 2, "one warning per skipped value");
    assert!(warnings.iter().any(|w| w.contains("image_protocol")));
    assert!(warnings.iter().any(|w| w.contains("show_images")));

    let (theme, warnings) = parse_theme_options(
        "[highlight.Selection]\nforeground = {}\nbackground = \"#112233\"\nstrikethrough = \"yes\"\n\n[ui.border]\npane = 7\ncomposer = \"curved\"\nmodal = \"double\"\n",
    )
    .expect("syntactically valid theme config should parse");
    let (expected, _) = parse_theme_options(
        "[highlight.Selection]\nbackground = \"#112233\"\n\n[ui.border]\nmodal = \"double\"\n",
    )
    .expect("expected theme config should parse");
    assert_eq!(theme, expected);
    assert_eq!(warnings.len(), 4);
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("foreground"))
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("strikethrough"))
    );
    assert!(warnings.iter().any(|warning| warning.contains("curved")));
    assert!(warnings.iter().any(|warning| warning.contains("pane")));

    let (theme, warnings) = parse_theme_options(
        "[theme]\nborder = \"red\"\n\n[highlight.FutureGroup]\nforeground = \"red\"\n\n[highlight.Selection]\nfuture = true\n\n[ui]\nfuture = true\n\n[ui.border]\nfuture = \"plain\"\n",
    )
    .expect("unknown theme fields should not fail parsing");
    assert_eq!(theme, ThemeOptions::default());
    assert_eq!(warnings.len(), 5);
}

#[test]
fn valid_config_reports_no_warnings() {
    let (_, warnings) =
        parse_app_options("[display]\nshow_avatars = false\n").expect("valid config should parse");

    assert!(warnings.is_empty());

    let mut content = String::new();
    for group in HighlightGroup::ALL {
        content.push_str(&format!(
            "\n[highlight.{}]\nforeground = \"terminal_default\"\n",
            group.name()
        ));
    }
    content.push_str(
        "\n[ui.border]\ndefault = \"plain\"\ncomposer = \"rounded\"\nmodal = \"thick\"\n\n[ui.indicator]\nselection = \"❯ \"\n",
    );
    let (theme, warnings) =
        parse_theme_options(&content).expect("the highlight inventory should parse");
    assert_eq!(theme.highlights().len(), HighlightGroup::COUNT);
    assert_eq!(theme.border_shapes().default, Some(BorderShape::Plain));
    assert_eq!(
        theme.border_shapes().get(BorderSurface::Composer),
        Some(BorderShape::Rounded)
    );
    assert_eq!(
        theme.border_shapes().get(BorderSurface::Modal),
        Some(BorderShape::Thick)
    );
    assert_eq!(theme.selection_marker(), Some("❯ "));
    assert!(warnings.is_empty());
}

#[test]
fn theme_selection_marker_parses_from_ui_indicator() {
    let (theme, warnings) = parse_theme_options("[ui.indicator]\nselection = \"❯ \"\n")
        .expect("selection marker config should parse");

    assert_eq!(theme.selection_marker(), Some("❯ "));
    assert!(warnings.is_empty());
}

#[test]
fn invalid_theme_selection_markers_warn_and_fall_back() {
    for (selection, expected_warning) in [
        ("", "non-zero display width"),
        ("\u{0301}", "non-zero display width"),
        ("first\nsecond", "single line"),
    ] {
        let content = format!(
            "[ui.indicator]\nselection = {}\n",
            toml::Value::String(selection.to_owned())
        );
        let (theme, warnings) =
            parse_theme_options(&content).expect("invalid selection marker TOML should parse");

        assert_eq!(theme.selection_marker(), None, "{selection:?}");
        assert_eq!(warnings.len(), 1, "{selection:?}");
        assert!(warnings[0].contains(expected_warning), "{warnings:?}");
    }
}

#[test]
fn syntax_error_still_fails() {
    assert!(parse_app_options("[display]\nshow_avatars = ").is_err());
    assert!(parse_theme_options("[highlight.Border]\nforeground = ").is_err());
}

#[test]
fn config_options_ignore_keymap_sections() {
    let config: AppOptions = toml::from_str(
        "[keymap]\nStartComposer = { keys = [\"c\"] }\n\n[display]\nshow_avatars = false\n",
    )
    .expect("config should ignore keymap table");

    assert!(!config.display.show_avatars);
}

#[test]
fn keymap_options_parse_partial_toml() {
    let keymap = parse_keymap_options(
        "[keymap]\nleader = \"space\"\nStartComposer = \"<leader>e\"\nReplyMessage = \"<leader>m r\"\n\n[keymap.groups]\n\"<C-w>\" = \"Window\"\n",
    );

    assert_eq!(keymap.leader.as_deref(), Some("space"));
    assert_eq!(
        keymap.mappings.get("StartComposer"),
        Some(&crate::config::KeymapBinding::one("<leader>e"))
    );
    assert_eq!(
        keymap.mappings.get("ReplyMessage"),
        Some(&crate::config::KeymapBinding::one("<leader>m r"))
    );
    assert_eq!(
        keymap.groups.get("<C-w>").map(String::as_str),
        Some("Window")
    );
    assert!(keymap.guild_actions.is_empty());
    assert!(keymap.channel_actions.is_empty());
    assert!(keymap.message_actions.is_empty());
    assert!(keymap.member_actions.is_empty());
    assert!(keymap.notification_inbox_actions.is_empty());
    assert!(keymap.composer.is_empty());
}

#[test]
fn keymap_options_parse_multi_key_bindings() {
    let keymap = parse_keymap_options(
        "[keymap]\nChannelSwitcher = { keys = [\"<C-w>f\", \"<leader><C-w>\"], description = \"find channel\" }\nOpenPaneFilter = { keys = \"<C-f>\" }\n\n[keymap.composer]\nSubmit = [\"<A-enter>\", \"<C-enter>\", \"<S-enter>\"]\n",
    );

    assert_eq!(
        keymap.mappings.get("ChannelSwitcher"),
        Some(&crate::config::KeymapBinding {
            keys: vec!["<C-w>f".to_owned(), "<leader><C-w>".to_owned()],
            description: Some("find channel".to_owned()),
        })
    );
    assert_eq!(
        keymap.mappings.get("OpenPaneFilter"),
        Some(&crate::config::KeymapBinding::one("<C-f>"))
    );

    assert_eq!(
        keymap.composer.get("Submit"),
        Some(&crate::config::KeymapBinding {
            keys: vec![
                "<A-enter>".to_owned(),
                "<C-enter>".to_owned(),
                "<S-enter>".to_owned(),
            ],
            description: None,
        })
    );
}

#[test]
fn keymap_options_parse_disabled_bindings() {
    let keymap = parse_keymap_options(
        "[keymap]\nPlayMedia = \"\"\nOpenPaneFilter = false\n\n[keymap.message_actions]\nPlayMedia = false\nOpenUrl = \"\"\n",
    );

    assert_eq!(
        keymap.mappings.get("PlayMedia"),
        Some(&crate::config::KeymapBinding::disabled())
    );
    assert_eq!(
        keymap.mappings.get("OpenPaneFilter"),
        Some(&crate::config::KeymapBinding::disabled())
    );
    assert_eq!(
        keymap.message_actions.get("PlayMedia"),
        Some(&crate::config::KeymapBinding::disabled())
    );
    assert_eq!(
        keymap.message_actions.get("OpenUrl"),
        Some(&crate::config::KeymapBinding::disabled())
    );
}

#[test]
fn keymap_options_parse_action_table_bindings() {
    let keymap = parse_keymap_options(
        "[keymap.VoiceDeafen]\nkeys = [\"dd\"]\ndescription = \"deafen voice\"\n",
    );

    assert_eq!(
        keymap.mappings.get("VoiceDeafen"),
        Some(&crate::config::KeymapBinding {
            keys: vec!["dd".to_owned()],
            description: Some("deafen voice".to_owned()),
        })
    );
}

#[test]
fn keymap_options_parse_scoped_action_bindings() {
    let keymap = parse_keymap_options(
        "[keymap.guild_actions]\nToggleMute = { keys = [\"m\"], description = \"mute server\" }\n\n[keymap.channel_actions]\nToggleMute = \"x\"\n\n[keymap.message_actions]\nGoToReferencedMessage = { keys = [\"g\"], description = \"go to referenced message\" }\n\n[keymap.member_actions]\nShowProfile = \"p\"\n\n[keymap.composer]\nOpenEditor = \"<C-o>\"\nDeletePreviousWord = { keys = [\"<A-backspace>\"], description = \"delete word\" }\n",
    );

    assert_eq!(
        keymap.guild_actions.get("ToggleMute"),
        Some(&crate::config::KeymapBinding {
            keys: vec!["m".to_owned()],
            description: Some("mute server".to_owned()),
        })
    );
    assert_eq!(
        keymap.channel_actions.get("ToggleMute"),
        Some(&crate::config::KeymapBinding::one("x"))
    );
    assert_eq!(
        keymap.member_actions.get("ShowProfile"),
        Some(&crate::config::KeymapBinding::one("p"))
    );
    assert_eq!(
        keymap.message_actions.get("GoToReferencedMessage"),
        Some(&crate::config::KeymapBinding {
            keys: vec!["g".to_owned()],
            description: Some("go to referenced message".to_owned()),
        })
    );
    assert_eq!(
        keymap.composer.get("OpenEditor"),
        Some(&crate::config::KeymapBinding::one("<C-o>"))
    );
    assert_eq!(
        keymap.composer.get("DeletePreviousWord"),
        Some(&crate::config::KeymapBinding {
            keys: vec!["<A-backspace>".to_owned()],
            description: Some("delete word".to_owned()),
        })
    );
}

#[test]
fn keymap_invalid_binding_is_skipped_without_discarding_the_rest() {
    let (keymap, warnings) = super::parse_keymap_options(
            "[keymap]\nleader = \"space\"\nStartComposer = \"<leader>e\"\nReplyMessage = 123\n\n[keymap.guild_actions]\nToggleMute = \"m\"\nBadAction = 7\n",
        )
        .expect("syntactically valid keymap should parse");

    assert_eq!(keymap.leader.as_deref(), Some("space"));
    assert_eq!(
        keymap.mappings.get("StartComposer"),
        Some(&crate::config::KeymapBinding::one("<leader>e")),
        "valid top-level binding applies"
    );
    assert!(
        !keymap.mappings.contains_key("ReplyMessage"),
        "invalid top-level binding is skipped"
    );
    assert_eq!(
        keymap.guild_actions.get("ToggleMute"),
        Some(&crate::config::KeymapBinding::one("m")),
        "valid action binding survives a bad sibling"
    );
    assert!(
        !keymap.guild_actions.contains_key("BadAction"),
        "invalid action binding is skipped"
    );
    assert_eq!(warnings.len(), 2);
    assert!(warnings.iter().any(|w| w.contains("ReplyMessage")));
    assert!(warnings.iter().any(|w| w.contains("BadAction")));
}

#[test]
fn keymap_action_maps_list_stays_in_sync_with_struct() {
    use std::collections::{BTreeMap, BTreeSet};

    // Built field by field on purpose: adding a named map to KeymapOptions
    // fails to compile here, a reminder to also list it in
    // KEYMAP_ACTION_MAPS so its bindings keep field-level tolerance.
    let action = || BTreeMap::from([("Probe".to_owned(), KeymapBinding::one("a"))]);
    let keymap = KeymapOptions {
        leader: Some("space".to_owned()),
        groups: BTreeMap::from([("Probe".to_owned(), "value".to_owned())]),
        guild_actions: action(),
        channel_actions: action(),
        message_actions: action(),
        member_actions: action(),
        thread_actions: action(),
        notification_inbox_actions: action(),
        composer: action(),
        mappings: BTreeMap::new(),
    };

    let serialized = toml::Value::try_from(&keymap).expect("keymap serializes");
    let named_maps: BTreeSet<&str> = serialized
        .as_table()
        .expect("keymap is a table")
        .iter()
        .filter(|(_, value)| value.is_table())
        .map(|(key, _)| key.as_str())
        .collect();

    let listed: BTreeSet<&str> = super::parse::KEYMAP_ACTION_MAPS.iter().copied().collect();
    assert_eq!(named_maps, listed);
}

#[test]
fn ui_state_parses_persisted_voice_participant_playback_settings() {
    let (ui_state, warnings) = super::parse_ui_state_options(
        r#"
[ui_state]
[[ui_state.voice_participant_playback]]
user_id = "42"
volume = 250
muted = true
"#,
    )
    .expect("participant playback state should parse");

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(ui_state.voice_participant_playback.len(), 1);
    let option = ui_state.voice_participant_playback[0];
    assert_eq!(option.user_id, crate::discord::ids::Id::new(42));
    assert_eq!(
        option.settings.volume,
        VoiceParticipantVolumePercent::new(200),
        "persisted volume is clamped at the domain boundary"
    );
    assert!(option.settings.muted);
}

#[test]
fn voice_volume_config_values_are_clamped() {
    let config: AppOptions =
        toml::from_str("[voice]\nmicrophone_volume = 250\nvoice_output_volume = -10\n")
            .expect("voice volume config should parse");

    assert_eq!(config.voice.microphone_volume, VoiceVolumePercent::new(200));
    assert_eq!(config.voice.voice_output_volume, VoiceVolumePercent::new(0));
}

#[test]
fn noise_suppression_defaults_to_enabled_and_can_be_disabled() {
    let disabled: AppOptions =
        toml::from_str("[voice]\nnoise_suppression = false\n").expect("voice config should parse");
    let missing: AppOptions = toml::from_str("[voice]\n").expect("voice config should parse");

    assert!(!disabled.voice.noise_suppression);
    assert!(missing.voice.noise_suppression);
}

#[test]
fn push_to_talk_defaults_to_disabled_and_can_be_enabled() {
    let defaults: AppOptions = toml::from_str("[voice]\n").expect("voice config should parse");
    let push_to_talk: AppOptions =
        toml::from_str("[voice]\npush_to_talk = true\npush_to_talk_shortcut = \"control+F8\"\n")
            .expect("push-to-talk config should parse");

    assert!(!defaults.voice.push_to_talk);
    assert_eq!(defaults.voice.push_to_talk_shortcut, "F8");
    assert!(push_to_talk.voice.push_to_talk);
    assert_eq!(push_to_talk.voice.push_to_talk_shortcut, "control+F8");
}

#[test]
fn voice_audio_sources_default_to_system_and_preserve_selected_device_ids() {
    let defaults: AppOptions = toml::from_str("[voice]\n").expect("voice config should parse");
    let selected: AppOptions = toml::from_str(
        "[voice]\ninput_source = \"CoreAudio:input-id\"\noutput_source = \"CoreAudio:output-id\"\n",
    )
    .expect("voice source config should parse");

    assert_eq!(defaults.voice.input_source, None);
    assert_eq!(defaults.voice.output_source, None);
    assert_eq!(
        selected.voice.input_source.as_deref(),
        Some("CoreAudio:input-id")
    );
    assert_eq!(
        selected.voice.output_source.as_deref(),
        Some("CoreAudio:output-id")
    );
}

#[test]
fn options_save_and_load_round_trip() {
    let path = test_config_path();
    let options = AppOptions {
        display: DisplayOptions {
            disable_image_preview: true,
            show_avatars: false,
            show_images: false,
            media_playback: false,
            image_preview_quality: ImagePreviewQualityPreset::Original,
            attachment_viewer_quality: ImagePreviewQualityPreset::Original,
            image_protocol: ImageProtocolPreference::Kitty,
            animate_previews: AnimatePreviews::Never,
            show_custom_emoji: false,
            circular_avatars: true,
            hour_format_24: false,
        },
        composer: ComposerOptions {
            emojis_as_links: true,
            ping_on_reply: false,
        },
        reactions: Default::default(),
        credentials: CredentialOptions {
            store: CredentialStoreMode::Plain,
        },
        notifications: NotificationOptions {
            desktop_notifications: false,
            notification_icon: Some("/tmp/icon.svg".to_string()),
            notification_sound: Some(std::path::PathBuf::from("/tmp/message.wav")),
            voice_join_sound: Some(std::path::PathBuf::from("/tmp/join.wav")),
            voice_leave_sound: Some(std::path::PathBuf::from("/tmp/leave.wav")),
        },
        voice: VoiceOptions {
            self_mute: true,
            self_deaf: true,
            input_source: Some("CoreAudio:input-id".to_owned()),
            output_source: Some("CoreAudio:output-id".to_owned()),
            allow_microphone_transmit: true,
            push_to_talk: true,
            push_to_talk_shortcut: "control+F8".to_owned(),
            noise_suppression: true,
            microphone_sensitivity: MicrophoneSensitivityDb::new(-50),
            microphone_volume: VoiceVolumePercent::new(80),
            voice_output_volume: VoiceVolumePercent::new(60),
        },
        presence: PresenceOptions {
            share_rich_presence: false,
        },
    };

    save_options_to_path(&path, &options).expect("config should save");
    let saved = fs::read_to_string(&path).expect("config should be readable");
    assert!(!saved.contains("[keymap"));
    assert!(!saved.contains("[ui_state"));
    let (loaded, warnings) = load_options_from_path(&path).expect("config should load");
    assert!(warnings.is_empty());

    assert_eq!(loaded.display, options.display);
    assert_eq!(loaded.composer, options.composer);
    assert_eq!(loaded.reactions, options.reactions);
    assert_eq!(loaded.notifications, options.notifications);
    assert_eq!(loaded.voice, options.voice);
    assert_eq!(loaded.presence, options.presence);
    let _ = fs::remove_file(&path);
    if let Some(parent) = path.parent() {
        let _ = fs::remove_dir_all(parent);
    }
}

#[test]
fn keymap_options_load_from_path_defaults_when_missing() {
    let path = test_keymap_path();

    let (loaded, warnings) =
        load_keymap_options_from_path(&path).expect("missing keymap should load");

    assert_eq!(loaded, KeymapOptions::default());
    assert!(warnings.is_empty());
}

#[test]
fn keymap_options_load_from_path_reads_keymap_file() {
    let path = test_keymap_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("test keymap parent should be created");
    }
    fs::write(&path, "[keymap]\nStartComposer = { keys = [\"c\"] }\n")
        .expect("test keymap should be written");

    let (loaded, _) = load_keymap_options_from_path(&path).expect("keymap should load");

    assert_eq!(
        loaded.mappings.get("StartComposer"),
        Some(&crate::config::KeymapBinding {
            keys: vec!["c".to_owned()],
            description: None,
        })
    );
    let _ = fs::remove_file(&path);
    if let Some(parent) = path.parent() {
        let _ = fs::remove_dir_all(parent);
    }
}

fn test_config_path() -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("concord-config-test-{unique}"))
        .join("config.toml")
}

fn test_keymap_path() -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("concord-keymap-test-{unique}"))
        .join("keymap.toml")
}

fn parse_keymap_options(toml: &str) -> KeymapOptions {
    toml::from_str::<KeymapFileOptions>(toml)
        .expect("keymap config should parse")
        .keymap
}

#[test]
fn reaction_favorite_emojis_parse_and_normalize() {
    let (options, warnings) = parse_app_options(
        r#"[reactions]
favorite_emojis = ["🔥", "not-an-emoji", "👍", "🔥", "❤️", "😂", "🎉", "😮", "😢", "🙏", "👀", "💯", "✨"]
"#,
    )
    .expect("reactions config should parse");

    assert_eq!(
        options.reactions.favorite_emojis,
        vec![
            "🔥".to_owned(),
            "👍".to_owned(),
            "❤️".to_owned(),
            "😂".to_owned(),
            "🎉".to_owned(),
            "😮".to_owned(),
            "😢".to_owned(),
            "🙏".to_owned(),
            "👀".to_owned(),
            "💯".to_owned(),
        ]
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("not a valid unicode emoji"))
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("duplicates an earlier entry"))
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("truncated to 10 entries"))
    );
}

#[test]
fn reaction_favorite_emojis_default_empty() {
    let (options, warnings) =
        parse_app_options("[display]\n").expect("empty reactions should use defaults");
    assert!(options.reactions.favorite_emojis.is_empty());
    assert!(warnings.is_empty());
}
