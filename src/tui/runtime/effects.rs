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
        AppEvent, ChannelInfo, DiscordClient, MessageInfo, SequencedAppEvent, VoiceSoundKind,
    },
    logging,
};

use super::super::{
    media::{
        AvatarImageCache, EmojiImageCache, ImagePreviewCache, MediaImageDecodeResult,
        spawn_media_image_decode,
    },
    state::{DashboardState, DesktopNotification},
};

pub(super) const MAX_DRAINED_EFFECT_EVENTS: usize = 1024;
static NOTIFICATION_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);

pub(in crate::tui) struct EffectContext<'a> {
    pub(in crate::tui) state: &'a mut DashboardState,
    pub(in crate::tui) client: &'a DiscordClient,
    pub(in crate::tui) image_previews: &'a mut ImagePreviewCache,
    pub(in crate::tui) avatar_images: &'a mut AvatarImageCache,
    pub(in crate::tui) emoji_images: &'a mut EmojiImageCache,
    pub(in crate::tui) media_decode_tx: &'a mpsc::UnboundedSender<MediaImageDecodeResult>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::tui) struct EffectProcessingOutcome {
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
pub(in crate::tui) fn effect_forces_redraw(event: &AppEvent) -> bool {
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
    let member_hydration_messages = member_hydration_messages_for_event(&event);
    let thread_owner_hydration_infos = thread_owner_hydration_infos_for_event(&event);

    dispatch_runtime_side_effects(&event, ctx);
    record_media_event(&event, ctx);
    push_dashboard_effect(event, ctx);
    enqueue_missing_message_author_requests(member_hydration_messages, ctx);
    enqueue_missing_thread_owner_requests(thread_owner_hydration_infos, ctx);

    outcome
}

fn member_hydration_messages_for_event(event: &AppEvent) -> Option<Vec<MessageInfo>> {
    match event {
        AppEvent::MessageHistoryLoaded { messages, .. }
        | AppEvent::MessageHistoryRefreshed { messages, .. }
        | AppEvent::MessageHistoryAfterLoaded { messages, .. }
        | AppEvent::MessageHistoryAroundLoaded { messages, .. }
        | AppEvent::MessageSearchLoaded {
            page: crate::discord::MessageSearchPage { messages, .. },
        }
        | AppEvent::PinnedMessagesLoaded { messages, .. }
        | AppEvent::ForumPostsLoaded {
            first_messages: messages,
            ..
        } => Some(messages.clone()),
        _ => None,
    }
}

fn thread_owner_hydration_infos_for_event(event: &AppEvent) -> Option<Vec<ChannelInfo>> {
    match event {
        AppEvent::ForumPostsLoaded { threads, .. } => Some(threads.clone()),
        _ => None,
    }
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
    let preview_jobs = ctx.image_previews.record_event(event);
    for job in preview_jobs
        .into_iter()
        .chain(ctx.avatar_images.record_event(event))
        .chain(ctx.emoji_images.record_event(event))
    {
        spawn_media_image_decode(job, ctx.media_decode_tx.clone());
    }
}

