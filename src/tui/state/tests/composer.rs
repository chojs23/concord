use super::*;
use crate::discord::AppCommand;
use crate::discord::{ApplicationCommandInfo, ApplicationCommandOptionInfo};
use serde_json::json;

fn application_command(
    name: &str,
    options: Vec<ApplicationCommandOptionInfo>,
) -> ApplicationCommandInfo {
    ApplicationCommandInfo {
        application_id: Id::new(200),
        version: "1".to_owned(),
        application_name: Some("TestBot".to_owned()),
        description: format!("{name} command"),
        options,
        raw: json!({
            "id": "100",
            "application_id": "200",
            "version": "1",
            "name": name,
        }),
        ..ApplicationCommandInfo::test(Id::new(100), name)
    }
}

fn application_command_option(
    kind: u64,
    name: &str,
    required: bool,
    options: Vec<ApplicationCommandOptionInfo>,
) -> ApplicationCommandOptionInfo {
    ApplicationCommandOptionInfo {
        description: format!("{name} option"),
        required,
        options,
        ..ApplicationCommandOptionInfo::test(kind, name)
    }
}

fn state_with_application_command(command: ApplicationCommandInfo) -> DashboardState {
    let mut state = state_with_writable_channel();
    state.push_event(AppEvent::ApplicationCommandsLoaded {
        guild_id: Some(Id::new(1)),
        commands: vec![command],
    });
    state.start_composer();
    state
}

fn submit_composer_text(input: &str) -> Option<AppCommand> {
    let mut state = state_with_writable_channel();
    state.start_composer();
    for value in input.chars() {
        state.push_composer_char(value);
    }
    state.submit_composer()
}

fn state_with_command_mentions(command: ApplicationCommandInfo) -> DashboardState {
    let me: Id<UserMarker> = Id::new(10);
    let guild: Id<GuildMarker> = Id::new(1);
    let general: Id<ChannelMarker> = Id::new(2);
    let rules: Id<ChannelMarker> = Id::new(3);
    let mut state = DashboardState::new();
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(me),
    });
    state.push_event(AppEvent::GuildCreate {
        guild_id: guild,
        name: "guild".to_owned(),
        member_count: Some(2),
        owner_id: Some(me),
        channels: vec![
            positioned_text_channel_info(guild, general, "general", 0),
            positioned_text_channel_info(guild, rules, "rules", 1),
        ],
        members: vec![
            member_with_username(me, "me", "me"),
            member_with_username(Id::new(20), "Sally", "salamander"),
        ],
        presences: vec![
            (me, PresenceStatus::Online),
            (Id::new(20), PresenceStatus::Online),
        ],
        roles: vec![
            role_info(Id::new(guild.get()), "@everyone", 0x400 | 0x800),
            RoleInfo {
                color: Some(0xFFAA00),
                ..role_info(Id::new(30), "moderators", 0)
            },
        ],
        emojis: Vec::new(),
    });
    state.activate_guild(ActiveGuildScope::Guild(guild));
    state.activate_channel(general);
    state.push_event(AppEvent::ApplicationCommandsLoaded {
        guild_id: Some(guild),
        commands: vec![command],
    });
    state.start_composer();
    state
}

fn push_foreign_custom_emojis(state: &mut DashboardState) {
    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(9),
        emojis: vec![
            CustomEmojiInfo::test(Id::new(60), "wave_foreign"),
            CustomEmojiInfo {
                animated: true,
                ..CustomEmojiInfo::test(Id::new(61), "dance_foreign")
            },
        ],
    });
}

fn type_composer_text(state: &mut DashboardState, value: &str) {
    for ch in value.chars() {
        state.push_composer_char(ch);
    }
}

fn assert_slash_invocation(command: Option<AppCommand>, command_name: &str, content: &str) {
    let Some(AppCommand::RunApplicationCommand { invocation }) = command else {
        panic!("expected slash command invocation");
    };
    assert_eq!(invocation.guild_id, Some(Id::new(1)));
    assert_eq!(invocation.channel_id, Id::new(2));
    assert_eq!(
        invocation
            .command_identity
            .map(|identity| (identity.id, identity.application_id)),
        Some((Id::new(100), Id::new(200)))
    );
    assert_eq!(invocation.command_name, command_name);
    assert_eq!(invocation.content, content);
}

#[test]
fn start_composer_refused_in_read_only_channel() {
    let mut state = state_with_read_only_channel();
    state.start_composer();
    assert!(
        !state.is_composing(),
        "composer must not open when SEND_MESSAGES is denied"
    );
}

#[test]
fn start_composer_queues_application_command_load_when_missing() {
    let mut state = state_with_writable_channel();

    state.start_composer();

    assert_eq!(
        state.drain_pending_commands(),
        vec![AppCommand::LoadApplicationCommands {
            guild_id: Some(Id::new(1)),
        }]
    );
}

#[test]
fn submit_composer_drops_message_when_send_revoked_after_open() {
    // Open the composer with SEND_MESSAGES granted, type something, then
    // simulate a permission overwrite arriving that revokes SEND. Submit
    // must refuse rather than silently fire a request that would 403.
    let mut state = state_with_writable_channel();
    state.start_composer();
    state.push_composer_char('h');
    state.push_composer_char('i');
    assert!(state.is_composing());

    // Apply a CHANNEL_UPDATE that strips SEND_MESSAGES via a channel
    // overwrite on @everyone (role id == guild id == 1).
    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        permission_overwrites: vec![PermissionOverwriteInfo {
            deny: 0x800,
            ..PermissionOverwriteInfo::test(1, PermissionOverwriteKind::Role)
        }],
        ..positioned_text_channel_info(Id::new(1), Id::new(2), "general", 0)
    }));
    assert_eq!(state.submit_composer(), None);
    assert!(!state.is_composing());
}

