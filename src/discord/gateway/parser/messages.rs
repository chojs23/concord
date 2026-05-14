use serde_json::Value;

use crate::{
    discord::{
        AttachmentInfo, EmbedFieldInfo, EmbedInfo, MentionInfo, MessageInfo, MessageKind,
        MessageReferenceInfo, MessageSnapshotInfo, PollAnswerInfo, PollInfo, ReactionEmoji,
        ReactionInfo, ReplyInfo,
        events::{AppEvent, AttachmentUpdate},
        ids::{
            Id,
            marker::{
                AttachmentMarker, ChannelMarker, EmojiMarker, GuildMarker, MessageMarker,
                RoleMarker, UserMarker,
            },
        },
    },
    logging,
};

use super::shared::{
    display_name_from_parts, display_name_from_parts_or_unknown, parse_id, raw_user_avatar_url,
};

pub(crate) fn parse_message_info(data: &Value) -> Option<MessageInfo> {
    let channel_id = parse_id::<ChannelMarker>(data.get("channel_id")?)?;
    let message_id = parse_id::<MessageMarker>(data.get("id")?)?;
    let author = data.get("author")?;
    let author_id = parse_id::<UserMarker>(author.get("id")?)?;
    let author_name = message_author_display_name(data, author);
    let author_avatar_url = raw_user_avatar_url(author_id, author);
    let author_role_ids = parse_message_author_role_ids(data);
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    let message_kind = data
        .get("type")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .map(MessageKind::new)
        .unwrap_or_default();
    let content = data
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let sticker_names = parse_sticker_names(data.get("sticker_items"));
    let mentions = parse_mentions(data.get("mentions"));
    let attachments = parse_attachments(data.get("attachments"));
    let embeds = parse_embeds(data.get("embeds"));
    let reply = data.get("referenced_message").and_then(parse_reply_info);
    let poll = data
        .get("poll")
        .and_then(parse_poll_info)
        .or_else(|| parse_poll_result_embed(data.get("embeds")));
    let reference = data
        .get("message_reference")
        .map(parse_message_reference_info);
    let source_channel_id = reference
        .as_ref()
        .and_then(|reference| reference.channel_id);
    let forwarded_snapshots =
        parse_message_snapshots(data.get("message_snapshots"), source_channel_id);
    let edited_timestamp = data
        .get("edited_timestamp")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some(MessageInfo {
        guild_id,
        channel_id,
        message_id,
        author_id,
        author: author_name,
        author_avatar_url,
        author_role_ids,
        message_kind,
        reference,
        reply,
        poll,
        pinned: data.get("pinned").and_then(Value::as_bool).unwrap_or(false),
        reactions: parse_reactions(data.get("reactions")),
        content,
        sticker_names,
        mentions,
        attachments,
        embeds,
        forwarded_snapshots,
        edited_timestamp,
    })
}

fn parse_message_author_role_ids(data: &Value) -> Vec<Id<RoleMarker>> {
    data.get("member")
        .and_then(|member| member.get("roles"))
        .and_then(Value::as_array)
        .map(|roles| roles.iter().filter_map(parse_id::<RoleMarker>).collect())
        .unwrap_or_default()
}

fn parse_message_reference_info(value: &Value) -> MessageReferenceInfo {
    MessageReferenceInfo {
        guild_id: value.get("guild_id").and_then(parse_id::<GuildMarker>),
        channel_id: value.get("channel_id").and_then(parse_id::<ChannelMarker>),
        message_id: value.get("message_id").and_then(parse_id::<MessageMarker>),
    }
}

pub(super) fn parse_attachments(value: Option<&Value>) -> Vec<AttachmentInfo> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_attachment).collect())
        .unwrap_or_default()
}

pub(super) fn parse_sticker_names(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("name").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn parse_embeds(value: Option<&Value>) -> Vec<EmbedInfo> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_embed).collect())
        .unwrap_or_default()
}

