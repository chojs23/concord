use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use chrono::{TimeZone, Utc};

use crate::discord::ids::{
    Id,
    marker::{ApplicationMarker, ChannelMarker, EmojiMarker, GuildMarker, UserMarker},
};

use crate::{
    AppError,
    discord::{
        ApplicationCommandInfo, ApplicationCommandInteraction, ApplicationCommandInteractionOption,
        BASE_ATTACHMENT_LIMIT_BYTES, ChannelInfo, GuildFolder, MessageAttachmentUpload,
        MessageSearchAuthorType, MessageSearchHas, MessageSearchQuery, ReactionEmoji,
        ReplyReference,
    },
};

use super::{
    application_commands::{
        application_command_interaction_body, application_command_option_body,
        parse_application_command_index,
    },
    forum::{
        ForumPostPage, ForumSearchSort, create_forum_post_request_body, is_search_index_warming,
        merge_forum_pages, merge_pinned_forum_posts, parse_create_forum_post_response,
        parse_forum_first_messages, parse_forum_threads,
    },
    messages::{
        MessageEditRequest, edit_message_request_body, message_multipart_form,
        message_request_body, message_request_body_with_tts, upload_content_type,
        validate_message_content, validate_message_payload,
    },
    notification_settings::mute_request_body,
    polls::poll_vote_request_body,
    profile::parse_user_profile_response,
    reactions::{next_reaction_users_after, reaction_route_component},
    search::{message_search_date_snowflake_bounds, message_search_query_params},
    user_settings::settings_proto_request_body,
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

    validate_message_payload("   ", &attachments, BASE_ATTACHMENT_LIMIT_BYTES)
        .expect("file-only messages should be valid");

    let body = message_request_body(
        "",
        Some(ReplyReference {
            message_id: Id::new(44),
            mention_author: true,
        }),
        &attachments,
    );
    assert_eq!(body["content"], "");
    assert_eq!(body["message_reference"]["message_id"], "44");
    assert!(body.get("allowed_mentions").is_none());
    assert_eq!(body["attachments"][0]["id"], 0);
    assert_eq!(body["attachments"][0]["filename"], "cat.png");
}

#[test]
fn message_request_body_suppresses_reply_ping_when_disabled() {
    let body = message_request_body(
        "hi",
        Some(ReplyReference {
            message_id: Id::new(44),
            mention_author: false,
        }),
        &[],
    );
    assert_eq!(body["message_reference"]["message_id"], "44");
    assert_eq!(body["allowed_mentions"]["replied_user"], false);
    assert_eq!(body["allowed_mentions"]["parse"][0], "users");
}

#[test]
fn forum_post_request_body_nests_message_and_tags() {
    let body = create_forum_post_request_body(
        "Need help",
        "The client crashes",
        &[Id::new(101), Id::new(102)],
        &[],
        BASE_ATTACHMENT_LIMIT_BYTES,
    )
    .expect("forum post body should build");

    assert_eq!(body["name"], "Need help");
    assert_eq!(body["message"]["content"], "The client crashes");
    assert_eq!(body["applied_tags"], serde_json::json!(["101", "102"]));
}

#[test]
fn forum_post_request_body_trims_title_once() {
    let body = create_forum_post_request_body(
        "  Need help  ",
        "Body",
        &[],
        &[],
        BASE_ATTACHMENT_LIMIT_BYTES,
    )
    .expect("padded title should build");

    assert_eq!(body["name"], "Need help");
}

#[test]
fn forum_post_request_body_validates_title_and_message() {
    let error = create_forum_post_request_body(" ", "body", &[], &[], BASE_ATTACHMENT_LIMIT_BYTES)
        .expect_err("empty title must fail");
    assert!(matches!(error, AppError::DiscordRequest(_)));

    let error = create_forum_post_request_body("title", " ", &[], &[], BASE_ATTACHMENT_LIMIT_BYTES)
        .expect_err("empty body must fail");
    assert!(matches!(error, AppError::EmptyMessageContent));
}