#[test]
fn active_channel_is_cleared_when_view_permission_is_revoked() {
    let mut state = state_with_writable_channel();
    state.start_composer();
    assert_eq!(state.selected_channel_id(), Some(Id::new(2)));
    assert!(state.is_composing());

    state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
        permission_overwrites: vec![PermissionOverwriteInfo {
            deny: 0x400,
            ..PermissionOverwriteInfo::test(1, PermissionOverwriteKind::Role)
        }],
        ..positioned_text_channel_info(Id::new(1), Id::new(2), "general", 0)
    }));

    assert_eq!(state.selected_channel_id(), None);
    assert!(!state.is_composing());
    assert!(state.channel_pane_entries().is_empty());
}

#[test]
fn debug_channel_visibility_reports_active_guild_counts() {
    // The fixture's channel denies VIEW_CHANNEL on @everyone, so it
    // shows up in the hidden bucket.
    let state = state_with_view_denied_channel();
    let stats = state.debug_channel_visibility();
    assert_eq!(
        stats,
        ChannelVisibilityStats {
            visible: 0,
            hidden: 1,
        }
    );
}

#[test]
fn submit_builtin_text_slash_commands_as_messages() {
    for (input, expected) in [
        ("/me waves", "_waves_"),
        ("/spoiler secret words", "||secret words||"),
        ("/shrug hello", r"hello ¯\_(ツ)_/¯"),
    ] {
        assert_eq!(
            submit_composer_text(input),
            Some(AppCommand::SendMessage {
                channel_id: Id::new(2),
                content: expected.to_owned(),
                reply_to: None,
                attachments: Vec::new(),
            }),
            "{input:?} should send transformed content",
        );
    }
}

#[test]
fn submit_builtin_tts_and_nick_commands_use_specific_app_commands() {
    assert_eq!(
        submit_composer_text("/tts hello there"),
        Some(AppCommand::SendTtsMessage {
            channel_id: Id::new(2),
            content: "hello there".to_owned(),
        })
    );

    for (input, expected_nick) in [("/nick Neo Prime", "Neo Prime"), ("/nick", "")] {
        let Some(AppCommand::UpdateUserProfile { update }) = submit_composer_text(input) else {
            panic!("{input:?} should update the current user's guild nickname");
        };
        assert_eq!(update.user_id, Id::new(10));
        assert_eq!(update.guild_id, Some(Id::new(1)));
        assert_eq!(
            update.guild.and_then(|guild| guild.nickname),
            Some(expected_nick.to_owned())
        );
    }
}

#[test]
fn builtin_command_picker_precedes_app_commands_and_blocks_gif_send() {
    let mut state = state_with_application_command(application_command("gif", Vec::new()));
    type_composer_text(&mut state, "/gi");
    let labels = state
        .composer_command_candidates()
        .into_iter()
        .map(|entry| entry.label)
        .collect::<Vec<_>>();

    assert_eq!(labels.first().map(String::as_str), Some("/gif"));

    state.clear_composer_input();
    type_composer_text(&mut state, "/gif cats");

    assert_eq!(state.submit_composer(), None);
    assert_eq!(
        state.toast_message().map(|toast| toast.text),
        Some("GIF slash commands are not supported in Concord yet")
    );
}

#[test]
fn no_match_emoji_query_closes_active_command_picker() {
    let mut state = state_with_application_command(application_command("poll", Vec::new()));
    type_composer_text(&mut state, "/po");
    assert_eq!(state.composer_command_query(), Some("/po"));

    state.insert_composer_text_at_cursor(" :qq");

    assert_eq!(state.composer_command_query(), None);
    assert_eq!(state.composer_emoji_query(), None);
}

#[test]
fn submit_slash_command_emits_direct_interaction_options() {
    let command = application_command(
        "echo",
        vec![
            application_command_option(3, "text", true, Vec::new()),
            application_command_option(5, "loud", false, Vec::new()),
        ],
    );
    let mut state = state_with_application_command(command);
    type_composer_text(&mut state, "/echo text:hello world loud:true");

    assert_slash_invocation(
        state.submit_composer(),
        "echo",
        "/echo text:hello world loud:true",
    );
}

#[test]
fn submit_slash_subcommand_emits_nested_interaction_options() {
    let command = application_command(
        "poll",
        vec![application_command_option(
            1,
            "create",
            false,
            vec![application_command_option(3, "question", true, Vec::new())],
        )],
    );
    let mut state = state_with_application_command(command);
    type_composer_text(&mut state, "/poll create question:favorite color");

    assert_slash_invocation(
        state.submit_composer(),
        "poll",
        "/poll create question:favorite color",
    );
}

#[test]
fn submit_slash_subcommand_accepts_single_option_shorthand() {
    for input in [
        "/anime search:naruto uzumaki",
        "/anime search: naruto uzumaki",
    ] {
        let command = application_command(
            "anime",
            vec![application_command_option(
                1,
                "search",
                false,
                vec![application_command_option(3, "query", true, Vec::new())],
            )],
        );
        let mut state = state_with_application_command(command);
        type_composer_text(&mut state, input);

        assert_slash_invocation(state.submit_composer(), "anime", input);
    }
}

#[test]
fn submit_slash_subcommand_rejects_empty_single_option_shorthand() {
    let command = application_command(
        "anime",
        vec![application_command_option(
            1,
            "search",
            false,
            vec![application_command_option(3, "query", false, Vec::new())],
        )],
    );
    let mut state = state_with_application_command(command);
    type_composer_text(&mut state, "/anime search:");

    assert_eq!(state.submit_composer(), None);
    assert_eq!(state.composer_input(), "/anime search:");
}

