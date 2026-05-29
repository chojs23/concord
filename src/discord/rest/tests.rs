use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{TimeZone, Utc};

use crate::discord::ids::{
    Id,
    marker::{ApplicationMarker, ChannelMarker, EmojiMarker, GuildMarker, UserMarker},
};

use crate::{
    AppError,
    discord::{
        ApplicationCommandInfo, ApplicationCommandInteraction, ApplicationCommandInteractionOption,
        ChannelInfo, MAX_UPLOAD_FILE_BYTES, MessageAttachmentUpload, MessageSearchAuthorType,
        MessageSearchHas, MessageSearchQuery, ReactionEmoji,
        rest::{
            ForumPostPage, ForumSearchSort, REACTION_USERS_MAX_PAGES,
            application_command_interaction_body, application_command_option_body,
            is_search_index_warming, merge_forum_pages, message_multipart_form,
            message_request_body, message_search_date_snowflake_bounds,
            message_search_query_params, mute_request_body, next_reaction_users_after,
            parse_application_command_index, parse_forum_first_messages, parse_forum_threads,
            parse_user_profile_response, poll_vote_request_body, reaction_route_component,
            upload_content_type, validate_message_content, validate_message_payload,
        },
    },
};

#[test]
fn rejects_invalid_message_content() {
    let error = validate_message_content("   ").expect_err("blank messages must fail");
    assert!(matches!(error, AppError::EmptyMessageContent));

    let content = "x".repeat(2_001);
    let error = validate_message_content(&content).expect_err("oversized message must fail");
    assert!(matches!(error, AppError::MessageTooLong { len: 2_001 }));
}

#[test]
fn validates_attachment_only_message_payload() {
    let attachments = vec![MessageAttachmentUpload::from_path(
        "/tmp/cat.png".into(),
        "cat.png".to_owned(),
        2_048,
    )];

    validate_message_payload("   ", &attachments).expect("file-only messages should be valid");

    let body = message_request_body("", Some(Id::new(44)), &attachments);
    assert_eq!(body["content"], "");
    assert_eq!(body["message_reference"]["message_id"], "44");
    assert_eq!(body["attachments"][0]["id"], 0);
    assert_eq!(body["attachments"][0]["filename"], "cat.png");
}

#[test]
fn application_command_interaction_body_nests_subcommand_options_for_guild_command() {
    let interaction = ApplicationCommandInteraction {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        command: ApplicationCommandInfo {
            application_id: Id::<ApplicationMarker>::new(200),
            version: "1".to_owned(),
            application_name: Some("ModBot".to_owned()),
            description: "moderation".to_owned(),
            raw: serde_json::json!({ "name": "mod", "guild_id": "1" }),
            ..ApplicationCommandInfo::test(Id::<ApplicationMarker>::new(100), "mod")
        },
        options: vec![ApplicationCommandInteractionOption {
            kind: 2,
            name: "admin".to_owned(),
            value: None,
            options: vec![ApplicationCommandInteractionOption {
                kind: 1,
                name: "ban".to_owned(),
                value: None,
                options: vec![ApplicationCommandInteractionOption {
                    kind: 6,
                    name: "user".to_owned(),
                    value: Some(serde_json::json!("123")),
                    options: Vec::new(),
                }],
            }],
        }],
    };

    let body = application_command_interaction_body(&interaction, "session");

    assert_eq!(
        body["data"]["options"],
        serde_json::json!([
            {
                "type": 2,
                "name": "admin",
                "options": [
                    {
                        "type": 1,
                        "name": "ban",
                        "options": [
                            { "type": 6, "name": "user", "value": "123" }
                        ]
                    }
                ]
            }
        ])
    );
    assert_eq!(body["data"]["guild_id"], "1");
    assert!(body["data"]["options"][0].get("value").is_none());
    assert!(
        body["data"]["options"][0]["options"][0]
            .get("value")
            .is_none()
    );
}