fn push_dashboard_effect(event: AppEvent, ctx: &mut EffectContext<'_>) {
    if matches!(event, AppEvent::GatewayClosed) {
        handle_gateway_closed(ctx.state);
        return;
    }
    if matches!(event, AppEvent::SignedOut) {
        ctx.state.sign_out();
        return;
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

fn enqueue_missing_message_author_requests(
    messages: Option<Vec<MessageInfo>>,
    ctx: &mut EffectContext<'_>,
) {
    let Some(messages) = messages else {
        return;
    };
    let missing = ctx.state.missing_message_author_member_requests(&messages);
    let requests = ctx
        .client
        .next_message_author_member_requests(missing, std::time::Instant::now());
    ctx.state.enqueue_message_author_member_requests(requests);
}

fn enqueue_missing_thread_owner_requests(
    threads: Option<Vec<ChannelInfo>>,
    ctx: &mut EffectContext<'_>,
) {
    let Some(threads) = threads else {
        return;
    };
    let missing = ctx.state.missing_thread_owner_member_requests(&threads);
    let requests = ctx
        .client
        .next_message_author_member_requests(missing, std::time::Instant::now());
    ctx.state.enqueue_message_author_member_requests(requests);
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

pub(in crate::tui) fn process_sequenced_effect(
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

pub(in crate::tui) fn process_deferred_effects(
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
    use tokio::sync::mpsc;

    use crate::discord::ids::Id;
    use crate::discord::{
        AppCommand, AppEvent, ChannelInfo, ForumPostArchiveState, MemberInfo,
        MessageHistoryAfterMode, MessageInfo, RoleInfo,
    };

    use super::*;

    #[test]
    fn message_history_loaded_enqueues_missing_author_member_request() {
        let guild_id = Id::new(1);
        let channel_id = Id::new(2);
        let author_id = Id::new(99);
        let mut state = DashboardState::new();
        push_guild_with_channel(
            &mut state,
            guild_id,
            channel_info(guild_id, channel_id, None, "general", "GuildText"),
        );

        process_effect_in_default_context(
            &mut state,
            AppEvent::MessageHistoryLoaded {
                channel_id,
                before: None,
                messages: vec![message_info(guild_id, channel_id, Id::new(20), author_id)],
            },
        );

        assert_eq!(
            state.drain_pending_commands(),
            vec![AppCommand::LoadGuildMembersByIds {
                guild_id,
                user_ids: vec![author_id],
            }]
        );
    }

    #[test]
    fn forum_posts_loaded_enqueues_missing_preview_author_member_request() {
        let guild_id = Id::new(1);
        let forum_id = Id::new(2);
        let thread_id = Id::new(3);
        let author_id = Id::new(99);
        let mut state = DashboardState::new();
        push_guild_with_channel(
            &mut state,
            guild_id,
            channel_info(guild_id, forum_id, None, "forum", "forum"),
        );

        process_effect_in_default_context(
            &mut state,
            AppEvent::ForumPostsLoaded {
                channel_id: forum_id,
                archive_state: ForumPostArchiveState::Active,
                offset: 0,
                next_offset: 1,
                threads: vec![channel_info(
                    guild_id,
                    thread_id,
                    Some(forum_id),
                    "welcome",
                    "GuildPublicThread",
                )],
                first_messages: vec![message_info(guild_id, thread_id, Id::new(20), author_id)],
                has_more: false,
            },
        );

        assert_eq!(
            state.drain_pending_commands(),
            vec![AppCommand::LoadGuildMembersByIds {
                guild_id,
                user_ids: vec![author_id],
            }]
        );
    }

    #[test]
    fn forum_posts_loaded_enqueues_missing_thread_owner_member_request() {
        let guild_id = Id::new(1);
        let forum_id = Id::new(2);
        let thread_id = Id::new(3);
        let owner_id = Id::new(99);
        let mut state = DashboardState::new();
        push_guild_with_channel(
            &mut state,
            guild_id,
            channel_info(guild_id, forum_id, None, "forum", "forum"),
        );

        process_effect_in_default_context(
            &mut state,
            AppEvent::ForumPostsLoaded {
                channel_id: forum_id,
                archive_state: ForumPostArchiveState::Active,
                offset: 0,
                next_offset: 1,
                threads: vec![ChannelInfo {
                    owner_id: Some(owner_id),
                    ..channel_info(
                        guild_id,
                        thread_id,
                        Some(forum_id),
                        "welcome",
                        "GuildPublicThread",
                    )
                }],
                first_messages: Vec::new(),
                has_more: false,
            },
        );

        assert_eq!(
            state.drain_pending_commands(),
            vec![AppCommand::LoadGuildMembersByIds {
                guild_id,
                user_ids: vec![owner_id],
            }]
        );
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
            state.push_event(AppEvent::MessageHistoryLoaded {
                channel_id,
                before: None,
                messages: vec![message_info(guild_id, channel_id, Id::new(20), Id::new(99))],
            });

            process_effect_in_default_context(&mut state, event);

            assert_eq!(
                state.drain_pending_commands(),
                vec![AppCommand::LoadMessageHistoryAfter {
                    channel_id,
                    after: Id::new(20),
                    mode: MessageHistoryAfterMode::CatchUp,
                }]
            );
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
        state.push_event(AppEvent::GuildCreate {
            guild_id,
            name: "guild".to_owned(),
            member_count: None,
            owner_id: None,
            channels: vec![channel],
            members: Vec::<MemberInfo>::new(),
            presences: Vec::new(),
            roles: vec![RoleInfo::test(Id::new(guild_id.get()), "@everyone")],
            emojis: Vec::new(),
        });
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
        let mut image_previews = ImagePreviewCache::new();
        let mut avatar_images = AvatarImageCache::new();
        let mut emoji_images = EmojiImageCache::new();
        let (media_decode_tx, _media_decode_rx) = mpsc::unbounded_channel();
        let mut ctx = EffectContext {
            state,
            client: &client,
            image_previews: &mut image_previews,
            avatar_images: &mut avatar_images,
            emoji_images: &mut emoji_images,
            media_decode_tx: &media_decode_tx,
        };

        process_effect_event(event, &mut ctx);
    }
}
