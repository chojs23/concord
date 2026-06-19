use super::*;
use crate::discord::{
    AppCommand, AttachmentDownloadId, MESSAGE_FLAG_SUPPRESS_EMBEDS, MediaPlaybackSource,
    MediaPlaybackTarget,
};

fn message_action(actions: &[MessageActionItem], kind: MessageActionKind) -> &MessageActionItem {
    actions
        .iter()
        .find(|action| action.kind == kind)
        .expect("message action should exist")
}

fn message_action_index(actions: &[MessageActionItem], kind: MessageActionKind) -> usize {
    actions
        .iter()
        .position(|action| action.kind == kind)
        .expect("message action should exist")
}

#[test]
fn message_action_items_reflect_selected_message_capabilities() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert_eq!(
        actions.iter().map(|action| action.kind).collect::<Vec<_>>(),
        vec![
            MessageActionKind::CopyContent,
            MessageActionKind::OpenReactionPicker,
            MessageActionKind::Reply,
            MessageActionKind::OpenDeleteConfirmation,
            MessageActionKind::Edit,
            MessageActionKind::OpenUrl,
            MessageActionKind::RemoveEmbeds,
            MessageActionKind::PlayMedia,
            MessageActionKind::ViewAttachment,
            MessageActionKind::GoToReferencedMessage,
            MessageActionKind::ShowProfile,
            MessageActionKind::OpenPinConfirmation,
            MessageActionKind::OpenThread,
            MessageActionKind::ShowReactionUsers,
            MessageActionKind::OpenPollVotePicker,
        ]
    );
    assert!(message_action(&actions, MessageActionKind::CopyContent).enabled);
    assert!(message_action(&actions, MessageActionKind::Reply).enabled);
    assert_eq!(
        message_action(&actions, MessageActionKind::ShowProfile).label,
        "show message sender profile"
    );
    assert!(message_action(&actions, MessageActionKind::ShowProfile).enabled);
    assert!(!message_action(&actions, MessageActionKind::GoToReferencedMessage).enabled);
    assert!(!message_action(&actions, MessageActionKind::RemoveEmbeds).enabled);
    assert!(!message_action(&actions, MessageActionKind::PlayMedia).enabled);
    assert!(!message_action(&actions, MessageActionKind::OpenThread).enabled);
    assert!(!message_action(&actions, MessageActionKind::ShowReactionUsers).enabled);
    assert!(!message_action(&actions, MessageActionKind::OpenPollVotePicker).enabled);
}

#[test]
fn remove_embeds_message_action_emits_command_for_unsuppressed_embeds() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            embeds: vec![youtube_embed()],
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();
    assert!(message_action(&actions, MessageActionKind::RemoveEmbeds).enabled);

    assert_eq!(
        state.activate_message_action_kind(MessageActionKind::RemoveEmbeds),
        None
    );
    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );

    assert_eq!(
        state.confirm_message_confirmation(),
        Some(AppCommand::RemoveMessageEmbeds {
            channel_id: Id::new(2),
            message_id: Id::new(1),
        })
    );
}

#[test]
fn remove_embeds_message_action_is_disabled_after_suppression() {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            embeds: vec![youtube_embed()],
            flags: MESSAGE_FLAG_SUPPRESS_EMBEDS,
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert!(!message_action(&actions, MessageActionKind::RemoveEmbeds).enabled);
}

#[test]
fn disabled_image_previews_do_not_hide_attachment_view_action() {
    let mut state = state_with_image_messages(1, &[1]);
    state.open_options_popup();
    state.toggle_selected_display_option();
    state.focus_pane(FocusPane::Messages);

    state.direct_open_selected_message_attachment_viewer();

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::AttachmentViewer));
}

#[test]
fn direct_attachment_message_action_opens_attachment_viewer() {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some(String::new()),
            attachments: vec![image_attachment(10)],
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);

    state.direct_open_selected_message_attachment_viewer();

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::AttachmentViewer));
    assert_eq!(
        state.selected_attachment_viewer_item(),
        Some(super::AttachmentViewerItem {
            index: 1,
            total: 1,
            filename: "image-10.png".to_owned(),
            url: Some("https://cdn.discordapp.com/image-10.png".to_owned()),
            size_bytes: 2048,
            is_image: true,
            is_video: false,
        })
    );
}

