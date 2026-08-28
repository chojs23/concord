use std::{
    collections::VecDeque,
    io::{Write, stdout},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(target_os = "macos")]
use std::sync::Once;

use tokio::sync::mpsc;

use crate::{
    config::NotificationOptions,
    discord::{
        AppEvent, DiscordClient, SequencedAppEvent, VoiceSoundKind,
        ids::{
            Id,
            marker::{GuildMarker, UserMarker},
        },
    },
    logging,
};

use super::super::{
    media::MediaImageDecodeResult,
    state::{DashboardState, DesktopNotification},
};
use super::media_runtime::DashboardMediaRuntime;

pub(super) const MAX_DRAINED_EFFECT_EVENTS: usize = 1024;
static NOTIFICATION_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);

pub(super) struct EffectContext<'a> {
    pub(super) state: &'a mut DashboardState,
    pub(super) client: &'a DiscordClient,
    pub(super) media_runtime: &'a mut DashboardMediaRuntime,
    pub(super) media_decode_tx: &'a mpsc::UnboundedSender<MediaImageDecodeResult>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct EffectProcessingOutcome {
    pub(super) processed_event: bool,
    pub(super) force_redraw: bool,
}

impl EffectProcessingOutcome {
    fn processed(event: &AppEvent) -> Self {
        Self {
            processed_event: true,
            force_redraw: effect_forces_redraw(event),
        }
    }

    pub(super) fn combine(&mut self, other: Self) {
        self.processed_event |= other.processed_event;
        self.force_redraw |= other.force_redraw;
    }
}

/// Some redraws are needed for state that the dashboard view signature does not
/// cover, mainly the media caches (an inline preview, avatar, or emoji finishing
/// or failing to load) and connection-lifecycle events. Force a redraw for those
/// regardless of whether the visible signature changed.
pub(super) fn effect_forces_redraw(event: &AppEvent) -> bool {
    matches!(
        event,
        AppEvent::AttachmentPreviewLoaded { .. }
            | AppEvent::AttachmentPreviewLoadFailed { .. }
            | AppEvent::AttachmentDownloadStarted { .. }
            | AppEvent::AttachmentDownloadProgress { .. }
            | AppEvent::AttachmentDownloadCompleted { .. }
            | AppEvent::AttachmentDownloadFailed { .. }
            | AppEvent::GatewayError { .. }
            | AppEvent::MediaPlaybackWindowReady { .. }
            | AppEvent::GatewayResumed
            | AppEvent::GatewayReidentified
            | AppEvent::SignedOut
            | AppEvent::GatewayClosed
    )
}

pub(super) fn process_effect_event(
    event: AppEvent,
    ctx: &mut EffectContext<'_>,
) -> EffectProcessingOutcome {
    let outcome = EffectProcessingOutcome::processed(&event);
    let now = std::time::Instant::now();
    let missing_members = missing_members_for_effect(&event, ctx.state, now);

    dispatch_runtime_side_effects(&event, ctx);
    record_media_event(&event, ctx);
    push_dashboard_effect(event, ctx);
    enqueue_member_hydration_requests(missing_members, ctx, now);

    outcome
}

fn missing_members_for_effect(
    event: &AppEvent,
    state: &DashboardState,
    now: std::time::Instant,
) -> Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)> {
    let mut missing = Vec::new();
    let messages = match event {
        AppEvent::MessageCreate { message } => Some(std::slice::from_ref(message)),
        AppEvent::MessageHistoryLoaded { messages, .. }
        | AppEvent::MessageHistoryRefreshed { messages, .. }
        | AppEvent::MessageHistoryAfterLoaded { messages, .. }
        | AppEvent::MessageHistoryAroundLoaded { messages, .. }
        | AppEvent::InboxMentionsLoaded { messages, .. }
        | AppEvent::InboxChannelMessagesLoaded { messages, .. }
        | AppEvent::MessageSearchLoaded {
            page: crate::discord::MessageSearchPage { messages, .. },
        }
        | AppEvent::PinnedMessagesLoaded { messages, .. } => Some(messages.as_slice()),
        _ => None,
    };
    if let Some(messages) = messages {
        missing.extend(state.missing_message_author_member_requests(messages));
    }
    if let AppEvent::MessageUpdateDispatch { update } = event
        && let Some(mentions) = update.fields.mentions.as_ref()
    {
        missing.extend(state.missing_channel_user_member_requests(
            update.channel_id,
            update.guild_id,
            mentions.iter().map(|mention| mention.user_id),
        ));
    }
    if let AppEvent::ReactionUsersLoaded {
        channel_id, users, ..
    } = event
    {
        missing.extend(state.missing_channel_user_member_requests(
            *channel_id,
            None,
            users.iter().map(|user| user.user_id),
        ));
    }
    // Persistent voice, typing, thread, and permission demands are retryable
    // background work. Append them only after users surfaced by this event so
    // the first 100-ID Gateway request resolves the active view.
    missing.extend(state.observed_member_hydration_requests(now));
    missing
}