#[test]
fn submit_slash_subcommand_group_emits_nested_interaction_options() {
    let command = application_command(
        "mod",
        vec![application_command_option(
            2,
            "admin",
            false,
            vec![application_command_option(
                1,
                "ban",
                false,
                vec![
                    application_command_option(6, "user", true, Vec::new()),
                    application_command_option(3, "reason", false, Vec::new()),
                ],
            )],
        )],
    );
    let mut state = state_with_application_command(command);
    type_composer_text(&mut state, "/mod admin ban user:<@123> reason:spam links");

    assert_slash_invocation(
        state.submit_composer(),
        "mod",
        "/mod admin ban user:<@123> reason:spam links",
    );
}

#[test]
fn slash_option_value_pickers_insert_id_markup() {
    let command = application_command(
        "target",
        vec![
            application_command_option(6, "member", false, Vec::new()),
            application_command_option(8, "role", false, Vec::new()),
            application_command_option(7, "channel", false, Vec::new()),
        ],
    );

    for (input, visible, submitted) in [
        (
            "/target member:@s",
            "/target member:@Sally ",
            "/target member:<@20>",
        ),
        (
            "/target role:@mod",
            "/target role:@moderators ",
            "/target role:<@&30>",
        ),
        (
            "/target role:@ev",
            "/target role:@everyone ",
            "/target role:<@&1>",
        ),
        (
            "/target channel:#ru",
            "/target channel:#rules ",
            "/target channel:<#3>",
        ),
    ] {
        let mut state = state_with_command_mentions(command.clone());
        type_composer_text(&mut state, input);

        assert!(
            state.confirm_composer_mention(),
            "picker should confirm for {input}"
        );
        assert_eq!(state.composer_input(), visible);
        assert_slash_invocation(state.submit_composer(), "target", submitted);
    }
}

#[test]
fn slash_option_picker_marks_optional_and_required_options() {
    let command = application_command(
        "achievements",
        vec![
            application_command_option(6, "member", false, Vec::new()),
            application_command_option(3, "scope", true, Vec::new()),
        ],
    );
    let mut state = state_with_command_mentions(command);
    type_composer_text(&mut state, "/achievements ");

    let details = state
        .composer_command_candidates()
        .into_iter()
        .map(|candidate| (candidate.label, candidate.detail))
        .collect::<Vec<_>>();

    assert!(
        details
            .iter()
            .any(|(label, detail)| { label == "member:" && detail.starts_with("optional - ") })
    );
    assert!(
        details
            .iter()
            .any(|(label, detail)| { label == "scope:" && detail.starts_with("required - ") })
    );
}

#[test]
fn submit_slash_command_rejects_invalid_typed_options() {
    let command = application_command(
        "roll",
        vec![application_command_option(4, "sides", true, Vec::new())],
    );
    let mut state = state_with_application_command(command);
    type_composer_text(&mut state, "/roll sides:many");

    assert_eq!(state.submit_composer(), None);
    assert_eq!(state.composer_input(), "/roll sides:many");
}

#[test]
fn submit_slash_command_waits_for_required_options() {
    let command = application_command(
        "poll",
        vec![application_command_option(
            1,
            "create",
            false,
            vec![application_command_option(3, "question", true, Vec::new())],
        )],
    );
    let mut state = state_with_application_command(command);
    type_composer_text(&mut state, "/poll create");

    assert_eq!(state.submit_composer(), None);
    assert_eq!(state.composer_input(), "/poll create");
}

#[test]
fn submit_slash_command_preserves_unparsed_free_text() {
    let command = application_command(
        "echo",
        vec![application_command_option(3, "text", false, Vec::new())],
    );
    let mut state = state_with_application_command(command);
    type_composer_text(&mut state, "/echo hello world");

    assert_eq!(state.submit_composer(), None);
    assert_eq!(state.composer_input(), "/echo hello world");
}

#[test]
fn confirming_slash_command_immediately_shows_subcommands() {
    let command = application_command(
        "poll",
        vec![application_command_option(
            1,
            "create",
            false,
            vec![application_command_option(3, "question", true, Vec::new())],
        )],
    );
    let mut state = state_with_application_command(command);
    type_composer_text(&mut state, "/po");

    assert!(state.confirm_composer_command());

    assert_eq!(state.composer_input(), "/poll ");
    let candidates = state.composer_command_candidates();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].label, "create");
    assert_eq!(candidates[0].replacement, "create ");
}

#[test]
fn subcommand_picker_hides_used_leaf_options() {
    let command = application_command(
        "poll",
        vec![application_command_option(
            1,
            "create",
            false,
            vec![
                application_command_option(3, "question", true, Vec::new()),
                application_command_option(4, "duration", false, Vec::new()),
            ],
        )],
    );
    let mut state = state_with_application_command(command);
    type_composer_text(&mut state, "/poll create question:favorite color ");

    let labels = state
        .composer_command_candidates()
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();

    assert!(!labels.iter().any(|label| label == "question:"));
    assert!(labels.iter().any(|label| label == "duration:"));
}

#[test]
fn subcommand_group_picker_hides_used_leaf_options() {
    let command = application_command(
        "mod",
        vec![application_command_option(
            2,
            "admin",
            false,
            vec![application_command_option(
                1,
                "ban",
                false,
                vec![
                    application_command_option(6, "user", true, Vec::new()),
                    application_command_option(3, "reason", false, Vec::new()),
                ],
            )],
        )],
    );
    let mut state = state_with_application_command(command);
    type_composer_text(&mut state, "/mod admin ban user:<@123> ");

    let labels = state
        .composer_command_candidates()
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();

    assert!(!labels.iter().any(|label| label == "user:"));
    assert!(labels.iter().any(|label| label == "reason:"));
}