#[test]
fn attachment_viewer_navigation_clamps_and_downloads_current_attachment() {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some(String::new()),
            attachments: vec![image_attachment(10), image_attachment(11)],
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);
    state.direct_open_selected_message_attachment_viewer();

    state.move_attachment_viewer_previous();
    assert_eq!(
        state
            .selected_attachment_viewer_item()
            .map(|item| item.index),
        Some(1)
    );

    state.move_attachment_viewer_next();
    state.move_attachment_viewer_next();
    assert_eq!(
        state
            .selected_attachment_viewer_item()
            .map(|item| item.index),
        Some(2)
    );

    let command = state.download_selected_attachment_viewer_attachment();

    assert_eq!(
        command,
        Some(AppCommand::DownloadAttachment {
            id: AttachmentDownloadId::new(0),
            url: "https://cdn.discordapp.com/image-11.png".to_owned(),
            filename: "image-11.png".to_owned(),
            source: DownloadAttachmentSource::AttachmentViewer,
        })
    );
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::AttachmentViewer));
    assert!(state.attachment_downloads().is_empty());
}

#[test]
fn attachment_viewer_play_selected_attachment_only_plays_videos() {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some(String::new()),
            attachments: vec![image_attachment(10), video_attachment(11)],
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);
    state.direct_open_selected_message_attachment_viewer();

    assert_eq!(state.play_selected_attachment_viewer_attachment(), None);

    state.move_attachment_viewer_next();

    assert_eq!(
        state.play_selected_attachment_viewer_attachment(),
        Some(AppCommand::PlayMedia {
            target: MediaPlaybackTarget {
                url: "https://cdn.discordapp.com/clip-11.mp4".to_owned(),
                label: "clip-11.mp4".to_owned(),
                source: MediaPlaybackSource::AttachmentViewer,
            },
            request_id: None,
        })
    );
}

#[test]
fn attachment_viewer_download_uses_original_url_not_preview_proxy() {
    let mut state = state_with_messages(1);
    let mut attachment = image_attachment(10);
    attachment.url = "https://cdn.discordapp.com/original/photo.png".to_owned();
    attachment.proxy_url = concat!(
        "https://media.discordapp.net/attachments/1/10/photo.png",
        "?format=webp&width=160&height=90"
    )
    .to_owned();
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some(String::new()),
            attachments: vec![attachment],
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);
    state.direct_open_selected_message_attachment_viewer();

    let command = state.download_selected_attachment_viewer_attachment();

    assert_eq!(
        command,
        Some(AppCommand::DownloadAttachment {
            id: AttachmentDownloadId::new(0),
            url: "https://cdn.discordapp.com/original/photo.png".to_owned(),
            filename: "image-10.png".to_owned(),
            source: DownloadAttachmentSource::AttachmentViewer,
        })
    );
}

#[test]
fn attachment_download_events_update_global_progress() {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some(String::new()),
            attachments: vec![image_attachment(10)],
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);
    state.direct_open_selected_message_attachment_viewer();
    let id = AttachmentDownloadId::new(7);

    state.push_event(AppEvent::AttachmentDownloadStarted {
        id,
        filename: "cat.png".to_owned(),
        total_bytes: Some(100),
        source: DownloadAttachmentSource::AttachmentViewer,
    });

    assert_eq!(state.attachment_downloads().len(), 1);

    state.close_attachment_viewer();
    state.push_event(AppEvent::AttachmentDownloadProgress {
        id,
        downloaded_bytes: 40,
        total_bytes: Some(100),
    });

    assert_eq!(state.attachment_downloads()[0].downloaded_bytes, 40);

    state.push_event(AppEvent::AttachmentDownloadCompleted {
        id,
        path: "/tmp/cat.png".to_owned(),
        source: DownloadAttachmentSource::AttachmentViewer,
    });

    assert!(state.attachment_downloads().is_empty());
    assert_eq!(
        state.toast_message().map(|toast| toast.text),
        Some("Downloaded to /tmp/cat.png")
    );

    let failed_id = AttachmentDownloadId::new(8);
    state.push_event(AppEvent::AttachmentDownloadStarted {
        id: failed_id,
        filename: "dog.png".to_owned(),
        total_bytes: None,
        source: DownloadAttachmentSource::AttachmentViewer,
    });
    state.push_event(AppEvent::AttachmentDownloadFailed {
        id: failed_id,
        filename: "dog.png".to_owned(),
        message: "network reset".to_owned(),
        source: DownloadAttachmentSource::AttachmentViewer,
    });

    assert!(state.attachment_downloads().is_empty());
    assert_eq!(
        state.toast_message().map(|toast| toast.text),
        Some("Download dog.png failed: network reset")
    );
}

