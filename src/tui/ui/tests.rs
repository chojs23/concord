use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::discord::ids::{Id, marker::MessageMarker};
use crate::discord::test_builders::{
    MessageCreateFixture, guild_message_create_fixture, message_create_event,
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
};
use unicode_width::UnicodeWidthStr;

use super::{
    ACCENT, DIM, ImagePreview, ImagePreviewState, MENTION_ORANGE, MemberEntry, READ_DIM,
    SELECTED_FORUM_POST_BORDER, SELECTED_MESSAGE_BORDER, UNREAD_BRIGHT,
    attachment_viewer_image_area, attachment_viewer_popup, background_media_occlusion_areas,
    centered_viewer_preview_area, channel_switcher_cursor_position, channel_switcher_lines,
    channel_unread_decoration, composer_content_line_count, composer_cursor_position,
    composer_lines, composer_lines_with_loaded_custom_emoji_urls, composer_prompt_line_count,
    composer_text, date_separator_line, debug_log_popup_lines, dm_presence_dot_span,
    emoji_picker_lines, emoji_reaction_picker_lines, emoji_reaction_picker_lines_for_width,
    emoji_reaction_picker_lines_with_existing, emoji_reaction_picker_lines_with_own_reactions,
    filtered_emoji_reaction_picker_lines, focus_pane_at, format_message_sent_time,
    forum_post_reaction_rows_for_test, forum_post_reaction_summary,
    forum_post_scrollbar_visible_count, forum_post_tag_rows_for_test, forum_post_viewport_lines,
    inline_image_preview_area, inline_image_preview_row, keymap_help_popup_lines,
    leader_action_lines_for_test, member_display_label, member_name_style,
    message_action_menu_lines, message_author_style, message_body_custom_emoji_rows,
    message_delete_confirmation_lines, message_item_lines, message_pin_confirmation_lines,
    message_remove_embeds_confirmation_lines, message_url_picker_lines_for_width,
    message_viewport_layout, message_viewport_lines, new_messages_notice_line, options_popup_lines,
    poll_vote_picker_lines, primary_activity_summary, quit_confirmation_lines,
    reaction_users_popup_lines, reaction_users_visible_line_count, render_channels, render_guilds,
    render_header, render_members, selected_avatar_x_offset, selected_message_card_width,
    selected_message_content_x_offset, sync_view_heights, toast_area, toast_line,
    user_profile_popup_has_avatar, user_profile_popup_lines,
    user_profile_popup_lines_with_activities, user_profile_popup_text_geometry,
};
use crate::tui::message::time::{
    discord_epoch_unix_millis, format_unix_millis_with_offset, message_starts_new_day,
    test_message_id_for_unix_millis,
};
use crate::{
    config::{DisplayOptions, UiStateOptions, VoiceOptions},
    discord::{
        ActivityEmoji, ActivityInfo, ActivityKind, AppEvent, ApplicationCommandInfo,
        ApplicationCommandOptionInfo, AttachmentDownloadId, AttachmentInfo, ChannelInfo,
        ChannelNotificationOverrideInfo, ChannelRecipientState, ChannelState, ChannelUnreadState,
        ChannelVisibilityStats, CustomEmojiInfo, EmbedInfo, GuildFolder, GuildMemberListUpdateInfo,
        GuildMemberState, GuildNotificationSettingsInfo, MemberInfo, MentionInfo,
        MessageAttachmentUpload, MessageInfo, MessageInteractionInfo, MessageKind,
        MessageSearchPage, MessageSearchQuery, MessageSnapshotInfo, MessageState, MutualGuildInfo,
        NotificationLevel, PollAnswerInfo, PollInfo, PresenceStatus, ReactionEmoji, ReactionInfo,
        ReactionUserInfo, ReactionUsersInfo, ReadStateInfo, ReplyInfo, RoleInfo,
        UserGuildSettingsInfo, UserProfileInfo, UserSettingsInfo, VoiceConnectionStatus,
        VoiceStateInfo,
    },
    tui::{
        format::{TextHighlightKind, truncate_display_width, truncate_display_width_from},
        message::format::{
            MessageContentLine, format_message_content, format_message_content_lines,
            format_message_content_lines_with_loaded_custom_emoji_urls, lay_out_reaction_chips,
            mention_highlight_style, poll_box_border, poll_card_inner_width,
            reaction_line_test_spans, wrap_text_lines,
        },
        state::{
            AppliedForumTag, AttachmentDownloadProgressView, AttachmentViewerZoom,
            ChannelSwitcherItem, ChannelThreadItem, DashboardState, DisplayOptionItem,
            EmojiPickerEntry, EmojiReactionItem, FocusPane, MessageActionItem, MessageActionKind,
            PollVotePickerItem,
        },
        ui::{MouseTarget, PopupListTarget, mouse_target_at},
    },
};