#[test]
fn command_picker_detail_includes_application_name() {
    let command = application_command(
        "echo",
        vec![application_command_option(3, "text", false, Vec::new())],
    );
    let mut state = state_with_application_command(command);
    type_composer_text(&mut state, "/e");

    let candidates = state.composer_command_candidates();

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].label, "/echo");
    assert_eq!(candidates[0].detail, "TestBot - echo command");
}

#[test]
fn typing_at_sign_at_start_opens_mention_picker() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');

    assert_eq!(state.composer_mention_query(), Some(""));
    assert!(!state.composer_mention_candidates().is_empty());
}

#[test]
fn typing_at_sign_after_letter_does_not_trigger_picker() {
    // `me@` should not open the picker because the user is mid-word.
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    for ch in "me".chars() {
        state.push_composer_char(ch);
    }
    state.push_composer_char('@');

    assert_eq!(state.composer_mention_query(), None);
    assert_eq!(state.composer_input(), "me@");
}

#[test]
fn typing_after_at_filters_candidates_by_substring() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('s');

    assert_eq!(state.composer_mention_query(), Some("s"));
    let names: Vec<_> = state
        .composer_mention_candidates()
        .into_iter()
        .map(|entry| entry.display_name)
        .collect();
    assert!(
        names.iter().all(|name| name.to_lowercase().contains('s')),
        "expected only `s` matches, got {names:?}"
    );
    assert!(names.iter().any(|name| name == "Sally"));
    assert!(names.iter().any(|name| name == "Sammy"));
    assert!(!names.iter().any(|name| name == "Bob"));

    state.push_event(AppEvent::GuildMemberUpsert {
        guild_id: Id::new(1),
        member: member_with_username(Id::new(30), "Offline Sally", "offlinesally"),
    });
    let names: Vec<_> = state
        .composer_mention_candidates()
        .into_iter()
        .map(|entry| entry.display_name)
        .collect();
    assert!(names.iter().any(|name| name == "Offline Sally"));
}

#[test]
fn backspace_shrinks_query_then_closes_picker() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('s');

    state.pop_composer_char();
    assert_eq!(state.composer_mention_query(), Some(""));
    assert_eq!(state.composer_input(), "@");

    state.pop_composer_char();
    assert_eq!(state.composer_mention_query(), None);
    assert_eq!(state.composer_input(), "");
}

#[test]
fn confirm_inserts_display_name_and_submit_expands_to_wire_format() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('s');
    // First match (alphabetical within "starts_with s") is "Sally" (id 20).
    assert!(state.confirm_composer_mention());
    assert_eq!(state.composer_input(), "@Sally ");
    assert_eq!(state.composer_mention_query(), None);

    state.push_composer_char('h');
    state.push_composer_char('i');

    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "<@20> hi".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn confirm_mention_in_middle_keeps_trailing_text() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    for value in "hello @sworld".chars() {
        state.push_composer_char(value);
    }
    for _ in 0.."world".len() {
        state.move_composer_cursor_left();
    }

    assert_eq!(state.composer_mention_query(), Some("s"));
    assert!(state.confirm_composer_mention());

    assert_eq!(state.composer_input(), "hello @Sally world");
    assert_eq!(state.composer_cursor_byte_index(), "hello @Sally ".len());
    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "hello <@20> world".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn role_and_channel_mentions_expand_to_wire_format() {
    let command = application_command("noop", Vec::new());
    for (input, query, visible, wire) in [
        ("@mod", Some("mod"), "@moderators ", "<@&30>"),
        ("#ru", Some("ru"), "#rules ", "<#3>"),
    ] {
        let mut state = state_with_command_mentions(command.clone());
        for value in input.chars() {
            state.push_composer_char(value);
        }

        assert_eq!(state.composer_mention_query(), query);
        if input == "@mod" {
            let moderator = state
                .composer_mention_candidates()
                .into_iter()
                .find(|entry| entry.display_name == "moderators")
                .expect("moderator role should be suggested");
            assert_eq!(moderator.role_color, Some(0xFFAA00));
            assert_eq!(moderator.visible_text(), "@moderators");
        }
        assert!(state.confirm_composer_mention());
        assert_eq!(state.composer_input(), visible);
        assert_eq!(
            state.submit_composer(),
            Some(AppCommand::SendMessage {
                channel_id: Id::new(2),
                content: wire.to_owned(),
                reply_to: None,
                attachments: Vec::new(),
            })
        );
    }
}

#[test]
fn role_mention_picker_avoids_duplicate_everyone_prefix() {
    let command = application_command("noop", Vec::new());
    let mut state = state_with_command_mentions(command);
    for value in "@ev".chars() {
        state.push_composer_char(value);
    }

    let everyone = state
        .composer_mention_candidates()
        .into_iter()
        .find(|entry| entry.display_name == "@everyone")
        .expect("@everyone should match without typing the second @");
    assert_eq!(everyone.display_label(), "everyone");
    assert_eq!(everyone.visible_text(), "@everyone");
    assert!(state.confirm_composer_mention());
    assert_eq!(state.composer_input(), "@everyone ");
    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "@everyone".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn cancel_composer_clears_pending_upload_state() {
    let mut attachments = state_with_channel_tree();
    attachments.focus_pane(FocusPane::Channels);
    attachments.confirm_selected_channel();
    attachments.start_composer();
    attachments.add_pending_composer_attachments(vec![MessageAttachmentUpload::from_path(
        "/tmp/cat.png".into(),
        "cat.png".to_owned(),
        2_048,
    )]);

    attachments.cancel_composer();

    assert_eq!(attachments.pending_composer_attachments(), &[]);

    let mut processing = state_with_messages(1);
    processing.start_composer();
    assert!(processing.begin_clipboard_paste());

    processing.cancel_composer();

    assert!(!processing.clipboard_paste_pending());
}

#[test]
fn pending_attachments_are_capped_at_upload_limit() {
    let mut state = state_with_channel_tree();
    state.focus_pane(FocusPane::Channels);
    state.confirm_selected_channel();
    state.start_composer();
    let attachments = (0..crate::discord::MAX_UPLOAD_ATTACHMENT_COUNT + 2)
        .map(|index| {
            MessageAttachmentUpload::from_path(
                format!("/tmp/{index}.txt").into(),
                format!("{index}.txt"),
                1,
            )
        })
        .collect();

    state.add_pending_composer_attachments(attachments);

    assert_eq!(
        state.pending_composer_attachments().len(),
        crate::discord::MAX_UPLOAD_ATTACHMENT_COUNT
    );
}

#[test]
fn move_selection_navigates_filtered_list() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('s');
    let candidates = state.composer_mention_candidates();
    assert!(candidates.len() >= 2);

    state.move_active_composer_picker_selection(1);
    assert_eq!(state.composer_mention_selected(), 1);

    state.move_active_composer_picker_selection(-5);
    assert_eq!(state.composer_mention_selected(), 0);
}