#[test]
fn normal_message_actions_show_disabled_dynamic_actions() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert!(!message_action(&actions, MessageActionKind::OpenThread).enabled);
    assert!(!message_action(&actions, MessageActionKind::ShowReactionUsers).enabled);
    assert!(!message_action(&actions, MessageActionKind::OpenPollVotePicker).enabled);
}

#[test]
fn own_regular_message_actions_show_disabled_dynamic_actions() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert!(!message_action(&actions, MessageActionKind::OpenThread).enabled);
    assert!(!message_action(&actions, MessageActionKind::ShowReactionUsers).enabled);
    assert!(!message_action(&actions, MessageActionKind::OpenPollVotePicker).enabled);
}

#[test]
fn own_reply_message_actions_show_disabled_dynamic_actions() {
    let mut state = state_with_message_ids([]);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    push_reply_message_with_attachments(&mut state, 1, 99, Some("reply body"), Vec::new());
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert!(!message_action(&actions, MessageActionKind::OpenThread).enabled);
    assert!(!message_action(&actions, MessageActionKind::ShowReactionUsers).enabled);
    assert!(!message_action(&actions, MessageActionKind::OpenPollVotePicker).enabled);
}

#[test]
fn edit_reply_action_prefills_composer_without_reply_target_and_submits_edit_command() {
    let mut state = state_with_message_ids([]);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    push_reply_message_with_attachments(&mut state, 1, 99, Some("reply body"), Vec::new());
    state.focus_pane(FocusPane::Messages);

    state.direct_edit_selected_message();

    assert_eq!(state.composer_input(), "reply body");
    assert!(state.reply_target_message_state().is_none());
    state.push_composer_char('!');

    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::EditMessage {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            content: "reply body!".to_owned(),
        })
    );
}

#[test]
fn other_user_direct_edit_does_not_start_composer() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::Ready {
        user: "me".to_owned(),
        user_id: Some(Id::new(10)),
    });
    state.focus_pane(FocusPane::Messages);

    state.direct_edit_selected_message();

    assert!(!state.is_composing());
}

#[test]
fn unhydrated_guild_permissions_keep_other_user_delete_available() {
    let mut state =
        state_with_other_user_message_permissions_hydrating_member(PERM_VIEW_CHANNEL, Vec::new());
    state.focus_pane(FocusPane::Messages);

    state.open_selected_message_delete_confirmation();

    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );
}

#[test]
fn other_user_message_actions_include_delete_with_manage_messages() {
    let mut state = state_with_other_user_message_permissions(
        PERM_VIEW_CHANNEL | PERM_READ_MESSAGE_HISTORY | PERM_MANAGE_MESSAGES,
        Vec::new(),
    );
    state.focus_pane(FocusPane::Messages);

    state.direct_edit_selected_message();
    assert!(!state.is_composing());

    state.open_selected_message_delete_confirmation();

    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );
    assert_eq!(
        state.confirm_message_confirmation(),
        Some(AppCommand::DeleteMessage {
            channel_id: Id::new(2),
            message_id: Id::new(1),
        })
    );
}

#[test]
fn other_user_delete_requires_manage_messages() {
    let mut state = state_with_other_user_message_permissions(
        PERM_VIEW_CHANNEL | PERM_READ_MESSAGE_HISTORY,
        Vec::new(),
    );
    state.focus_pane(FocusPane::Messages);

    state.open_selected_message_delete_confirmation();

    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );
}

#[test]
fn direct_edit_message_prefills_composer_and_submits_edit_command() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    state.focus_pane(FocusPane::Messages);

    state.direct_edit_selected_message();

    assert_eq!(state.composer_input(), "msg 1");
    state.push_composer_char('!');

    assert_eq!(
        state.submit_composer(),
        Some(AppCommand::EditMessage {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            content: "msg 1!".to_owned(),
        })
    );
    assert!(!state.is_composing());
}

