use std::collections::{BTreeMap, HashSet};

use crate::discord::ids::{
    Id,
    marker::{AttachmentMarker, ChannelMarker, MessageMarker},
};
use crate::discord::{
    AttachmentInfo, MessageAttachmentUpload, MessageReferenceInfo, MessageState, ReplyInfo,
    ReplyReference,
};

use super::{DashboardState, MessagePaneSource};

#[derive(Debug, Default)]
pub(super) struct PendingMessageUiState {
    by_channel: BTreeMap<Id<ChannelMarker>, Vec<MessageState>>,
}

impl PendingMessageUiState {
    pub(super) fn messages_for_channel(&self, channel_id: Id<ChannelMarker>) -> &[MessageState] {
        self.by_channel
            .get(&channel_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn contains_message(&self, message: &MessageState) -> bool {
        self.messages_for_channel(message.channel_id)
            .iter()
            .any(|pending| std::ptr::eq(pending, message))
    }

    fn insert(&mut self, message: MessageState) {
        self.by_channel
            .entry(message.channel_id)
            .or_default()
            .push(message);
    }

    fn remove(&mut self, channel_id: Id<ChannelMarker>, nonce: Id<MessageMarker>) -> bool {
        let Some(messages) = self.by_channel.get_mut(&channel_id) else {
            return false;
        };
        let previous_len = messages.len();
        messages.retain(|message| message.id != nonce);
        let removed = messages.len() != previous_len;
        if messages.is_empty() {
            self.by_channel.remove(&channel_id);
        }
        removed
    }
}

impl DashboardState {
    pub(super) fn stage_pending_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        nonce: Id<MessageMarker>,
        content: &str,
        reply_to: Option<ReplyReference>,
        uploads: &[MessageAttachmentUpload],
    ) {
        let Some(author_id) = self.current_user_id() else {
            return;
        };
        let Some(author) = self.current_user().map(str::to_owned) else {
            return;
        };

        let guild_id = self
            .discord
            .cache
            .channel(channel_id)
            .and_then(|channel| channel.guild_id);
        let confirmed_messages = self.discord.cache.messages_for_channel(channel_id);
        let author_avatar_url = confirmed_messages
            .iter()
            .rev()
            .find(|message| message.author_id == author_id)
            .and_then(|message| message.author_avatar_url.clone());
        let reply = reply_to.and_then(|reference| {
            confirmed_messages
                .iter()
                .find(|message| message.id == reference.message_id)
                .map(|message| ReplyInfo {
                    author_id: Some(message.author_id),
                    author: message.author.clone(),
                    content: message.content.clone(),
                    sticker_names: message.sticker_names.clone(),
                    mentions: message.mentions.clone(),
                })
        });
        let reference = reply_to.map(|reply| MessageReferenceInfo {
            guild_id,
            channel_id: Some(channel_id),
            message_id: Some(reply.message_id),
        });
        let attachments = uploads
            .iter()
            .enumerate()
            .map(|(index, upload)| AttachmentInfo {
                id: Id::<AttachmentMarker>::new(
                    u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1),
                ),
                filename: upload.filename.clone(),
                url: String::new(),
                proxy_url: String::new(),
                content_type: None,
                size: upload.size_bytes,
                width: None,
                height: None,
                description: None,
            })
            .collect();

        self.pending_messages.insert(MessageState {
            id: nonce,
            nonce: Some(nonce),
            guild_id,
            channel_id,
            author_id,
            author,
            author_avatar_url,
            content: (!content.is_empty()).then(|| content.to_owned()),
            reference,
            reply,
            attachments,
            ..MessageState::default()
        });
        self.clear_message_row_content_metrics_cache();

        if self.message_pane_source() == Some(MessagePaneSource::ChannelMessages { channel_id }) {
            self.messages.message_auto_follow = true;
            self.follow_latest_message();
        }
    }

    pub(in crate::tui) fn remove_pending_message(
        &mut self,
        channel_id: Id<ChannelMarker>,
        nonce: Id<MessageMarker>,
    ) -> bool {
        let removed = self.pending_messages.remove(channel_id, nonce);
        if removed {
            self.clear_message_row_content_metrics_cache();
            self.clamp_message_viewport();
        }
        removed
    }

    pub(super) fn reconcile_pending_messages_with_cache(&mut self) {
        let confirmed = self
            .pending_messages
            .by_channel
            .keys()
            .copied()
            .map(|channel_id| {
                let nonces = self
                    .discord
                    .cache
                    .messages_for_channel(channel_id)
                    .into_iter()
                    .filter_map(|message| message.nonce)
                    .collect::<HashSet<_>>();
                (channel_id, nonces)
            })
            .collect::<BTreeMap<_, _>>();

        let previous_len = self
            .pending_messages
            .by_channel
            .values()
            .map(Vec::len)
            .sum::<usize>();
        self.pending_messages
            .by_channel
            .retain(|channel_id, messages| {
                let confirmed_nonces = &confirmed[channel_id];
                messages.retain(|message| !confirmed_nonces.contains(&message.id));
                !messages.is_empty()
            });
        let current_len = self
            .pending_messages
            .by_channel
            .values()
            .map(Vec::len)
            .sum::<usize>();
        if current_len != previous_len {
            self.clear_message_row_content_metrics_cache();
        }
    }

    pub fn message_is_pending(&self, message: &MessageState) -> bool {
        self.pending_messages.contains_message(message)
    }

    #[cfg(test)]
    pub(crate) fn insert_pending_message_for_test(&mut self, message: MessageState) {
        self.pending_messages.insert(message);
    }
}
