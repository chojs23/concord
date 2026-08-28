use tokio::sync::mpsc;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};
use crate::{
    DiscordClient,
    discord::{AppCommand, GuildMemberSearchSurface},
};

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
        let autocomplete_query = state
            .composer_mention_query()
            .or_else(|| state.message_search_member_query())
            .map(str::to_owned);
        dirty |= self
            .schedule_member_search(
                state,
                client,
                commands,
                GuildMemberSearchSurface::Autocomplete,
                autocomplete_query.as_deref(),
                now,
            )
            .await;
        let popup_query = state.member_search_popup_query().map(str::to_owned);
        dirty |= self
            .schedule_member_search(
                state,
                client,
                commands,
                GuildMemberSearchSurface::Popup,
                popup_query.as_deref(),
                now,
            )
            .await;
        self.schedule_message_history(state, client, commands, &mut dirty)
            .await;
        self.schedule_pinned_messages(state, client, commands, &mut dirty)
            .await;
        self.schedule_archived_threads(state, client, commands, &mut dirty)
            .await;
        self.schedule_forum_post_data(state, client, commands, &mut dirty)
            .await;
        self.schedule_member_requests(state, client, now, &mut dirty)
            .await;
        self.schedule_thread_previews(state, client, commands, &mut dirty)
            .await;
        self.schedule_member_list_subscription(state, client, commands, now, &mut dirty)
            .await;

        dirty
    }

    async fn schedule_member_search(
        &self,
        state: &mut DashboardState,
        client: &DiscordClient,
        commands: &mpsc::Sender<AppCommand>,
        surface: GuildMemberSearchSurface,
        query: Option<&str>,
        now: std::time::Instant,
    ) -> bool {
        client.set_guild_member_search_target(surface, state.selected_guild_id(), query, now);
        let Some((guild_id, query)) = client.next_due_guild_member_search(surface, now) else {
            return false;
        };
        send_command(
            state,
            commands,
            AppCommand::SearchGuildMembers {
                guild_id,
                query,
                limit: surface.result_limit(),
            },
        )
        .await
        .is_channel_closed()
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

    async fn schedule_forum_post_data(
        &mut self,
        state: &mut DashboardState,
        client: &DiscordClient,
        commands: &mpsc::Sender<AppCommand>,
        dirty: &mut bool,
    ) {
        if let Some(crate::discord::ForumPostDataRequestTarget {
            guild_id,
            channel_id,
            thread_ids,
        }) = client.next_forum_post_data_request(state.selected_forum_post_data_target())
            && send_command(
                state,
                commands,
                AppCommand::LoadForumPostData {
                    guild_id,
                    channel_id,
                    thread_ids: thread_ids.clone(),
                },
            )
            .await
            .is_channel_closed()
        {
            client.mark_forum_post_data_request_failed(channel_id, &thread_ids);
            *dirty = true;
        }
    }

    async fn schedule_archived_threads(
        &mut self,
        state: &mut DashboardState,
        client: &DiscordClient,
        commands: &mpsc::Sender<AppCommand>,
        dirty: &mut bool,
    ) {
        if let Some(crate::discord::ArchivedThreadRequestTarget {
            guild_id,
            channel_id,
            cursor,
        }) = client.next_archived_thread_request(state.selected_archived_thread_request_target())
        {
            let before = cursor.clone().into_before();
            if send_command(
                state,
                commands,
                AppCommand::LoadArchivedThreads {
                    guild_id,
                    channel_id,
                    before,
                },
            )
            .await
            .is_channel_closed()
            {
                client.mark_archived_thread_request_send_failed(channel_id, &cursor);
                *dirty = true;
            }
        }
    }

    async fn schedule_member_requests(
        &mut self,
        state: &mut DashboardState,
        client: &DiscordClient,
        now: std::time::Instant,
        dirty: &mut bool,
    ) {
        let hydration_requests = client
            .next_member_hydration_requests(state.observed_member_hydration_requests(now), now);
        if state.enqueue_guild_member_by_id_requests(hydration_requests) {
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
                let thread_id = state.thread_member_list_subscription_target().and_then(
                    |(thread_guild_id, thread_id)| {
                        (thread_guild_id == guild_id).then_some(thread_id)
                    },
                );
                (
                    guild_id,
                    channel_id,
                    thread_id,
                    state.member_subscription_top_bucket(),
                    state.member_list_refresh_generation(guild_id),
                    state.member_subscription_ranges(),
                )
            });
        client.set_member_list_subscription_target(target, now);
        if let Some((guild_id, channel_id, thread_id, ranges)) =
            client.next_due_member_list_subscription(now)
            && send_command(
                state,
                commands,
                AppCommand::UpdateMemberListSubscription {
                    guild_id,
                    channel_id,
                    thread_id,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::test_builders::{GuildCreateFixture, guild_create_event};

    #[tokio::test]
    async fn member_search_surfaces_schedule_independent_limits() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let guild_id = Id::new(1);
        let mut state = DashboardState::new();
        state.push_event(guild_create_event(GuildCreateFixture::new(guild_id)));
        assert!(state.confirm_selected_guild());
        let client = DiscordClient::new("test-token".to_owned()).expect("token is valid header");
        let (commands, mut command_rx) = mpsc::channel(4);
        let scheduler = DashboardCommandScheduler::default();
        let mut now = std::time::Instant::now();

        for (surface, query) in [
            (GuildMemberSearchSurface::Autocomplete, "alice"),
            (GuildMemberSearchSurface::Popup, "a"),
        ] {
            assert!(
                !scheduler
                    .schedule_member_search(
                        &mut state,
                        &client,
                        &commands,
                        surface,
                        Some(query),
                        now,
                    )
                    .await
            );
            let deadline = client
                .guild_member_search_deadline(surface)
                .expect("member search should be debounced");
            assert!(
                !scheduler
                    .schedule_member_search(
                        &mut state,
                        &client,
                        &commands,
                        surface,
                        Some(query),
                        deadline,
                    )
                    .await
            );
            assert_eq!(
                command_rx.try_recv(),
                Ok(AppCommand::SearchGuildMembers {
                    guild_id,
                    query: query.to_owned(),
                    limit: surface.result_limit(),
                })
            );
            now = deadline + std::time::Duration::from_millis(1);
        }
    }
}
