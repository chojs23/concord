use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, UserMarker},
};
use crate::{DiscordClient, discord::AppEvent, logging};

pub(super) async fn load_members_by_ids(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    user_ids: Vec<Id<UserMarker>>,
) {
    publish_gateway_result(
        &client,
        client.request_guild_members_by_ids(guild_id, user_ids),
    )
    .await;
}

pub(super) async fn search_members(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    query: String,
    limit: u16,
) {
    publish_gateway_result(&client, client.search_guild_members(guild_id, query, limit)).await;
}

pub(super) async fn set_selected_guild(client: DiscordClient, guild_id: Option<Id<GuildMarker>>) {
    client
        .publish_event(AppEvent::SelectedGuildChanged { guild_id })
        .await;
}

pub(super) async fn set_selected_message_channel(
    client: DiscordClient,
    channel_id: Option<Id<ChannelMarker>>,
) {
    client.update_rest_page_referer(channel_id);
    client
        .publish_event(AppEvent::SelectedMessageChannelChanged { channel_id })
        .await;
}

pub(super) async fn subscribe_direct_message(client: DiscordClient, channel_id: Id<ChannelMarker>) {
    publish_gateway_result(&client, client.subscribe_direct_message(channel_id)).await;
}

pub(super) async fn subscribe_guild_channel(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
) {
    publish_gateway_result(
        &client,
        client.subscribe_guild_channel(guild_id, channel_id),
    )
    .await;
}

pub(super) async fn update_member_list_subscription(
    client: DiscordClient,
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    thread_id: Option<Id<ChannelMarker>>,
    ranges: Vec<(u32, u32)>,
) {
    publish_gateway_result(
        &client,
        client.update_member_list_subscription(guild_id, channel_id, thread_id, ranges),
    )
    .await;
}

async fn publish_gateway_result(client: &DiscordClient, result: std::result::Result<(), String>) {
    if let Err(message) = result {
        publish_gateway_error(client, message).await;
    }
}

async fn publish_gateway_error(client: &DiscordClient, message: String) {
    logging::error("app", &message);
    client
        .publish_event(AppEvent::GatewayError { message })
        .await;
}