#[test]
fn forum_post_create_response_parses_thread_and_nested_first_message() {
    let response = parse_create_forum_post_response(
        &serde_json::json!({
            "id": "30",
            "type": 11,
            "name": "Need help",
            "thread_metadata": {
                "archived": false,
                "locked": false
            },
            "message": {
                "id": "30",
                "channel_id": "30",
                "author": {
                    "id": "10",
                    "username": "neo"
                },
                "type": 0,
                "content": "Body",
                "timestamp": "2026-01-01T00:00:00.000000+00:00",
                "edited_timestamp": null,
                "pinned": false,
                "mention_everyone": false,
                "mentions": [],
                "mention_roles": [],
                "attachments": [],
                "embeds": []
            }
        }),
        Some(Id::new(20)),
    )
    .expect("create response should parse");

    assert_eq!(response.thread.channel_id, Id::new(30));
    assert_eq!(response.thread.parent_id, Some(Id::new(20)));
    assert_eq!(
        response.first_message.map(|message| message.message_id),
        Some(Id::new(30))
    );
}

#[test]
fn guild_folder_settings_proto_includes_name_and_color() {
    let body = settings_proto_request_body(&[GuildFolder {
        id: Some(42),
        name: Some("work".to_owned()),
        color: Some(0x00aaff),
        guild_ids: vec![Id::new(1), Id::new(2)],
    }]);
    let settings = body["settings"]
        .as_str()
        .expect("settings body should be base64");
    let decoded = BASE64_STANDARD
        .decode(settings)
        .expect("settings body should decode");

    assert!(decoded.windows(b"work".len()).any(|bytes| bytes == b"work"));
    assert!(
        decoded
            .windows(4)
            .any(|bytes| bytes == [0x08, 0xff, 0xd5, 0x02])
    );
}

#[test]
fn message_request_body_sets_tts_only_when_requested() {
    let tts = message_request_body_with_tts("hello", None, &[], true);
    assert_eq!(tts["tts"], true);
}

#[test]
fn edit_message_request_body_sets_only_requested_fields() {
    let (content_body, content_action) =
        edit_message_request_body(MessageEditRequest::Content("hello"))
            .expect("content edit body should build");
    let (flags_body, flags_action) = edit_message_request_body(MessageEditRequest::Flags(4_100))
        .expect("flags edit body should build");

    assert_eq!(content_body, serde_json::json!({ "content": "hello" }));
    assert_eq!(content_action, "edit message");
    assert_eq!(flags_body, serde_json::json!({ "flags": 4_100 }));
    assert_eq!(flags_action, "update message flags");
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
fn enforces_per_file_upload_limit() {
    let too_large_file = vec![MessageAttachmentUpload::from_path(
        "/tmp/large.bin".into(),
        "large.bin".to_owned(),
        BASE_ATTACHMENT_LIMIT_BYTES + 1,
    )];
    let error = validate_message_payload("", &too_large_file, BASE_ATTACHMENT_LIMIT_BYTES)
        .expect_err("oversized attachment must fail");
    assert!(matches!(error, AppError::AttachmentTooLarge { .. }));

    let many_sub_limit_files = ["a.bin", "b.bin", "c.bin"]
        .into_iter()
        .map(|name| {
            MessageAttachmentUpload::from_path(
                format!("/tmp/{name}").into(),
                name.to_owned(),
                BASE_ATTACHMENT_LIMIT_BYTES - 1,
            )
        })
        .collect::<Vec<_>>();
    validate_message_payload("", &many_sub_limit_files, BASE_ATTACHMENT_LIMIT_BYTES)
        .expect("files each under the per-file limit are accepted even if their sum exceeds it");
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
    std::fs::write(
        &path,
        vec![0_u8; (BASE_ATTACHMENT_LIMIT_BYTES + 1) as usize],
    )
    .expect("oversized temp file can be written");

    let result = message_multipart_form(
        message_request_body("", None, std::slice::from_ref(&attachment)),
        &[attachment],
        BASE_ATTACHMENT_LIMIT_BYTES,
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
        vec![0_u8; (BASE_ATTACHMENT_LIMIT_BYTES + 1) as usize],
    );

    let error = validate_message_payload("", &[attachment], BASE_ATTACHMENT_LIMIT_BYTES)
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
    // A full page (100 raw entries) hands back the last entry's id as the cursor
    // for the next page.
    let full: Vec<serde_json::Value> = (1..=100)
        .map(|id| serde_json::json!({ "id": id.to_string() }))
        .collect();
    assert_eq!(
        next_reaction_users_after(&full),
        Id::<UserMarker>::new_checked(100)
    );

    // A short page means there is nothing more to fetch.
    let short: Vec<serde_json::Value> = (1..=99)
        .map(|id| serde_json::json!({ "id": id.to_string() }))
        .collect();
    assert_eq!(next_reaction_users_after(&short), None);

    // A full page whose last entry lacks a parseable id yields no cursor.
    let mut malformed = full.clone();
    malformed[99] = serde_json::json!({ "id": "not-a-number" });
    assert_eq!(next_reaction_users_after(&malformed), None);
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
        None,
        &serde_json::json!({
            "user": { "id": "10", "username": "test-user" },
            "guild_member": { "roles": ["90", "91"] }
        }),
        None,
    );

    assert_eq!(profile.role_ids, vec![Id::new(90), Id::new(91)]);
}