fn dispatch_runtime_side_effects(event: &AppEvent, ctx: &EffectContext<'_>) {
    if let Some(notification) = ctx.state.desktop_notification_for_event(event) {
        dispatch_desktop_notification(notification, ctx.state.desktop_notification_icon());
    }
    if ctx.state.notification_sound_for_event(event) {
        dispatch_notification_sound(ctx.state.notification_options());
    }
    if let AppEvent::VoiceSound { kind } = event {
        dispatch_voice_sound(*kind, ctx.state.notification_options());
    }
}

fn record_media_event(event: &AppEvent, ctx: &mut EffectContext<'_>) {
    ctx.media_runtime.record_event(event, ctx.media_decode_tx);
}

fn push_dashboard_effect(event: AppEvent, ctx: &mut EffectContext<'_>) {
    if let AppEvent::RichPresenceDetected { activities } = event {
        ctx.state.set_detected_rich_presence(activities);
        return;
    }
    if matches!(event, AppEvent::GatewayClosed) {
        handle_gateway_closed(ctx.state);
        return;
    }
    if matches!(event, AppEvent::SignedOut) {
        ctx.state.sign_out();
        return;
    }
    if matches!(event, AppEvent::GatewayReidentified)
        && let Some(command) = ctx.state.selected_channel_subscription_command()
    {
        ctx.state.enqueue_pending_command(command);
    }
    if matches!(
        event,
        AppEvent::GatewayResumed | AppEvent::GatewayReidentified
    ) && let Some(command) = ctx.state.selected_message_history_catch_up_command()
    {
        ctx.state.enqueue_pending_command(command);
    }
    ctx.state.push_effect(event);
}

fn enqueue_member_hydration_requests(
    missing: Vec<(Id<GuildMarker>, Vec<Id<UserMarker>>)>,
    ctx: &mut EffectContext<'_>,
    now: std::time::Instant,
) {
    let requests = ctx.client.next_member_hydration_requests(missing, now);
    ctx.state.enqueue_member_hydration_requests(requests);
}

fn dispatch_desktop_notification(notification: DesktopNotification, icon: Option<String>) {
    let title = notification.title;
    let body = notification.body;
    spawn_notification_task("notification", "desktop notification", move || {
        deliver_desktop_notification(&title, &body, icon.as_deref())
    });
}

fn dispatch_voice_sound(kind: VoiceSoundKind, notification_options: NotificationOptions) {
    spawn_notification_task("voice", "voice sound", move || {
        play_voice_sound(kind, notification_options)
    });
}

fn dispatch_notification_sound(notification_options: NotificationOptions) {
    spawn_notification_task("notification", "message sound", move || {
        play_notification_sound(notification_options)
    });
}

#[cfg(feature = "voice-playback")]
pub(in crate::tui) fn dispatch_push_to_talk_sound(pressed: bool) {
    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            super::notification_audio::play_push_to_talk_sound(pressed)
        })
        .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                log_notification_failure_once(
                    "voice",
                    format!("push-to-talk sound failed: {error}"),
                );
            }
            Err(error) => {
                log_notification_failure_once(
                    "voice",
                    format!("push-to-talk sound task failed: {error}"),
                );
            }
        }
    });
}

