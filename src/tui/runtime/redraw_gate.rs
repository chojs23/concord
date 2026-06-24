//! Lightweight redraw gate.
//!
//! Foreground input always redraws immediately, so it does not need a gate.
//! Background Discord traffic (presence, typing, off-screen messages) is
//! different: most of it does not change what is currently on screen, and
//! redrawing for it just rebuilds an identical frame. To avoid that, we hash the
//! parts of the dashboard that a *background* event can change and only redraw
//! when that hash moves.
//!
//! This deliberately ignores purely input-driven state (scroll offsets,
//! selection indices, which popup is open, composer text, option values): those
//! only change in response to a key or mouse event, which already triggers an
//! immediate redraw. Leaving them out keeps the hash small. Media-cache changes
//! (an inline preview or avatar finishing or failing to load) live outside the
//! dashboard state, so they are handled separately by `effect_forces_redraw`.

use std::collections::hash_map::DefaultHasher;
use std::fmt::{self, Write as _};
use std::hash::{Hash as _, Hasher as _};

use crate::tui::state::DashboardState;

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

    // Header.
    hash_dbg(&mut hasher, &state.current_user());
    hash_dbg(&mut hasher, &state.current_voice_self_status());
    hash_dbg(&mut hasher, &state.update_available_version());

    // Message pane: the live chat plus its footers.
    hash_dbg(&mut hasher, &state.visible_messages());
    hash_dbg(&mut hasher, &state.visible_thread_card_items());
    hash_dbg(&mut hasher, &state.typing_footer_for_selected_channel());
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
    for entry in state.visible_channel_pane_entries() {
        hash_dbg(&mut hasher, &entry);
        if let Some(channel) = entry.channel_state() {
            hash_dbg(&mut hasher, &state.channel_unread(channel.id));
            state
                .channel_unread_message_count(channel.id)
                .hash(&mut hasher);
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
    hash_dbg(&mut hasher, &state.attachment_downloads());
    hash_dbg(&mut hasher, &state.reaction_users_popup());
    hash_dbg(&mut hasher, &state.existing_emoji_reactions());
    hash_dbg(&mut hasher, &state.own_emoji_reactions());
    hash_dbg(&mut hasher, &state.filtered_emoji_reaction_items());
    hash_dbg(&mut hasher, &state.poll_vote_picker_items());
    hash_dbg(&mut hasher, &state.composer_mention_candidates());
    hash_dbg(&mut hasher, &state.composer_emoji_candidates());
    hash_dbg(&mut hasher, &state.composer_command_candidates());

    // Notification inbox: its messages stream in from background REST responses.
    hash_dbg(&mut hasher, &state.notification_inbox_items());
    hash_dbg(&mut hasher, &state.notification_inbox_mentions_status());
    state.notification_inbox_unread_count().hash(&mut hasher);
    state.notification_inbox_mention_count().hash(&mut hasher);

    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::view_signature;
    use crate::discord::AppEvent;
    use crate::tui::state::DashboardState;

    #[test]
    fn view_signature_is_stable_and_tracks_visible_changes() {
        let state = DashboardState::new();
        let before = view_signature(&state);
        // Recomputing over unchanged state yields the same fingerprint, so a
        // background event that changes nothing visible will not redraw.
        assert_eq!(before, view_signature(&state));

        // A header-visible change (the update banner) moves the fingerprint.
        let mut state = state;
        state.push_event(AppEvent::UpdateAvailable {
            latest_version: "9.9.9".to_owned(),
        });
        assert_ne!(before, view_signature(&state));
    }
}