#[test]
fn user_profile_parser_resolves_avatar_url() {
    let with_avatar = parse_user_profile_response(
        Id::new(10),
        None,
        &serde_json::json!({
            "user": { "id": "10", "username": "test-user", "avatar": "abc123" }
        }),
        None,
    );
    assert_eq!(
        with_avatar.avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/avatars/10/abc123.png")
    );

    let default_profile = parse_user_profile_response(
        Id::new(10),
        None,
        &serde_json::json!({
            "user": { "id": "10", "username": "test-user", "discriminator": "0" }
        }),
        None,
    );
    assert_eq!(
        default_profile.avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/embed/avatars/0.png")
    );

    let guild_avatar = parse_user_profile_response(
        Id::new(10),
        Some(Id::new(77)),
        &serde_json::json!({
            "user": { "id": "10", "username": "test-user", "avatar": "abc123" },
            "guild_member": { "avatar": "def456" }
        }),
        None,
    );
    assert_eq!(
        guild_avatar.avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/guilds/77/users/10/avatars/def456.png")
    );
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

#[test]
fn merge_pinned_forum_posts_lifts_pins_absent_from_the_activity_body() {
    let forum_id = Id::<ChannelMarker>::new(20);
    // Activity body with no pin loaded (the real pin sits beyond page 0).
    let body = ForumPostPage {
        next_offset: 25,
        threads: vec![
            forum_thread_info(forum_id, 100, 10, "recent-a"),
            forum_thread_info(forum_id, 200, 20, "recent-b"),
        ],
        first_messages: Vec::new(),
        has_more: true,
    };
    let pins = ForumPostPage {
        next_offset: 22,
        threads: vec![
            pinned_forum_thread(forum_id, 999, "PIN: read first"),
            forum_thread_info(forum_id, 100, 10, "recent-a-relevance-copy"),
            forum_thread_info(forum_id, 300, 30, "relevance-noise"),
        ],
        first_messages: vec![crate::discord::MessageInfo::test(
            Id::new(999),
            Id::new(9990),
        )],
        has_more: false,
    };

    let merged = merge_pinned_forum_posts(body, pins);

    // Pin prepended, body kept, and the duplicate (100) plus non-pinned noise
    // (300) dropped.
    assert_eq!(
        merged
            .threads
            .iter()
            .map(|thread| thread.channel_id.get())
            .collect::<Vec<_>>(),
        vec![999, 100, 200],
    );
    assert!(
        merged.threads[0].thread_pinned().unwrap_or(false),
        "pin must lead so the display layer can lift it"
    );
    // The pin's starter message is carried over so its preview renders.
    assert_eq!(
        merged
            .first_messages
            .iter()
            .map(|message| message.channel_id.get())
            .collect::<Vec<_>>(),
        vec![999],
    );
    // Pagination keeps following the activity body, untouched by the harvest.
    assert_eq!(merged.next_offset, 25);
    assert!(merged.has_more);

    // When relevance surfaces no new pin, the body is returned unchanged.
    let body = ForumPostPage {
        next_offset: 25,
        threads: vec![pinned_forum_thread(forum_id, 999, "already-loaded pin")],
        first_messages: Vec::new(),
        has_more: false,
    };
    let pins = ForumPostPage {
        next_offset: 22,
        threads: vec![
            pinned_forum_thread(forum_id, 999, "same pin from relevance"),
            forum_thread_info(forum_id, 400, 40, "relevance-noise"),
        ],
        first_messages: Vec::new(),
        has_more: false,
    };
    let merged = merge_pinned_forum_posts(body, pins);
    assert_eq!(
        merged
            .threads
            .iter()
            .map(|thread| thread.channel_id.get())
            .collect::<Vec<_>>(),
        vec![999],
        "an already-loaded pin is not duplicated and noise is ignored"
    );
}

fn pinned_forum_thread(parent_id: Id<ChannelMarker>, thread_id: u64, name: &str) -> ChannelInfo {
    ChannelInfo {
        flags: Some(1 << 1),
        ..forum_thread(parent_id, thread_id, name)
    }
}