#[test]
fn application_command_interaction_body_omits_data_guild_id_for_global_command() {
    let interaction = ApplicationCommandInteraction {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        command: ApplicationCommandInfo {
            application_id: Id::<ApplicationMarker>::new(200),
            version: "1".to_owned(),
            application_name: Some("MusicBot".to_owned()),
            description: "search music".to_owned(),
            raw: serde_json::json!({
                "id": "100",
                "application_id": "200",
                "name": "search",
                "version": "1",
                "integration_types": [0],
            }),
            ..ApplicationCommandInfo::test(Id::<ApplicationMarker>::new(100), "search")
        },
        options: Vec::new(),
    };

    let body = application_command_interaction_body(&interaction, "session");

    assert_eq!(body["guild_id"], "1");
    assert!(body["data"].get("guild_id").is_none());
}

#[test]
fn application_command_index_joins_application_names() {
    let commands = parse_application_command_index(&serde_json::json!({
        "applications": [
            { "id": "200", "name": "PollBot" }
        ],
        "application_commands": [
            {
                "id": "100",
                "application_id": "200",
                "version": "1",
                "name": "poll",
                "description": "Create a poll",
                "options": []
            }
        ]
    }));

    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].application_name.as_deref(), Some("PollBot"));
}

#[test]
fn application_command_option_body_keeps_value_and_options_exclusive() {
    let option = ApplicationCommandInteractionOption {
        kind: 3,
        name: "text".to_owned(),
        value: Some(serde_json::json!("hello")),
        options: vec![ApplicationCommandInteractionOption {
            kind: 3,
            name: "nested".to_owned(),
            value: Some(serde_json::json!("ignored")),
            options: Vec::new(),
        }],
    };

    let body = application_command_option_body(&option);

    assert_eq!(body["value"], serde_json::json!("hello"));
    assert!(body.get("options").is_none());
}

#[test]
fn rejects_attachment_upload_limits() {
    let too_large_file = vec![MessageAttachmentUpload::from_path(
        "/tmp/large.bin".into(),
        "large.bin".to_owned(),
        MAX_UPLOAD_FILE_BYTES + 1,
    )];
    let error =
        validate_message_payload("", &too_large_file).expect_err("oversized attachment must fail");
    assert!(matches!(error, AppError::AttachmentTooLarge { .. }));

    let too_large_total = vec![
        MessageAttachmentUpload::from_path(
            "/tmp/a.bin".into(),
            "a.bin".to_owned(),
            MAX_UPLOAD_FILE_BYTES - 1,
        ),
        MessageAttachmentUpload::from_path(
            "/tmp/b.bin".into(),
            "b.bin".to_owned(),
            MAX_UPLOAD_FILE_BYTES - 1,
        ),
        MessageAttachmentUpload::from_path(
            "/tmp/c.bin".into(),
            "c.bin".to_owned(),
            MAX_UPLOAD_FILE_BYTES - 1,
        ),
    ];
    let error = validate_message_payload("", &too_large_total)
        .expect_err("oversized attachment total must fail");
    assert!(matches!(error, AppError::AttachmentsTooLarge { .. }));
}

#[tokio::test]
async fn multipart_form_rechecks_current_file_size() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after unix epoch")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("concord-rest-{unique}"));
    std::fs::create_dir_all(&directory).expect("temp upload directory can be created");
    let path = directory.join("changed.bin");
    std::fs::write(&path, [0_u8]).expect("small temp file can be written");
    let attachment = MessageAttachmentUpload::from_path(path.clone(), "changed.bin".to_owned(), 1);
    std::fs::write(&path, vec![0_u8; (MAX_UPLOAD_FILE_BYTES + 1) as usize])
        .expect("oversized temp file can be written");

    let result = message_multipart_form(
        message_request_body("", None, std::slice::from_ref(&attachment)),
        &[attachment],
    )
    .await;
    let Err(error) = result else {
        panic!("multipart form must re-check actual file size");
    };

    assert!(matches!(error, AppError::AttachmentTooLarge { .. }));
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(directory);
}