#[test]
fn direct_delete_message_submits_delete_command_for_own_message() {
    let mut state = state_with_messages(1);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    state.focus_pane(FocusPane::Messages);

    state.open_selected_message_delete_confirmation();

    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );
    assert_eq!(
        state.confirm_message_confirmation(),
        Some(AppCommand::DeleteMessage {
            channel_id: Id::new(2),
            message_id: Id::new(1),
        })
    );
}

#[test]
fn own_attachment_only_message_can_be_deleted_but_not_edited() {
    let mut state = state_with_message_ids([]);
    state.push_event(AppEvent::Ready {
        user: "neo".to_owned(),
        user_id: Some(Id::new(99)),
    });
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        content: None,
        attachments: vec![image_attachment(1)],
        ..guild_message_create_fixture()
    }));
    state.focus_pane(FocusPane::Messages);

    state.direct_edit_selected_message();
    assert!(!state.is_composing());

    state.open_selected_message_delete_confirmation();

    assert!(
        state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );
    assert_eq!(
        state.confirm_message_confirmation(),
        Some(AppCommand::DeleteMessage {
            channel_id: Id::new(2),
            message_id: Id::new(1),
        })
    );
}

#[test]
fn direct_pin_message_requires_pin_messages_permission() {
    let mut without_pin = state_with_other_user_message_permissions(
        PERM_VIEW_CHANNEL | PERM_READ_MESSAGE_HISTORY,
        Vec::new(),
    );
    without_pin.focus_pane(FocusPane::Messages);

    without_pin.direct_open_selected_message_pin_confirmation();

    assert!(
        !without_pin
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );

    let mut with_pin = state_with_other_user_message_permissions(
        PERM_VIEW_CHANNEL | PERM_READ_MESSAGE_HISTORY | PERM_PIN_MESSAGES,
        Vec::new(),
    );
    with_pin.focus_pane(FocusPane::Messages);

    with_pin.direct_open_selected_message_pin_confirmation();

    assert!(
        with_pin
            .is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageConfirmation)
    );
}

#[test]
fn reply_attachment_action_can_open_attachment_viewer() {
    let mut state = state_with_message_ids([]);
    push_reply_message_with_attachments(
        &mut state,
        1,
        99,
        Some("reply image"),
        vec![image_attachment(1)],
    );
    state.focus_pane(FocusPane::Messages);
    let actions = state.selected_message_action_items();
    assert!(message_action(&actions, MessageActionKind::ViewAttachment).enabled);
    assert!(!message_action(&actions, MessageActionKind::OpenThread).enabled);
    assert!(!message_action(&actions, MessageActionKind::ShowReactionUsers).enabled);
    assert!(!message_action(&actions, MessageActionKind::OpenPollVotePicker).enabled);

    state.direct_open_selected_message_attachment_viewer();

    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::AttachmentViewer));
    assert_eq!(
        state.selected_attachment_viewer_item(),
        Some(super::AttachmentViewerItem {
            index: 1,
            total: 1,
            filename: "image-1.png".to_owned(),
            url: Some("https://cdn.discordapp.com/image-1.png".to_owned()),
            size_bytes: 2048,
            is_image: true,
            is_video: false,
        })
    );
}

#[test]
fn direct_message_url_opens_single_url_from_message_content() {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some("read https://example.com/docs.".to_owned()),
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);
    assert_eq!(
        state.direct_open_selected_message_url(),
        Some(AppCommand::OpenUrl {
            url: "https://example.com/docs".to_owned(),
        })
    );
    assert!(!state.is_message_action_context_active());
}

#[test]
fn direct_message_url_opens_url_picker_for_multiple_urls() {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some("one https://one.example two <https://two.example/path>,".to_owned()),
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);
    assert_eq!(state.direct_open_selected_message_url(), None);
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageUrlPicker));
    assert!(!state.is_message_action_context_active());
    assert_eq!(state.selected_message_url_index(), Some(0));

    assert_eq!(
        state.activate_message_url_shortcut(
            "2".parse::<crate::tui::keybindings::KeyChord>()
                .expect("2 should parse"),
        ),
        Some(AppCommand::OpenUrl {
            url: "https://two.example/path".to_owned(),
        })
    );
    assert!(
        !state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::MessageUrlPicker)
    );
    assert!(!state.is_message_action_context_active());
}

