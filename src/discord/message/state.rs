use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker, WebhookMarker},
};
use crate::discord::{
    AttachmentInfo, AttachmentMediaType, EmbedInfo, InlinePreviewInfo, MemberInfo, MentionInfo,
    MessageInfo, MessageInteractionInfo, MessageKind, MessageReferenceInfo, MessageSnapshotInfo,
    MessageUpdateEventFields, PollInfo, ReactionEmoji, ReactionInfo, ReplyInfo, StickerInfo,
};
use crate::discord::{
    member::{selected_member_role_color, selected_role_ids_color},
    profile::UserProfileCacheKey,
    state::{
        ChannelMessageTimeline, DiscordState, MessageSegment, MessageTimelineFocus,
        OLDER_HISTORY_EXTRA_WINDOW_MULTIPLIER, is_fallback_identity, touch_recent,
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageState {
    pub id: Id<MessageMarker>,
    pub nonce: Option<Id<MessageMarker>>,
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub webhook_id: Option<Id<WebhookMarker>>,
    pub author_id: Id<UserMarker>,
    pub author: String,
    pub author_avatar_url: Option<String>,
    pub author_is_bot: bool,
    pub message_kind: MessageKind,
    pub interaction: Option<MessageInteractionInfo>,
    pub reference: Option<MessageReferenceInfo>,
    pub reply: Option<ReplyInfo>,
    pub poll: Option<PollInfo>,
    pub pinned: bool,
    pub reactions: Vec<ReactionInfo>,
    pub content: Option<String>,
    pub stickers: Vec<StickerInfo>,
    pub mentions: Vec<MentionInfo>,
    pub mention_everyone: bool,
    pub mention_roles: Vec<Id<RoleMarker>>,
    pub flags: u64,
    pub attachments: Vec<AttachmentInfo>,
    pub embeds: Vec<EmbedInfo>,
    pub forwarded_snapshots: Vec<MessageSnapshotInfo>,
    pub edited_timestamp: Option<String>,
}

impl Default for MessageState {
    fn default() -> Self {
        Self {
            id: Id::new(1),
            nonce: None,
            guild_id: None,
            channel_id: Id::new(1),
            webhook_id: None,
            author_id: Id::new(1),
            author: String::new(),
            author_avatar_url: None,
            author_is_bot: false,
            message_kind: MessageKind::default(),
            interaction: None,
            reference: None,
            reply: None,
            poll: None,
            pinned: false,
            reactions: Vec::new(),
            content: None,
            stickers: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            mention_roles: Vec::new(),
            flags: 0,
            attachments: Vec::new(),
            embeds: Vec::new(),
            forwarded_snapshots: Vec::new(),
            edited_timestamp: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MessageCapabilities {
    pub is_reply: bool,
    pub is_forwarded: bool,
    pub has_poll: bool,
    pub has_image: bool,
    pub has_video: bool,
    pub has_audio: bool,
    pub has_file: bool,
}

impl MessageState {
    pub(in crate::discord) fn redact_body(&mut self) {
        self.reference = None;
        self.reply = None;
        self.poll = None;
        self.content = None;
        self.stickers.clear();
        self.mentions.clear();
        self.attachments.clear();
        self.embeds.clear();
        self.forwarded_snapshots.clear();
        self.edited_timestamp = None;
    }

    pub fn attachments_in_display_order(&self) -> impl Iterator<Item = &AttachmentInfo> {
        self.attachments.iter().chain(
            self.forwarded_snapshots
                .iter()
                .flat_map(|snapshot| snapshot.attachments.iter()),
        )
    }

    fn stickers_in_display_order(&self) -> impl Iterator<Item = &StickerInfo> {
        self.stickers.iter().chain(
            self.forwarded_snapshots
                .iter()
                .flat_map(|snapshot| snapshot.stickers.iter()),
        )
    }

    pub fn first_inline_preview(&self) -> Option<InlinePreviewInfo<'_>> {
        self.attachments_in_display_order()
            .find_map(AttachmentInfo::inline_preview_info)
            .or_else(|| {
                self.embeds
                    .iter()
                    .chain(
                        self.forwarded_snapshots
                            .iter()
                            .flat_map(|snapshot| snapshot.embeds.iter()),
                    )
                    .find_map(EmbedInfo::inline_preview_info)
            })
            .or_else(|| {
                self.stickers_in_display_order()
                    .find_map(StickerInfo::inline_preview_info)
            })
    }

    pub fn inline_previews(&self) -> Vec<InlinePreviewInfo<'_>> {
        self.attachments_in_display_order()
            .filter_map(AttachmentInfo::inline_preview_info)
            .chain(
                self.embeds
                    .iter()
                    .chain(
                        self.forwarded_snapshots
                            .iter()
                            .flat_map(|snapshot| snapshot.embeds.iter()),
                    )
                    .filter_map(EmbedInfo::inline_preview_info),
            )
            .chain(
                self.stickers_in_display_order()
                    .filter_map(StickerInfo::inline_preview_info),
            )
            .collect()
    }

    pub fn capabilities(&self) -> MessageCapabilities {
        let mut capabilities = MessageCapabilities {
            is_reply: self.reply.is_some(),
            is_forwarded: !self.forwarded_snapshots.is_empty(),
            ..MessageCapabilities::default()
        };

        // Poll and attachment actions are valid for chat messages, including
        // replies. Other non-regular messages can still be rendered as
        // replies/forwards, but subtype-like action facets should not leak
        // onto system messages.
        if !self.message_kind.is_regular_or_reply() {
            return capabilities;
        }

        capabilities.has_poll = self.poll.is_some();
        for attachment in self.attachments_in_display_order() {
            if let Some(media_type) = attachment.media_type() {
                match media_type {
                    AttachmentMediaType::Image => capabilities.has_image = true,
                    AttachmentMediaType::Video => capabilities.has_video = true,
                    AttachmentMediaType::Audio => capabilities.has_audio = true,
                };
            } else {
                capabilities.has_file = true;
            };
        }
        if self.first_inline_preview().is_some() {
            capabilities.has_image = true;
        }

        capabilities
    }
}

pub(in crate::discord) type MessageAuthorRoleIds =
    BTreeMap<(Id<ChannelMarker>, Id<MessageMarker>), Vec<Id<RoleMarker>>>;

pub(in crate::discord) struct MessageUpdateFields {
    pub(in crate::discord) body: MessageUpdateEventFields,
    pub(in crate::discord) reactions: Option<Vec<ReactionInfo>>,
    pub(in crate::discord) retain_body: bool,
}

impl ChannelMessageTimeline {
    fn message_history_gap_after(&self, lower_id: Id<MessageMarker>) -> Option<Id<MessageMarker>> {
        let index = self
            .segments
            .iter()
            .position(|segment| segment.message_ids.back() == Some(&lower_id))?;
        self.segments
            .get(index.saturating_add(1))?
            .message_ids
            .front()
            .copied()
    }

    fn contains_message(&self, message_id: Id<MessageMarker>) -> bool {
        self.messages.iter().any(|message| message.id == message_id)
    }

    fn merge_messages(&mut self, incoming: Vec<MessageState>) -> Vec<Id<MessageMarker>> {
        let incoming_ids = incoming
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>();
        let mut by_id = self
            .messages
            .drain(..)
            .map(|message| (message.id, message))
            .collect::<BTreeMap<_, _>>();

        for message in incoming {
            by_id
                .entry(message.id)
                .and_modify(|existing| merge_message(existing, &message))
                .or_insert(message);
        }

        self.messages = by_id.into_values().collect();
        incoming_ids
    }

    fn replace_latest(&mut self, message_ids: &[Id<MessageMarker>]) {
        self.segments.clear();
        self.historical_mode = false;
        self.active_focus = MessageTimelineFocus::Newest;
        if !message_ids.is_empty() {
            self.segments.push_back(MessageSegment {
                message_ids: sorted_message_ids(message_ids),
                active: true,
                reaches_live_edge: true,
            });
        }
    }

    fn merge_latest(
        &mut self,
        message_ids: &[Id<MessageMarker>],
        recent_limit: usize,
    ) -> Vec<Id<MessageMarker>> {
        if message_ids.is_empty() {
            return self.trim_to_retention_limit(recent_limit);
        }

        let mut merge_indexes = self.overlapping_segment_indexes(message_ids);
        if let Some(live_index) = self.live_segment_index() {
            merge_indexes.insert(live_index);
        }
        let make_active = !self.segments.iter().any(|segment| segment.active);
        self.merge_segment_ids(message_ids, merge_indexes, make_active, true);
        if self.segments.len() > 1 {
            self.historical_mode = true;
        }
        self.trim_to_retention_limit(recent_limit)
    }

    fn merge_older(
        &mut self,
        before: Id<MessageMarker>,
        message_ids: &[Id<MessageMarker>],
        recent_limit: usize,
    ) -> Vec<Id<MessageMarker>> {
        self.historical_mode = true;
        self.active_focus = MessageTimelineFocus::Oldest;
        let mut merge_indexes = self.overlapping_segment_indexes(message_ids);
        if let Some(anchor_index) = self.segment_index_containing(before) {
            merge_indexes.insert(anchor_index);
        }
        self.merge_segment_ids(message_ids, merge_indexes, true, false);
        self.trim_to_retention_limit(recent_limit)
    }

    fn merge_around(
        &mut self,
        target: Id<MessageMarker>,
        message_ids: &[Id<MessageMarker>],
        recent_limit: usize,
    ) -> Vec<Id<MessageMarker>> {
        self.historical_mode = true;
        self.active_focus = MessageTimelineFocus::Around(target);
        let mut merge_indexes = self.overlapping_segment_indexes(message_ids);
        if let Some(target_index) = self.segment_index_containing(target) {
            merge_indexes.insert(target_index);
        }
        self.merge_segment_ids(message_ids, merge_indexes, true, false);
        self.trim_to_retention_limit(recent_limit)
    }

    fn merge_detached(
        &mut self,
        message_ids: &[Id<MessageMarker>],
        recent_limit: usize,
    ) -> Vec<Id<MessageMarker>> {
        let merge_indexes = self.overlapping_segment_indexes(message_ids);
        let make_active = !self.segments.iter().any(|segment| segment.active);
        self.merge_segment_ids(message_ids, merge_indexes, make_active, false);
        if self.segments.len() > 1 {
            self.historical_mode = true;
        }
        self.trim_to_retention_limit(recent_limit)
    }

    fn merge_newer(
        &mut self,
        after: Id<MessageMarker>,
        message_ids: &[Id<MessageMarker>],
        has_more: bool,
        recent_limit: usize,
    ) -> Vec<Id<MessageMarker>> {
        self.historical_mode = true;
        self.active_focus = MessageTimelineFocus::Around(after);

        let Some(anchor_index) = self.segment_index_containing(after) else {
            let merge_indexes = self.overlapping_segment_indexes(message_ids);
            self.merge_segment_ids(message_ids, merge_indexes, true, !has_more);
            return self.trim_to_retention_limit(recent_limit);
        };

        let next_index = (anchor_index + 1 < self.segments.len()).then_some(anchor_index + 1);
        let reached_next = next_index.is_some_and(|index| {
            self.segments[index]
                .message_ids
                .iter()
                .any(|message_id| message_ids.contains(message_id))
        });
        let closes_boundary = message_ids.is_empty() || reached_next || !has_more;

        let mut merge_indexes = self.overlapping_segment_indexes(message_ids);
        merge_indexes.insert(anchor_index);
        if closes_boundary && let Some(next_index) = next_index {
            merge_indexes.insert(next_index);
        }
        let reaches_live_edge = next_index.is_none() && !has_more;
        self.merge_segment_ids(message_ids, merge_indexes, true, reaches_live_edge);
        self.trim_to_retention_limit(recent_limit)
    }

    fn insert_live_message(
        &mut self,
        message_id: Id<MessageMarker>,
        inserted: bool,
        recent_limit: usize,
    ) -> Vec<Id<MessageMarker>> {
        if inserted {
            let mut merge_indexes = BTreeSet::new();
            if let Some(live_index) = self.live_segment_index() {
                merge_indexes.insert(live_index);
            }
            let make_active = !self.segments.iter().any(|segment| segment.active);
            self.merge_segment_ids(
                std::slice::from_ref(&message_id),
                merge_indexes,
                make_active,
                true,
            );
        }
        if self.segments.len() > 1 {
            self.historical_mode = true;
        }
        self.trim_to_retention_limit(recent_limit)
    }

    fn remove_messages(&mut self, message_ids: &[Id<MessageMarker>]) {
        let removed = message_ids.iter().copied().collect::<BTreeSet<_>>();
        self.messages
            .retain(|message| !removed.contains(&message.id));
        for segment in &mut self.segments {
            segment
                .message_ids
                .retain(|message_id| !removed.contains(message_id));
        }
        self.segments
            .retain(|segment| !segment.message_ids.is_empty());
        self.ensure_active_segment();
    }

    fn segment_index_containing(&self, message_id: Id<MessageMarker>) -> Option<usize> {
        self.segments
            .iter()
            .position(|segment| segment.message_ids.contains(&message_id))
    }

    fn live_segment_index(&self) -> Option<usize> {
        self.segments
            .iter()
            .rposition(|segment| segment.reaches_live_edge)
    }

    fn overlapping_segment_indexes(&self, message_ids: &[Id<MessageMarker>]) -> BTreeSet<usize> {
        self.segments
            .iter()
            .enumerate()
            .filter_map(|(index, segment)| {
                segment
                    .message_ids
                    .iter()
                    .any(|message_id| message_ids.contains(message_id))
                    .then_some(index)
            })
            .collect()
    }

    fn merge_segment_ids(
        &mut self,
        message_ids: &[Id<MessageMarker>],
        merge_indexes: BTreeSet<usize>,
        make_active: bool,
        reaches_live_edge: bool,
    ) {
        if make_active {
            for segment in &mut self.segments {
                segment.active = false;
            }
        }

        let mut merged_ids = message_ids.iter().copied().collect::<BTreeSet<_>>();
        let mut active = make_active;
        let mut live = reaches_live_edge;
        for index in &merge_indexes {
            let segment = &self.segments[*index];
            merged_ids.extend(segment.message_ids.iter().copied());
            active |= segment.active;
            live |= segment.reaches_live_edge;
        }
        for index in merge_indexes.into_iter().rev() {
            self.segments.remove(index);
        }

        if merged_ids.is_empty() {
            self.ensure_active_segment();
            return;
        }

        self.segments.push_back(MessageSegment {
            message_ids: merged_ids.into_iter().collect(),
            active,
            reaches_live_edge: live,
        });
        self.segments
            .make_contiguous()
            .sort_by_key(|segment| segment.message_ids.front().copied());
        self.ensure_active_segment();
    }

    fn trim_to_retention_limit(&mut self, recent_limit: usize) -> Vec<Id<MessageMarker>> {
        if recent_limit == 0 {
            return self.retain_cached_message_ids(&BTreeSet::new(), None);
        }

        if !self.historical_mode {
            if self.messages.len() <= recent_limit {
                return Vec::new();
            }
            let keep = self
                .messages
                .iter()
                .rev()
                .take(recent_limit)
                .map(|message| message.id)
                .collect::<BTreeSet<_>>();
            let active_target = keep.iter().next_back().copied();
            return self.retain_cached_message_ids(&keep, active_target);
        }

        let extended_limit = recent_limit.saturating_mul(OLDER_HISTORY_EXTRA_WINDOW_MULTIPLIER);
        if self.messages.len() <= extended_limit {
            return Vec::new();
        }

        let active_index = self
            .segments
            .iter()
            .position(|segment| segment.active)
            .or_else(|| self.live_segment_index())
            .or((!self.segments.is_empty()).then_some(0));
        let live_index = self.live_segment_index();
        let mut keep = BTreeSet::new();
        let mut active_target = None;

        if let Some(active_index) = active_index {
            let active_window = segment_window(
                &self.segments[active_index].message_ids,
                recent_limit,
                self.active_focus,
            );
            active_target = focused_message_id(&active_window, self.active_focus);
            keep.extend(active_window);
        }
        if let Some(live_index) = live_index {
            keep.extend(
                self.segments[live_index]
                    .message_ids
                    .iter()
                    .rev()
                    .take(recent_limit)
                    .copied(),
            );
        }

        self.retain_cached_message_ids(&keep, active_target)
    }

    fn retain_cached_message_ids(
        &mut self,
        keep: &BTreeSet<Id<MessageMarker>>,
        active_target: Option<Id<MessageMarker>>,
    ) -> Vec<Id<MessageMarker>> {
        let evicted = self
            .messages
            .iter()
            .filter(|message| !keep.contains(&message.id))
            .map(|message| message.id)
            .collect::<Vec<_>>();
        if evicted.is_empty() {
            return evicted;
        }
        self.messages.retain(|message| keep.contains(&message.id));

        let previous_segments = std::mem::take(&mut self.segments);
        for segment in previous_segments {
            let mut runs = Vec::new();
            let mut current = VecDeque::new();
            for message_id in segment.message_ids {
                if keep.contains(&message_id) {
                    current.push_back(message_id);
                } else if !current.is_empty() {
                    runs.push(std::mem::take(&mut current));
                }
            }
            if !current.is_empty() {
                runs.push(current);
            }
            let final_run = runs.len().saturating_sub(1);
            for (index, message_ids) in runs.into_iter().enumerate() {
                self.segments.push_back(MessageSegment {
                    message_ids,
                    active: false,
                    reaches_live_edge: segment.reaches_live_edge && index == final_run,
                });
            }
        }
        self.segments
            .make_contiguous()
            .sort_by_key(|segment| segment.message_ids.front().copied());

        if let Some(active_target) = active_target
            && let Some(index) = self.segment_index_containing(active_target)
        {
            self.segments[index].active = true;
        }
        self.ensure_active_segment();
        evicted
    }

    fn ensure_active_segment(&mut self) {
        if self.segments.iter().any(|segment| segment.active) {
            return;
        }
        if let Some(index) = self
            .live_segment_index()
            .or((!self.segments.is_empty()).then_some(0))
        {
            self.segments[index].active = true;
        }
    }
}

fn sorted_message_ids(message_ids: &[Id<MessageMarker>]) -> VecDeque<Id<MessageMarker>> {
    message_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn segment_window(
    message_ids: &VecDeque<Id<MessageMarker>>,
    limit: usize,
    focus: MessageTimelineFocus,
) -> Vec<Id<MessageMarker>> {
    if message_ids.len() <= limit {
        return message_ids.iter().copied().collect();
    }
    match focus {
        MessageTimelineFocus::Oldest => message_ids.iter().take(limit).copied().collect(),
        MessageTimelineFocus::Newest => message_ids
            .iter()
            .skip(message_ids.len().saturating_sub(limit))
            .copied()
            .collect(),
        MessageTimelineFocus::Around(target) => {
            let target_index = message_ids
                .iter()
                .position(|message_id| *message_id == target)
                .unwrap_or_else(|| {
                    message_ids
                        .iter()
                        .position(|message_id| *message_id > target)
                        .unwrap_or(message_ids.len().saturating_sub(1))
                });
            let start = target_index
                .saturating_sub(limit / 2)
                .min(message_ids.len().saturating_sub(limit));
            message_ids
                .iter()
                .skip(start)
                .take(limit)
                .copied()
                .collect()
        }
    }
}

fn focused_message_id(
    message_ids: &[Id<MessageMarker>],
    focus: MessageTimelineFocus,
) -> Option<Id<MessageMarker>> {
    match focus {
        MessageTimelineFocus::Oldest => message_ids.first().copied(),
        MessageTimelineFocus::Newest => message_ids.last().copied(),
        MessageTimelineFocus::Around(target) => message_ids
            .iter()
            .copied()
            .find(|message_id| *message_id == target)
            .or_else(|| message_ids.first().copied()),
    }
}

impl DiscordState {
    pub(in crate::discord) fn should_retain_live_message_body(
        &self,
        channel_id: Id<ChannelMarker>,
        author_id: Id<UserMarker>,
        mentions: &[MentionInfo],
    ) -> bool {
        self.session.current_user_id == Some(author_id)
            || self
                .session
                .current_user_id
                .is_some_and(|user_id| mentions.iter().any(|mention| mention.user_id == user_id))
            || self.should_retain_channel_message_body(channel_id)
    }

    pub(in crate::discord) fn retained_live_message_warms_channel(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> bool {
        self.should_retain_channel_message_body(channel_id)
    }

    pub fn channel_message_bodies_are_cold(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.message_cache
            .cold_message_channels
            .contains(&channel_id)
    }

    fn channel_message_bodies_are_warm(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.message_cache
            .warm_message_channels
            .contains(&channel_id)
    }

    pub(in crate::discord) fn should_retain_channel_message_body(
        &self,
        channel_id: Id<ChannelMarker>,
    ) -> bool {
        !self.session.selected_message_channel_known
            || self.session.selected_message_channel_id == Some(channel_id)
            || self.channel_message_bodies_are_warm(channel_id)
    }

    pub(in crate::discord) fn should_retain_message_update_body(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> bool {
        self.should_retain_channel_message_body(channel_id)
            || self
                .message_cache
                .pinned_messages
                .get(&channel_id)
                .is_some_and(|messages| messages.iter().any(|message| message.id == message_id))
    }

    pub fn messages_for_channel(&self, channel_id: Id<ChannelMarker>) -> Vec<&MessageState> {
        self.message_cache
            .timelines
            .get(&channel_id)
            .map(|timeline| timeline.messages.iter().collect())
            .unwrap_or_default()
    }

    pub(crate) fn channel_has_cached_messages(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.message_cache
            .timelines
            .get(&channel_id)
            .is_some_and(|timeline| !timeline.messages.is_empty())
    }

    pub(crate) fn channel_cached_message_count_from(
        &self,
        channel_id: Id<ChannelMarker>,
        author_id: Id<UserMarker>,
    ) -> usize {
        self.message_cache
            .timelines
            .get(&channel_id)
            .map_or(0, |timeline| {
                timeline
                    .messages
                    .iter()
                    .filter(|message| message.author_id == author_id)
                    .count()
            })
    }

    pub fn message_history_gap_after(
        &self,
        channel_id: Id<ChannelMarker>,
        lower_id: Id<MessageMarker>,
    ) -> Option<Id<MessageMarker>> {
        self.message_cache
            .timelines
            .get(&channel_id)?
            .message_history_gap_after(lower_id)
    }

    pub(in crate::discord) fn redact_channel_message_bodies(
        &mut self,
        channel_id: Id<ChannelMarker>,
    ) {
        let Some(timeline) = self.message_cache_mut().timelines.get_mut(&channel_id) else {
            return;
        };
        for message in &mut timeline.messages {
            message.redact_body();
        }
    }

    pub(in crate::discord) fn touch_warm_message_channel(&mut self, channel_id: Id<ChannelMarker>) {
        let message_cache = self.message_cache_mut();
        touch_recent(&mut message_cache.warm_message_channels, channel_id);
        message_cache.cold_message_channels.remove(&channel_id);
        self.evict_warm_message_channels_if_needed();
    }

    fn evict_warm_message_channels_if_needed(&mut self) {
        let max_warm_channels = self.message_cache.max_warm_message_channels.max(1);
        while self.message_cache.warm_message_channels.len() > max_warm_channels {
            let Some(evicted_index) =
                self.message_cache
                    .warm_message_channels
                    .iter()
                    .position(|channel_id| {
                        Some(*channel_id) != self.session.selected_message_channel_id
                    })
            else {
                break;
            };
            let Some(evicted_channel_id) = self
                .message_cache_mut()
                .warm_message_channels
                .remove(evicted_index)
            else {
                break;
            };
            self.redact_channel_message_bodies(evicted_channel_id);
            if self
                .message_cache
                .timelines
                .contains_key(&evicted_channel_id)
            {
                self.message_cache_mut()
                    .cold_message_channels
                    .insert(evicted_channel_id);
            }
        }
    }

    pub fn pinned_messages_for_channel(&self, channel_id: Id<ChannelMarker>) -> Vec<&MessageState> {
        self.message_cache
            .pinned_messages
            .get(&channel_id)
            .map(|messages| messages.iter().rev().collect())
            .unwrap_or_default()
    }

    pub fn message_author_role_color(
        &self,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<u32> {
        let roles = self.guild_details.roles.get(&guild_id)?;
        if let Some(member) = self
            .guild_details
            .members
            .get(&guild_id)
            .and_then(|members| members.get(&user_id))
            && member.role_ids_known
        {
            return selected_member_role_color(member, roles);
        }

        if let Some(role_ids) = self.profiles.profile_role_ids.get(&(guild_id, user_id)) {
            return selected_role_ids_color(role_ids, roles);
        }

        let role_ids = self
            .message_cache
            .message_author_role_ids
            .get(&(channel_id, message_id))?;
        selected_role_ids_color(role_ids, roles)
    }

    pub fn user_role_color(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> Option<u32> {
        let roles = self.guild_details.roles.get(&guild_id)?;
        if let Some(member) = self
            .guild_details
            .members
            .get(&guild_id)
            .and_then(|members| members.get(&user_id))
            && member.role_ids_known
        {
            return selected_member_role_color(member, roles);
        }

        let role_ids = self.profiles.profile_role_ids.get(&(guild_id, user_id))?;
        selected_role_ids_color(role_ids, roles)
    }

    pub(in crate::discord) fn message_author_display_name(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        author_id: Id<UserMarker>,
        fallback: &str,
    ) -> String {
        if guild_id.is_none() {
            return self.private_user_display_name(author_id, Some(fallback), None);
        }
        if let Some(member) = guild_id
            .and_then(|guild_id| self.guild_details.members.get(&guild_id))
            .and_then(|members| members.get(&author_id))
            && !is_fallback_identity(member.username.as_deref(), &member.display_name)
        {
            return member.display_name.clone();
        }
        self.profiles
            .user_profiles
            .get(&UserProfileCacheKey::new(author_id, guild_id))
            .map(|profile| profile.display_name().to_owned())
            .or_else(|| {
                self.session
                    .ready_users
                    .get(&author_id)
                    .map(|user| user.display_name.clone())
                    .filter(|name| name != "unknown")
            })
            .unwrap_or_else(|| fallback.to_owned())
    }

    pub(in crate::discord) fn message_author_avatar_url(
        &self,
        guild_id: Option<Id<GuildMarker>>,
        author_id: Id<UserMarker>,
        fallback: &Option<String>,
    ) -> Option<String> {
        guild_id
            .and_then(|guild_id| self.guild_details.members.get(&guild_id))
            .and_then(|members| members.get(&author_id))
            .and_then(|member| member.avatar_url.clone())
            .or_else(|| fallback.clone())
    }

    fn for_each_cached_message_mut(&mut self, mut update: impl FnMut(&mut MessageState)) {
        let message_cache = self.message_cache_mut();
        for messages in message_cache
            .timelines
            .values_mut()
            .map(|timeline| &mut timeline.messages)
            .chain(message_cache.pinned_messages.values_mut())
        {
            for message in messages {
                update(message);
            }
        }
    }

    fn update_cached_messages_in_channel(
        &mut self,
        channel_id: Id<ChannelMarker>,
        mut update: impl FnMut(&mut VecDeque<MessageState>),
    ) {
        if let Some(timeline) = self.message_cache_mut().timelines.get_mut(&channel_id) {
            update(&mut timeline.messages);
        }
        if let Some(messages) = self
            .message_cache_mut()
            .pinned_messages
            .get_mut(&channel_id)
        {
            update(messages);
        }
    }

    pub(in crate::discord) fn refresh_message_author_display_name(
        &mut self,
        guild_id: Id<GuildMarker>,
        member: &MemberInfo,
    ) {
        self.refresh_message_author_display_names(guild_id, std::slice::from_ref(member));
    }

    /// Batch variant: resolves every member's display identity up front, then
    /// updates the whole message cache in a single pass. Member-list syncs
    /// carry up to 1000 members, so one scan per member would be quadratic.
    pub(in crate::discord) fn refresh_message_author_display_names(
        &mut self,
        guild_id: Id<GuildMarker>,
        members: &[MemberInfo],
    ) {
        let mut identities: HashMap<Id<UserMarker>, (String, Option<String>)> = HashMap::new();
        for member in members {
            // If this member payload is a fallback ("unknown", no username),
            // avoid clobbering messages that already have a real name. Try the
            // profile cache for a better name. Otherwise skip this member.
            let display_name =
                if is_fallback_identity(member.username.as_deref(), &member.display_name) {
                    match self
                        .profiles
                        .user_profiles
                        .get(&UserProfileCacheKey::new(member.user_id, Some(guild_id)))
                    {
                        Some(profile) => profile.display_name().to_owned(),
                        None => continue,
                    }
                } else {
                    member.display_name.clone()
                };
            identities.insert(member.user_id, (display_name, member.avatar_url.clone()));
        }
        if identities.is_empty() {
            return;
        }

        let message_cache = self.message_cache_mut();
        for messages in message_cache
            .timelines
            .values_mut()
            .map(|timeline| &mut timeline.messages)
            .chain(message_cache.pinned_messages.values_mut())
        {
            for message in messages.iter_mut().filter(|m| m.guild_id == Some(guild_id)) {
                if message.webhook_id.is_none()
                    && let Some((display_name, avatar_url)) = identities.get(&message.author_id)
                {
                    message.author = display_name.clone();
                    if avatar_url.is_some() || message.author_avatar_url.is_none() {
                        message.author_avatar_url = avatar_url.clone();
                    }
                }
                if let Some(reply) = message.reply.as_mut()
                    && let Some((display_name, _)) = reply
                        .author_id
                        .and_then(|author_id| identities.get(&author_id))
                {
                    reply.author = display_name.clone();
                }
                if let Some(interaction) = message.interaction.as_mut()
                    && let Some((display_name, _)) = interaction
                        .user_id
                        .and_then(|user_id| identities.get(&user_id))
                {
                    interaction.user = display_name.clone();
                }
            }
        }
    }

    pub(in crate::discord) fn refresh_message_author_from_profile(
        &mut self,
        guild_id: Option<Id<GuildMarker>>,
        user_id: Id<UserMarker>,
        display_name: &str,
        avatar_url: Option<&str>,
    ) {
        self.for_each_cached_message_mut(|message| {
            if message.guild_id == guild_id {
                if message.webhook_id.is_none() && message.author_id == user_id {
                    message.author = display_name.to_owned();
                    if avatar_url.is_some() || message.author_avatar_url.is_none() {
                        message.author_avatar_url = avatar_url.map(str::to_owned);
                    }
                }
                if let Some(reply) = &mut message.reply
                    && reply.author_id == Some(user_id)
                {
                    reply.author = display_name.to_owned();
                }
                if let Some(interaction) = &mut message.interaction
                    && interaction.user_id == Some(user_id)
                {
                    interaction.user = display_name.to_owned();
                }
            }
        });
    }

    pub(in crate::discord) fn upsert_message(&mut self, mut message: MessageState) {
        let channel_id = message.channel_id;
        let message_id = message.id;
        message.guild_id = message
            .guild_id
            .or_else(|| self.channel_guild_id(channel_id));
        let recent_limit = self.message_cache.max_messages_per_channel;
        let timeline = self
            .message_cache_mut()
            .timelines
            .entry(message.channel_id)
            .or_default();
        let inserted = if let Some(existing) = timeline
            .messages
            .iter_mut()
            .find(|item| item.id == message.id)
        {
            merge_duplicate_message_create(existing, &message);
            false
        } else {
            timeline.messages.push_back(message);
            timeline
                .messages
                .make_contiguous()
                .sort_by_key(|message| message.id);
            true
        };
        let evicted_message_ids = timeline.insert_live_message(message_id, inserted, recent_limit);
        for evicted_message_id in evicted_message_ids {
            self.prune_message_author_role_ids_if_unreferenced(channel_id, evicted_message_id);
        }
        self.record_channel_message_id(channel_id, message_id);
        if inserted {
            self.increment_thread_message_counts(channel_id);
        }
    }

    pub(in crate::discord) fn add_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: ReactionEmoji,
    ) {
        self.update_cached_messages_in_channel(channel_id, |messages| {
            add_reaction_in(messages, message_id, emoji.clone());
        });
    }

    pub(in crate::discord) fn remove_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) {
        self.update_cached_messages_in_channel(channel_id, |messages| {
            remove_reaction_in(messages, message_id, emoji);
        });
    }

    pub(in crate::discord) fn add_gateway_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        user_id: Id<UserMarker>,
        emoji: ReactionEmoji,
    ) {
        let is_current_user = self.session.current_user_id == Some(user_id);
        self.update_cached_messages_in_channel(channel_id, |messages| {
            add_gateway_reaction_in(messages, message_id, is_current_user, emoji.clone());
        });
    }

    pub(in crate::discord) fn remove_gateway_reaction(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        user_id: Id<UserMarker>,
        emoji: &ReactionEmoji,
    ) {
        let is_current_user = self.session.current_user_id == Some(user_id);
        self.update_cached_messages_in_channel(channel_id, |messages| {
            remove_gateway_reaction_in(messages, message_id, is_current_user, emoji);
        });
    }

    pub(in crate::discord) fn clear_gateway_reactions(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        self.update_cached_messages_in_channel(channel_id, |messages| {
            clear_gateway_reactions_in(messages, message_id);
        });
    }

    pub(in crate::discord) fn clear_gateway_reaction_emoji(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) {
        self.update_cached_messages_in_channel(channel_id, |messages| {
            clear_gateway_reaction_emoji_in(messages, message_id, emoji);
        });
    }

    pub(in crate::discord) fn update_current_user_poll_vote(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        answer_ids: &[u8],
    ) {
        self.update_cached_messages_in_channel(channel_id, |messages| {
            update_current_user_poll_vote_in(messages, message_id, answer_ids);
        });
    }

    /// Shared spine of every history merge: hydrate the incoming page, hand it
    /// to the channel's timeline, then let `place` decide which segment the new
    /// ids belong to. `place` returns the messages it evicted.
    fn merge_history_with(
        &mut self,
        channel_id: Id<ChannelMarker>,
        history: &[MessageInfo],
        place: impl FnOnce(
            &mut ChannelMessageTimeline,
            &[Id<MessageMarker>],
            usize,
        ) -> Vec<Id<MessageMarker>>,
    ) {
        let incoming = self.message_states_from_history(channel_id, history);
        let recent_limit = self.message_cache.max_messages_per_channel;
        let timeline = self
            .message_cache_mut()
            .timelines
            .entry(channel_id)
            .or_default();
        let incoming_ids = timeline.merge_messages(incoming);
        let evicted = place(timeline, &incoming_ids, recent_limit);
        self.finish_message_timeline_merge(channel_id, evicted);
    }

    pub(in crate::discord) fn merge_message_history(
        &mut self,
        channel_id: Id<ChannelMarker>,
        before: Option<Id<MessageMarker>>,
        history: &[MessageInfo],
    ) {
        self.merge_history_with(channel_id, history, |timeline, ids, limit| match before {
            Some(before) => timeline.merge_older(before, ids, limit),
            None => timeline.merge_latest(ids, limit),
        });
    }

    pub(in crate::discord) fn replace_message_history(
        &mut self,
        channel_id: Id<ChannelMarker>,
        history: &[MessageInfo],
    ) {
        if let Some(timeline) = self.message_cache_mut().timelines.remove(&channel_id) {
            for message in timeline.messages {
                self.prune_message_author_role_ids_if_unreferenced(channel_id, message.id);
            }
        }
        self.merge_history_with(channel_id, history, |timeline, ids, limit| {
            timeline.replace_latest(ids);
            timeline.trim_to_retention_limit(limit)
        });
        self.touch_warm_message_channel(channel_id);
    }

    pub(in crate::discord) fn merge_message_history_around(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        history: &[MessageInfo],
    ) {
        self.merge_history_with(channel_id, history, |timeline, ids, limit| {
            timeline.merge_around(message_id, ids, limit)
        });
    }

    pub(in crate::discord) fn merge_detached_message_history(
        &mut self,
        channel_id: Id<ChannelMarker>,
        history: &[MessageInfo],
    ) {
        self.merge_history_with(channel_id, history, |timeline, ids, limit| {
            timeline.merge_detached(ids, limit)
        });
    }

    pub(in crate::discord) fn merge_message_history_after(
        &mut self,
        channel_id: Id<ChannelMarker>,
        after: Id<MessageMarker>,
        history: &[MessageInfo],
        has_more: bool,
    ) {
        self.merge_history_with(channel_id, history, |timeline, ids, limit| {
            timeline.merge_newer(after, ids, has_more, limit)
        });
    }

    fn message_states_from_history(
        &mut self,
        channel_id: Id<ChannelMarker>,
        history: &[MessageInfo],
    ) -> Vec<MessageState> {
        let channel_guild_id = self.channel_guild_id(channel_id);
        let mut incoming = Vec::new();
        for message in history
            .iter()
            .filter(|message| message.channel_id == channel_id)
        {
            self.record_message_author_role_ids(message);
            let mut state = self.message_state_from_info(channel_guild_id, message);
            if self.pinned_message_known(channel_id, state.id) {
                state.pinned = true;
            }
            incoming.push(state);
        }
        incoming
    }

    fn finish_message_timeline_merge(
        &mut self,
        channel_id: Id<ChannelMarker>,
        evicted_message_ids: Vec<Id<MessageMarker>>,
    ) {
        for message_id in evicted_message_ids {
            self.prune_message_author_role_ids_if_unreferenced(channel_id, message_id);
        }
        let last_message_id = self
            .message_cache
            .timelines
            .get(&channel_id)
            .and_then(|timeline| timeline.messages.back())
            .map(|message| message.id);
        if let Some(last_message_id) = last_message_id {
            self.record_channel_message_id(channel_id, last_message_id);
        }
    }

    pub(in crate::discord) fn replace_pinned_messages(
        &mut self,
        channel_id: Id<ChannelMarker>,
        pins: &[MessageInfo],
    ) {
        let channel_guild_id = self.channel_guild_id(channel_id);
        let previous_pin_ids = self
            .message_cache
            .pinned_messages
            .get(&channel_id)
            .map(|messages| {
                messages
                    .iter()
                    .map(|message| message.id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut by_id = BTreeMap::new();
        for pin in pins
            .iter()
            .filter(|message| message.channel_id == channel_id)
        {
            self.record_message_author_role_ids(pin);
            let mut pinned = self.message_state_from_info(channel_guild_id, pin);
            pinned.pinned = true;
            if let Some(existing) = self
                .message_cache_mut()
                .timelines
                .get_mut(&channel_id)
                .and_then(|timeline| {
                    timeline
                        .messages
                        .iter_mut()
                        .find(|message| message.id == pinned.id)
                })
            {
                merge_message(existing, &pinned);
            }
            by_id.insert(pinned.id, pinned);
        }

        let loaded_pin_ids = by_id.keys().copied().collect::<Vec<_>>();
        if let Some(timeline) = self.message_cache_mut().timelines.get_mut(&channel_id) {
            for message in &mut timeline.messages {
                message.pinned = loaded_pin_ids.contains(&message.id);
            }
        }

        self.message_cache_mut()
            .pinned_messages
            .insert(channel_id, by_id.into_values().collect());
        for previous_pin_id in previous_pin_ids {
            self.prune_message_author_role_ids_if_unreferenced(channel_id, previous_pin_id);
        }
    }

    pub(in crate::discord) fn message_state_from_info(
        &self,
        channel_guild_id: Option<Id<GuildMarker>>,
        message: &MessageInfo,
    ) -> MessageState {
        let guild_id = message.guild_id.or(channel_guild_id);
        // A webhook author is a per-message display identity, not a guild
        // member. Resolving it through member or profile caches would replace
        // persona names and avatars supplied by tools such as Tupperbox.
        let (author, author_avatar_url) = if message.webhook_id.is_some() {
            (message.author.clone(), message.author_avatar_url.clone())
        } else {
            (
                self.message_author_display_name(guild_id, message.author_id, &message.author),
                self.message_author_avatar_url(
                    guild_id,
                    message.author_id,
                    &message.author_avatar_url,
                ),
            )
        };
        MessageState {
            id: message.message_id,
            nonce: message.nonce,
            guild_id,
            channel_id: message.channel_id,
            webhook_id: message.webhook_id,
            author_id: message.author_id,
            author,
            author_avatar_url,
            author_is_bot: message.author_is_bot,
            message_kind: message.message_kind,
            interaction: message.interaction.clone(),
            reference: message.reference.clone(),
            reply: message.reply.clone(),
            poll: message.poll.clone(),
            pinned: message.pinned,
            reactions: message.reactions.clone(),
            content: message.content.clone(),
            stickers: message.stickers.clone(),
            mentions: message.mentions.clone(),
            mention_everyone: message.mention_everyone,
            mention_roles: message.mention_roles.clone(),
            flags: message.flags,
            attachments: message.attachments.clone(),
            embeds: message.embeds.clone(),
            forwarded_snapshots: message.forwarded_snapshots.clone(),
            edited_timestamp: message.edited_timestamp.clone(),
        }
    }

    fn pinned_message_known(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> bool {
        self.message_cache
            .pinned_messages
            .get(&channel_id)
            .is_some_and(|messages| messages.iter().any(|message| message.id == message_id))
    }

    pub(in crate::discord) fn update_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        update: MessageUpdateFields,
    ) {
        self.update_cached_messages_in_channel(channel_id, |messages| {
            update_message_in(messages, message_id, &update);
        });
    }

    pub(in crate::discord) fn set_cached_message_pinned(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        pinned: bool,
    ) {
        let normal_message = self
            .message_cache_mut()
            .timelines
            .get_mut(&channel_id)
            .and_then(|timeline| {
                timeline
                    .messages
                    .iter_mut()
                    .find(|message| message.id == message_id)
                    .map(|message| {
                        message.pinned = pinned;
                        message.clone()
                    })
            });

        if pinned {
            if let Some(mut message) = normal_message {
                message.pinned = true;
                upsert_sorted_message(
                    self.message_cache_mut()
                        .pinned_messages
                        .entry(channel_id)
                        .or_default(),
                    message,
                );
            }
        } else {
            let removed_from_pins = self
                .message_cache_mut()
                .pinned_messages
                .get_mut(&channel_id)
                .is_some_and(|messages| {
                    let before = messages.len();
                    messages.retain(|message| message.id != message_id);
                    messages.len() != before
                });
            if removed_from_pins {
                self.prune_message_author_role_ids_if_unreferenced(channel_id, message_id);
            }
        }
    }

    pub(in crate::discord) fn invalidate_pinned_messages(&mut self, channel_id: Id<ChannelMarker>) {
        let previous_pin_ids = self
            .message_cache_mut()
            .pinned_messages
            .remove(&channel_id)
            .map(|messages| {
                messages
                    .into_iter()
                    .map(|message| message.id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for previous_pin_id in previous_pin_ids {
            self.prune_message_author_role_ids_if_unreferenced(channel_id, previous_pin_id);
        }
    }

    pub(in crate::discord) fn delete_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        self.delete_messages(channel_id, &[message_id]);
    }

    pub(in crate::discord) fn delete_messages(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_ids: &[Id<MessageMarker>],
    ) {
        if let Some(timeline) = self.message_cache_mut().timelines.get_mut(&channel_id) {
            timeline.remove_messages(message_ids);
        }
        if let Some(messages) = self
            .message_cache_mut()
            .pinned_messages
            .get_mut(&channel_id)
        {
            messages.retain(|message| !message_ids.contains(&message.id));
        }
        for message_id in message_ids {
            self.message_cache_mut()
                .message_author_role_ids
                .remove(&(channel_id, *message_id));
        }
    }

    fn record_message_author_role_ids(&mut self, message: &MessageInfo) {
        self.record_author_role_ids(
            message.channel_id,
            message.message_id,
            &message.author_role_ids,
            message.author_role_ids_present,
        );
    }

    pub(in crate::discord) fn record_author_role_ids(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        author_role_ids: &[Id<RoleMarker>],
        author_role_ids_present: bool,
    ) {
        let key = (channel_id, message_id);
        if author_role_ids.is_empty() && !author_role_ids_present {
            return;
        }

        self.message_cache_mut()
            .message_author_role_ids
            .insert(key, author_role_ids.to_vec());
    }

    fn prune_message_author_role_ids_if_unreferenced(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) {
        let is_still_cached = self
            .message_cache
            .timelines
            .get(&channel_id)
            .is_some_and(|timeline| timeline.contains_message(message_id))
            || self
                .message_cache
                .pinned_messages
                .get(&channel_id)
                .is_some_and(|messages| messages.iter().any(|message| message.id == message_id));
        if !is_still_cached {
            self.message_cache_mut()
                .message_author_role_ids
                .remove(&(channel_id, message_id));
        }
    }
}

fn merge_message(existing: &mut MessageState, incoming: &MessageState) {
    merge_shared_message_fields(existing, incoming);
    existing.author_is_bot |= incoming.author_is_bot;
    if let Some(content) = &incoming.content
        && (!content.is_empty() || message_content_is_empty(existing))
    {
        existing.content = Some(content.clone());
    }
    if !incoming.stickers.is_empty() || existing.stickers.is_empty() {
        existing.stickers = incoming.stickers.clone();
    }
    existing.mentions = merge_message_mentions(&existing.mentions, &incoming.mentions);
    existing.mention_everyone = incoming.mention_everyone;
    existing.mention_roles = incoming.mention_roles.clone();
    existing.flags = incoming.flags;
    if !incoming.embeds.is_empty() || existing.embeds.is_empty() {
        existing.embeds = incoming.embeds.clone();
    }
    if incoming.edited_timestamp.is_some() || existing.edited_timestamp.is_none() {
        existing.edited_timestamp = incoming.edited_timestamp.clone();
    }
}

fn merge_duplicate_message_create(existing: &mut MessageState, incoming: &MessageState) {
    merge_shared_message_fields(existing, incoming);
    if incoming.reference.is_some() || existing.reference.is_none() {
        existing.reference = incoming.reference.clone();
    }
    if incoming.content.is_some() {
        existing.content = incoming.content.clone();
    }
    if !incoming.mentions.is_empty() || existing.mentions.is_empty() {
        existing.mentions = merge_message_mentions(&existing.mentions, &incoming.mentions);
    }
    existing.mention_everyone = incoming.mention_everyone;
    existing.mention_roles = incoming.mention_roles.clone();
    existing.flags = incoming.flags;
}

fn merge_shared_message_fields(existing: &mut MessageState, incoming: &MessageState) {
    existing.guild_id = incoming.guild_id.or(existing.guild_id);
    existing.channel_id = incoming.channel_id;
    existing.webhook_id = incoming.webhook_id.or(existing.webhook_id);
    existing.author_id = incoming.author_id;
    existing.author = preferred_user_label(&existing.author, &incoming.author);
    if incoming.author_avatar_url.is_some() || existing.author_avatar_url.is_none() {
        existing.author_avatar_url = incoming.author_avatar_url.clone();
    }
    existing.message_kind = incoming.message_kind;
    if let Some(incoming_reply) = incoming.reply.as_ref() {
        let mut merged = incoming_reply.clone();
        if let Some(previous) = existing.reply.as_ref()
            && (merged.author_id == previous.author_id || merged.author_id.is_none())
        {
            merged.author_id = merged.author_id.or(previous.author_id);
            merged.author = preferred_user_label(&previous.author, &merged.author);
        }
        existing.reply = Some(merged);
    }
    if let Some(incoming_interaction) = incoming.interaction.as_ref() {
        let mut merged = incoming_interaction.clone();
        if let Some(previous) = existing.interaction.as_ref()
            && (merged.user_id == previous.user_id || merged.user_id.is_none())
        {
            merged.user_id = merged.user_id.or(previous.user_id);
            merged.user = preferred_user_label(&previous.user, &merged.user);
            if merged.command_name.is_none() {
                merged.command_name = previous.command_name.clone();
            }
        }
        existing.interaction = Some(merged);
    }
    if incoming.poll.is_some() || existing.poll.is_none() {
        existing.poll = incoming.poll.clone();
    }
    existing.pinned = existing.pinned || incoming.pinned;
    existing.reactions = incoming.reactions.clone();
    if !incoming.attachments.is_empty() || existing.attachments.is_empty() {
        existing.attachments = incoming.attachments.clone();
    }
    if !incoming.forwarded_snapshots.is_empty() || existing.forwarded_snapshots.is_empty() {
        existing.forwarded_snapshots = incoming.forwarded_snapshots.clone();
    }
}

fn user_label_is_fallback(label: &str) -> bool {
    label.is_empty() || label == "unknown"
}

fn preferred_user_label(existing: &str, incoming: &str) -> String {
    if user_label_is_fallback(incoming) && !user_label_is_fallback(existing) {
        existing.to_owned()
    } else {
        incoming.to_owned()
    }
}

fn message_content_is_empty(message: &MessageState) -> bool {
    message
        .content
        .as_deref()
        .map(str::is_empty)
        .unwrap_or(true)
}

fn update_message_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    update: &MessageUpdateFields,
) {
    let Some(existing) = messages.iter_mut().find(|item| item.id == message_id) else {
        return;
    };
    if let Some(poll) = &update.body.poll {
        existing.poll = Some(poll.clone());
    }
    if let Some(pinned) = update.body.pinned {
        existing.pinned = pinned;
    }
    if let Some(reactions) = &update.reactions {
        existing.reactions = reactions.clone();
    }
    if update.retain_body {
        if let Some(content) = &update.body.content {
            existing.content = Some(content.clone());
        }
        if let Some(stickers) = &update.body.stickers {
            existing.stickers = stickers.clone();
        }
        if let Some(mentions) = &update.body.mentions {
            existing.mentions = mentions.clone();
        }
        if let Some(mention_everyone) = update.body.mention_everyone {
            existing.mention_everyone = mention_everyone;
        }
        if let Some(mention_roles) = &update.body.mention_roles {
            existing.mention_roles = mention_roles.clone();
        }
        if let Some(flags) = update.body.flags {
            existing.flags = flags;
        }
        if let Some(embeds) = &update.body.embeds {
            existing.embeds = embeds.clone();
        }
        if let Some(edited_timestamp) = &update.body.edited_timestamp {
            existing.edited_timestamp = Some(edited_timestamp.clone());
        }
        if let Some(attachments) = update.body.attachments.replacement() {
            existing.attachments = attachments.to_vec();
        }
    }
}

fn add_reaction_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    emoji: ReactionEmoji,
) {
    let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
        return;
    };
    if let Some(reaction) = message
        .reactions
        .iter_mut()
        .find(|reaction| reaction.emoji == emoji)
    {
        if !reaction.me {
            reaction.count = reaction.count.saturating_add(1);
        }
        reaction.me = true;
    } else {
        message.reactions.push(ReactionInfo {
            emoji,
            count: 1,
            me: true,
        });
    }
}

fn remove_reaction_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    emoji: &ReactionEmoji,
) {
    let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
        return;
    };
    if let Some(reaction) = message
        .reactions
        .iter_mut()
        .find(|reaction| &reaction.emoji == emoji)
    {
        if reaction.me {
            reaction.count = reaction.count.saturating_sub(1);
        }
        reaction.me = false;
    }
    message.reactions.retain(|reaction| reaction.count > 0);
}

fn add_gateway_reaction_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    is_current_user: bool,
    emoji: ReactionEmoji,
) {
    let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
        return;
    };
    if let Some(reaction) = message
        .reactions
        .iter_mut()
        .find(|reaction| reaction.emoji == emoji)
    {
        if !(is_current_user && reaction.me) {
            reaction.count = reaction.count.saturating_add(1);
        }
        if is_current_user {
            reaction.me = true;
        }
    } else {
        message.reactions.push(ReactionInfo {
            emoji,
            count: 1,
            me: is_current_user,
        });
    }
}

fn remove_gateway_reaction_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    is_current_user: bool,
    emoji: &ReactionEmoji,
) {
    let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
        return;
    };
    if let Some(reaction) = message
        .reactions
        .iter_mut()
        .find(|reaction| &reaction.emoji == emoji)
    {
        if !is_current_user || reaction.me {
            reaction.count = reaction.count.saturating_sub(1);
        }
        if is_current_user {
            reaction.me = false;
        }
    }
    message.reactions.retain(|reaction| reaction.count > 0);
}

fn clear_gateway_reactions_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
) {
    let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
        return;
    };
    message.reactions.clear();
}