#[test]
fn rejects_oversized_memory_backed_attachment() {
    let attachment = MessageAttachmentUpload::from_bytes(
        "clipboard-image.png".to_owned(),
        vec![0_u8; (MAX_UPLOAD_FILE_BYTES + 1) as usize],
    );

    let error = validate_message_payload("", &[attachment])
        .expect_err("oversized memory-backed attachment must fail");

    assert!(matches!(error, AppError::AttachmentTooLarge { .. }));
}

#[test]
fn upload_content_type_uses_common_media_types() {
    assert_eq!(upload_content_type("clip.MP4"), "video/mp4");
    assert_eq!(upload_content_type("song.mp3"), "audio/mpeg");
    assert_eq!(
        upload_content_type("sheet.xlsx"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );
    assert_eq!(
        upload_content_type("unknown.concord"),
        "application/octet-stream"
    );
}

#[test]
fn reaction_route_component_formats_unicode_and_custom_reactions() {
    let custom = ReactionEmoji::Custom {
        id: Id::<EmojiMarker>::new(42),
        name: Some("party".to_owned()),
        animated: true,
    };
    let cases = [
        (ReactionEmoji::Unicode("🎉".to_owned()), "%F0%9F%8E%89"),
        (custom, "party%3A42"),
    ];

    for (reaction, expected) in cases {
        assert_eq!(reaction_route_component(&reaction), expected);
    }
}

#[test]
fn reaction_user_pagination_continues_only_after_full_pages() {
    let last_user_id = Id::new(123);

    assert_eq!(
        next_reaction_users_after(100, Some(last_user_id), 1),
        Some(last_user_id)
    );
    assert_eq!(next_reaction_users_after(99, Some(last_user_id), 1), None);
    assert_eq!(next_reaction_users_after(100, None, 1), None);
    assert_eq!(
        next_reaction_users_after(100, Some(last_user_id), REACTION_USERS_MAX_PAGES),
        None
    );
}

#[test]
fn forum_thread_page_filters_or_fills_parent_and_supplies_guild() {
    let guild_id = Id::<GuildMarker>::new(1);
    let forum_id = Id::<ChannelMarker>::new(20);
    let raw = serde_json::json!({
        "threads": [
            {
                "id": "30",
                "parent_id": "20",
                "guild_id": "1",
                "owner_id": "88",
                "type": 11,
                "name": "welcome",
                "thread_metadata": { "archived": false, "locked": false }
            },
            {
                "id": "31",
                "parent_id": "21",
                "type": 11,
                "name": "other-forum-post"
            }
        ],
        "has_more": false
    });

    let threads = parse_forum_threads(&raw, Some(guild_id), forum_id, false);

    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].guild_id, Some(guild_id));
    assert_eq!(threads[0].channel_id, Id::new(30));
    assert_eq!(threads[0].parent_id, Some(forum_id));
    assert_eq!(threads[0].name, "welcome");
    assert_eq!(threads[0].owner_id, Some(Id::new(88)));

    let raw = serde_json::json!({
        "threads": [
            {
                "id": "30",
                "type": 11,
                "name": "welcome",
                "thread_metadata": { "archived": false, "locked": false }
            }
        ],
        "has_more": false
    });

    let threads = parse_forum_threads(&raw, Some(guild_id), forum_id, true);

    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].parent_id, Some(forum_id));
}

