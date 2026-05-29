use std::{
    collections::VecDeque,
    io::{Write, stdout},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::sync::Once;

use tokio::sync::mpsc;

use crate::{
    config::NotificationOptions,
    discord::{AppEvent, DiscordClient, SequencedAppEvent, VoiceSoundKind},
    logging,
};

use super::super::{
    media::{
        AvatarImageCache, EmojiImageCache, ImagePreviewCache, ImagePreviewDecodeResult,
        spawn_image_preview_decode,
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
    pub(in crate::tui) preview_decode_tx: &'a mpsc::UnboundedSender<ImagePreviewDecodeResult>,
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

pub(in crate::tui) fn effect_forces_redraw(event: &AppEvent) -> bool {
    // Attachment preview events are the shared media-completion path for
    // inline previews, avatars, emoji images, and profile-popup avatars. They
    // must redraw even when the visible dashboard signature is unchanged.
    matches!(
        event,
        AppEvent::AttachmentPreviewLoaded { .. }
            | AppEvent::AttachmentPreviewLoadFailed { .. }
            | AppEvent::GatewayError { .. }
            | AppEvent::GatewayClosed
    )
}

pub(super) fn process_effect_event(
    event: AppEvent,
    ctx: &mut EffectContext<'_>,
) -> EffectProcessingOutcome {
    let outcome = EffectProcessingOutcome::processed(&event);
    let member_hydration_messages = match &event {
        AppEvent::MessageHistoryLoaded { messages, .. }
        | AppEvent::PinnedMessagesLoaded { messages, .. }
        | AppEvent::ForumPostsLoaded {
            first_messages: messages,
            ..
        } => Some(messages.clone()),
        _ => None,
    };
    let thread_owner_hydration_infos = match &event {
        AppEvent::ForumPostsLoaded { threads, .. } => Some(threads.clone()),
        _ => None,
    };
    if let Some(notification) = ctx.state.desktop_notification_for_event(&event) {
        dispatch_desktop_notification(notification);
    }
    if let AppEvent::VoiceSound { kind } = event {
        dispatch_voice_sound(kind, ctx.state.notification_options());
    }
    for job in ctx.image_previews.record_event(&event) {
        spawn_image_preview_decode(job, ctx.preview_decode_tx.clone());
    }
    ctx.avatar_images.record_event(&event);
    ctx.emoji_images.record_event(&event);
    if matches!(event, AppEvent::GatewayClosed) {
        handle_gateway_closed(ctx.state);
    } else {
        ctx.state.push_effect(event);
    }
    if let Some(messages) = member_hydration_messages {
        let missing = ctx.state.missing_message_author_member_requests(&messages);
        let requests = ctx
            .client
            .next_message_author_member_requests(missing, std::time::Instant::now());
        ctx.state.enqueue_message_author_member_requests(requests);
    }
    if let Some(threads) = thread_owner_hydration_infos {
        let missing = ctx.state.missing_thread_owner_member_requests(&threads);
        let requests = ctx
            .client
            .next_message_author_member_requests(missing, std::time::Instant::now());
        ctx.state.enqueue_message_author_member_requests(requests);
    }
    outcome
}

fn dispatch_desktop_notification(notification: DesktopNotification) {
    tokio::spawn(async move {
        let title = notification.title;
        let body = notification.body;
        let result =
            tokio::task::spawn_blocking(move || deliver_desktop_notification(&title, &body)).await;

        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                log_notification_failure_once(
                    "notification",
                    format!("desktop notification and fallbacks failed: {error}"),
                );
                ring_terminal_bell();
            }
            Err(error) => {
                log_notification_failure_once(
                    "notification",
                    format!("desktop notification task failed: {error}"),
                );
                ring_terminal_bell();
            }
        }
    });
}

fn dispatch_voice_sound(kind: VoiceSoundKind, notification_options: NotificationOptions) {
    tokio::spawn(async move {
        let result =
            tokio::task::spawn_blocking(move || play_voice_sound(kind, notification_options)).await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                log_notification_failure_once("voice", format!("voice sound failed: {error}"));
                ring_terminal_bell();
            }
            Err(error) => {
                log_notification_failure_once("voice", format!("voice sound task failed: {error}"));
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

fn deliver_desktop_notification(title: &str, body: &str) -> std::result::Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        deliver_macos_notification(title, body)
    }
    #[cfg(not(target_os = "macos"))]
    {
        deliver_notify_rust_notification(title, body)
    }
}

fn play_voice_sound(
    kind: VoiceSoundKind,
    notification_options: NotificationOptions,
) -> std::result::Result<(), String> {
    let custom_path = voice_sound_path(kind, &notification_options);
    #[cfg(target_os = "macos")]
    {
        play_macos_voice_sound(kind, custom_path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        play_non_macos_voice_sound(kind, custom_path)
    }
}

fn voice_sound_path(kind: VoiceSoundKind, options: &NotificationOptions) -> Option<&Path> {
    match kind {
        VoiceSoundKind::Join => options.voice_join_sound.as_deref(),
        VoiceSoundKind::Leave => options.voice_leave_sound.as_deref(),
    }
}

#[cfg(not(target_os = "macos"))]
fn deliver_notify_rust_notification(title: &str, body: &str) -> std::result::Result<(), String> {
    notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .show()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(all(feature = "voice-playback", not(target_os = "macos")))]
fn play_non_macos_voice_sound(
    kind: VoiceSoundKind,
    custom_path: Option<&Path>,
) -> std::result::Result<(), String> {
    super::notification_audio::play_voice_sound(kind, custom_path)
}

#[cfg(all(not(feature = "voice-playback"), not(target_os = "macos")))]
fn play_non_macos_voice_sound(
    kind: VoiceSoundKind,
    custom_path: Option<&Path>,
) -> std::result::Result<(), String> {
    let _ = kind;
    let _ = custom_path;
    ring_terminal_bell();
    Ok(())
}

#[cfg(target_os = "macos")]
fn deliver_macos_notification(title: &str, body: &str) -> std::result::Result<(), String> {
    init_macos_notification_identity();
    // macOS can accept a notify-rust notification without presenting it or
    // playing its sound when the terminal app is frontmost. Keep every visual
    // notification path silent and let afplay own exactly one audible alert.
    let visual_result = deliver_macos_visual_notification(title, body);
    play_macos_sound_fallback().map_err(|sound_error| match visual_result {
        Ok(()) => format!("macOS notification sound failed: {sound_error}"),
        Err(visual_error) => {
            format!("macOS visual notification failed: {visual_error}; sound failed: {sound_error}")
        }
    })
}

#[cfg(target_os = "macos")]
fn deliver_macos_visual_notification(title: &str, body: &str) -> std::result::Result<(), String> {
    match notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .show()
    {
        Ok(_) => Ok(()),
        Err(primary_error) => {
            deliver_macos_fallback_notification(title, body).map_err(|fallback_error| {
                format!(
                    "notify-rust failed: {primary_error}; macOS fallback failed: {fallback_error}"
                )
            })
        }
    }
}

#[cfg(target_os = "macos")]
fn init_macos_notification_identity() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let Some(app_name) = std::env::var("TERM_PROGRAM")
            .ok()
            .and_then(|program| macos_terminal_app_name(&program))
        else {
            return;
        };
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

#[cfg(target_os = "macos")]
fn deliver_macos_fallback_notification(title: &str, body: &str) -> std::result::Result<(), String> {
    run_terminal_notifier(title, body).or_else(|terminal_error| {
        run_osascript_notification(title, body)
            .map_err(|osascript_error| format!("{terminal_error}; {osascript_error}"))
    })
}

#[cfg(target_os = "macos")]
fn run_terminal_notifier(title: &str, body: &str) -> std::result::Result<(), String> {
    command_success(
        Command::new("terminal-notifier")
            .args(["-title", title, "-message", body, "-group", "concord"]),
        "terminal-notifier",
    )
}

#[cfg(target_os = "macos")]
fn run_osascript_notification(title: &str, body: &str) -> std::result::Result<(), String> {
    let script = format!(
        "display notification {} with title {}",
        applescript_string(body),
        applescript_string(title),
    );
    command_success(Command::new("osascript").args(["-e", &script]), "osascript")
}

#[cfg(target_os = "macos")]
fn play_macos_sound_fallback() -> std::result::Result<(), String> {
    command_success(
        Command::new("afplay").arg("/System/Library/Sounds/Ping.aiff"),
        "afplay",
    )
}

#[cfg(target_os = "macos")]
fn play_macos_voice_sound(
    kind: VoiceSoundKind,
    custom_path: Option<&Path>,
) -> std::result::Result<(), String> {
    let default_path = match kind {
        VoiceSoundKind::Join => Path::new("/System/Library/Sounds/Ping.aiff"),
        VoiceSoundKind::Leave => Path::new("/System/Library/Sounds/Pop.aiff"),
    };
    command_success(
        Command::new("afplay").arg(custom_path.unwrap_or(default_path)),
        "afplay",
    )
}

#[cfg(target_os = "macos")]
fn command_success(command: &mut Command, label: &str) -> std::result::Result<(), String> {
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{label} exited with {status}")),
        Err(error) => Err(format!("{label} failed to start: {error}")),
    }
}

#[cfg(any(target_os = "macos", test))]
pub(in crate::tui) fn applescript_string(value: &str) -> String {
    let mut escaped = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' | '\r' => escaped.push(' '),
            _ => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
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
        AppCommand, AppEvent, ChannelInfo, ForumPostArchiveState, MemberInfo, MessageInfo, RoleInfo,
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
        let (preview_decode_tx, _preview_decode_rx) = mpsc::unbounded_channel();
        let mut ctx = EffectContext {
            state,
            client: &client,
            image_previews: &mut image_previews,
            avatar_images: &mut avatar_images,
            emoji_images: &mut emoji_images,
            preview_decode_tx: &preview_decode_tx,
        };

        process_effect_event(event, &mut ctx);
    }
}