#[test]
fn direct_play_media_prefers_video_attachment() {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some("also https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
            attachments: vec![video_attachment(10)],
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);

    assert_eq!(
        state.direct_play_selected_message_media(),
        Some(AppCommand::PlayMedia {
            target: MediaPlaybackTarget {
                url: "https://cdn.discordapp.com/clip-10.mp4".to_owned(),
                label: "clip-10.mp4".to_owned(),
                source: MediaPlaybackSource::Message,
            },
            request_id: None,
        })
    );
}

#[test]
fn direct_play_media_uses_youtube_url() {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some("watch https://youtu.be/dQw4w9WgXcQ".to_owned()),
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);

    assert_eq!(
        state.direct_play_selected_message_media(),
        Some(AppCommand::PlayMedia {
            target: MediaPlaybackTarget {
                url: "https://youtu.be/dQw4w9WgXcQ".to_owned(),
                label: "media URL".to_owned(),
                source: MediaPlaybackSource::Message,
            },
            request_id: None,
        })
    );
}

#[test]
fn direct_play_media_ignores_plain_urls() {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some("read https://example.com/docs".to_owned()),
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);

    assert_eq!(state.direct_play_selected_message_media(), None);
}

#[test]
fn message_action_detects_markdown_link_urls() {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some(
                "[Tweet](<https://x.com/i/status/2055068765671305537>) • [@steelers](<https://x.com/steelers>) • [FxTwitter](https://fxtwitter.com/i/status/2055068765671305537)"
                    .to_owned(),
            ),
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();

    let urls = state.selected_message_url_items();

    assert_eq!(
        urls.into_iter().map(|item| item.url).collect::<Vec<_>>(),
        vec![
            "https://x.com/i/status/2055068765671305537",
            "https://x.com/steelers",
            "https://fxtwitter.com/i/status/2055068765671305537",
        ]
    );
}

#[test]
fn message_action_detects_embed_urls() {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some("embed below".to_owned()),
            embeds: vec![EmbedInfo {
                color: None,
                provider_name: None,
                author_name: None,
                title: Some("Release notes".to_owned()),
                description: Some("Read [docs](<https://docs.example/release>)".to_owned()),
                timestamp: None,
                fields: vec![EmbedFieldInfo {
                    name: "Links".to_owned(),
                    value: "Status https://status.example".to_owned(),
                }],
                footer_text: None,
                url: Some("https://app.example/releases/1".to_owned()),
                thumbnail_url: Some("https://media.example/thumb.jpg".to_owned()),
                thumbnail_proxy_url: None,
                thumbnail_width: None,
                thumbnail_height: None,
                image_url: Some("https://media.example/image.jpg".to_owned()),
                image_proxy_url: None,
                image_width: None,
                image_height: None,
                video_url: Some("https://media.example/video.mp4".to_owned()),
            }],
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();

    let urls = state.selected_message_url_items();

    assert_eq!(
        urls.into_iter().map(|item| item.url).collect::<Vec<_>>(),
        vec!["https://app.example/releases/1"]
    );
}

#[test]
fn message_action_detects_urls_in_reply_quote_and_forwarded_snapshot() {
    let mut state = state_with_messages(1);
    state.push_event(latest_history_loaded(
        Id::new(2),
        vec![MessageInfo {
            content: Some("see above".to_owned()),
            reply: Some(ReplyInfo {
                author_id: None,
                author: "alice".to_owned(),
                content: Some("check https://reply.example/page".to_owned()),
                sticker_names: Vec::new(),
                mentions: Vec::new(),
            }),
            forwarded_snapshots: vec![MessageSnapshotInfo {
                content: Some("forwarded https://forward.example/doc".to_owned()),
                embeds: vec![youtube_embed()],
                ..MessageSnapshotInfo::test()
            }],
            ..message_info(Id::new(2), 1)
        }],
    ));
    state.focus_pane(FocusPane::Messages);
    state.open_selected_message_actions();

    let urls = state.selected_message_url_items();

    assert_eq!(
        urls.into_iter().map(|item| item.url).collect::<Vec<_>>(),
        vec![
            "https://reply.example/page",
            "https://forward.example/doc",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        ]
    );
}

#[test]
fn non_regular_message_actions_only_show_supported_actions() {
    let mut state = state_with_message_ids([]);
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        message_kind: MessageKind::new(7),
        content: None,
        attachments: vec![video_attachment(1)],
        ..guild_message_create_fixture()
    }));
    state.focus_pane(FocusPane::Messages);

    let actions = state.selected_message_action_items();

    assert!(!message_action(&actions, MessageActionKind::Edit).enabled);
    assert!(message_action(&actions, MessageActionKind::ViewAttachment).enabled);
    assert!(!message_action(&actions, MessageActionKind::OpenThread).enabled);
    assert!(!message_action(&actions, MessageActionKind::ShowReactionUsers).enabled);
    assert!(!message_action(&actions, MessageActionKind::OpenPollVotePicker).enabled);
}