fn parse_embed(value: &Value) -> Option<EmbedInfo> {
    if value.get("type").and_then(Value::as_str) == Some("poll_result") {
        return None;
    }

    let fields = value
        .get("fields")
        .and_then(Value::as_array)
        .map(|fields| fields.iter().filter_map(parse_embed_field).collect())
        .unwrap_or_default();
    let embed = EmbedInfo {
        color: value
            .get("color")
            .and_then(Value::as_u64)
            .and_then(|color| u32::try_from(color).ok()),
        provider_name: value
            .get("provider")
            .and_then(|provider| provider.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        author_name: value
            .get("author")
            .and_then(|author| author.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        title: value
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_owned),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        fields,
        footer_text: value
            .get("footer")
            .and_then(|footer| footer.get("text"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        url: value.get("url").and_then(Value::as_str).map(str::to_owned),
        thumbnail_url: value
            .get("thumbnail")
            .and_then(|thumbnail| thumbnail.get("url"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        thumbnail_proxy_url: value
            .get("thumbnail")
            .and_then(|thumbnail| thumbnail.get("proxy_url"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        thumbnail_width: value
            .get("thumbnail")
            .and_then(|thumbnail| thumbnail.get("width"))
            .and_then(Value::as_u64),
        thumbnail_height: value
            .get("thumbnail")
            .and_then(|thumbnail| thumbnail.get("height"))
            .and_then(Value::as_u64),
        image_url: value
            .get("image")
            .and_then(|image| image.get("url"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        image_proxy_url: value
            .get("image")
            .and_then(|image| image.get("proxy_url"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        image_width: value
            .get("image")
            .and_then(|image| image.get("width"))
            .and_then(Value::as_u64),
        image_height: value
            .get("image")
            .and_then(|image| image.get("height"))
            .and_then(Value::as_u64),
        video_url: value
            .get("video")
            .and_then(|video| video.get("url"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    };

    embed_has_renderable_content(&embed).then_some(embed)
}

fn parse_embed_field(value: &Value) -> Option<EmbedFieldInfo> {
    Some(EmbedFieldInfo {
        name: value.get("name")?.as_str()?.to_owned(),
        value: value.get("value")?.as_str()?.to_owned(),
    })
}

fn embed_has_renderable_content(embed: &EmbedInfo) -> bool {
    embed.provider_name.is_some()
        || embed.author_name.is_some()
        || embed.title.is_some()
        || embed.description.is_some()
        || !embed.fields.is_empty()
        || embed.footer_text.is_some()
        || embed.url.is_some()
        || embed.thumbnail_url.is_some()
        || embed.image_url.is_some()
        || embed.video_url.is_some()
}

fn parse_message_snapshots(
    value: Option<&Value>,
    source_channel_id: Option<Id<ChannelMarker>>,
) -> Vec<MessageSnapshotInfo> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| parse_message_snapshot(item, source_channel_id))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_reply_info(value: &Value) -> Option<ReplyInfo> {
    if value.is_null() {
        return None;
    }

    let author = value.get("author")?;
    let author_id = author.get("id").and_then(parse_id::<UserMarker>);
    let author_name = message_author_display_name(value, author);
    let content = value
        .get("content")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let sticker_names = parse_sticker_names(value.get("sticker_items"));
    let mentions = parse_mentions(value.get("mentions"));

    Some(ReplyInfo {
        author_id,
        author: author_name,
        content,
        sticker_names,
        mentions,
    })
}

pub(super) fn parse_mentions(value: Option<&Value>) -> Vec<MentionInfo> {
    value
        .and_then(Value::as_array)
        .map(|mentions| mentions.iter().filter_map(parse_mention_info).collect())
        .unwrap_or_default()
}

fn parse_reactions(value: Option<&Value>) -> Vec<ReactionInfo> {
    value
        .and_then(Value::as_array)
        .map(|reactions| reactions.iter().filter_map(parse_reaction_info).collect())
        .unwrap_or_default()
}

fn parse_reaction_info(value: &Value) -> Option<ReactionInfo> {
    Some(ReactionInfo {
        emoji: parse_reaction_emoji(value.get("emoji")?)?,
        count: value.get("count").and_then(Value::as_u64).unwrap_or(0),
        me: value.get("me").and_then(Value::as_bool).unwrap_or(false),
    })
}

pub(super) fn parse_reaction_emoji(value: &Value) -> Option<ReactionEmoji> {
    if let Some(id) = value.get("id").and_then(parse_id::<EmojiMarker>) {
        return Some(ReactionEmoji::Custom {
            id,
            name: value.get("name").and_then(Value::as_str).map(str::to_owned),
            animated: value
                .get("animated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    value
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(|name| ReactionEmoji::Unicode(name.to_owned()))
}

fn parse_mention_info(value: &Value) -> Option<MentionInfo> {
    let user_id = parse_id::<UserMarker>(value.get("id")?)?;
    let member = value.get("member");
    let nick = value
        .get("member")
        .and_then(|member| member.get("nick"))
        .and_then(Value::as_str);
    let global_name = value.get("global_name").and_then(Value::as_str);
    let username = value.get("username").and_then(Value::as_str);
    let display_name = display_name_from_parts(nick, global_name, username)?;
    log_mention_raw_fields(user_id, member, nick, global_name, username, display_name);

    Some(MentionInfo {
        user_id,
        guild_nick: nick.filter(|value| !value.is_empty()).map(str::to_owned),
        display_name: display_name.to_owned(),
    })
}

fn log_mention_raw_fields(
    user_id: Id<UserMarker>,
    member: Option<&Value>,
    nick: Option<&str>,
    global_name: Option<&str>,
    username: Option<&str>,
    display_name: &str,
) {
    logging::debug(
        "gateway",
        format!(
            "mention raw fields user_id={} has_member={} nick={} global_name={} username={} display_name={}",
            user_id.get(),
            member.is_some(),
            log_optional_name(nick),
            log_optional_name(global_name),
            log_optional_name(username),
            display_name,
        ),
    );
}

fn log_optional_name(value: Option<&str>) -> &str {
    value.unwrap_or("<missing>")
}

fn message_author_display_name(message: &Value, author: &Value) -> String {
    let nick = message
        .get("member")
        .and_then(|member| member.get("nick"))
        .and_then(Value::as_str);
    let global_name = author.get("global_name").and_then(Value::as_str);
    let username = author.get("username").and_then(Value::as_str);
    display_name_from_parts_or_unknown(nick, global_name, username)
}

pub(super) fn parse_poll_info(value: &Value) -> Option<PollInfo> {
    if value.is_null() {
        return None;
    }

    let question = value
        .get("question")
        .and_then(|question| question.get("text"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("<no question text>")
        .to_owned();
    let answers: Vec<PollAnswerInfo> = value
        .get("answers")
        .and_then(Value::as_array)
        .map(|answers| answers.iter().filter_map(parse_poll_answer_info).collect())
        .unwrap_or_default();
    let allow_multiselect = value
        .get("allow_multiselect")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let results = value.get("results");
    let results_finalized = results
        .and_then(|results| results.get("is_finalized"))
        .and_then(Value::as_bool);
    let total_votes = results
        .and_then(|results| results.get("answer_counts"))
        .and_then(Value::as_array)
        .map(|counts| {
            counts
                .iter()
                .filter_map(|count| count.get("count").and_then(Value::as_u64))
                .sum()
        });

    Some(PollInfo {
        question,
        answers: answers
            .into_iter()
            .map(|mut answer| {
                if let Some(count) = poll_answer_count(results, answer.answer_id) {
                    answer.vote_count = Some(count.0);
                    answer.me_voted = count.1;
                }
                answer
            })
            .collect(),
        allow_multiselect,
        results_finalized,
        total_votes,
    })
}

pub(super) fn parse_poll_result_embed(value: Option<&Value>) -> Option<PollInfo> {
    let embed = value?
        .as_array()?
        .iter()
        .find(|embed| embed.get("type").and_then(Value::as_str) == Some("poll_result"))?;
    let fields = embed.get("fields")?.as_array()?;
    let mut question = None;
    let mut winner_id = None;
    let mut winner_text = None;
    let mut winner_votes = None;
    let mut total_votes = None;

    for field in fields {
        let Some(name) = field.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(value) = field.get("value").and_then(Value::as_str) else {
            continue;
        };
        match name {
            "poll_question_text" => question = Some(value.to_owned()),
            "victor_answer_id" => winner_id = value.parse::<u8>().ok(),
            "victor_answer_text" => winner_text = Some(value.to_owned()),
            "victor_answer_votes" => winner_votes = value.parse::<u64>().ok(),
            "total_votes" => total_votes = value.parse::<u64>().ok(),
            _ => {}
        }
    }

    let answers = winner_text
        .map(|text| {
            vec![PollAnswerInfo {
                answer_id: winner_id.unwrap_or(1),
                text,
                vote_count: winner_votes,
                me_voted: false,
            }]
        })
        .unwrap_or_default();
    Some(PollInfo {
        question: question.unwrap_or_else(|| "Poll results".to_owned()),
        answers,
        allow_multiselect: false,
        results_finalized: Some(true),
        total_votes,
    })
}

fn parse_poll_answer_info(value: &Value) -> Option<PollAnswerInfo> {
    let answer_id = value
        .get("answer_id")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())?;
    let text = value
        .get("poll_media")
        .and_then(|media| media.get("text"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("<no answer text>")
        .to_owned();

    Some(PollAnswerInfo {
        answer_id,
        text,
        vote_count: None,
        me_voted: false,
    })
}

fn poll_answer_count(results: Option<&Value>, answer_id: u8) -> Option<(u64, bool)> {
    results?
        .get("answer_counts")?
        .as_array()?
        .iter()
        .find(|count| {
            count
                .get("id")
                .and_then(Value::as_u64)
                .is_some_and(|id| id == u64::from(answer_id))
        })
        .map(|count| {
            (
                count.get("count").and_then(Value::as_u64).unwrap_or(0),
                count
                    .get("me_voted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )
        })
}

fn parse_message_snapshot(
    value: &Value,
    source_channel_id: Option<Id<ChannelMarker>>,
) -> Option<MessageSnapshotInfo> {
    let message = value.get("message")?;
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let sticker_names = parse_sticker_names(message.get("sticker_items"));
    let attachments = parse_attachments(message.get("attachments"));
    let embeds = parse_embeds(message.get("embeds"));
    let mentions = parse_mentions(message.get("mentions"));
    let timestamp = message
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::to_owned);

    if content.as_deref().is_some_and(|value| !value.is_empty())
        || !sticker_names.is_empty()
        || !attachments.is_empty()
        || !embeds.is_empty()
        || source_channel_id.is_some()
        || timestamp.is_some()
    {
        Some(MessageSnapshotInfo {
            content,
            sticker_names,
            mentions,
            attachments,
            embeds,
            source_channel_id,
            timestamp,
        })
    } else {
        None
    }
}

fn parse_attachment(value: &Value) -> Option<AttachmentInfo> {
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .or_else(|| value.get("proxy_url").and_then(Value::as_str))?
        .to_owned();
    let proxy_url = value
        .get("proxy_url")
        .and_then(Value::as_str)
        .unwrap_or(url.as_str())
        .to_owned();

    Some(AttachmentInfo {
        id: parse_id::<AttachmentMarker>(value.get("id")?)?,
        filename: value.get("filename")?.as_str()?.to_owned(),
        url,
        proxy_url,
        content_type: value
            .get("content_type")
            .and_then(Value::as_str)
            .map(str::to_owned),
        size: value
            .get("size")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        width: value.get("width").and_then(Value::as_u64),
        height: value.get("height").and_then(Value::as_u64),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

pub(super) fn parse_message_create(data: &Value) -> Option<AppEvent> {
    let message = parse_message_info(data)?;
    Some(AppEvent::MessageCreate {
        guild_id: message.guild_id,
        channel_id: message.channel_id,
        message_id: message.message_id,
        author_id: message.author_id,
        author: message.author,
        author_avatar_url: message.author_avatar_url,
        author_role_ids: message.author_role_ids,
        message_kind: message.message_kind,
        reference: message.reference,
        reply: message.reply,
        poll: message.poll,
        content: message.content,
        sticker_names: message.sticker_names,
        mentions: message.mentions,
        attachments: message.attachments,
        embeds: message.embeds,
        forwarded_snapshots: message.forwarded_snapshots,
    })
}

pub(super) fn parse_message_update(data: &Value) -> Option<AppEvent> {
    let channel_id = parse_id::<ChannelMarker>(data.get("channel_id")?)?;
    let message_id = parse_id::<MessageMarker>(data.get("id")?)?;
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    let content = data
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let sticker_names = data
        .get("sticker_items")
        .map(|value| parse_sticker_names(Some(value)));
    let attachments = if data.get("attachments").is_some() {
        AttachmentUpdate::Replace(parse_attachments(data.get("attachments")))
    } else {
        AttachmentUpdate::Unchanged
    };
    let poll = data
        .get("poll")
        .and_then(parse_poll_info)
        .or_else(|| parse_poll_result_embed(data.get("embeds")));
    let embeds = data.get("embeds").map(|value| parse_embeds(Some(value)));
    let mentions = data
        .get("mentions")
        .map(|value| parse_mentions(Some(value)));
    let edited_timestamp = data
        .get("edited_timestamp")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some(AppEvent::MessageUpdate {
        guild_id,
        channel_id,
        message_id,
        poll,
        content,
        sticker_names,
        mentions,
        attachments,
        embeds,
        edited_timestamp,
    })
}

pub(super) fn parse_message_delete(data: &Value) -> Option<AppEvent> {
    let channel_id = parse_id::<ChannelMarker>(data.get("channel_id")?)?;
    let message_id = parse_id::<MessageMarker>(data.get("id")?)?;
    let guild_id = data.get("guild_id").and_then(parse_id::<GuildMarker>);
    Some(AppEvent::MessageDelete {
        guild_id,
        channel_id,
        message_id,
    })
}

pub(super) fn parse_message_ack(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::MessageAck {
        channel_id: parse_id::<ChannelMarker>(data.get("channel_id")?)?,
        message_id: parse_id::<MessageMarker>(data.get("message_id")?)?,
        mention_count: data
            .get("mention_count")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
    })
}

pub(super) fn parse_message_reaction_add(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::MessageReactionAdd {
        guild_id: data.get("guild_id").and_then(parse_id::<GuildMarker>),
        channel_id: parse_id::<ChannelMarker>(data.get("channel_id")?)?,
        message_id: parse_id::<MessageMarker>(data.get("message_id")?)?,
        user_id: parse_id::<UserMarker>(data.get("user_id")?)?,
        emoji: parse_reaction_emoji(data.get("emoji")?)?,
    })
}

pub(super) fn parse_message_reaction_remove(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::MessageReactionRemove {
        guild_id: data.get("guild_id").and_then(parse_id::<GuildMarker>),
        channel_id: parse_id::<ChannelMarker>(data.get("channel_id")?)?,
        message_id: parse_id::<MessageMarker>(data.get("message_id")?)?,
        user_id: parse_id::<UserMarker>(data.get("user_id")?)?,
        emoji: parse_reaction_emoji(data.get("emoji")?)?,
    })
}

pub(super) fn parse_message_reaction_remove_all(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::MessageReactionRemoveAll {
        guild_id: data.get("guild_id").and_then(parse_id::<GuildMarker>),
        channel_id: parse_id::<ChannelMarker>(data.get("channel_id")?)?,
        message_id: parse_id::<MessageMarker>(data.get("message_id")?)?,
    })
}

pub(super) fn parse_message_reaction_remove_emoji(data: &Value) -> Option<AppEvent> {
    Some(AppEvent::MessageReactionRemoveEmoji {
        guild_id: data.get("guild_id").and_then(parse_id::<GuildMarker>),
        channel_id: parse_id::<ChannelMarker>(data.get("channel_id")?)?,
        message_id: parse_id::<MessageMarker>(data.get("message_id")?)?,
        emoji: parse_reaction_emoji(data.get("emoji")?)?,
    })
}