#[test]
fn mention_picker_keeps_more_than_visible_candidates_selectable() {
    let mut state = state_with_writable_channel_and_members();
    for index in 0..10 {
        state.push_event(AppEvent::GuildMemberUpsert {
            guild_id: Id::new(1),
            member: member_with_username(
                Id::new(100 + index),
                format!("Scroll {index:02}"),
                format!("scroll{index:02}"),
            ),
        });
    }
    state.start_composer();
    for ch in "@sc".chars() {
        state.push_composer_char(ch);
    }

    let candidates = state.composer_mention_candidates();
    assert!(
        candidates.len() > 8,
        "picker should keep every matching candidate, got {candidates:?}"
    );

    state.move_active_composer_picker_selection(9);

    assert_eq!(state.composer_mention_selected(), 9);
    assert!(state.confirm_composer_mention());
    assert_eq!(state.composer_input(), "@Scroll 09 ");
}

#[test]
fn composer_pickers_keep_selection_moving_when_reversing_scroll() {
    const VISIBLE: usize = 8;

    let mut mention_state = state_with_writable_channel_and_members();
    for index in 0..16 {
        mention_state.push_event(AppEvent::GuildMemberUpsert {
            guild_id: Id::new(1),
            member: member_with_username(
                Id::new(200 + index),
                format!("Scroll {index:02}"),
                format!("scroll{index:02}"),
            ),
        });
    }
    mention_state.start_composer();
    type_composer_text(&mut mention_state, "@scroll");

    let mention_count = mention_state.composer_mention_candidates().len();
    mention_state.move_active_composer_picker_selection(10);
    let mention_start = mention_state.composer_mention_window_start(VISIBLE, mention_count);
    mention_state.move_active_composer_picker_selection(-1);

    assert_eq!(
        mention_state.composer_mention_window_start(VISIBLE, mention_count),
        mention_start,
        "moving upward once should move the picker cursor before scrolling the viewport"
    );
    assert_eq!(
        mention_state.composer_mention_selected() - mention_start,
        3,
        "mention picker cursor should not remain pinned to the bottom row"
    );
    let cramped_mention_start = mention_state.composer_mention_window_start(3, mention_count);
    assert_eq!(
        mention_state.composer_mention_selected() - cramped_mention_start,
        1,
        "cramped mention picker cursor should stay off the bottom row"
    );

    let mut emoji_state = state_with_writable_channel();
    emoji_state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(1),
        emojis: (0..16)
            .map(|index| {
                CustomEmojiInfo::test(Id::new(400 + index), format!("overflow_{index:02}"))
            })
            .collect(),
    });
    emoji_state.start_composer();
    type_composer_text(&mut emoji_state, ":ov");

    let emoji_count = emoji_state.composer_emoji_candidates().len();
    emoji_state.move_active_composer_picker_selection(10);
    let emoji_start = emoji_state.composer_emoji_window_start(VISIBLE, emoji_count);
    emoji_state.move_active_composer_picker_selection(-1);

    assert_eq!(
        emoji_state.composer_emoji_window_start(VISIBLE, emoji_count),
        emoji_start,
        "moving upward once should move the emoji cursor before scrolling the viewport"
    );
    assert_eq!(
        emoji_state.composer_emoji_selected() - emoji_start,
        3,
        "emoji picker cursor should not remain pinned to the bottom row"
    );

    let mut command_state = state_with_writable_channel();
    command_state.push_event(AppEvent::ApplicationCommandsLoaded {
        guild_id: Some(Id::new(1)),
        commands: (0..16)
            .map(|index| {
                ApplicationCommandInfo::test(Id::new(300 + index), format!("scroll{index:02}"))
            })
            .collect(),
    });
    command_state.start_composer();
    type_composer_text(&mut command_state, "/scroll");

    let command_count = command_state.composer_command_candidates().len();
    command_state.move_active_composer_picker_selection(10);
    let command_start = command_state.composer_command_window_start(VISIBLE, command_count);
    command_state.move_active_composer_picker_selection(-1);

    assert_eq!(
        command_state.composer_command_window_start(VISIBLE, command_count),
        command_start,
        "moving upward once should move the command cursor before scrolling the viewport"
    );
    assert_eq!(
        command_state.composer_command_selected() - command_start,
        3,
        "slash command picker cursor should not remain pinned to the bottom row"
    );
}

#[test]
fn cancel_picker_keeps_typed_text() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('s');

    state.cancel_active_composer_picker();
    assert_eq!(state.composer_mention_query(), None);
    assert_eq!(state.composer_input(), "@s");
}

