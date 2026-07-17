use tokio::sync::mpsc;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::{DiscordClient, discord::AppCommand};

use super::super::{commands::send_or_record_closed as send_command, state::DashboardState};

#[derive(Default)]
pub(super) struct DashboardCommandScheduler {
    last_reported_active_guild: Option<Id<GuildMarker>>,
    last_reported_message_channel: Option<Id<ChannelMarker>>,
}

impl DashboardCommandScheduler {
    pub(super) async fn schedule_state_driven_commands(
        &mut self,
        state: &mut DashboardState,
        client: &DiscordClient,
        commands: &mpsc::Sender<AppCommand>,
    ) -> bool {
        let mut dirty = false;
        let now = std::time::Instant::now();

        self.report_active_selection(state, commands, &mut dirty)
            .await;
        self.schedule_mention_member_search(state, client, commands, now, &mut dirty)
            .await;
        self.schedule_message_history(state, client, commands, &mut dirty)
            .await;
        self.schedule_pinned_messages(state, client, commands, &mut dirty)
            .await;
        self.schedule_forum_posts(state, client, commands, &mut dirty)
            .await;
        self.schedule_member_requests(state, client, commands, now, &mut dirty)
            .await;
        self.schedule_thread_previews(state, client, commands, &mut dirty)
            .await;
        self.schedule_member_list_subscription(state, client, commands, now, &mut dirty)
            .await;

        dirty
    }

    async fn schedule_mention_member_search(
        &mut self,
        state: &mut DashboardState,
        client: &DiscordClient,
        commands: &mpsc::Sender<AppCommand>,
        now: std::time::Instant,
        dirty: &mut bool,
    ) {
        client.set_mention_member_search_target(
            state.selected_guild_id(),
            state
                .composer_mention_query()
                .or_else(|| state.search_popup_member_query()),
            now,
        );
        if let Some((guild_id, query)) = client.next_due_mention_member_search(now)
            && send_command(
                state,
                commands,
                AppCommand::SearchGuildMembers { guild_id, query },
            )
            .await
            .is_channel_closed()
        {
            *dirty = true;
        }
    }

    async fn schedule_message_history(
        &mut self,
        state: &mut DashboardState,
        client: &DiscordClient,
        commands: &mpsc::Sender<AppCommand>,
        dirty: &mut bool,
    ) {
        let needs_reload = state.selected_message_history_needs_reload();
        let is_stale = state.selected_message_history_is_stale();
        if let Some(channel_id) = client
            .next_message_history_request(state.selected_message_history_channel_id(), needs_reload)
        {
            let command = if is_stale {
                AppCommand::RefreshMessageHistory { channel_id }
            } else {
                AppCommand::LoadMessageHistory {
                    channel_id,
                    before: None,
                }
            };
            if send_command(state, commands, command)
                .await
                .is_channel_closed()
            {
                client.mark_message_history_request_failed(channel_id);
                *dirty = true;
            }
        }
    }

    async fn report_active_selection(
        &mut self,
        state: &mut DashboardState,
        commands: &mpsc::Sender<AppCommand>,
        dirty: &mut bool,
    ) {
        let active_guild = state.selected_guild_id();
        if active_guild != self.last_reported_active_guild {
            self.last_reported_active_guild = active_guild;
            if send_command(
                state,
                commands,
                AppCommand::SetSelectedGuild {
                    guild_id: active_guild,
                },
            )
            .await
            .is_channel_closed()
            {
                *dirty = true;
            }
        }

        let active_message_channel = state.selected_message_history_channel_id();
        if active_message_channel != self.last_reported_message_channel {
            self.last_reported_message_channel = active_message_channel;
            if send_command(
                state,
                commands,
                AppCommand::SetSelectedMessageChannel {
                    channel_id: active_message_channel,
                },
            )
            .await
            .is_channel_closed()
            {
                *dirty = true;
            }
        }
    }