mod channel_switcher;
mod composer;
mod media;
mod messages;
mod misc;
mod panes;
mod popups;

fn user_guild_settings_init(settings: Vec<GuildNotificationSettingsInfo>) -> AppEvent {
    AppEvent::UserGuildSettingsInit {
        settings: settings
            .into_iter()
            .map(|notification_settings| UserGuildSettingsInfo {
                notification_settings,
                extra_fields: BTreeMap::new(),
            })
            .collect(),
    }
}

fn guild_member_list_counts_event(
    guild_id: Id<crate::discord::ids::marker::GuildMarker>,
    online: u32,
) -> AppEvent {
    AppEvent::GuildMemberListUpdate {
        update: GuildMemberListUpdateInfo {
            guild_id,
            list_id: None,
            member_count: None,
            online_count: Some(online),
            members: Vec::new(),
            presences: Vec::new(),
            groups: Vec::new(),
            ops: Vec::new(),
            extra_fields: BTreeMap::new(),
        },
    }
}

fn find_cell(buffer: &Buffer, text: &str) -> Option<(u16, u16)> {
    for row in 0..buffer.area.height {
        let line = (0..buffer.area.width)
            .map(|col| buffer[(col, row)].symbol().to_owned())
            .collect::<String>();
        if let Some(col) = line.find(text) {
            return Some((col as u16, row));
        }
    }
    None
}

fn default_message_viewport_layout() -> super::MessageViewportLayout {
    message_viewport_layout(200, 80, 80, 16, 3)
}

fn narrow_message_viewport_layout(content_width: usize) -> super::MessageViewportLayout {
    message_viewport_layout(content_width, 80, 80, 16, 3)
}

fn selected_message_viewport_layout(content_width: usize) -> super::MessageViewportLayout {
    message_viewport_layout(
        content_width,
        80,
        selected_message_card_width(80, true),
        16,
        3,
    )
}

fn rendered_guild_rows(state: &DashboardState, width: u16, height: u16) -> Vec<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");
    terminal
        .draw(|frame| render_guilds(frame, frame.area(), state))
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|col| buffer[(col, row)].symbol().to_owned())
                .collect::<String>()
        })
        .collect()
}

fn current_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_millis()
        .try_into()
        .expect("current unix millis should fit in u64")
}

fn assert_notice_floats_at_list_bottom_above_composer(dump: &[String], label: &str) {
    let notice_row = dump
        .iter()
        .position(|line| line.contains(label))
        .expect("new messages notice should render");
    let composer_row = dump
        .iter()
        .position(|line| line.contains("Message Input"))
        .expect("composer should render");

    assert_eq!(
        notice_row.saturating_add(1),
        composer_row,
        "new messages notice should float on the message-list bottom above composer:\n{}",
        dump.join("\n")
    );
}

fn render_dashboard_dump(width: u16, height: u16, state: &mut DashboardState) -> Vec<String> {
    render_dashboard_dump_with_previews(width, height, state, Vec::new())
}

fn render_dashboard_dump_with_previews(
    width: u16,
    height: u16,
    state: &mut DashboardState,
    image_previews: Vec<ImagePreview<'_>>,
) -> Vec<String> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal should build");
    terminal
        .draw(|frame| {
            sync_view_heights(frame.area(), state);
            super::render(frame, state, image_previews, Vec::new(), Vec::new(), None);
        })
        .expect("draw");

    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|col| buffer[(col, row)].symbol().to_owned())
                .collect::<String>()
        })
        .collect()
}