#[test]
fn typing_colon_plus_two_letters_opens_emoji_picker() {
    let mut state = state_with_writable_channel();
    state.start_composer();
    state.push_composer_char(':');
    state.push_composer_char('h');

    assert_eq!(state.composer_emoji_query(), None);

    state.push_composer_char('e');

    assert_eq!(state.composer_emoji_query(), Some("he"));
    let shortcodes: Vec<_> = state
        .composer_emoji_candidates()
        .into_iter()
        .map(|entry| entry.shortcode)
        .collect();
    assert!(
        shortcodes.iter().any(|shortcode| shortcode == "heart"),
        "expected `heart` in emoji candidates, got {shortcodes:?}"
    );
}

#[test]
fn unavailable_custom_emojis_stay_visible_but_not_selectable() {
    for (label, query, shortcode, wire_format, set_capability) in [
        (
            "animated emoji without Nitro",
            ":pa",
            "party_time",
            "<a:party_time:50>",
            Some(false),
        ),
        (
            "animated emoji with unknown Nitro state",
            ":pa",
            "party_time",
            "<a:party_time:50>",
            None,
        ),
        (
            "server-unavailable static emoji",
            ":go",
            "gone",
            "<:gone:51>",
            None,
        ),
    ] {
        let mut state = state_with_custom_emojis();
        if let Some(has_nitro) = set_capability {
            state.push_event(AppEvent::CurrentUserCapabilities { has_nitro });
        }
        state.start_composer();
        for ch in query.chars() {
            state.push_composer_char(ch);
        }

        let candidates = state.composer_emoji_candidates();
        let entry = candidates
            .iter()
            .find(|entry| entry.shortcode == shortcode)
            .unwrap_or_else(|| panic!("{label} should stay visible in suggestions"));

        assert!(!entry.available, "{label} should be unavailable");
        assert!(
            !entry.available_as_link,
            "{label} should not be link-available"
        );
        assert_eq!(entry.wire_format.as_deref(), Some(wire_format));
        assert!(
            !state.confirm_composer_emoji(),
            "{label} should not confirm"
        );
        assert_eq!(state.composer_input(), query);
    }
}

#[test]
fn active_emoji_candidates_refresh_when_nitro_capability_changes() {
    let mut state = state_with_custom_emojis();
    state.start_composer();
    for ch in ":pa".chars() {
        state.push_composer_char(ch);
    }

    let before = state
        .composer_emoji_candidates()
        .into_iter()
        .find(|entry| entry.shortcode == "party_time")
        .expect("animated custom emoji should stay visible in suggestions");
    assert!(!before.available);
    assert!(!before.available_as_link);

    state.push_event(AppEvent::CurrentUserCapabilities { has_nitro: true });

    let after = state
        .composer_emoji_candidates()
        .into_iter()
        .find(|entry| entry.shortcode == "party_time")
        .expect("active emoji suggestions should refresh after capability changes");
    assert!(after.available);
    assert!(!after.available_as_link);
}

