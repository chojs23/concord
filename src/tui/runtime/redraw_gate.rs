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

use ratatui::layout::Rect;

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

    hasher.finish()
}

/// Fingerprint of everything that determines WHERE images sit on screen, and
/// whether a modal covers them: the terminal area, every scroll/selection
/// offset, the visible content whose length shifts the rows below it, and which
/// popup is open. When this changes between drawn frames an image has moved or
/// been covered/uncovered. Terminal graphics are a pixel layer that the cell
/// diff cannot erase by itself (and kitty placements survive plain overwrites),
/// so the caller draws one image-free frame first to repaint those cells before
/// the images are redrawn. Composer/popup *text* is deliberately excluded so
/// plain typing (which does not move images) never triggers that extra frame.
pub(super) fn image_layout_signature(state: &DashboardState, area: Rect) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_dbg(&mut hasher, &area);

    // Message pane: vertical scroll, selection, and the content whose wrapping
    // and count shift every image below it.
    hash_dbg(&mut hasher, &state.message_pane_source());
    hash_dbg(&mut hasher, &state.selected_guild_id());
    hash_dbg(&mut hasher, &state.selected_channel_id());
    state.selected_message().hash(&mut hasher);
    state.message_scroll().hash(&mut hasher);
    state.message_line_scroll().hash(&mut hasher);
    state.new_messages_count().hash(&mut hasher);
    // Only whether the typing strip is shown matters for layout, not its text.
    state
        .typing_footer_for_selected_channel()
        .is_some()
        .hash(&mut hasher);
    hash_dbg(&mut hasher, &state.visible_messages());
    hash_dbg(&mut hasher, &state.visible_thread_card_items());

    // Member pane and sidebars: scroll offsets, roster order, and entries all
    // move the avatars/emoji rendered in them.
    state.member_horizontal_scroll().hash(&mut hasher);
    state.guild_horizontal_scroll().hash(&mut hasher);
    state.channel_horizontal_scroll().hash(&mut hasher);
    let member_start = state.member_scroll();
    for entry in state
        .flattened_members()
        .into_iter()
        .skip(member_start)
        .take(state.member_content_height())
    {
        hash_dbg(&mut hasher, &(entry.user_id(), entry.status()));
    }
    hash_dbg(&mut hasher, &state.visible_guild_pane_entries());
    hash_dbg(&mut hasher, &state.visible_channel_pane_entries());

    // Overlays cover/uncover images; popup scroll moves images inside them.
    hash_dbg(&mut hasher, &state.active_modal_popup_kind());
    hash_dbg(&mut hasher, &state.leader_keymap_prefix());
    state.is_leader_action_mode().hash(&mut hasher);
    state.is_any_action_context_active().hash(&mut hasher);
    state
        .user_profile_popup_has_avatar_preview()
        .hash(&mut hasher);
    state.user_profile_popup_scroll().hash(&mut hasher);

    hasher.finish()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{image_layout_signature, view_signature};
    use crate::discord::AppEvent;
    use crate::tui::keybindings::KeyChord;
    use crate::tui::state::DashboardState;
    use ratatui::layout::Rect;

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

    #[test]
    fn image_layout_signature_is_stable_and_tracks_repositioning() {
        let state = DashboardState::new();
        let area = Rect::new(0, 0, 80, 24);
        // Stable for unchanged state + area, so steady frames draw no extra
        // image-free frame.
        assert_eq!(
            image_layout_signature(&state, area),
            image_layout_signature(&state, area)
        );
        // A resize repositions every image, so the signature must move.
        assert_ne!(
            image_layout_signature(&state, area),
            image_layout_signature(&state, Rect::new(0, 0, 100, 24))
        );
    }

    #[test]
    fn image_layout_signature_tracks_leader_popup_geometry_context() {
        let area = Rect::new(0, 0, 80, 24);
        let mut state = DashboardState::new();
        state.open_leader();
        let before = image_layout_signature(&state, area);

        state.push_leader_keymap_key(KeyChord::from_str("v").expect("v should parse"));

        assert_ne!(before, image_layout_signature(&state, area));
    }
}