#[test]
fn message_action_items_keep_poll_actions_for_attachment_messages() {
    let mut state = state_with_image_messages(1, &[1]);
    state.focus_pane(FocusPane::Messages);
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        poll: Some(poll_info(false)),
        content: Some(String::new()),
        attachments: vec![image_attachment(1)],
        ..guild_message_create_fixture()
    }));

    let actions = state.selected_message_action_items();

    assert!(!message_action(&actions, MessageActionKind::OpenThread).enabled);
    assert!(!message_action(&actions, MessageActionKind::ShowReactionUsers).enabled);
    assert!(message_action(&actions, MessageActionKind::OpenPollVotePicker).enabled);
}

#[test]
fn single_select_poll_action_opens_picker_and_submits_one_answer() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        poll: Some(poll_info(false)),
        content: Some(String::new()),
        ..guild_message_create_fixture()
    }));
    state.open_selected_message_actions();

    let poll_index = message_action_index(
        &state.selected_message_action_items(),
        MessageActionKind::OpenPollVotePicker,
    );
    assert!(state.select_message_action_row(poll_index));
    assert_eq!(state.activate_selected_message_action(), None);
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::PollVotePicker));

    state.move_poll_vote_picker_down();
    state.toggle_selected_poll_vote_answer();
    let command = state.activate_poll_vote_picker();

    assert_eq!(
        command,
        Some(AppCommand::VotePoll {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            answer_ids: vec![2],
        })
    );
}

#[test]
fn single_select_poll_picker_normalizes_multiple_initial_votes() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    let mut poll = poll_info(false);
    poll.answers[1].me_voted = true;
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        poll: Some(poll),
        content: Some(String::new()),
        ..guild_message_create_fixture()
    }));
    state.open_selected_message_actions();
    let poll_index = message_action_index(
        &state.selected_message_action_items(),
        MessageActionKind::OpenPollVotePicker,
    );
    assert!(state.select_message_action_row(poll_index));
    assert_eq!(state.activate_selected_message_action(), None);

    assert_eq!(
        state.poll_vote_picker_items().map(|items| {
            items
                .iter()
                .map(|item| (item.answer_id, item.selected))
                .collect::<Vec<_>>()
        }),
        Some(vec![(1, true), (2, false)])
    );
    assert_eq!(
        state.activate_poll_vote_picker(),
        Some(AppCommand::VotePoll {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            answer_ids: vec![1],
        })
    );
}

#[test]
fn multi_select_poll_action_opens_picker_and_submits_selected_answers() {
    let mut state = state_with_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(Id::new(1)),
        channel_id: Id::new(2),
        message_id: Id::new(1),
        author_id: Id::new(99),
        poll: Some(poll_info(true)),
        content: Some(String::new()),
        ..guild_message_create_fixture()
    }));

    let actions = state.selected_message_action_items();
    assert_eq!(
        message_action(&actions, MessageActionKind::OpenPollVotePicker).label,
        "choose poll votes"
    );

    state.open_selected_message_actions();
    let poll_index = message_action_index(
        &state.selected_message_action_items(),
        MessageActionKind::OpenPollVotePicker,
    );
    assert!(state.select_message_action_row(poll_index));
    assert_eq!(state.activate_selected_message_action(), None);
    assert!(state.is_active_modal_popup(crate::tui::state::ActiveModalPopupKind::PollVotePicker));
    assert_eq!(
        state.poll_vote_picker_items().map(|items| {
            items
                .iter()
                .map(|item| (item.answer_id, item.selected))
                .collect::<Vec<_>>()
        }),
        Some(vec![(1, true), (2, false)])
    );

    state.move_poll_vote_picker_down();
    state.toggle_selected_poll_vote_answer();
    let command = state.activate_poll_vote_picker();

    assert_eq!(
        command,
        Some(AppCommand::VotePoll {
            channel_id: Id::new(2),
            message_id: Id::new(1),
            answer_ids: vec![1, 2],
        })
    );
}