#[test]
fn forum_first_messages_are_filtered_to_loaded_posts() {
    let guild_id = Id::<GuildMarker>::new(1);
    let forum_id = Id::<ChannelMarker>::new(20);
    let threads = vec![forum_thread(forum_id, 30, "welcome")];
    let raw = serde_json::json!({
        "first_messages": [
            {
                "id": "300",
                "channel_id": "30",
                "guild_id": "1",
                "author": { "id": "10", "username": "neo" },
                "type": 0,
                "pinned": false,
                "content": "hello from the first post",
                "mentions": [],
                "attachments": [],
                "embeds": []
            },
            {
                "id": "301",
                "channel_id": "31",
                "guild_id": "1",
                "author": { "id": "11", "username": "other" },
                "type": 0,
                "pinned": false,
                "content": "other forum",
                "mentions": [],
                "attachments": [],
                "embeds": []
            }
        ]
    });

    let messages = parse_forum_first_messages(&raw, &threads);

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].guild_id, Some(guild_id));
    assert_eq!(messages[0].channel_id, Id::new(30));
    assert_eq!(messages[0].author, "neo");
    assert_eq!(
        messages[0].content.as_deref(),
        Some("hello from the first post")
    );
}

#[test]
fn forum_first_messages_ignore_non_discord_alias_fields() {
    let forum_id = Id::<ChannelMarker>::new(20);
    let threads = vec![forum_thread(forum_id, 30, "welcome")];
    let raw = serde_json::json!({
        "messages": [
            {
                "id": "300",
                "channel_id": "30",
                "guild_id": "1",
                "author": { "id": "10", "username": "neo" },
                "type": 0,
                "pinned": false,
                "content": "archived search preview",
                "mentions": [],
                "attachments": [],
                "embeds": []
            }
        ],
        "most_recent_messages": [
            {
                "id": "300",
                "channel_id": "30",
                "guild_id": "1",
                "author": { "id": "10", "username": "neo" },
                "type": 0,
                "pinned": false,
                "content": "duplicate preview",
                "mentions": [],
                "attachments": [],
                "embeds": []
            }
        ]
    });

    let messages = parse_forum_first_messages(&raw, &threads);

    assert!(messages.is_empty());
}

#[test]
fn forum_search_sort_serializes_to_discord_query_value() {
    assert_eq!(
        ForumSearchSort::LastMessageTime.as_str(),
        "last_message_time"
    );
    assert_eq!(ForumSearchSort::CreationTime.as_str(), "creation_time");
}

#[test]
fn merge_forum_pages_dedupes_threads_and_keeps_last_message_time_has_more() {
    let forum_id = Id::<ChannelMarker>::new(20);
    let active = ForumPostPage {
        next_offset: 25,
        threads: vec![
            forum_thread_info(forum_id, 100, 10, "active-only"),
            forum_thread_info(forum_id, 200, 20, "shared"),
        ],
        first_messages: Vec::new(),
        has_more: true,
    };
    let recent = ForumPostPage {
        next_offset: 25,
        threads: vec![
            forum_thread_info(forum_id, 200, 99, "shared-from-creation"),
            forum_thread_info(forum_id, 300, 30, "creation-only"),
        ],
        first_messages: Vec::new(),
        // Ignore `has_more` from the creation_time side. Pagination beyond
        // the first page only follows last_message_time.
        has_more: false,
    };

    let merged = merge_forum_pages(active, recent);

    let names: Vec<_> = merged
        .threads
        .iter()
        .map(|thread| thread.name.as_str())
        .collect();
    assert_eq!(names, vec!["active-only", "shared", "creation-only"]);
    assert_eq!(
        merged
            .threads
            .iter()
            .map(|thread| (thread.channel_id.get(), thread.owner_id.map(Id::get)))
            .collect::<Vec<_>>(),
        vec![(100, Some(10)), (200, Some(20)), (300, Some(30))]
    );
    assert!(merged.has_more, "must follow last_message_time has_more");
    assert_eq!(merged.next_offset, 25);
}

fn forum_thread_info(
    parent_id: Id<ChannelMarker>,
    thread_id: u64,
    owner_id: u64,
    name: &str,
) -> ChannelInfo {
    ChannelInfo {
        owner_id: Some(Id::<UserMarker>::new(owner_id)),
        ..forum_thread(parent_id, thread_id, name)
    }
}