fn message_with_attachment(content: Option<String>, attachment: AttachmentInfo) -> MessageState {
    MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_is_bot: false,
        message_kind: crate::discord::MessageKind::regular(),
        interaction: None,
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content,
        mentions: Vec::new(),
        attachments: vec![attachment],
        embeds: Vec::new(),
        forwarded_snapshots: Vec::new(),
        ..MessageState::default()
    }
}

fn message_with_content(content: Option<String>) -> MessageState {
    let mut message = message_with_attachment(content, image_attachment());
    message.attachments.clear();
    message
}

fn youtube_embed() -> EmbedInfo {
    EmbedInfo {
        color: Some(0xff0000),
        provider_name: Some("YouTube".to_owned()),
        title: Some("Example Video".to_owned()),
        description: Some("A video description".to_owned()),
        url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
        thumbnail_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_owned()),
        thumbnail_width: Some(480),
        thumbnail_height: Some(360),
        image_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_owned()),
        image_width: Some(480),
        image_height: Some(360),
        ..EmbedInfo::test()
    }
}

fn state_with_message() -> DashboardState {
    state_with_message_id(Id::new(1), "hello")
}

fn state_with_file_attachment_message() -> DashboardState {
    let mut state = state_with_message();
    state.push_event(message_create_event(MessageCreateFixture {
        message_id: Id::new(2),
        content: Some("file".to_owned()),
        attachments: vec![file_attachment()],
        ..guild_message_create_fixture()
    }));
    state.jump_bottom();
    state.move_down();
    state
}

fn state_with_message_id(message_id: Id<MessageMarker>, content: &str) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.focus_pane(FocusPane::Messages);
    state.push_event(message_create_event(MessageCreateFixture {
        channel_id,
        message_id,
        content: Some(content.to_owned()),
        ..guild_message_create_fixture()
    }));
    state
}

fn state_with_folder_settings() -> DashboardState {
    let first_guild = Id::new(1);
    let second_guild = Id::new(2);
    let mut state = DashboardState::new();

    for (guild_id, name) in [(first_guild, "first"), (second_guild, "second")] {
        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: name.to_owned(),
            member_count: None,
            channels: Vec::new(),
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
    }
    state.push_event(AppEvent::UserSettingsUpdate {
        settings: UserSettingsInfo {
            guild_folders: Some(vec![GuildFolder {
                id: Some(42),
                name: Some("folder".to_owned()),
                color: Some(0x00aaff),
                guild_ids: vec![first_guild, second_guild],
            }]),
            ..UserSettingsInfo::default()
        },
    });
    state.focus_pane(FocusPane::Guilds);
    state.open_selected_folder_settings();
    state
}

fn state_with_forum_posts(post_count: usize) -> DashboardState {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let mut state = DashboardState::new();

    state.push_event(AppEvent::GuildCreate {
        guild_id,
        name: "guild".to_owned(),
        member_count: None,
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "forum".to_owned(),
            ..ChannelInfo::test(forum_id, "GuildForum")
        }],
        members: Vec::new(),
        presences: Vec::new(),
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.focus_pane(FocusPane::Messages);

    let threads: Vec<_> = (0..post_count)
        .map(|index| {
            let id = 100 + u64::try_from(index).expect("post index should fit u64");
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(forum_id),
                last_message_id: Some(Id::new(10_000 + id)),
                name: format!("post {index}"),
                message_count: Some(0),
                total_message_sent: Some(1),
                thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
                flags: Some(0),
                ..ChannelInfo::test(Id::new(id), "GuildPublicThread")
            }
        })
        .collect();
    state.push_event(AppEvent::ForumPostsLoaded {
        channel_id: forum_id,
        archive_state: crate::discord::ForumPostArchiveState::Active,
        offset: 0,
        next_offset: threads.len(),
        threads,
        first_messages: Vec::new(),
        has_more: false,
    });
    state
}