#[test]
fn emoji_picker_keeps_more_than_visible_candidates_selectable() {
    let mut state = state_with_writable_channel();
    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(1),
        emojis: (0..10)
            .map(|index| {
                CustomEmojiInfo::test(Id::new(100 + index), format!("overflow_{index:02}"))
            })
            .collect(),
    });
    state.start_composer();
    for ch in ":ov".chars() {
        state.push_composer_char(ch);
    }

    let candidates = state.composer_emoji_candidates();
    assert!(
        candidates.len() > 8,
        "picker should keep every matching candidate, got {candidates:?}"
    );

    state.move_active_composer_picker_selection(9);

    assert_eq!(state.composer_emoji_selected(), 9);
    assert!(state.confirm_composer_emoji());
    assert_eq!(state.composer_input(), ":overflow_09: ");
    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "<:overflow_09:109>".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn custom_emoji_submit_keeps_readable_text_and_sends_wire_format() {
    let mut state = state_with_custom_emojis();
    state.push_event(AppEvent::CurrentUserCapabilities { has_nitro: true });
    state.start_composer();
    for ch in ":pa".chars() {
        state.push_composer_char(ch);
    }

    let entry = state
        .composer_emoji_candidates()
        .into_iter()
        .find(|entry| entry.shortcode == "party_time")
        .expect("animated custom emoji should stay visible in suggestions");
    assert!(entry.available);
    assert!(!entry.available_as_link);

    assert!(state.confirm_composer_emoji());

    assert_eq!(state.composer_input(), ":party_time: ");
    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "<a:party_time:50>".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );

    let guild_id = Id::new(1);
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id,
        emojis: vec![CustomEmojiInfo::test(Id::new(60), "wave")],
    });
    state.start_composer();
    for ch in ":wa".chars() {
        state.push_composer_char(ch);
    }

    assert!(state.confirm_composer_emoji());

    assert_eq!(state.composer_input(), ":wave: ");
    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "<:wave:60>".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn animated_current_guild_emoji_sends_link_without_nitro_when_enabled() {
    let mut state = state_with_custom_emojis();
    state.options.composer_options.emojis_as_links = true;
    state.push_event(AppEvent::CurrentUserCapabilities { has_nitro: false });
    state.start_composer();
    for ch in ":pa".chars() {
        state.push_composer_char(ch);
    }

    let entry = state
        .composer_emoji_candidates()
        .into_iter()
        .find(|entry| entry.shortcode == "party_time")
        .expect("animated custom emoji should be suggested as a link fallback");
    assert!(entry.available);
    assert!(entry.available_as_link);

    assert!(state.confirm_composer_emoji());

    assert_eq!(state.composer_input(), ":party_time: ");
    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "[party_time](https://cdn.discordapp.com/emojis/50.gif?size=48&name=party_time&lossless=true)".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn nitro_user_sends_foreign_custom_emojis_as_native_markup() {
    let mut state = state_with_custom_emojis();
    push_foreign_custom_emojis(&mut state);
    state.push_event(AppEvent::CurrentUserCapabilities { has_nitro: true });
    state.start_composer();
    for ch in ":wa".chars() {
        state.push_composer_char(ch);
    }

    assert!(state.confirm_composer_emoji());

    assert_eq!(state.composer_input(), ":wave_foreign: ");
    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "<:wave_foreign:60>".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );

    let mut state = state_with_custom_emojis();
    push_foreign_custom_emojis(&mut state);
    state.push_event(AppEvent::CurrentUserCapabilities { has_nitro: true });
    state.start_composer();
    for ch in ":da".chars() {
        state.push_composer_char(ch);
    }

    let entry = state
        .composer_emoji_candidates()
        .into_iter()
        .find(|entry| entry.shortcode == "dance_foreign")
        .expect("foreign animated emoji should be suggested as a link fallback");
    assert!(entry.available);
    assert!(!entry.available_as_link);

    assert!(state.confirm_composer_emoji());

    assert_eq!(state.composer_input(), ":dance_foreign: ");
    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "<a:dance_foreign:61>".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn foreign_custom_emoji_uses_link_fallback_without_nitro_when_enabled() {
    let mut state = state_with_custom_emojis();
    push_foreign_custom_emojis(&mut state);
    state.options.composer_options.emojis_as_links = true;
    state.push_event(AppEvent::CurrentUserCapabilities { has_nitro: false });
    state.start_composer();
    for ch in ":wa".chars() {
        state.push_composer_char(ch);
    }

    let entry = state
        .composer_emoji_candidates()
        .into_iter()
        .find(|entry| entry.shortcode == "wave_foreign")
        .expect("foreign custom emoji should be suggested as a link fallback");
    assert!(entry.available);
    assert!(entry.available_as_link);

    assert!(state.confirm_composer_emoji());

    assert_eq!(state.composer_input(), ":wave_foreign: ");
    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "[wave_foreign](https://cdn.discordapp.com/emojis/60.png?size=48&name=wave_foreign&lossless=true)".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn foreign_animated_emoji_uses_link_fallback_without_nitro_when_enabled() {
    let mut state = state_with_custom_emojis();
    push_foreign_custom_emojis(&mut state);
    state.options.composer_options.emojis_as_links = true;
    state.push_event(AppEvent::CurrentUserCapabilities { has_nitro: false });
    state.start_composer();
    for ch in ":da".chars() {
        state.push_composer_char(ch);
    }

    let entry = state
        .composer_emoji_candidates()
        .into_iter()
        .find(|entry| entry.shortcode == "dance_foreign")
        .expect("foreign animated emoji should be suggested as a link fallback");
    assert!(entry.available);
    assert!(entry.available_as_link);

    assert!(state.confirm_composer_emoji());

    assert_eq!(state.composer_input(), ":dance_foreign: ");
    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "[dance_foreign](https://cdn.discordapp.com/emojis/61.gif?size=48&name=dance_foreign&lossless=true)".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn foreign_custom_emoji_stays_hidden_without_nitro_or_link_fallback() {
    let mut state = state_with_custom_emojis();
    push_foreign_custom_emojis(&mut state);
    state.push_event(AppEvent::CurrentUserCapabilities { has_nitro: false });
    state.start_composer();
    for ch in ":wa".chars() {
        state.push_composer_char(ch);
    }

    assert!(
        state
            .composer_emoji_candidates()
            .iter()
            .all(|entry| entry.shortcode != "wave_foreign")
    );
}

#[test]
fn submit_expands_mention_and_following_custom_emoji_without_stale_ranges() {
    let mut state = state_with_writable_channel_and_members();
    state.push_event(AppEvent::CurrentUserCapabilities { has_nitro: true });
    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(1),
        emojis: vec![CustomEmojiInfo {
            animated: true,
            ..CustomEmojiInfo::test(Id::new(50), "party_time")
        }],
    });
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('s');
    assert!(state.confirm_composer_mention());
    for ch in ":pa".chars() {
        state.push_composer_char(ch);
    }
    assert!(state.confirm_composer_emoji());

    assert_eq!(state.composer_input(), "@Sally :party_time: ");
    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "<@20> <a:party_time:50>".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn confirm_emoji_inserts_unicode_and_closes_picker() {
    let mut state = state_with_writable_channel();
    state.start_composer();
    for ch in ":heart".chars() {
        state.push_composer_char(ch);
    }

    assert_eq!(state.composer_emoji_query(), Some("heart"));
    assert!(state.confirm_composer_emoji());

    assert_eq!(state.composer_input(), "❤️ ");
    assert_eq!(state.composer_emoji_query(), None);
}

#[test]
fn submit_expands_known_emoji_shortcodes_and_keeps_unknown_text() {
    let mut state = state_with_writable_channel();
    state.start_composer();
    for ch in "take :heart: :unknown:".chars() {
        state.push_composer_char(ch);
    }

    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "take ❤️ :unknown:".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn submit_preserves_empty_shortcode_colon_runs() {
    for (input, expected) in [
        ("::", "::"),
        (":::", ":::"),
        ("::::heart:", ":::❤️"),
        ("start :::heart: end", "start ::❤️ end"),
    ] {
        let mut state = state_with_writable_channel();
        state.start_composer();
        for ch in input.chars() {
            state.push_composer_char(ch);
        }

        assert_eq!(
            state.submit_composer(),
            Some(AppCommand::SendMessage {
                channel_id: Id::new(2),
                content: expected.to_owned(),
                reply_to: None,
                attachments: Vec::new(),
            }),
            "empty emoji shortcode spans should preserve {input:?}",
        );
    }
}

#[test]
fn submit_keeps_custom_emoji_markup_literal() {
    let mut state = state_with_writable_channel();
    state.start_composer();
    for ch in "custom <:heart:123> <a:party:456> :heart:".chars() {
        state.push_composer_char(ch);
    }

    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(2),
            content: "custom <:heart:123> <a:party:456> ❤️".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}

#[test]
fn no_match_emoji_query_does_not_open_hidden_picker() {
    let mut state = state_with_writable_channel();
    state.start_composer();
    for ch in ":qq".chars() {
        state.push_composer_char(ch);
    }

    assert_eq!(state.composer_emoji_query(), None);
}

#[test]
fn uppercase_emoji_query_matches_lowercase_shortcodes() {
    let mut state = state_with_writable_channel();
    state.start_composer();
    for ch in ":HE".chars() {
        state.push_composer_char(ch);
    }

    let shortcodes: Vec<_> = state
        .composer_emoji_candidates()
        .into_iter()
        .map(|entry| entry.shortcode)
        .collect();
    assert!(
        shortcodes.iter().any(|shortcode| shortcode == "heart"),
        "expected uppercase query to match `heart`, got {shortcodes:?}"
    );
}

#[test]
fn cancel_emoji_picker_keeps_typed_text() {
    let mut state = state_with_writable_channel();
    state.start_composer();
    for ch in ":he".chars() {
        state.push_composer_char(ch);
    }

    state.cancel_active_composer_picker();

    assert_eq!(state.composer_emoji_query(), None);
    assert_eq!(state.composer_input(), ":he");
}

#[test]
fn typing_footer_resolves_one_user_to_alias() {
    let mut state = state_with_writable_channel_and_members();
    let channel_id = Id::new(2);
    let user_id = Id::new(20);
    state.push_event(AppEvent::TypingStart {
        channel_id,
        user_id,
        display_name: Some("Live Nick".to_owned()),
    });

    assert_eq!(
        state.typing_footer_for_selected_channel(),
        Some("Live Nick is typing\u{2026}".to_owned())
    );

    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id,
        message_id: Id::new(100),
        author_id: user_id,
        author: "Live Nick".to_owned(),
        content: Some("sent".to_owned()),
        ..guild_message_create_fixture()
    }));

    assert_eq!(state.typing_footer_for_selected_channel(), None);
}

