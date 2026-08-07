//! Lightweight redraw gate.
//!
//! Foreground input always redraws immediately, so it does not need a gate.
//! Background Discord traffic (presence, typing, off-screen messages) is
//! different: most of it does not change what is currently on screen, and
//! redrawing for it just rebuilds an identical frame. To avoid that, we hash the
//! parts of the dashboard that a *background* event can change and only redraw
//! when that hash moves.
//!
//! This deliberately ignores most purely input-driven state (scroll offsets,
//! popup selection indices, which popup is open, and composer text): those only
//! change in response to a key or mouse event, which already triggers an
//! immediate redraw. Leaving them out keeps the hash small. Media-cache changes
//! (an inline preview or avatar finishing or failing to load) live outside the
//! dashboard state, so they are handled separately by `effect_forces_redraw`.

use std::collections::hash_map::DefaultHasher;
use std::fmt::{self, Write as _};
use std::hash::{Hash as _, Hasher as _};

use crate::tui::state::{
    ActiveModalPopupKind, ChannelPaneRow, DashboardState, NotificationInboxTab,
};

/// Hash a value's `Debug` output into the running hasher. Lets us fingerprint
/// view state without requiring every involved type to implement `Hash`.
fn hash_dbg<T: fmt::Debug>(hasher: &mut DefaultHasher, value: &T) {
    struct DebugSink<'a>(&'a mut DefaultHasher);
    impl fmt::Write for DebugSink<'_> {
        fn write_str(&mut self, value: &str) -> fmt::Result {
            self.0.write(value.as_bytes());
            Ok(())
        }
    }
    write!(DebugSink(hasher), "{value:?}").expect("writing into view hasher cannot fail");
}

