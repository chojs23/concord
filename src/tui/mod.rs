mod clipboard;
mod commands;
mod format;
mod fuzzy;
mod input;
mod keybindings;
mod login;
mod media;
mod message;
mod runtime;
mod selection;
mod state;
mod terminal;
mod text_cursor;
mod ui;

use tokio::sync::{mpsc, watch};

use crate::{
    Result,
    discord::{AppCommand, DiscordClient, SequencedAppEvent, SnapshotRevision},
};
pub async fn prompt_login(notice: Option<String>) -> Result<String> {
    login::prompt_login(notice).await
}

pub async fn run(
    mut effects: mpsc::Receiver<SequencedAppEvent>,
    mut snapshots: watch::Receiver<SnapshotRevision>,
    commands: mpsc::Sender<AppCommand>,
    client: DiscordClient,
) -> Result<()> {
    let mut terminal = ratatui::init();
    let _restore_guard = match terminal::TerminalRestoreGuard::new() {
        Ok(guard) => guard,
        Err(error) => {
            ratatui::restore();
            return Err(error);
        }
    };

    runtime::run_dashboard(
        &mut terminal,
        &mut effects,
        &mut snapshots,
        commands,
        client,
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use crate::discord::ids::{
        Id,
        marker::{AttachmentMarker, ChannelMarker, GuildMarker, MessageMarker},
    };
    use crate::discord::test_builders::{
        MessageCreateFixture, guild_message_create_fixture, message_create_event,
    };
    use crate::discord::{
        AppEvent, AttachmentInfo, AttachmentUpdate, ChannelInfo, DiscordClient,
        DownloadAttachmentSource, MemberInfo, ReadStateInfo, SequencedAppEvent,
    };

    use super::{
        media::{AvatarImageCache, EmojiImageCache, ImagePreviewCache},
        runtime::{
            effects::{self, effect_forces_redraw},
            redraw::{
                should_redraw_after_visible_signature_change,
                should_suppress_image_redraw_for_signature_change, visible_dashboard_signature,
            },
        },
    };
    use crate::tui::state::{DashboardState, FocusPane};

    #[test]
    fn effect_waits_until_snapshot_revision_catches_up() {
        let mut state = DashboardState::new();
        let mut image_previews = ImagePreviewCache::new();
        let mut avatar_images = AvatarImageCache::new();
        let mut emoji_images = EmojiImageCache::new();
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let (preview_decode_tx, _preview_decode_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut deferred_effects = VecDeque::new();

        {
            let mut ctx = effects::EffectContext {
                state: &mut state,
                client: &client,
                image_previews: &mut image_previews,
                avatar_images: &mut avatar_images,
                emoji_images: &mut emoji_images,
                preview_decode_tx: &preview_decode_tx,
            };
            effects::process_sequenced_effect(
                SequencedAppEvent {
                    revision: 2,
                    event: AppEvent::Ready {
                        user: "tester".to_owned(),
                        user_id: None,
                    },
                },
                1,
                &mut deferred_effects,
                &mut ctx,
            );
        }

        assert_eq!(deferred_effects.len(), 1);
        assert_eq!(state.current_user(), None);

        {
            let mut ctx = effects::EffectContext {
                state: &mut state,
                client: &client,
                image_previews: &mut image_previews,
                avatar_images: &mut avatar_images,
                emoji_images: &mut emoji_images,
                preview_decode_tx: &preview_decode_tx,
            };
            effects::process_deferred_effects(2, &mut deferred_effects, &mut ctx);
        }

        assert!(deferred_effects.is_empty());
        assert_eq!(state.current_user(), Some("tester"));
    }

    #[test]
    fn visible_signature_changes_when_new_messages_notice_count_changes() {
        let mut state = state_with_messages(10);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(5);
        state.clamp_message_viewport_for_image_previews(80, 16, 3);
        state.scroll_message_viewport_top();
        let before = visible_dashboard_signature(&state);

        push_message(&mut state, 11);
        let after = visible_dashboard_signature(&state);

        assert_eq!(before.messages.new_messages_count, 0);
        assert_eq!(after.messages.new_messages_count, 1);
        assert_ne!(before, after);
    }

    #[test]
    fn visible_signature_tracks_update_notices_and_open_mention_picker_candidates() {
        let mut state = DashboardState::new();
        let before = visible_dashboard_signature(&state);

        state.push_event(AppEvent::UpdateAvailable {
            latest_version: "9.9.9".to_owned(),
        });
        let after = visible_dashboard_signature(&state);

        assert_ne!(before, after);

        let mut state = state_with_messages(0);
        state.start_composer();
        for ch in "@al".chars() {
            state.push_composer_char(ch);
        }
        let before = visible_dashboard_signature(&state);

        state.push_event(AppEvent::GuildMemberUpsert {
            guild_id: Id::new(1),
            member: MemberInfo {
                username: Some("alice".to_owned()),
                ..MemberInfo::test(Id::new(10), "Alice")
            },
        });
        let after = visible_dashboard_signature(&state);

        assert_ne!(before, after);
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, false, false,
        ));

        let mut state = state_with_messages(1);
        state.focus_pane(FocusPane::Messages);
        let before = visible_dashboard_signature(&state);

        state.open_selected_message_actions();
        let after = visible_dashboard_signature(&state);

        assert_ne!(before, after);
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, false, false,
        ));

        let mut state = state_with_messages(1);
        state.focus_pane(FocusPane::Messages);
        state.open_emoji_reaction_picker();
        let before = visible_dashboard_signature(&state);

        state.start_emoji_reaction_filter();
        state.push_emoji_reaction_filter_char('s');
        let after = visible_dashboard_signature(&state);

        assert_ne!(before, after);
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, false, false,
        ));

        let mut state = DashboardState::new();
        let _ = state.open_user_profile_popup(Id::new(99), None);
        state.set_user_profile_popup_view_height(1);
        state.set_user_profile_popup_total_lines(3);
        let before = visible_dashboard_signature(&state);

        state.scroll_user_profile_popup_down();
        let after = visible_dashboard_signature(&state);

        assert_ne!(before, after);
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, false, false,
        ));
    }

    #[test]
    fn visible_signature_changes_when_attachment_download_message_changes() {
        let mut state = state_with_messages(0);
        push_image_message(&mut state, 1);
        assert!(state.open_attachment_viewer_for_selected_message());
        let before = visible_dashboard_signature(&state);

        state.push_event(AppEvent::AttachmentDownloadCompleted {
            path: "/tmp/cat.png".to_owned(),
            source: DownloadAttachmentSource::AttachmentViewer,
        });
        let after = visible_dashboard_signature(&state);

        assert_ne!(before, after);
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));
    }

    #[test]
    fn new_message_count_only_change_is_suppressed_while_images_are_visible() {
        let mut state = state_with_messages(5);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(3);
        state.scroll_message_viewport_top();
        let before = visible_dashboard_signature(&state);
        let mut after = before.clone();
        after.messages.new_messages_count = 1;

        assert_eq!(before.messages.new_messages_count, 0);
        assert_eq!(after.messages.new_messages_count, 1);
        assert!(should_suppress_image_redraw_for_signature_change(
            &before, &after, true,
        ));
        assert!(!should_suppress_image_redraw_for_signature_change(
            &before, &after, false,
        ));
        assert!(!should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, false, false,
        ));
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, true,
        ));
    }

    #[test]
    fn visible_channel_activity_redraws_while_images_are_visible() {
        let mut state = state_with_messages(10);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(5);
        state.clamp_message_viewport_for_image_previews(80, 16, 3);
        state.scroll_message_viewport_top();
        let before = visible_dashboard_signature(&state);

        push_message(&mut state, 11);
        let after = visible_dashboard_signature(&state);

        assert_ne!(before, after);
        assert_eq!(
            before.messages.visible_messages,
            after.messages.visible_messages
        );
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, false, false,
        ));
    }

    #[test]
    fn visible_sidebar_unread_state_redraws_while_images_are_visible() {
        let mut state = state_with_messages(10);
        state.focus_pane(FocusPane::Messages);
        state.push_event(AppEvent::ReadStateInit {
            entries: vec![read_state(2, Some(10), 0)],
        });
        let before = visible_dashboard_signature(&state);

        state.push_event(AppEvent::ReadStateInit {
            entries: vec![read_state(2, Some(10), 1)],
        });
        let after = visible_dashboard_signature(&state);

        assert_eq!(
            before.messages.visible_messages,
            after.messages.visible_messages
        );
        assert_ne!(
            before.channels.visible_channels,
            after.channels.visible_channels
        );
        assert!(!should_suppress_image_redraw_for_signature_change(
            &before, &after, true,
        ));
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, false, false,
        ));

        let mut state = state_with_active_dm_and_guild();
        state.focus_pane(FocusPane::Messages);
        let before = visible_dashboard_signature(&state);

        push_message(&mut state, 1);
        let after = visible_dashboard_signature(&state);

        assert_ne!(before, after);
        assert_eq!(
            before.messages.visible_messages,
            after.messages.visible_messages
        );
        assert_ne!(before.guilds.visible_guilds, after.guilds.visible_guilds);
        assert!(!should_suppress_image_redraw_for_signature_change(
            &before, &after, true,
        ));
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, false, false,
        ));
    }

    #[test]
    fn background_message_activity_redraws_while_channels_are_focused() {
        let mut state = state_with_messages(10);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(5);
        state.clamp_message_viewport_for_image_previews(80, 16, 3);
        state.scroll_message_viewport_top();
        state.focus_pane(FocusPane::Channels);
        let before = visible_dashboard_signature(&state);

        push_message(&mut state, 11);
        let after = visible_dashboard_signature(&state);

        assert_ne!(before, after);
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));
    }

    #[test]
    fn visible_message_changes_redraw_even_while_images_are_visible() {
        let mut state = state_with_messages(2);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(8);
        let before = visible_dashboard_signature(&state);

        push_message(&mut state, 3);
        let after = visible_dashboard_signature(&state);

        assert_ne!(
            before.messages.visible_messages,
            after.messages.visible_messages
        );
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));
    }

    #[test]
    fn visible_message_content_update_changes_signature() {
        let mut state = state_with_messages(1);
        state.focus_pane(FocusPane::Messages);
        state.set_message_view_height(8);
        let before = visible_dashboard_signature(&state);

        state.push_event(AppEvent::MessageUpdate {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            message_id: Id::new(1),
            poll: None,
            content: Some("edited msg 1".to_owned()),
            sticker_names: None,
            mentions: None,
            attachments: AttachmentUpdate::Unchanged,
            embeds: None,
            edited_timestamp: Some("2026-05-19T00:00:00.000Z".to_owned()),
        });
        let after = visible_dashboard_signature(&state);

        assert_ne!(
            before.messages.visible_messages,
            after.messages.visible_messages
        );
        assert!(should_redraw_after_visible_signature_change(
            &before, &after, true, false,
        ));
    }

    #[test]
    fn media_effects_force_redraw_without_signature_change() {
        let state = state_with_messages(1);
        let signature = visible_dashboard_signature(&state);
        let loaded = AppEvent::AttachmentPreviewLoaded {
            url: "https://cdn.discordapp.com/avatars/1/hash.png?size=32".to_owned(),
            bytes: Vec::new(),
        };
        let failed = AppEvent::AttachmentPreviewLoadFailed {
            url: "https://cdn.discordapp.com/emoji/1.png".to_owned(),
            message: "failed".to_owned(),
        };

        assert!(effect_forces_redraw(&loaded));
        assert!(effect_forces_redraw(&failed));
        assert!(should_redraw_after_visible_signature_change(
            &signature, &signature, true, true,
        ));
        assert!(!should_redraw_after_visible_signature_change(
            &signature, &signature, true, false,
        ));
    }

    #[test]
    fn gateway_error_forces_redraw_without_signature_change() {
        let state = state_with_messages(1);
        let signature = visible_dashboard_signature(&state);
        let error = AppEvent::GatewayError {
            message: "websocket closed before READY".to_owned(),
        };

        assert!(effect_forces_redraw(&error));
        assert!(should_redraw_after_visible_signature_change(
            &signature, &signature, false, true,
        ));
    }

    fn state_with_messages(count: u64) -> DashboardState {
        let guild_id: Id<GuildMarker> = Id::new(1);
        let channel_id: Id<ChannelMarker> = Id::new(2);
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
        for id in 1..=count {
            push_message(&mut state, id);
        }
        state
    }

    fn state_with_active_dm_and_guild() -> DashboardState {
        let guild_id: Id<GuildMarker> = Id::new(1);
        let guild_channel_id: Id<ChannelMarker> = Id::new(2);
        let dm_channel_id: Id<ChannelMarker> = Id::new(3);
        let mut state = DashboardState::new();
        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            channels: vec![ChannelInfo {
                guild_id: Some(guild_id),
                name: "general".to_owned(),
                ..ChannelInfo::test(guild_channel_id, "GuildText")
            }],
            members: Vec::new(),
            presences: Vec::new(),
            roles: Vec::new(),
            emojis: Vec::new(),
            owner_id: None,
        });
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            name: "dm".to_owned(),
            ..ChannelInfo::test(dm_channel_id, "DM")
        }));
        state.push_event(AppEvent::ActivateChannel {
            channel_id: dm_channel_id,
        });
        state
    }

    fn push_message(state: &mut DashboardState, message_id: u64) {
        state.push_event(message_create_event(MessageCreateFixture {
            message_id: Id::new(message_id),
            content: Some(format!("msg {message_id}")),
            ..guild_message_create_fixture()
        }));
    }

    fn push_image_message(state: &mut DashboardState, message_id: u64) {
        state.push_event(message_create_event(MessageCreateFixture {
            message_id: Id::new(message_id),
            content: Some(format!("image {message_id}")),
            attachments: vec![AttachmentInfo {
                id: Id::<AttachmentMarker>::new(message_id),
                filename: "cat.png".to_owned(),
                url: "https://cdn.discordapp.com/cat.png".to_owned(),
                proxy_url: "https://media.discordapp.net/cat.png".to_owned(),
                content_type: Some("image/png".to_owned()),
                size: 128,
                width: Some(32),
                height: Some(32),
                description: None,
            }],
            ..guild_message_create_fixture()
        }));
    }

    fn read_state(
        channel_id: u64,
        last_acked_message_id: Option<u64>,
        mention_count: u32,
    ) -> ReadStateInfo {
        ReadStateInfo {
            last_acked_message_id: last_acked_message_id.map(Id::<MessageMarker>::new),
            mention_count,
            ..ReadStateInfo::test(Id::new(channel_id))
        }
    }
}
