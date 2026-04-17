use crate::discord::ChannelState;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker},
};

use super::presentation::sort_channels;
use super::{ActiveGuildScope, DashboardState};

impl DashboardState {
    /// Returns the active guild plus the channel concord should attach the
    /// op-37 member-list subscription to. Prefers the user's currently open
    /// channel and falls back to the first text channel in the guild so the
    /// sidebar still updates while no channel is selected.
    pub fn member_list_subscription_target(&self) -> Option<(Id<GuildMarker>, Id<ChannelMarker>)> {
        let guild_id = match self.active_guild {
            ActiveGuildScope::Guild(guild_id) => guild_id,
            ActiveGuildScope::DirectMessages | ActiveGuildScope::Unset => return None,
        };
        let channel_id = self
            .active_channel_id
            .filter(|channel_id| {
                self.discord
                    .channel(*channel_id)
                    .is_some_and(|channel| self.is_member_list_subscription_channel(channel))
            })
            .or_else(|| self.guild_member_list_channel(guild_id))?;
        Some((guild_id, channel_id))
    }

    /// Highest 100-member bucket the user has scrolled the member sidebar
    /// into. Bucket 0 covers indexes 0..=99, bucket 1 covers 100..=199, etc.
    pub fn member_subscription_top_bucket(&self) -> u32 {
        let scroll = u32::try_from(self.member_scroll).unwrap_or(u32::MAX);
        let view = u32::try_from(self.member_view_height).unwrap_or(0);
        scroll.saturating_add(view) / 100
    }

    /// op-37 channel ranges that cover the member viewport plus a small
    /// trailing window. We anchor `[0, 99]` so the top of the sidebar always
    /// stays populated, then add up to two more buckets near the visible end
    /// so presence events keep flowing as the user scrolls. Capped at four
    /// ranges total because Discord rejects oversized channel range lists.
    pub fn member_subscription_ranges(&self) -> Vec<(u32, u32)> {
        let top = self.member_subscription_top_bucket();
        if top <= 2 {
            return (0..=top).map(|b| (b * 100, b * 100 + 99)).collect();
        }
        let near_start = top.saturating_sub(1);
        vec![
            (0, 99),
            (near_start * 100, near_start * 100 + 99),
            (top * 100, top * 100 + 99),
        ]
    }

    /// Picks a channel suitable for sending a guild op-37 subscription so
    /// Discord starts shipping `GUILD_MEMBER_LIST_UPDATE` events. Member-list
    /// updates only flow once the client subscribes to *some* channel in the
    /// guild; this lets the sidebar populate before the user opens a channel.
    pub fn guild_member_list_channel(
        &self,
        guild_id: Id<GuildMarker>,
    ) -> Option<Id<ChannelMarker>> {
        let mut candidates: Vec<&ChannelState> = self
            .discord
            .viewable_channels_for_guild(Some(guild_id))
            .into_iter()
            .filter(|channel| self.is_member_list_subscription_channel(channel))
            .collect();
        sort_channels(&mut candidates);
        candidates.first().map(|channel| channel.id)
    }

    fn is_member_list_subscription_channel(&self, channel: &ChannelState) -> bool {
        !channel.is_category()
            && !channel.is_thread()
            && !matches!(channel.kind.as_str(), "voice" | "GuildVoice")
            && self.discord.can_view_channel(channel)
    }
}
