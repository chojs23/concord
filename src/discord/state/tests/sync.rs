use super::*;
use crate::discord::state::ClientCacheState;

#[test]
fn client_cache_state_uses_channel_metadata_and_keeps_wire_defaults_out_of_state() {
    let mut state = DiscordState::default();
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        guild_id: Some(Id::new(10)),
        last_message_id: Some(Id::new(120)),
        ..ChannelInfo::test(Id::new(1), "GuildText")
    }));
    state.apply_event(&AppEvent::ChannelUpsert(ChannelInfo {
        last_message_id: Some(Id::new(500)),
        ..ChannelInfo::test(Id::new(2), "DM")
    }));

    let notifications = state.notifications_mut();
    notifications.read_state_version = Some(12);
    notifications.user_guild_settings_version = Some(34);
    notifications
        .read_states
        .entry(Id::new(1))
        .or_default()
        .last_acked_message_id = Some(Id::new(999));

    assert_eq!(
        state.client_cache_state(),
        ClientCacheState {
            highest_guild_message_id: Some(Id::new(120)),
            highest_private_message_id: Some(Id::new(500)),
            read_state_version: Some(12),
            user_guild_settings_version: Some(34),
        }
    );
}