fn state_with_unread_direct_messages() -> DashboardState {
    let mut state = DashboardState::new();
    for (channel_id, name, last_message_id) in [
        (Id::new(10), "old", Some(Id::new(100))),
        (Id::new(20), "new", Some(Id::new(200))),
        (Id::new(30), "empty", None),
    ] {
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            last_message_id,
            name: name.to_owned(),
            ..ChannelInfo::test(channel_id, "dm")
        }));
    }
    state.push_event(AppEvent::ReadStateInit {
        entries: vec![
            ReadStateInfo {
                last_acked_message_id: Some(Id::new(100)),
                ..ReadStateInfo::test(Id::new(10))
            },
            ReadStateInfo {
                last_acked_message_id: Some(Id::new(100)),
                ..ReadStateInfo::test(Id::new(20))
            },
        ],
    });
    state
}

fn state_with_unread_direct_messages_with_loaded_unread_messages(count: u64) -> DashboardState {
    let mut state = state_with_unread_direct_messages();
    state.push_event(AppEvent::MessageHistoryLoaded {
        channel_id: Id::new(20),
        before: None,
        messages: (0..count)
            .map(|offset| MessageInfo {
                guild_id: None,
                author_id: Id::new(99),
                author: "neo".to_owned(),
                content: Some(format!("dm {offset}")),
                ..MessageInfo::test(Id::new(20), Id::new(101 + offset))
            })
            .collect(),
    });
    state
}

fn push_message(state: &mut DashboardState, message_id: u64, content: &str) {
    push_message_with_id(state, Id::new(message_id), content);
}

fn push_message_with_id(state: &mut DashboardState, message_id: Id<MessageMarker>, content: &str) {
    state.push_event(message_create_event(MessageCreateFixture {
        message_id,
        content: Some(content.to_owned()),
        ..guild_message_create_fixture()
    }));
}

fn message_info(message_id: u64, author: &str, content: &str, pinned: bool) -> MessageInfo {
    MessageInfo {
        guild_id: Some(Id::new(1)),
        author_id: Id::new(99),
        author: author.to_owned(),
        pinned,
        content: Some(content.to_owned()),
        ..MessageInfo::test(Id::new(2), Id::new(message_id))
    }
}

fn message_with_forwarded_snapshot(snapshot: MessageSnapshotInfo) -> MessageState {
    MessageState {
        id: Id::new(1),
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        author_avatar_url: None,
        author_is_bot: false,
        message_kind: crate::discord::MessageKind::regular(),
        interaction: None,
        reference: None,
        reply: None,
        poll: None,
        pinned: false,
        reactions: Vec::new(),
        content: Some(String::new()),
        mentions: Vec::new(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        forwarded_snapshots: vec![snapshot],
        ..MessageState::default()
    }
}

fn poll_info(allow_multiselect: bool) -> PollInfo {
    PollInfo {
        answers: vec![
            PollAnswerInfo {
                vote_count: Some(2),
                me_voted: true,
                ..PollAnswerInfo::test(1, "Soup")
            },
            PollAnswerInfo {
                vote_count: Some(1),
                ..PollAnswerInfo::test(2, "Noodles")
            },
        ],
        allow_multiselect,
        results_finalized: Some(false),
        total_votes: Some(3),
        ..PollInfo::test("What should we eat?")
    }
}

fn forwarded_snapshot(
    content: Option<&str>,
    attachments: Vec<AttachmentInfo>,
) -> MessageSnapshotInfo {
    MessageSnapshotInfo {
        content: content.map(str::to_owned),
        attachments,
        ..MessageSnapshotInfo::test()
    }
}

fn state_with_member(user_id: u64, display_name: &str) -> DashboardState {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: vec![member_info(user_id, display_name)],
        presences: vec![(Id::new(user_id), PresenceStatus::Online)],
        roles: Vec::new(),
        emojis: Vec::new(),
        owner_id: None,
    });
    state
}

fn state_with_role(role_id: u64, name: &str) -> DashboardState {
    let mut state = DashboardState::new();
    state.push_event(AppEvent::GuildCreate {
        guild_id: Id::new(1),
        name: "guild".to_owned(),
        member_count: None,
        channels: Vec::new(),
        members: Vec::new(),
        presences: Vec::new(),
        roles: vec![RoleInfo {
            position: 1,
            ..RoleInfo::test(Id::new(role_id), name)
        }],
        emojis: Vec::new(),
        owner_id: None,
    });
    state
}