    async fn schedule_pinned_messages(
        &mut self,
        state: &mut DashboardState,
        client: &DiscordClient,
        commands: &mpsc::Sender<AppCommand>,
        dirty: &mut bool,
    ) {
        if let Some(channel_id) =
            client.next_pinned_message_request(state.pinned_message_view_channel_id())
            && send_command(
                state,
                commands,
                AppCommand::LoadPinnedMessages { channel_id },
            )
            .await
            .is_channel_closed()
        {
            client.mark_pinned_message_request_failed(channel_id);
            *dirty = true;
        }
    }

    async fn schedule_forum_posts(
        &mut self,
        state: &mut DashboardState,
        client: &DiscordClient,
        commands: &mpsc::Sender<AppCommand>,
        dirty: &mut bool,
    ) {
        if let Some((guild_id, channel_id, archive_state, offset)) =
            client.next_forum_post_request(state.selected_forum_channel_with_load_more())
            && send_command(
                state,
                commands,
                AppCommand::LoadForumPosts {
                    guild_id,
                    channel_id,
                    archive_state,
                    offset,
                },
            )
            .await
            .is_channel_closed()
        {
            client.mark_forum_post_request_failed(channel_id, archive_state, offset);
            *dirty = true;
        }
    }

    async fn schedule_member_requests(
        &mut self,
        state: &mut DashboardState,
        client: &DiscordClient,
        commands: &mpsc::Sender<AppCommand>,
        now: std::time::Instant,
        dirty: &mut bool,
    ) {
        if let Some(guild_id) = client.next_member_request(state.selected_guild_id()) {
            // User accounts may request the complete guild member list only
            // with moderation permissions. The normal web client uses an
            // op-37 channel subscription for member-list streaming instead.
            match state.guild_member_list_channel(guild_id) {
                Some(channel_id) => {
                    if send_command(
                        state,
                        commands,
                        AppCommand::SubscribeGuildChannel {
                            guild_id,
                            channel_id,
                        },
                    )
                    .await
                    .is_channel_closed()
                    {
                        client.remove_member_request(guild_id);
                        *dirty = true;
                    }
                }
                None => client.remove_member_request(guild_id),
            }
        }

        let initial_unknown_requests = client
            .next_initial_unknown_member_requests(state.initial_unknown_member_requests(), now);
        if state.enqueue_guild_member_by_id_requests(initial_unknown_requests) {
            *dirty = true;
        }
    }

    async fn schedule_thread_previews(
        &mut self,
        state: &mut DashboardState,
        client: &DiscordClient,
        commands: &mpsc::Sender<AppCommand>,
        dirty: &mut bool,
    ) {
        for (channel_id, latest_message_id) in
            client.next_thread_preview_requests(state.missing_thread_preview_load_requests())
        {
            if send_command(
                state,
                commands,
                AppCommand::LoadThreadPreview {
                    channel_id,
                    message_id: latest_message_id,
                },
            )
            .await
            .is_channel_closed()
            {
                client.remove_thread_preview_request((channel_id, latest_message_id));
                *dirty = true;
            }
        }
    }

    async fn schedule_member_list_subscription(
        &mut self,
        state: &mut DashboardState,
        client: &DiscordClient,
        commands: &mpsc::Sender<AppCommand>,
        now: std::time::Instant,
        dirty: &mut bool,
    ) {
        let target = state
            .member_list_subscription_target()
            .map(|(guild_id, channel_id)| {
                (
                    guild_id,
                    channel_id,
                    state.member_subscription_top_bucket(),
                    state.member_subscription_ranges(),
                )
            });
        client.set_member_list_subscription_target(target, now);
        if let Some((guild_id, channel_id, ranges)) = client.next_due_member_list_subscription(now)
            && send_command(
                state,
                commands,
                AppCommand::UpdateMemberListSubscription {
                    guild_id,
                    channel_id,
                    ranges,
                },
            )
            .await
            .is_channel_closed()
        {
            *dirty = true;
        }
    }
}