#[test]
fn search_index_warming_error_is_detected() {
    let warming = AppError::DiscordRequest("forum post search index is not ready".to_owned());
    let other = AppError::DiscordRequest("forum post search failed: 500".to_owned());

    assert!(is_search_index_warming(&warming));
    assert!(!is_search_index_warming(&other));
    assert!(!is_search_index_warming(&AppError::EmptyMessageContent));
}

#[test]
fn message_search_date_filters_build_inclusive_snowflake_bounds() {
    let equal =
        message_search_date_snowflake_bounds("equal:2026-05-30").expect("equal date bounds");
    let range = message_search_date_snowflake_bounds("gte:2026-05-01,lte:2026-05-30")
        .expect("range date bounds");
    let lower_only = message_search_date_snowflake_bounds("gte:2026-05-30").expect("lower bound");
    let upper_only = message_search_date_snowflake_bounds("lte:2026-05-30").expect("upper bound");

    assert!(equal.min_id.is_some());
    assert!(equal.max_id.is_some());
    assert!(equal.min_id < equal.max_id);
    assert!(range.min_id < range.max_id);
    assert_eq!(lower_only.max_id, None);
    assert_eq!(upper_only.min_id, None);
    assert_eq!(
        message_search_date_snowflake_bounds("before:2026-05-30"),
        None
    );
}

#[test]
fn message_search_query_params_repeats_multi_value_filters() {
    let query = MessageSearchQuery {
        has: vec![MessageSearchHas::Link, MessageSearchHas::Image],
        author_type: vec![MessageSearchAuthorType::User, MessageSearchAuthorType::Bot],
        ..Default::default()
    };

    let params = message_search_query_params(&query);

    assert!(params.contains(&("has", "link".to_owned())));
    assert!(params.contains(&("has", "image".to_owned())));
    assert!(params.contains(&("author_type", "user".to_owned())));
    assert!(params.contains(&("author_type", "bot".to_owned())));
}

#[test]
fn poll_vote_request_body_uses_numeric_answer_ids() {
    assert_eq!(
        poll_vote_request_body(&[1, 2]),
        serde_json::json!({ "answer_ids": [1, 2] })
    );
    assert_eq!(
        poll_vote_request_body(&[]),
        serde_json::json!({ "answer_ids": [] })
    );
}

#[test]
fn mute_request_body_includes_selected_time_window() {
    let end_time = Utc
        .with_ymd_and_hms(2026, 5, 10, 12, 30, 45)
        .single()
        .expect("valid test timestamp");

    assert_eq!(
        mute_request_body(true, Some(end_time), Some(900)),
        serde_json::json!({
            "muted": true,
            "mute_config": {
                "end_time": "2026-05-10T12:30:45.000Z",
                "selected_time_window": 900,
            },
        })
    );
    assert_eq!(
        mute_request_body(true, None, Some(-1)),
        serde_json::json!({
            "muted": true,
            "mute_config": {
                "end_time": null,
                "selected_time_window": -1,
            },
        })
    );
    assert_eq!(
        mute_request_body(false, None, None),
        serde_json::json!({
            "muted": false,
            "mute_config": null,
        })
    );
}

#[test]
fn user_profile_parser_keeps_guild_member_roles() {
    let profile = parse_user_profile_response(
        Id::new(10),
        &serde_json::json!({
            "user": { "id": "10", "username": "test-user" },
            "guild_member": { "roles": ["90", "91"] }
        }),
        None,
    );

    assert_eq!(profile.role_ids, vec![Id::new(90), Id::new(91)]);
}

fn forum_thread(parent_id: Id<ChannelMarker>, thread_id: u64, name: &str) -> ChannelInfo {
    ChannelInfo {
        guild_id: Some(Id::new(1)),
        parent_id: Some(parent_id),
        name: name.to_owned(),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
        ..ChannelInfo::test(Id::new(thread_id), "public_thread")
    }
}
