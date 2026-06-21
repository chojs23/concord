use std::{
    collections::hash_map::DefaultHasher,
    fmt::{self, Write as _},
    hash::Hasher,
};

use crate::{
    config,
    discord::{ChannelUnreadState, PresenceStatus},
    tui::state,
};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};

use super::super::state::{ActiveModalPopupKind, DashboardState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct VisibleDashboardSignature {
    layout: LayoutSignature,
    header: HeaderSignature,
    overlay: OverlaySignature,
    passive_overlay: PassiveOverlaySignature,
    pub(in crate::tui) guilds: GuildPaneSignature,
    pub(in crate::tui) channels: ChannelPaneSignature,
    pub(in crate::tui) messages: MessagePaneSignature,
    members: MemberPaneSignature,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LayoutSignature {
    focus: state::FocusPane,
    guild_pane_visible: bool,
    channel_pane_visible: bool,
    member_pane_visible: bool,
    selected_guild_id: Option<Id<GuildMarker>>,
    selected_channel_id: Option<Id<ChannelMarker>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HeaderSignature {
    current_user: Option<String>,
    current_voice_self_status: (bool, bool),
    update_available_version: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OverlaySignature {
    active_modal_popup_kind: Option<ActiveModalPopupKind>,
    leader_active: bool,
    leader_action_mode: bool,
    leader_title: Option<String>,
    leader_shortcuts: Vec<(String, String, bool)>,
    channel_switcher: ChannelSwitcherSignature,
    channel_action_threads_phase: bool,
    popups: VisiblePopupSignature,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PassiveOverlaySignature {
    attachment_downloads: Vec<DebugSignature>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChannelSwitcherSignature {
    open: bool,
    query: Option<String>,
    query_cursor: Option<usize>,
    selected: Option<usize>,
    result_count: usize,
    items: Vec<ChannelSwitcherItemSignature>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct GuildPaneSignature {
    guild_horizontal_scroll: usize,
    pub(in crate::tui) visible_guilds: Vec<GuildEntrySignature>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct ChannelPaneSignature {
    channel_horizontal_scroll: usize,
    pub(in crate::tui) visible_channels: Vec<ChannelEntrySignature>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct MessagePaneSignature {
    source: Option<state::MessagePaneSource>,
    selected_message: usize,
    message_scroll: usize,
    message_line_scroll: usize,
    pub(in crate::tui) new_messages_count: usize,
    message_pane_title: String,
    typing_footer: Option<String>,
    composer_picker: ComposerPickerSignature,
    pub(in crate::tui) visible_messages: Vec<DebugSignature>,
    visible_forum_posts: Vec<DebugSignature>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ComposerPickerSignature {
    None,
    Mention {
        query: String,
        selected: usize,
        candidates: DebugSignature,
    },
    Emoji {
        query: String,
        selected: usize,
        candidates: DebugSignature,
    },
    Command {
        query: String,
        selected: usize,
        candidates: DebugSignature,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MemberPaneSignature {
    selected_member: usize,
    member_scroll: usize,
    member_horizontal_scroll: usize,
    visible_members: Vec<MemberEntrySignature>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VisiblePopupSignature {
    message_actions: MessageActionPopupSignature,
    message_url_picker: MessageUrlPickerPopupSignature,
    attachment_viewer: AttachmentViewerPopupSignature,
    leaders: LeaderPopupSignature,
    options: OptionsPopupSignature,
    message_interactions: MessageInteractionPopupSignature,
    profile: ProfilePopupSignature,
    diagnostics: DiagnosticsPopupSignature,
    search: DebugSignature,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MessageActionPopupSignature {
    message_action_open: bool,
    selected_message_action_index: Option<usize>,
    message_action_items: DebugSignature,
    message_confirmation_lines: DebugSignature,
    quit_confirmation_open: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MessageUrlPickerPopupSignature {
    message_url_picker_open: bool,
    selected_message_url_index: Option<usize>,
    message_url_items: DebugSignature,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AttachmentViewerPopupSignature {
    attachment_viewer_open: bool,
    selected_attachment_viewer_item: DebugSignature,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LeaderPopupSignature {
    guild_leader_action_open: bool,
    guild_action_items: DebugSignature,
    channel_leader_action_open: bool,
    selected_channel_action_index: Option<usize>,
    channel_action_items: DebugSignature,
    channel_thread_items: DebugSignature,
    member_leader_action_open: bool,
    member_action_items: DebugSignature,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OptionsPopupSignature {
    options_open: bool,
    options_title: &'static str,
    selected_option: Option<usize>,
    display_options: config::DisplayOptions,
    notification_options: config::NotificationOptions,
    voice_options: config::VoiceOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MessageInteractionPopupSignature {
    emoji_picker_open: bool,
    selected_emoji_reaction_index: Option<usize>,
    emoji_reaction_filter: Option<String>,
    filtered_emoji_reaction_items: DebugSignature,
    existing_emoji_reactions: DebugSignature,
    own_emoji_reactions: DebugSignature,
    reaction_users_open: bool,
    reaction_users_popup: DebugSignature,
    poll_vote_picker_open: bool,
    selected_poll_vote_picker_index: Option<usize>,
    poll_vote_picker_items: DebugSignature,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProfilePopupSignature {
    user_profile_open: bool,
    user_profile_data: DebugSignature,
    user_profile_error: Option<String>,
    user_profile_status: PresenceStatus,
    user_profile_settings: DebugSignature,
    user_profile_scroll: usize,
    user_profile_avatar_url: Option<String>,
    user_profile_activities: DebugSignature,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DiagnosticsPopupSignature {
    debug_log_open: bool,
    debug_log_lines: DebugSignature,
    debug_channel_visibility: DebugSignature,
    keymap_help_open: bool,
    keymap_help: DebugSignature,
    keymap_popup_scroll: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MemberEntrySignature {
    user_id: Id<UserMarker>,
    display_name: String,
    username: Option<String>,
    is_bot: bool,
    status: PresenceStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) struct DebugSignature(u64);

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChannelSwitcherItemSignature {
    channel_id: Id<ChannelMarker>,
    group_label: String,
    parent_label: Option<String>,
    channel_label: String,
    depth: usize,
    unread: ChannelUnreadState,
    unread_message_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) struct GuildEntrySignature {
    row: DebugSignature,
    unread_count: Option<usize>,
    unread_state: Option<ChannelUnreadState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) struct ChannelEntrySignature {
    row: DebugSignature,
    unread: Option<ChannelUnreadState>,
    unread_message_count: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct VisibleDashboardChangeSet {
    layout: bool,
    overlay: bool,
    passive_overlay: bool,
    header: bool,
    guilds: bool,
    channels: bool,
    messages: bool,
    members: bool,
    new_message_notice: bool,
}

impl VisibleDashboardChangeSet {
    fn only_members_changed(self) -> bool {
        self.members
            && !self.layout
            && !self.overlay
            && !self.passive_overlay
            && !self.header
            && !self.guilds
            && !self.channels
            && !self.messages
            && !self.new_message_notice
    }

    fn only_new_message_notice_changed(self) -> bool {
        self.new_message_notice
            && !self.layout
            && !self.overlay
            && !self.passive_overlay
            && !self.header
            && !self.guilds
            && !self.channels
            && !self.messages
            && !self.members
    }
}

pub(in crate::tui) fn visible_dashboard_signature(
    state: &DashboardState,
) -> VisibleDashboardSignature {
    VisibleDashboardSignature {
        layout: layout_signature(state),
        header: header_signature(state),
        overlay: overlay_signature(state),
        passive_overlay: passive_overlay_signature(state),
        guilds: guild_pane_signature(state),
        channels: channel_pane_signature(state),
        messages: message_pane_signature(state),
        members: member_pane_signature(state),
    }
}

fn layout_signature(state: &DashboardState) -> LayoutSignature {
    LayoutSignature {
        focus: state.focus(),
        guild_pane_visible: state.is_pane_visible(state::FocusPane::Guilds),
        channel_pane_visible: state.is_pane_visible(state::FocusPane::Channels),
        member_pane_visible: state.is_pane_visible(state::FocusPane::Members),
        selected_guild_id: state.selected_guild_id(),
        selected_channel_id: state.selected_channel_id(),
    }
}

fn header_signature(state: &DashboardState) -> HeaderSignature {
    HeaderSignature {
        current_user: state.current_user().map(str::to_owned),
        current_voice_self_status: state.current_voice_self_status(),
        update_available_version: state.update_available_version().map(str::to_owned),
    }
}

fn overlay_signature(state: &DashboardState) -> OverlaySignature {
    let channel_switcher_open = state.is_active_modal_popup(ActiveModalPopupKind::ChannelSwitcher);
    let leader_active = state.is_active_modal_popup(ActiveModalPopupKind::Leader);
    let channel_switcher_items = if channel_switcher_open {
        state.channel_switcher_items()
    } else {
        Vec::new()
    };
    OverlaySignature {
        active_modal_popup_kind: state.active_modal_popup_kind(),
        leader_active,
        leader_action_mode: state.is_leader_action_mode(),
        leader_title: leader_active.then(|| state.leader_keymap_title()),
        leader_shortcuts: state
            .leader_keymap_shortcuts()
            .into_iter()
            .map(|item| (item.key, item.label, item.has_children))
            .collect(),
        channel_switcher: ChannelSwitcherSignature {
            open: channel_switcher_open,
            query: state.channel_switcher_query().map(str::to_owned),
            query_cursor: state.channel_switcher_query_cursor_byte_index(),
            selected: state.selected_channel_switcher_index(),
            result_count: channel_switcher_items.len(),
            items: channel_switcher_item_signature(&channel_switcher_items),
        },
        channel_action_threads_phase: state.is_channel_action_threads_phase(),
        popups: visible_popup_signature(state),
    }
}

fn visible_popup_signature(state: &DashboardState) -> VisiblePopupSignature {
    let message_action_open = state.is_message_action_context_active();
    let message_url_picker_open =
        state.is_active_modal_popup(ActiveModalPopupKind::MessageUrlPicker);
    let attachment_viewer_open =
        state.is_active_modal_popup(ActiveModalPopupKind::AttachmentViewer);
    let options_open = state.is_active_modal_popup(ActiveModalPopupKind::Options);
    let emoji_picker_open = state.is_active_modal_popup(ActiveModalPopupKind::EmojiReactionPicker);
    let reaction_users_open = state.is_active_modal_popup(ActiveModalPopupKind::ReactionUsers);
    let poll_vote_picker_open = state.is_active_modal_popup(ActiveModalPopupKind::PollVotePicker);
    let user_profile_open = state.is_active_modal_popup(ActiveModalPopupKind::UserProfile);
    let debug_log_open = state.is_active_modal_popup(ActiveModalPopupKind::DebugLog);
    let keymap_help_open = state.is_active_modal_popup(ActiveModalPopupKind::KeymapHelp);
    let quit_confirmation_open =
        state.is_active_modal_popup(ActiveModalPopupKind::QuitConfirmation);

    VisiblePopupSignature {
        message_actions: MessageActionPopupSignature {
            message_action_open,
            selected_message_action_index: state.selected_message_action_index(),
            message_action_items: if message_action_open {
                debug_signature(&state.selected_message_action_items())
            } else {
                debug_signature(&())
            },
            message_confirmation_lines: debug_signature(&state.message_confirmation_lines()),
            quit_confirmation_open,
        },
        message_url_picker: MessageUrlPickerPopupSignature {
            message_url_picker_open,
            selected_message_url_index: state.selected_message_url_index(),
            message_url_items: if message_url_picker_open {
                debug_signature(&state.selected_message_url_items())
            } else {
                debug_signature(&())
            },
        },
        attachment_viewer: AttachmentViewerPopupSignature {
            attachment_viewer_open,
            selected_attachment_viewer_item: debug_signature(
                &state.selected_attachment_viewer_item(),
            ),
        },
        leaders: LeaderPopupSignature {
            guild_leader_action_open: state.is_guild_leader_action_active(),
            guild_action_items: debug_signature(&state.selected_guild_action_items()),
            channel_leader_action_open: state.is_channel_leader_action_active(),
            selected_channel_action_index: state.selected_channel_action_index(),
            channel_action_items: debug_signature(&state.selected_channel_action_items()),
            channel_thread_items: debug_signature(&state.channel_action_thread_items()),
            member_leader_action_open: state.is_member_leader_action_active(),
            member_action_items: debug_signature(&state.selected_member_action_items()),
        },
        options: OptionsPopupSignature {
            options_open,
            options_title: state.options_popup_title(),
            selected_option: state.selected_option_index(),
            display_options: state.display_options(),
            notification_options: state.notification_options(),
            voice_options: state.voice_options(),
        },
        message_interactions: MessageInteractionPopupSignature {
            emoji_picker_open,
            selected_emoji_reaction_index: if emoji_picker_open {
                state.selected_emoji_reaction_index_for_len(
                    state.filtered_emoji_reaction_items().len(),
                )
            } else {
                None
            },
            emoji_reaction_filter: state.emoji_reaction_filter().map(str::to_owned),
            filtered_emoji_reaction_items: if emoji_picker_open {
                debug_signature(&state.filtered_emoji_reaction_items())
            } else {
                debug_signature(&())
            },
            existing_emoji_reactions: if emoji_picker_open {
                debug_signature(&state.existing_emoji_reactions())
            } else {
                debug_signature(&())
            },
            own_emoji_reactions: if emoji_picker_open {
                debug_signature(&state.own_emoji_reactions())
            } else {
                debug_signature(&())
            },
            reaction_users_open,
            reaction_users_popup: debug_signature(&state.reaction_users_popup()),
            poll_vote_picker_open,
            selected_poll_vote_picker_index: state.selected_poll_vote_picker_index(),
            poll_vote_picker_items: debug_signature(&state.poll_vote_picker_items()),
        },
        profile: ProfilePopupSignature {
            user_profile_open,
            user_profile_data: debug_signature(&state.user_profile_popup_data()),
            user_profile_error: state.user_profile_popup_load_error().map(str::to_owned),
            user_profile_status: state.user_profile_popup_status(),
            user_profile_settings: debug_signature(&(
                state.user_profile_settings_tab(),
                state.user_profile_settings_active_field(),
                state.user_profile_settings_editing_field(),
                state.user_profile_settings_status(),
                state.user_profile_settings_presence_status(),
                state.user_profile_settings_dirty_count(),
                state.user_profile_settings_saving(),
                state.user_profile_status_picker_rows(),
            )),
            user_profile_scroll: state.user_profile_popup_scroll(),
            user_profile_avatar_url: state
                .user_profile_popup_pending_avatar_preview_key()
                .map(str::to_owned)
                .or_else(|| state.user_profile_popup_avatar_url().map(str::to_owned)),
            user_profile_activities: debug_signature(&state.user_profile_popup_activities()),
        },
        diagnostics: DiagnosticsPopupSignature {
            debug_log_open,
            debug_log_lines: if debug_log_open {
                debug_signature(&state.debug_log_lines())
            } else {
                debug_signature(&())
            },
            debug_channel_visibility: if debug_log_open {
                debug_signature(&state.debug_channel_visibility())
            } else {
                debug_signature(&())
            },
            keymap_help_open,
            keymap_help: if keymap_help_open {
                debug_signature(&state.keymap_binding_summaries())
            } else {
                debug_signature(&())
            },
            keymap_popup_scroll: state.keymap_popup_scroll(),
        },
        search: debug_signature(&state.search_popup_view()),
    }
}

fn passive_overlay_signature(state: &DashboardState) -> PassiveOverlaySignature {
    PassiveOverlaySignature {
        attachment_downloads: state
            .attachment_downloads()
            .into_iter()
            .map(|download| debug_signature(&download))
            .collect(),
    }
}

fn guild_pane_signature(state: &DashboardState) -> GuildPaneSignature {
    GuildPaneSignature {
        guild_horizontal_scroll: state.guild_horizontal_scroll(),
        visible_guilds: state
            .visible_guild_pane_entries()
            .into_iter()
            .map(|entry| {
                if matches!(entry, state::GuildPaneEntry::DirectMessages) {
                    return GuildEntrySignature {
                        row: debug_signature(&entry),
                        unread_count: Some(state.direct_message_unread_count()),
                        unread_state: None,
                    };
                }
                if let Some(guild) = entry.guild_state() {
                    return GuildEntrySignature {
                        row: debug_signature(&entry),
                        unread_count: None,
                        unread_state: Some(state.sidebar_guild_unread(guild.id)),
                    };
                }
                GuildEntrySignature {
                    row: debug_signature(&entry),
                    unread_count: None,
                    unread_state: None,
                }
            })
            .collect(),
    }
}

fn channel_pane_signature(state: &DashboardState) -> ChannelPaneSignature {
    ChannelPaneSignature {
        channel_horizontal_scroll: state.channel_horizontal_scroll(),
        visible_channels: state
            .visible_channel_pane_entries()
            .into_iter()
            .map(|entry| {
                if let Some(channel) = entry.channel_state() {
                    return ChannelEntrySignature {
                        row: debug_signature(&entry),
                        unread: Some(state.channel_unread(channel.id)),
                        unread_message_count: Some(state.channel_unread_message_count(channel.id)),
                    };
                }
                ChannelEntrySignature {
                    row: debug_signature(&entry),
                    unread: None,
                    unread_message_count: None,
                }
            })
            .collect(),
    }
}

fn message_pane_signature(state: &DashboardState) -> MessagePaneSignature {
    MessagePaneSignature {
        source: state.message_pane_source(),
        selected_message: state.selected_message(),
        message_scroll: state.message_scroll(),
        message_line_scroll: state.message_line_scroll(),
        new_messages_count: state.new_messages_count(),
        message_pane_title: state.message_pane_title(),
        typing_footer: state.typing_footer_for_selected_channel(),
        composer_picker: composer_picker_signature(state),
        visible_messages: state
            .visible_messages()
            .into_iter()
            .map(debug_signature)
            .collect(),
        visible_forum_posts: state
            .visible_forum_post_items()
            .into_iter()
            .map(|post| debug_signature(&post))
            .collect(),
    }
}

fn member_pane_signature(state: &DashboardState) -> MemberPaneSignature {
    let member_start = state.member_scroll();
    let member_end = member_start.saturating_add(state.member_content_height());
    MemberPaneSignature {
        selected_member: state.selected_member(),
        member_scroll: state.member_scroll(),
        member_horizontal_scroll: state.member_horizontal_scroll(),
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

fn visible_dashboard_changes(
    before: &VisibleDashboardSignature,
    after: &VisibleDashboardSignature,
) -> VisibleDashboardChangeSet {
    VisibleDashboardChangeSet {
        layout: before.layout != after.layout,
        overlay: before.overlay != after.overlay,
        passive_overlay: before.passive_overlay != after.passive_overlay,
        header: before.header != after.header,
        guilds: before.guilds != after.guilds,
        channels: before.channels != after.channels,
        messages: before.messages.selected_message != after.messages.selected_message
            || before.messages.source != after.messages.source
            || before.messages.message_scroll != after.messages.message_scroll
            || before.messages.message_line_scroll != after.messages.message_line_scroll
            || before.messages.message_pane_title != after.messages.message_pane_title
            || before.messages.typing_footer != after.messages.typing_footer
            || before.messages.composer_picker != after.messages.composer_picker
            || before.messages.visible_messages != after.messages.visible_messages
            || before.messages.visible_forum_posts != after.messages.visible_forum_posts,
        members: before.members != after.members,
        new_message_notice: before.messages.new_messages_count != after.messages.new_messages_count,
    }
}

fn composer_picker_signature(state: &DashboardState) -> ComposerPickerSignature {
    if let Some(query) = state.composer_mention_query() {
        return ComposerPickerSignature::Mention {
            query: query.to_owned(),
            selected: state.composer_mention_selected(),
            candidates: debug_signature(&state.composer_mention_candidates()),
        };
    }
    if let Some(query) = state.composer_emoji_query() {
        return ComposerPickerSignature::Emoji {
            query: query.to_owned(),
            selected: state.composer_emoji_selected(),
            candidates: debug_signature(&state.composer_emoji_candidates()),
        };
    }
    if let Some(query) = state.composer_command_query() {
        return ComposerPickerSignature::Command {
            query: query.to_owned(),
            selected: state.composer_command_selected(),
            candidates: debug_signature(&state.composer_command_candidates()),
        };
    }
    ComposerPickerSignature::None
}

fn channel_switcher_item_signature(
    items: &[state::ChannelSwitcherItem],
) -> Vec<ChannelSwitcherItemSignature> {
    items
        .iter()
        .map(|item| ChannelSwitcherItemSignature {
            channel_id: item.channel_id,
            group_label: item.group_label.clone(),
            parent_label: item.parent_label.clone(),
            channel_label: item.channel_label.clone(),
            depth: item.depth,
            unread: item.unread,
            unread_message_count: item.unread_message_count,
        })
        .collect()
}

struct DebugSignatureWriter {
    hasher: DefaultHasher,
}

impl fmt::Write for DebugSignatureWriter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.hasher.write(value.as_bytes());
        Ok(())
    }
}

fn debug_signature<T: fmt::Debug>(value: &T) -> DebugSignature {
    let mut writer = DebugSignatureWriter {
        hasher: DefaultHasher::new(),
    };
    write!(&mut writer, "{value:?}").expect("writing into signature hasher cannot fail");
    DebugSignature(writer.hasher.finish())
}

pub(in crate::tui) fn should_suppress_image_redraw_for_signature_change(
    before: &VisibleDashboardSignature,
    after: &VisibleDashboardSignature,
    image_surfaces_visible: bool,
) -> bool {
    let changes = visible_dashboard_changes(before, after);
    image_surfaces_visible
        && ((after.layout.focus != state::FocusPane::Members && changes.only_members_changed())
            || (after.layout.focus != state::FocusPane::Channels
                && changes.only_new_message_notice_changed()))
}

pub(in crate::tui) fn should_redraw_after_visible_signature_change(
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

pub(in crate::tui) fn should_refresh_image_protocols_after_visible_signature_change(
    before: &VisibleDashboardSignature,
    after: &VisibleDashboardSignature,
    image_surfaces_visible: bool,
) -> bool {
    image_surfaces_visible
        && before.overlay.active_modal_popup_kind != after.overlay.active_modal_popup_kind
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
        || state.composer_attachment_preview_has_image_surface()
        || state.forum_post_attachment_preview_has_image_surface()
        || (state.show_avatars() && state.user_profile_popup_has_avatar_preview())
}
