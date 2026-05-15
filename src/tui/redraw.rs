use crate::{
    config,
    discord::{MessageState, PresenceStatus},
    tui::state,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};

use super::state::DashboardState;

#[derive(Default)]
pub(super) struct RedrawDiagnostics {
    pub(super) key_presses: u32,
    pub(super) mouse_events: u32,
    pub(super) resizes: u32,
    pub(super) terminal_closed: u32,
    pub(super) preview_decodes: u32,
    pub(super) snapshot_events: u32,
    pub(super) effect_events: u32,
    pub(super) redraw_timer_fires: u32,
    pub(super) media_requests: u32,
    pub(super) request_failures: u32,
    pub(super) visible_image_previews_max: usize,
    pub(super) snapshot_message_changes: u32,
    pub(super) snapshot_member_changes: u32,
    pub(super) snapshot_channel_changes: u32,
    pub(super) snapshot_guild_changes: u32,
    pub(super) snapshot_popup_changes: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct VisibleDashboardSignature {
    focus: state::FocusPane,
    leader_active: bool,
    leader_action_mode: bool,
    channel_switcher_open: bool,
    channel_switcher_query: Option<String>,
    channel_switcher_query_cursor: Option<usize>,
    channel_switcher_selected: Option<usize>,
    channel_switcher_result_count: usize,
    channel_switcher_items: Vec<String>,
    guild_pane_visible: bool,
    channel_pane_visible: bool,
    member_pane_visible: bool,
    current_user: Option<String>,
    update_available_version: Option<String>,
    selected_guild_id: Option<Id<GuildMarker>>,
    selected_channel_id: Option<Id<ChannelMarker>>,
    guild_horizontal_scroll: usize,
    channel_horizontal_scroll: usize,
    selected_message: usize,
    message_scroll: usize,
    message_line_scroll: usize,
    pub(super) new_messages_count: usize,
    selected_member: usize,
    member_scroll: usize,
    member_horizontal_scroll: usize,
    channel_action_threads_phase: bool,
    message_pane_title: String,
    typing_footer: Option<String>,
    popups: VisiblePopupSignature,
    visible_guilds: Vec<String>,
    pub(super) visible_channels: Vec<String>,
    pub(super) visible_messages: Vec<MessageState>,
    visible_forum_posts: Vec<state::ChannelThreadItem>,
    visible_members: Vec<MemberEntrySignature>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VisiblePopupSignature {
    message_action_open: bool,
    image_viewer_open: bool,
    image_viewer_download_message: Option<String>,
    guild_leader_action_open: bool,
    channel_leader_action_open: bool,
    member_leader_action_open: bool,
    options_open: bool,
    selected_option: Option<usize>,
    display_options: config::DisplayOptions,
    emoji_picker_open: bool,
    reaction_users_open: bool,
    poll_vote_picker_open: bool,
    user_profile_open: bool,
    debug_log_open: bool,
    user_profile_data: String,
    user_profile_error: Option<String>,
    user_profile_status: PresenceStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MemberEntrySignature {
    user_id: Id<UserMarker>,
    display_name: String,
    username: Option<String>,
    is_bot: bool,
    status: PresenceStatus,
}

pub(super) fn visible_dashboard_signature(state: &DashboardState) -> VisibleDashboardSignature {
    let member_start = state.member_scroll();
    let member_end = member_start.saturating_add(state.member_content_height());
    VisibleDashboardSignature {
        focus: state.focus(),
        leader_active: state.is_leader_active(),
        leader_action_mode: state.is_leader_action_mode(),
        channel_switcher_open: state.is_channel_switcher_open(),
        channel_switcher_query: state.channel_switcher_query().map(str::to_owned),
        channel_switcher_query_cursor: state.channel_switcher_query_cursor_byte_index(),
        channel_switcher_selected: state.selected_channel_switcher_index(),
        channel_switcher_items: channel_switcher_item_signature(state),
        channel_switcher_result_count: state.channel_switcher_items().len(),
        guild_pane_visible: state.is_pane_visible(state::FocusPane::Guilds),
        channel_pane_visible: state.is_pane_visible(state::FocusPane::Channels),
        member_pane_visible: state.is_pane_visible(state::FocusPane::Members),
        current_user: state.current_user().map(str::to_owned),
        update_available_version: state.update_available_version().map(str::to_owned),
        selected_guild_id: state.selected_guild_id(),
        selected_channel_id: state.selected_channel_id(),
        guild_horizontal_scroll: state.guild_horizontal_scroll(),
        channel_horizontal_scroll: state.channel_horizontal_scroll(),
        selected_message: state.selected_message(),
        message_scroll: state.message_scroll(),
        message_line_scroll: state.message_line_scroll(),
        new_messages_count: state.new_messages_count(),
        selected_member: state.selected_member(),
        member_scroll: state.member_scroll(),
        member_horizontal_scroll: state.member_horizontal_scroll(),
        channel_action_threads_phase: state.is_channel_action_threads_phase(),
        message_pane_title: state.message_pane_title(),
        typing_footer: state.typing_footer_for_selected_channel(),
        popups: VisiblePopupSignature {
            message_action_open: state.is_message_action_menu_open(),
            image_viewer_open: state.is_image_viewer_open(),
            image_viewer_download_message: state.image_viewer_download_message().map(str::to_owned),
            guild_leader_action_open: state.is_guild_leader_action_active(),
            channel_leader_action_open: state.is_channel_leader_action_active(),
            member_leader_action_open: state.is_member_leader_action_active(),
            options_open: state.is_options_popup_open(),
            selected_option: state.selected_option_index(),
            display_options: state.display_options(),
            emoji_picker_open: state.is_emoji_reaction_picker_open(),
            reaction_users_open: state.is_reaction_users_popup_open(),
            poll_vote_picker_open: state.is_poll_vote_picker_open(),
            user_profile_open: state.is_user_profile_popup_open(),
            debug_log_open: state.is_debug_log_popup_open(),
            user_profile_data: format!("{:?}", state.user_profile_popup_data()),
            user_profile_error: state.user_profile_popup_load_error().map(str::to_owned),
            user_profile_status: state.user_profile_popup_status(),
        },
        visible_guilds: state
            .visible_guild_pane_entries()
            .into_iter()
            .map(|entry| match entry {
                state::GuildPaneEntry::DirectMessages => {
                    format!("{entry:?} unread={}", state.direct_message_unread_count())
                }
                state::GuildPaneEntry::Guild { state: guild, .. } => {
                    format!(
                        "{entry:?} unread={:?}",
                        state.sidebar_guild_unread(guild.id)
                    )
                }
                state::GuildPaneEntry::FolderHeader { .. } => format!("{entry:?}"),
            })
            .collect(),
        visible_channels: state
            .visible_channel_pane_entries()
            .into_iter()
            .map(|entry| match entry {
                state::ChannelPaneEntry::Channel { state: channel, .. } => format!(
                    "{entry:?} unread={:?} unread_messages={}",
                    state.channel_unread(channel.id),
                    state.channel_unread_message_count(channel.id)
                ),
                state::ChannelPaneEntry::VoiceParticipant { .. } => format!("{entry:?}"),
                state::ChannelPaneEntry::CategoryHeader { .. } => format!("{entry:?}"),
            })
            .collect(),
        visible_messages: state.visible_messages().into_iter().cloned().collect(),
        visible_forum_posts: state.visible_forum_post_items(),
        visible_members: state
            .flattened_members()
            .into_iter()
            .skip(member_start)
            .take(member_end.saturating_sub(member_start))
            .map(|entry| MemberEntrySignature {
                user_id: entry.user_id(),
                display_name: entry.display_name(),
                username: entry.username(),
                is_bot: entry.is_bot(),
                status: entry.status(),
            })
            .collect(),
    }
}

pub(super) fn record_visible_signature_change(
    diagnostics: &mut RedrawDiagnostics,
    before: &VisibleDashboardSignature,
    after: &VisibleDashboardSignature,
) {
    if before.selected_channel_id != after.selected_channel_id
        || before.message_pane_title != after.message_pane_title
        || before.selected_message != after.selected_message
        || before.message_scroll != after.message_scroll
        || before.message_line_scroll != after.message_line_scroll
        || before.new_messages_count != after.new_messages_count
        || before.typing_footer != after.typing_footer
        || before.visible_messages != after.visible_messages
        || before.visible_forum_posts != after.visible_forum_posts
    {
        diagnostics.snapshot_message_changes =
            diagnostics.snapshot_message_changes.saturating_add(1);
    }

    if before.selected_member != after.selected_member
        || before.member_scroll != after.member_scroll
        || before.member_horizontal_scroll != after.member_horizontal_scroll
        || before.visible_members != after.visible_members
    {
        diagnostics.snapshot_member_changes = diagnostics.snapshot_member_changes.saturating_add(1);
    }

    if before.selected_channel_id != after.selected_channel_id
        || before.channel_horizontal_scroll != after.channel_horizontal_scroll
        || before.visible_channels != after.visible_channels
    {
        diagnostics.snapshot_channel_changes =
            diagnostics.snapshot_channel_changes.saturating_add(1);
    }

    if before.selected_guild_id != after.selected_guild_id
        || before.guild_horizontal_scroll != after.guild_horizontal_scroll
        || before.visible_guilds != after.visible_guilds
    {
        diagnostics.snapshot_guild_changes = diagnostics.snapshot_guild_changes.saturating_add(1);
    }

    if before.focus != after.focus
        || before.leader_active != after.leader_active
        || before.leader_action_mode != after.leader_action_mode
        || before.channel_switcher_open != after.channel_switcher_open
        || before.channel_switcher_query != after.channel_switcher_query
        || before.channel_switcher_query_cursor != after.channel_switcher_query_cursor
        || before.channel_switcher_selected != after.channel_switcher_selected
        || before.channel_switcher_result_count != after.channel_switcher_result_count
        || before.channel_switcher_items != after.channel_switcher_items
        || before.guild_pane_visible != after.guild_pane_visible
        || before.channel_pane_visible != after.channel_pane_visible
        || before.member_pane_visible != after.member_pane_visible
        || before.current_user != after.current_user
        || before.popups != after.popups
        || before.channel_action_threads_phase != after.channel_action_threads_phase
    {
        diagnostics.snapshot_popup_changes = diagnostics.snapshot_popup_changes.saturating_add(1);
    }
}

fn only_visible_member_signature_changed(
    before: &VisibleDashboardSignature,
    after: &VisibleDashboardSignature,
) -> bool {
    before.focus == after.focus
        && before.leader_active == after.leader_active
        && before.leader_action_mode == after.leader_action_mode
        && before.channel_switcher_open == after.channel_switcher_open
        && before.channel_switcher_query == after.channel_switcher_query
        && before.channel_switcher_query_cursor == after.channel_switcher_query_cursor
        && before.channel_switcher_selected == after.channel_switcher_selected
        && before.channel_switcher_result_count == after.channel_switcher_result_count
        && before.channel_switcher_items == after.channel_switcher_items
        && before.guild_pane_visible == after.guild_pane_visible
        && before.channel_pane_visible == after.channel_pane_visible
        && before.member_pane_visible == after.member_pane_visible
        && before.current_user == after.current_user
        && before.update_available_version == after.update_available_version
        && before.selected_guild_id == after.selected_guild_id
        && before.selected_channel_id == after.selected_channel_id
        && before.guild_horizontal_scroll == after.guild_horizontal_scroll
        && before.channel_horizontal_scroll == after.channel_horizontal_scroll
        && before.selected_message == after.selected_message
        && before.message_scroll == after.message_scroll
        && before.message_line_scroll == after.message_line_scroll
        && before.new_messages_count == after.new_messages_count
        && before.message_pane_title == after.message_pane_title
        && before.typing_footer == after.typing_footer
        && before.popups == after.popups
        && before.channel_action_threads_phase == after.channel_action_threads_phase
        && before.visible_guilds == after.visible_guilds
        && before.visible_channels == after.visible_channels
        && before.visible_messages == after.visible_messages
        && before.visible_forum_posts == after.visible_forum_posts
        && (before.selected_member != after.selected_member
            || before.member_scroll != after.member_scroll
            || before.member_horizontal_scroll != after.member_horizontal_scroll
            || before.visible_members != after.visible_members)
}

fn only_new_message_notice_changed(
    before: &VisibleDashboardSignature,
    after: &VisibleDashboardSignature,
) -> bool {
    before.focus == after.focus
        && before.leader_active == after.leader_active
        && before.leader_action_mode == after.leader_action_mode
        && before.channel_switcher_open == after.channel_switcher_open
        && before.channel_switcher_query == after.channel_switcher_query
        && before.channel_switcher_query_cursor == after.channel_switcher_query_cursor
        && before.channel_switcher_selected == after.channel_switcher_selected
        && before.channel_switcher_result_count == after.channel_switcher_result_count
        && before.channel_switcher_items == after.channel_switcher_items
        && before.guild_pane_visible == after.guild_pane_visible
        && before.channel_pane_visible == after.channel_pane_visible
        && before.member_pane_visible == after.member_pane_visible
        && before.current_user == after.current_user
        && before.update_available_version == after.update_available_version
        && before.selected_guild_id == after.selected_guild_id
        && before.selected_channel_id == after.selected_channel_id
        && before.guild_horizontal_scroll == after.guild_horizontal_scroll
        && before.channel_horizontal_scroll == after.channel_horizontal_scroll
        && before.selected_message == after.selected_message
        && before.message_scroll == after.message_scroll
        && before.message_line_scroll == after.message_line_scroll
        && before.selected_member == after.selected_member
        && before.member_scroll == after.member_scroll
        && before.member_horizontal_scroll == after.member_horizontal_scroll
        && before.message_pane_title == after.message_pane_title
        && before.typing_footer == after.typing_footer
        && before.popups == after.popups
        && before.channel_action_threads_phase == after.channel_action_threads_phase
        && before.visible_guilds == after.visible_guilds
        && before.visible_channels == after.visible_channels
        && before.visible_messages == after.visible_messages
        && before.visible_forum_posts == after.visible_forum_posts
        && before.visible_members == after.visible_members
        && before.new_messages_count != after.new_messages_count
}

fn channel_switcher_item_signature(state: &DashboardState) -> Vec<String> {
    if !state.is_channel_switcher_open() {
        return Vec::new();
    }
    state
        .channel_switcher_items()
        .into_iter()
        .map(|item| {
            format!(
                "{}:{}:{:?}:{}:{}:{:?}:{}",
                item.channel_id.get(),
                item.group_label,
                item.parent_label,
                item.channel_label,
                item.depth,
                item.unread,
                item.unread_message_count,
            )
        })
        .collect()
}

pub(super) fn should_suppress_image_redraw_for_signature_change(
    before: &VisibleDashboardSignature,
    after: &VisibleDashboardSignature,
    image_surfaces_visible: bool,
) -> bool {
    image_surfaces_visible
        && ((after.focus != state::FocusPane::Members
            && only_visible_member_signature_changed(before, after))
            || (after.focus != state::FocusPane::Channels
                && only_new_message_notice_changed(before, after)))
}

pub(super) fn should_redraw_after_visible_signature_change(
    before: &VisibleDashboardSignature,
    after: &VisibleDashboardSignature,
    image_surfaces_visible: bool,
    force_redraw: bool,
) -> bool {
    force_redraw
        || (before != after
            && !should_suppress_image_redraw_for_signature_change(
                before,
                after,
                image_surfaces_visible,
            ))
}

pub(super) fn image_surfaces_visible(
    state: &DashboardState,
    image_targets_visible: bool,
    avatar_targets_visible: bool,
    emoji_targets_visible: bool,
) -> bool {
    image_targets_visible
        || avatar_targets_visible
        || emoji_targets_visible
        || (state.show_avatars() && state.user_profile_popup_avatar_url().is_some())
}