/// Fingerprint of everything a background event could change on the visible
/// dashboard. Two frames with the same signature look identical, so a background
/// event that leaves it unchanged needs no redraw.
pub(super) fn view_signature(state: &DashboardState) -> u64 {
    let mut hasher = DefaultHasher::new();

    // Selection context, so the hash is compared against the right baseline when
    // the view switches channels or opens a popup.
    hash_dbg(&mut hasher, &state.message_pane_source());
    hash_dbg(&mut hasher, &state.selected_guild_id());
    hash_dbg(&mut hasher, &state.selected_channel_id());
    hash_dbg(&mut hasher, &state.active_modal_popup_kind());

    // Toasts are global overlays and several effect-only events change only
    // this state. Omitting them leaves success, failure, and loading feedback
    // stale until the next input-driven frame.
    hash_dbg(&mut hasher, &state.toast_message());

    // Header.
    hash_dbg(&mut hasher, &state.current_user());
    hash_dbg(&mut hasher, &state.current_voice_self_status());
    hash_dbg(&mut hasher, &state.active_voice_connection_label());
    hash_dbg(&mut hasher, &state.update_available_version());
    hash_dbg(&mut hasher, &state.stream_info_sections());

    // Message pane: the live chat plus its footers.
    hash_dbg(&mut hasher, &state.visible_messages());
    // Loading history around a referenced message moves the selection from a
    // background effect. Hash its stable id so that move schedules the frame
    // which centers the target, even when the currently visible rows did not
    // change during the cache merge.
    hash_dbg(
        &mut hasher,
        &state.selected_message_state().map(|message| message.id),
    );
    hash_dbg(&mut hasher, &state.visible_thread_card_items());
    hash_dbg(&mut hasher, &state.typing_footer_for_selected_channel());
    hash_dbg(&mut hasher, &state.composer_lock());
    state.new_messages_count().hash(&mut hasher);

    // Guild sidebar with its unread badges.
    state.direct_message_unread_count().hash(&mut hasher);
    for entry in state.visible_guild_pane_entries() {
        hash_dbg(&mut hasher, &entry);
        if let Some(guild) = entry.guild_state() {
            hash_dbg(&mut hasher, &state.sidebar_guild_unread(guild.id));
        }
    }

    // Channel sidebar with its unread badges.
    for row in state.visible_channel_pane_rows() {
        match row {
            ChannelPaneRow::Entry { entry, .. } => {
                hash_dbg(&mut hasher, &entry);
                if let Some(channel) = entry.channel_state() {
                    hash_dbg(&mut hasher, &state.channel_unread(channel.id));
                    state
                        .channel_unread_message_count(channel.id)
                        .hash(&mut hasher);
                }
            }
            ChannelPaneRow::Activity {
                owner_entry_index,
                recipient_id,
                activity,
                ..
            } => {
                hash_dbg(&mut hasher, &(owner_entry_index, recipient_id, activity));
            }
        }
    }

    // Member pane: presence and roster updates arrive in the background.
    let member_start = state.member_scroll();
    let member_take = state.member_content_height();
    for entry in state
        .flattened_members()
        .into_iter()
        .skip(member_start)
        .take(member_take)
    {
        hash_dbg(
            &mut hasher,
            &(
                entry.user_id(),
                entry.display_name(),
                entry.username(),
                entry.is_bot(),
                entry.status(),
            ),
        );
    }

    // Popups whose contents load or update from the background. (Their open/close
    // and navigation are input-driven and covered by the immediate redraw.)
    hash_dbg(&mut hasher, &state.selected_attachment_viewer_item());
    hash_dbg(&mut hasher, &state.user_profile_popup_data());
    hash_dbg(&mut hasher, &state.user_profile_popup_status());
    hash_dbg(&mut hasher, &state.user_profile_popup_load_error());
    hash_dbg(&mut hasher, &state.user_profile_popup_avatar_url());
    hash_dbg(&mut hasher, &state.user_profile_popup_activities());
    hash_dbg(&mut hasher, &state.user_profile_activity_picker_rows());
    hash_dbg(&mut hasher, &state.attachment_downloads());
    hash_dbg(&mut hasher, &state.reaction_users_popup());
    hash_dbg(&mut hasher, &state.existing_emoji_reactions());
    hash_dbg(&mut hasher, &state.own_emoji_reactions());
    hash_dbg(&mut hasher, &state.filtered_emoji_reaction_items());
    hash_dbg(&mut hasher, &state.poll_vote_picker_items());
    hash_dbg(&mut hasher, &state.composer_mention_candidates());
    hash_dbg(&mut hasher, &state.composer_emoji_candidates());
    hash_dbg(&mut hasher, &state.composer_command_candidates());

    // Voice device enumeration completes in the background while the Options
    // popup stays open. Hash the rendered rows rather than request internals so
    // every visible loading, success, or failure transition is covered.
    if state.is_active_modal_popup(ActiveModalPopupKind::Options) {
        hash_dbg(&mut hasher, &state.display_option_items());
    }

    // Search results and the forum loading placeholder both change from
    // background responses rather than direct input.
    hash_dbg(&mut hasher, &state.search_popup_view());
    state.selected_forum_posts_loading().hash(&mut hasher);

    // Notification inbox: only hash it while open. The active items, unread
    // mention badge, and loading state can all change from background events.
    if let Some(tab) = state.notification_inbox_tab() {
        hash_dbg(&mut hasher, &state.notification_inbox_items());
        if tab == NotificationInboxTab::Mentions {
            hash_dbg(&mut hasher, &state.notification_inbox_mentions_status());
        }
        state
            .notification_inbox_unread_mention_count()
            .hash(&mut hasher);
        state
            .notification_inbox_has_visible_loading_indicator()
            .hash(&mut hasher);
    }

    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::view_signature;
    use crate::discord::ids::Id;
    use crate::discord::{
        ActivityInfo, ActivityKind, AppCommand, AppEvent, ChannelInfo, ChannelRecipientInfo,
        MessageHistoryLoadTarget, MessageInfo, MessageSearchPage, PresenceEventFields,
        PresenceStatus, StreamCreateInfo, StreamUpdateInfo, VoiceScope,
    };
    use crate::tui::keybindings::OptionsCategoryShortcut;
    use crate::tui::state::{DashboardState, FocusPane};

    fn assert_signature_changes(
        label: &str,
        state: &mut DashboardState,
        change: impl FnOnce(&mut DashboardState),
    ) {
        let before = view_signature(state);
        change(state);
        assert_ne!(before, view_signature(state), "{label}");
    }

    #[test]
    fn view_signature_tracks_background_visible_state() {
        // An unchanged dashboard keeps a stable signature, while header and
        // search results loaded in the background move it.
        {
            let mut state = DashboardState::new();
            assert_eq!(view_signature(&state), view_signature(&state));
            assert_signature_changes("update banner", &mut state, |state| {
                state.push_event(AppEvent::UpdateAvailable {
                    latest_version: "9.9.9".to_owned(),
                });
            });

            let mut state = DashboardState::new();
            state.open_message_search_popup();
            state.push_search_char('x');
            let command = state.activate_search_popup().expect("search should start");
            let AppCommand::SearchMessages { query } = command else {
                panic!("expected message search command");
            };
            assert_signature_changes("message search results", &mut state, |state| {
                state.push_event(AppEvent::MessageSearchLoaded {
                    page: MessageSearchPage {
                        query,
                        messages: vec![MessageInfo::test(Id::new(20), Id::new(30))],
                        total_results: Some(1),
                        has_more: false,
                    },
                });
            });
        }

        // Composer history status is visible even when a history response has
        // no messages, so both success and failure must schedule a redraw.
        {
            let channel_id = Id::new(20);
            let mut state = DashboardState::new();
            state.push_event(AppEvent::ChannelUpsert(ChannelInfo::test(channel_id, "dm")));
            state.confirm_selected_guild();
            state.confirm_selected_channel();
            assert_signature_changes("message history loaded", &mut state, |state| {
                state.push_event(AppEvent::MessageHistoryLoaded {
                    channel_id,
                    before: None,
                    messages: Vec::new(),
                });
            });
            assert_signature_changes("message history failed", &mut state, |state| {
                state.push_event(AppEvent::MessageHistoryLoadFailed {
                    channel_id,
                    target: MessageHistoryLoadTarget::Latest,
                    message: "offline".to_owned(),
                });
            });
        }

        // Around-history loading can move only the selected message. The rows
        // visible before frame preparation stay unchanged, so the selection id
        // itself must move the signature.
        {
            let channel_id = Id::new(20);
            let mut state = DashboardState::new();
            state.push_event(AppEvent::ChannelUpsert(ChannelInfo::test(channel_id, "dm")));
            state.confirm_selected_guild();
            state.confirm_selected_channel();
            state.focus_pane(FocusPane::Messages);
            state.set_message_view_height(1);
            state.push_event(AppEvent::MessageHistoryLoaded {
                channel_id,
                before: None,
                messages: (1..=10)
                    .map(|message_id| MessageInfo::test(channel_id, Id::new(message_id)))
                    .collect(),
            });
            state.clamp_message_viewport_for_image_previews(80, 0, 0);
            let visible_before = state
                .visible_messages()
                .into_iter()
                .map(|message| message.id)
                .collect::<Vec<_>>();
            assert_signature_changes("referenced message selection", &mut state, |state| {
                state.push_event(AppEvent::MessageHistoryAroundLoaded {
                    channel_id,
                    message_id: Id::new(5),
                    messages: (4..=6)
                        .map(|message_id| MessageInfo::test(channel_id, Id::new(message_id)))
                        .collect(),
                });
            });
            assert_eq!(
                state.selected_message_state().map(|message| message.id),
                Some(Id::new(5))
            );
            assert_eq!(
                state
                    .visible_messages()
                    .into_iter()
                    .map(|message| message.id)
                    .collect::<Vec<_>>(),
                visible_before
            );
        }

        // Toast-only effects and async Voice Options rows are both rendered UI
        // even though neither changes the Discord snapshot.
        {
            let mut state = DashboardState::new();
            assert_signature_changes("toast", &mut state, |state| {
                state.push_effect(AppEvent::CaptchaRequired {
                    action: "send a message".to_owned(),
                });
            });

            let mut state = DashboardState::new();
            state.open_options_category_from_shortcut(OptionsCategoryShortcut::Voice);
            let commands = state.drain_pending_commands();
            let [AppCommand::LoadVoiceAudioSources { request_id }] = commands.as_slice() else {
                panic!("voice options should load audio sources");
            };
            let request_id = *request_id;
            assert_signature_changes("voice audio sources", &mut state, |state| {
                state.push_effect(AppEvent::VoiceAudioSourcesLoaded {
                    request_id,
                    inputs: vec![("mic-1".to_owned(), "Desk microphone".to_owned())],
                    outputs: vec![("speaker-1".to_owned(), "Headphones".to_owned())],
                    error: None,
                });
            });
        }

        // Presence and stream updates are snapshot-driven but affect visible
        // sidebar and header content.
        {
            let user_id = Id::new(10);
            let mut state = DashboardState::new();
            state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
                recipients: Some(vec![ChannelRecipientInfo {
                    status: Some(PresenceStatus::Online),
                    ..ChannelRecipientInfo::test(user_id, "alice")
                }]),
                ..ChannelInfo::test(Id::new(20), "dm")
            }));
            state.confirm_selected_guild();
            state.set_channel_view_height(4);
            state.push_event(AppEvent::PresenceUpdate {
                guild_id: None,
                presence: PresenceEventFields {
                    user_id,
                    status: PresenceStatus::Online,
                    activities: vec![ActivityInfo::test(ActivityKind::Playing, "Game A")],
                },
            });
            assert_signature_changes("visible DM activity", &mut state, |state| {
                state.push_event(AppEvent::PresenceUpdate {
                    guild_id: None,
                    presence: PresenceEventFields {
                        user_id,
                        status: PresenceStatus::Online,
                        activities: vec![ActivityInfo::test(ActivityKind::Playing, "Game B")],
                    },
                });
            });

            let guild_id = Id::new(1);
            let channel_id = Id::new(10);
            let current_user_id = Id::new(20);
            let viewer_id = Id::new(30);
            let scope = VoiceScope::Guild(guild_id);
            let mut state = DashboardState::new();
            state.push_event(AppEvent::Ready {
                user: "Me".to_owned(),
                user_id: Some(current_user_id),
            });
            state.push_event(AppEvent::ReadyUserDirectory {
                users: vec![ChannelRecipientInfo::test(viewer_id, "Viewer")],
            });
            state.push_event(AppEvent::StreamCreate {
                stream: StreamCreateInfo {
                    stream_key: "guild:1:10:20".to_owned(),
                    rtc_server_id: "100".to_owned(),
                    rtc_channel_id: Id::new(101),
                    viewer_ids: vec![viewer_id],
                    paused: false,
                },
            });
            state.show_stream_broadcast_preparing_toast(scope, channel_id);
            state.push_effect(AppEvent::StreamBroadcastStarted { scope, channel_id });
            assert_signature_changes("stream viewer list", &mut state, |state| {
                state.push_event(AppEvent::StreamUpdate {
                    stream: StreamUpdateInfo {
                        stream_key: "guild:1:10:20".to_owned(),
                        viewer_ids: Vec::new(),
                        paused: false,
                    },
                });
            });
        }
    }
}