fn spawn_notification_task<F>(target: &'static str, action: &'static str, task: F)
where
    F: FnOnce() -> std::result::Result<(), String> + Send + 'static,
{
    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(task).await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                log_notification_failure_once(target, format!("{action} failed: {error}"));
                ring_terminal_bell();
            }
            Err(error) => {
                log_notification_failure_once(target, format!("{action} task failed: {error}"));
                ring_terminal_bell();
            }
        }
    });
}

fn log_notification_failure_once(target: &str, message: String) {
    if !NOTIFICATION_FAILURE_LOGGED.swap(true, Ordering::Relaxed) {
        logging::error(target, message);
    }
}

fn deliver_desktop_notification(
    title: &str,
    body: &str,
    icon: Option<&str>,
) -> std::result::Result<(), String> {
    deliver_notify_rust_notification(title, body, icon)
}

fn play_voice_sound(
    kind: VoiceSoundKind,
    notification_options: NotificationOptions,
) -> std::result::Result<(), String> {
    let custom_path = voice_sound_path(kind, &notification_options);
    #[cfg(feature = "voice-playback")]
    {
        super::notification_audio::play_voice_sound(kind, custom_path)
    }
    #[cfg(not(feature = "voice-playback"))]
    {
        let _ = kind;
        let _ = custom_path;
        ring_terminal_bell();
        Ok(())
    }
}

fn play_notification_sound(
    notification_options: NotificationOptions,
) -> std::result::Result<(), String> {
    let custom_path = notification_options.notification_sound.as_deref();
    #[cfg(feature = "voice-playback")]
    {
        super::notification_audio::play_notification_sound(custom_path)
    }
    #[cfg(not(feature = "voice-playback"))]
    {
        let _ = custom_path;
        ring_terminal_bell();
        Ok(())
    }
}

fn voice_sound_path(kind: VoiceSoundKind, options: &NotificationOptions) -> Option<&Path> {
    match kind {
        VoiceSoundKind::Join => options.voice_join_sound.as_deref(),
        VoiceSoundKind::Leave => options.voice_leave_sound.as_deref(),
        VoiceSoundKind::StreamStart
        | VoiceSoundKind::StreamViewerJoin
        | VoiceSoundKind::StreamViewerLeave => None,
    }
}

fn deliver_notify_rust_notification(
    title: &str,
    body: &str,
    icon: Option<&str>,
) -> std::result::Result<(), String> {
    #[cfg(target_os = "macos")]
    init_macos_notification_identity();

    let mut notification = notify_rust::Notification::new();
    if let Some(icon) = icon {
        notification.icon(icon);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    notification.hint(notify_rust::Hint::SuppressSound(true));
    notification
        .summary(title)
        .body(body)
        .show()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn init_macos_notification_identity() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // macOS needs a real app bundle, so fall back to Terminal for terminals
        // we can't identify (e.g. kitty, tmux) -- otherwise notifications vanish.
        let app_name = std::env::var("TERM_PROGRAM")
            .ok()
            .and_then(|program| macos_terminal_app_name(&program))
            .unwrap_or("Terminal");
        let bundle_id = notify_rust::get_bundle_identifier_or_default(app_name);
        if bundle_id != "com.apple.Finder" {
            let _ = notify_rust::set_application(&bundle_id);
        }
    });
}

#[cfg(target_os = "macos")]
fn macos_terminal_app_name(term_program: &str) -> Option<&'static str> {
    match term_program {
        "Apple_Terminal" => Some("Terminal"),
        "iTerm.app" => Some("iTerm"),
        "WezTerm" => Some("WezTerm"),
        "WarpTerminal" => Some("Warp"),
        _ => None,
    }
}

fn ring_terminal_bell() {
    let mut output = stdout();
    let _ = output.write_all(b"\x07");
    let _ = output.flush();
}

pub(super) fn process_sequenced_effect(
    event: SequencedAppEvent,
    current_snapshot_revision: u64,
    deferred_effects: &mut VecDeque<SequencedAppEvent>,
    ctx: &mut EffectContext<'_>,
) -> EffectProcessingOutcome {
    if event.revision > current_snapshot_revision {
        deferred_effects.push_back(event);
        return EffectProcessingOutcome::default();
    }
    process_effect_event(event.event, ctx)
}