fn clear_gateway_reaction_emoji_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    emoji: &ReactionEmoji,
) {
    let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
        return;
    };
    message
        .reactions
        .retain(|reaction| &reaction.emoji != emoji);
}

fn update_current_user_poll_vote_in(
    messages: &mut VecDeque<MessageState>,
    message_id: Id<MessageMarker>,
    answer_ids: &[u8],
) {
    let Some(poll) = messages
        .iter_mut()
        .find(|message| message.id == message_id)
        .and_then(|message| message.poll.as_mut())
    else {
        return;
    };

    let mut added_votes = 0u64;
    let mut removed_votes = 0u64;
    for answer in &mut poll.answers {
        let next_me_voted = answer_ids.contains(&answer.answer_id);
        match (answer.me_voted, next_me_voted) {
            (false, true) => {
                answer.vote_count = Some(answer.vote_count.unwrap_or(0).saturating_add(1));
                added_votes = added_votes.saturating_add(1);
            }
            (true, false) => {
                answer.vote_count = Some(answer.vote_count.unwrap_or(0).saturating_sub(1));
                removed_votes = removed_votes.saturating_add(1);
            }
            _ => {}
        }
        answer.me_voted = next_me_voted;
    }
    if let Some(total_votes) = &mut poll.total_votes {
        *total_votes = total_votes
            .saturating_add(added_votes)
            .saturating_sub(removed_votes);
    }
}

fn upsert_sorted_message(messages: &mut VecDeque<MessageState>, message: MessageState) {
    let mut by_id: BTreeMap<Id<MessageMarker>, MessageState> = messages
        .drain(..)
        .map(|message| (message.id, message))
        .collect();
    by_id
        .entry(message.id)
        .and_modify(|existing| merge_message(existing, &message))
        .or_insert(message);
    *messages = by_id.into_values().collect();
}

fn merge_message_mentions(existing: &[MentionInfo], incoming: &[MentionInfo]) -> Vec<MentionInfo> {
    if incoming.is_empty() {
        return Vec::new();
    }

    incoming
        .iter()
        .map(|mention| {
            if mention.guild_nick.is_some() {
                mention.clone()
            } else {
                let previous = existing
                    .iter()
                    .find(|existing| existing.user_id == mention.user_id);
                previous
                    .filter(|previous| {
                        previous.guild_nick.is_some()
                            || !user_label_is_fallback(&previous.display_name)
                    })
                    .cloned()
                    .unwrap_or_else(|| mention.clone())
            }
        })
        .collect()
}