#[test]
fn typing_footer_excludes_current_user() {
    let mut state = state_with_writable_channel_and_members();
    // user_id 10 is the local user in the fixture's READY event.
    state.push_event(AppEvent::TypingStart {
        channel_id: Id::new(2),
        user_id: Id::new(10),
        display_name: Some("Local User".to_owned()),
    });

    assert_eq!(state.typing_footer_for_selected_channel(), None);
}

#[test]
fn typing_footer_pluralizes_at_two_three_and_more_typers() {
    let mut state = state_with_writable_channel_and_members();
    state.push_event(AppEvent::TypingStart {
        channel_id: Id::new(2),
        user_id: Id::new(20),
        display_name: None,
    });
    state.push_event(AppEvent::TypingStart {
        channel_id: Id::new(2),
        user_id: Id::new(21),
        display_name: None,
    });
    let footer = state
        .typing_footer_for_selected_channel()
        .expect("two typers should produce a footer");
    // Newest typer first, so id 21 (Sammy) leads.
    assert_eq!(footer, "Sammy and Sally are typing\u{2026}");

    state.push_event(AppEvent::TypingStart {
        channel_id: Id::new(2),
        user_id: Id::new(22),
        display_name: None,
    });
    let footer = state
        .typing_footer_for_selected_channel()
        .expect("three typers should produce a footer");
    assert_eq!(footer, "Bob, Sammy, and Sally are typing\u{2026}");

    state.push_event(AppEvent::TypingStart {
        channel_id: Id::new(2),
        user_id: Id::new(23),
        display_name: None,
    });
    let footer = state
        .typing_footer_for_selected_channel()
        .expect("four typers should still produce a footer");
    assert_eq!(footer, "Several people are typing\u{2026}");
}

#[test]
fn picker_matches_alias_with_multibyte_query() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('A');

    let candidates = state.composer_mention_candidates();
    assert!(
        candidates.iter().any(|entry| entry.display_name == "Alias"),
        "alias `Alias` must surface when typing `A`, got {:?}",
        candidates
            .iter()
            .map(|c| c.display_name.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn picker_matches_username_when_alias_does_not_contain_query() {
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('A');
    state.push_composer_char('l');

    let candidates = state.composer_mention_candidates();
    assert!(
        candidates
            .iter()
            .any(|entry| entry.username.as_deref() == Some("Alias123")),
        "username `Alias123` must match query `Al`, got {:?}",
        candidates
            .iter()
            .map(|c| (c.display_name.clone(), c.username.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn picker_ranks_alias_prefix_above_username_prefix() {
    // `s` should put display-name matches (Sally, Sammy) before any
    // username-only match. We don't have a username-only `s` match in the
    // fixture, but we still verify alias rows come first when both have
    // candidates.
    let mut state = state_with_writable_channel_and_members();
    state.start_composer();
    state.push_composer_char('@');
    state.push_composer_char('s');

    let candidates = state.composer_mention_candidates();
    let names: Vec<_> = candidates.iter().map(|c| c.display_name.clone()).collect();
    assert!(
        names
            .first()
            .map(|name| name.starts_with('S'))
            .unwrap_or(false),
        "alias-prefix matches must lead the list, got {names:?}"
    );
}

#[test]
fn composer_sends_to_opened_thread_channel() {
    let mut state = state_with_thread_created_message();
    state.focus_pane(FocusPane::Messages);
    state.activate_message_action_kind(MessageActionKind::OpenThread);

    state.start_composer();
    state.push_composer_char('h');
    state.push_composer_char('i');

    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::SendMessage {
            channel_id: Id::new(10),
            content: "hi".to_owned(),
            reply_to: None,
            attachments: Vec::new(),
        })
    );
}