pub(super) fn process_deferred_effects(
    current_snapshot_revision: u64,
    deferred_effects: &mut VecDeque<SequencedAppEvent>,
    ctx: &mut EffectContext<'_>,
) -> EffectProcessingOutcome {
    let mut outcome = EffectProcessingOutcome::default();
    for _ in 0..deferred_effects.len() {
        let Some(event) = deferred_effects.pop_front() else {
            break;
        };
        outcome.combine(process_sequenced_effect(
            event,
            current_snapshot_revision,
            deferred_effects,
            ctx,
        ));
    }
    outcome
}

pub(super) fn handle_gateway_closed(state: &mut DashboardState) {
    logging::error("tui", "gateway closed");
    state.push_effect(AppEvent::GatewayClosed);
    state.quit();
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use tokio::sync::mpsc;

    use crate::discord::ids::Id;
    use crate::discord::test_builders::{
        GuildCreateFixture, MessageHistoryLoadedFixture, ReactionUsersLoadedFixture,
        guild_create_event, message_history_loaded_event, reaction_users_loaded_event,
    };
    use crate::discord::{
        AppCommand, AppEvent, ChannelInfo, MemberInfo, MentionInfo, MessageHistoryAfterMode,
        MessageInfo, MessageUpdateDispatchInfo, MessageUpdateEventFields, ReactionEmoji,
        ReactionUserInfo, RoleInfo, SequencedAppEvent, VoiceStateInfo,
    };

    use super::*;

    #[test]
    fn effect_waits_until_snapshot_revision_catches_up() {
        let mut state = DashboardState::new();
        let mut media_runtime =
            DashboardMediaRuntime::new(crate::config::ImageProtocolPreference::Auto);
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let (media_decode_tx, _media_decode_rx) = mpsc::unbounded_channel();
        let mut deferred_effects = VecDeque::new();

        {
            let mut ctx = EffectContext {
                state: &mut state,
                client: &client,
                media_runtime: &mut media_runtime,
                media_decode_tx: &media_decode_tx,
            };
            process_sequenced_effect(
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
            let mut ctx = EffectContext {
                state: &mut state,
                client: &client,
                media_runtime: &mut media_runtime,
                media_decode_tx: &media_decode_tx,
            };
            process_deferred_effects(2, &mut deferred_effects, &mut ctx);
        }

        assert!(deferred_effects.is_empty());
        assert_eq!(state.current_user(), Some("tester"));
    }

    #[test]
    fn events_carrying_unloaded_authors_enqueue_a_member_request() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let author_id = Id::new(99);
        let message_id = Id::new(20);
        let text_channel = || channel_info(guild_id, channel_id, None, "general", "GuildText");

        // Every route that surfaces a message authored by someone the
        // member cache has never seen must ask for that member, or the row
        // renders with a raw id instead of a name.
        let cases = [
            (
                "live message",
                text_channel(),
                AppEvent::MessageCreate {
                    message: message_info(guild_id, channel_id, message_id, author_id),
                },
            ),
            (
                "message history",
                text_channel(),
                message_history_loaded_event(MessageHistoryLoadedFixture {
                    channel_id,
                    messages: vec![message_info(guild_id, channel_id, message_id, author_id)],
                    ..MessageHistoryLoadedFixture::new()
                }),
            ),
            (
                "inbox mentions",
                text_channel(),
                AppEvent::InboxMentionsLoaded {
                    request_id: 1,
                    before: None,
                    messages: vec![message_info(guild_id, channel_id, message_id, author_id)],
                    has_more: false,
                },
            ),
            (
                "inbox channel messages",
                text_channel(),
                AppEvent::InboxChannelMessagesLoaded {
                    request_id: 1,
                    channel_id,
                    messages: vec![message_info(guild_id, channel_id, message_id, author_id)],
                },
            ),
            (
                "reaction user",
                text_channel(),
                reaction_users_loaded_event(ReactionUsersLoadedFixture {
                    channel_id,
                    message_id,
                    emoji: ReactionEmoji::Unicode("👍".to_owned()),
                    users: vec![ReactionUserInfo::test(author_id, "unknown")],
                    next_after: None,
                    after: None,
                }),
            ),
            (
                "updated message mention",
                text_channel(),
                AppEvent::MessageUpdateDispatch {
                    update: MessageUpdateDispatchInfo {
                        guild_id: Some(guild_id),
                        channel_id,
                        message_id,
                        fields: MessageUpdateEventFields {
                            mentions: Some(vec![MentionInfo::test(author_id, "unknown")]),
                            ..MessageUpdateEventFields::default()
                        },
                        extra_fields: Default::default(),
                    },
                },
            ),
        ];

        for (label, channel, event) in cases {
            let mut state = DashboardState::new();
            push_guild_with_channel(&mut state, guild_id, channel);

            process_effect_in_default_context(&mut state, event);

            assert_eq!(
                state.drain_pending_commands(),
                vec![AppCommand::LoadGuildMembersByIds {
                    guild_id,
                    user_ids: vec![author_id],
                }],
                "{label}"
            );
        }
    }

    #[test]
    fn visible_message_authors_take_the_first_member_hydration_batch() {
        let guild_id = Id::new(1);
        let text_channel_id = Id::new(2);
        let voice_channel_id = Id::new(3);
        let author_id = Id::new(9_999);
        let mut state = DashboardState::new();
        state.push_event(guild_create_event(GuildCreateFixture {
            channels: vec![
                channel_info(guild_id, text_channel_id, None, "general", "GuildText"),
                channel_info(guild_id, voice_channel_id, None, "Lobby", "GuildVoice"),
            ],
            roles: vec![RoleInfo::test(Id::new(guild_id.get()), "@everyone")],
            ..GuildCreateFixture::new(guild_id)
        }));

        let background_user_ids = (1_000..1_100).map(Id::new).collect::<Vec<_>>();
        for user_id in &background_user_ids {
            state.push_event(AppEvent::VoiceStateUpdate {
                state: VoiceStateInfo::test(guild_id, Some(voice_channel_id), *user_id),
            });
        }

        process_effect_in_default_context(
            &mut state,
            AppEvent::MessageHistoryLoaded {
                channel_id: text_channel_id,
                before: None,
                messages: vec![message_info(
                    guild_id,
                    text_channel_id,
                    Id::new(20),
                    author_id,
                )],
            },
        );

        let commands = state.drain_pending_commands();
        let [
            AppCommand::LoadGuildMembersByIds {
                guild_id: first_guild_id,
                user_ids: first_user_ids,
            },
            AppCommand::LoadGuildMembersByIds {
                guild_id: second_guild_id,
                user_ids: second_user_ids,
            },
        ] = commands.as_slice()
        else {
            panic!("expected two member hydration batches, got {commands:?}");
        };
        assert_eq!(*first_guild_id, guild_id);
        assert_eq!(*second_guild_id, guild_id);
        assert_eq!(first_user_ids.first(), Some(&author_id));
        assert_eq!(first_user_ids.len(), 100);
        assert_eq!(second_user_ids, &background_user_ids[99..]);
    }

    #[test]
    fn voice_member_hydration_requests_only_unresolved_participants() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let user_id = Id::new(99);
        let voice_channel = || channel_info(guild_id, channel_id, None, "Lobby", "GuildVoice");

        let mut missing_state = DashboardState::new();
        push_guild_with_channel(&mut missing_state, guild_id, voice_channel());
        let missing_event = AppEvent::VoiceStateUpdate {
            state: VoiceStateInfo::test(guild_id, Some(channel_id), user_id),
        };
        missing_state.push_event(missing_event.clone());
        process_effect_in_default_context(&mut missing_state, missing_event);
        assert_eq!(
            missing_state.drain_pending_commands(),
            vec![AppCommand::LoadGuildMembersByIds {
                guild_id,
                user_ids: vec![user_id],
            }]
        );

        let mut complete_state = DashboardState::new();
        push_guild_with_channel(&mut complete_state, guild_id, voice_channel());
        let complete_event = AppEvent::VoiceStateUpdate {
            state: VoiceStateInfo {
                member: Some(MemberInfo {
                    username: Some("voice-user".to_owned()),
                    role_ids: vec![Id::new(10)],
                    ..MemberInfo::test(user_id, "Voice User")
                }),
                ..VoiceStateInfo::test(guild_id, Some(channel_id), user_id)
            },
        };
        complete_state.push_event(complete_event.clone());
        process_effect_in_default_context(&mut complete_state, complete_event);
        assert!(complete_state.drain_pending_commands().is_empty());
    }

    #[test]
    fn gateway_reconnect_events_enqueue_selected_channel_catch_up() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);

        for event in [AppEvent::GatewayResumed, AppEvent::GatewayReidentified] {
            let mut state = DashboardState::new();
            push_guild_with_channel(
                &mut state,
                guild_id,
                channel_info(guild_id, channel_id, None, "general", "GuildText"),
            );
            state.confirm_selected_guild();
            state.confirm_selected_channel();
            state.push_event(message_history_loaded_event(MessageHistoryLoadedFixture {
                channel_id,
                messages: vec![message_info(guild_id, channel_id, Id::new(20), Id::new(99))],
                ..MessageHistoryLoadedFixture::new()
            }));

            let reidentified = matches!(event, AppEvent::GatewayReidentified);
            process_effect_in_default_context(&mut state, event);

            let mut expected = Vec::new();
            if reidentified {
                expected.push(AppCommand::SubscribeGuildChannel {
                    guild_id,
                    channel_id,
                });
            }
            expected.push(AppCommand::LoadMessageHistoryAfter {
                channel_id,
                after: Id::new(20),
                mode: MessageHistoryAfterMode::CatchUp,
            });
            assert_eq!(state.drain_pending_commands(), expected);
        }
    }

    #[test]
    fn signed_out_effect_marks_dashboard_for_sign_out() {
        let mut state = DashboardState::new();

        process_effect_in_default_context(&mut state, AppEvent::SignedOut);

        assert!(state.should_quit());
        assert!(state.should_sign_out());
    }

    fn push_guild_with_channel(
        state: &mut DashboardState,
        guild_id: Id<crate::discord::ids::marker::GuildMarker>,
        channel: ChannelInfo,
    ) {
        state.push_event(guild_create_event(GuildCreateFixture {
            channels: vec![channel],
            roles: vec![RoleInfo::test(Id::new(guild_id.get()), "@everyone")],
            ..GuildCreateFixture::new(guild_id)
        }));
    }

    fn channel_info(
        guild_id: Id<crate::discord::ids::marker::GuildMarker>,
        channel_id: Id<crate::discord::ids::marker::ChannelMarker>,
        parent_id: Option<Id<crate::discord::ids::marker::ChannelMarker>>,
        name: &str,
        kind: &str,
    ) -> ChannelInfo {
        ChannelInfo {
            guild_id: Some(guild_id),
            parent_id,
            position: Some(0),
            name: name.to_owned(),
            ..ChannelInfo::test(channel_id, kind)
        }
    }

    fn message_info(
        guild_id: Id<crate::discord::ids::marker::GuildMarker>,
        channel_id: Id<crate::discord::ids::marker::ChannelMarker>,
        message_id: Id<crate::discord::ids::marker::MessageMarker>,
        author_id: Id<crate::discord::ids::marker::UserMarker>,
    ) -> MessageInfo {
        MessageInfo {
            guild_id: Some(guild_id),
            author_id,
            author: "neo".to_owned(),
            content: Some("hello".to_owned()),
            ..MessageInfo::test(channel_id, message_id)
        }
    }

    fn process_effect_in_default_context(state: &mut DashboardState, event: AppEvent) {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let mut media_runtime =
            DashboardMediaRuntime::new(crate::config::ImageProtocolPreference::Auto);
        let (media_decode_tx, _media_decode_rx) = mpsc::unbounded_channel();
        let mut ctx = EffectContext {
            state,
            client: &client,
            media_runtime: &mut media_runtime,
            media_decode_tx: &media_decode_tx,
        };

        process_effect_event(event, &mut ctx);
    }
}