fn member_info(user_id: u64, display_name: &str) -> MemberInfo {
    MemberInfo::test(Id::new(user_id), display_name)
}

fn user_profile_info(user_id: u64, username: &str) -> UserProfileInfo {
    UserProfileInfo::test(Id::new(user_id), username)
}

fn mention_info(user_id: u64, display_name: &str) -> MentionInfo {
    MentionInfo::test(Id::new(user_id), display_name.to_owned())
}

fn mention_info_with_nick(user_id: u64, nick: &str) -> MentionInfo {
    MentionInfo {
        guild_nick: Some(nick.to_owned()),
        ..MentionInfo::test(Id::new(user_id), nick.to_owned())
    }
}

fn channel_with_recipients(kind: &str, statuses: &[PresenceStatus]) -> ChannelState {
    ChannelState {
        id: Id::new(10),
        guild_id: None,
        parent_id: None,
        owner_id: None,
        position: None,
        last_message_id: None,
        name: "alice".to_owned(),
        kind: kind.to_owned(),
        message_count: None,
        member_count: None,
        total_message_sent: None,
        thread_metadata: None,
        flags: None,
        rate_limit_per_user: None,
        available_tags: Vec::new(),
        applied_tags: Vec::new(),
        current_user_joined_thread: false,
        current_user_thread_notification_flags: None,
        recipients: statuses
            .iter()
            .enumerate()
            .map(|(index, status)| ChannelRecipientState {
                user_id: Id::new(100 + u64::try_from(index).expect("index should fit u64")),
                display_name: format!("recipient {index}"),
                username: None,
                is_bot: false,
                avatar_url: None,
                status: *status,
            })
            .collect(),
        permission_overwrites: Vec::new(),
        is_message_request: None,
        is_spam: None,
    }
}

fn line_texts(lines: &[MessageContentLine]) -> Vec<&str> {
    lines.iter().map(|line| line.text.as_str()).collect()
}

fn poll_test_line(text: &str, width: usize) -> String {
    let inner_width = poll_card_inner_width(width);
    let padding = inner_width.saturating_sub(text.width());
    format!("│ {text}{} │", " ".repeat(padding))
}

fn line_texts_from_ratatui(lines: &[ratatui::text::Line<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

fn image_attachment() -> AttachmentInfo {
    AttachmentInfo {
        url: "https://cdn.discordapp.com/cat.png".to_owned(),
        proxy_url: "https://media.discordapp.net/cat.png".to_owned(),
        content_type: Some("image/png".to_owned()),
        size: 2048,
        width: Some(640),
        height: Some(480),
        ..AttachmentInfo::test(Id::new(3), "cat.png")
    }
}

fn image_attachments(count: u64) -> Vec<AttachmentInfo> {
    (0..count)
        .map(|index| {
            let id = 3 + index;
            let mut attachment = image_attachment();
            attachment.id = Id::new(id);
            attachment.filename = format!("image-{id}.png");
            attachment.url = format!("https://cdn.discordapp.com/image-{id}.png");
            attachment.proxy_url = format!("https://media.discordapp.net/image-{id}.png");
            attachment
        })
        .collect()
}

fn video_attachment() -> AttachmentInfo {
    AttachmentInfo {
        url: "https://cdn.discordapp.com/clip.mp4".to_owned(),
        proxy_url: "https://media.discordapp.net/clip.mp4".to_owned(),
        content_type: Some("video/mp4".to_owned()),
        size: 78_364_758,
        width: Some(1920),
        height: Some(1080),
        ..AttachmentInfo::test(Id::new(4), "clip.mp4")
    }
}

fn file_attachment() -> AttachmentInfo {
    AttachmentInfo {
        url: "https://cdn.discordapp.com/notes.txt".to_owned(),
        proxy_url: "https://media.discordapp.net/notes.txt".to_owned(),
        content_type: Some("text/plain".to_owned()),
        size: 42,
        ..AttachmentInfo::test(Id::new(5), "notes.txt")
    }
}
