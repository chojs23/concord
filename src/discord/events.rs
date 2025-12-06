use twilight_gateway::Event;
use twilight_model::{
    channel::{Channel, Message},
    gateway::{
        payload::incoming::{
            GuildCreate as GuildCreatePayload, MemberAdd, MemberUpdate,
            PresenceUpdate as PresenceUpdatePayload,
        },
        presence::{Status as TwilightStatus, UserOrId},
    },
    guild::Member as TwilightMember,
    id::{
        Id,
        marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker},
    },
    user::User as TwilightUser,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum PresenceStatus {
    Online,
    Idle,
    DoNotDisturb,
    Offline,
}

impl PresenceStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Idle => "Idle",
            Self::DoNotDisturb => "Do Not Disturb",
            Self::Offline => "Offline",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub parent_id: Option<Id<ChannelMarker>>,
    pub position: Option<i32>,
    pub last_message_id: Option<Id<MessageMarker>>,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberInfo {
    pub user_id: Id<UserMarker>,
    pub display_name: String,
    pub is_bot: bool,
}

/// One entry from the user's `guild_folders` setting. A folder with `id ==
/// None` and a single member is an ungrouped guild — Discord stores those as
/// "folders" too just for ordering. Real folders carry an integer id, an
/// optional name, and an optional RGB color.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuildFolder {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub color: Option<u32>,
    pub guild_ids: Vec<Id<GuildMarker>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageInfo {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub message_id: Id<MessageMarker>,
    pub author_id: Id<UserMarker>,
    pub author: String,
    pub content: Option<String>,
}

#[derive(Clone, Debug)]
pub enum AppEvent {
    Ready {
        user: String,
    },
    GuildCreate {
        guild_id: Id<GuildMarker>,
        name: String,
        channels: Vec<ChannelInfo>,
        members: Vec<MemberInfo>,
        presences: Vec<(Id<UserMarker>, PresenceStatus)>,
    },
    GuildUpdate {
        guild_id: Id<GuildMarker>,
        name: String,
    },
    GuildDelete {
        guild_id: Id<GuildMarker>,
    },
    ChannelUpsert(ChannelInfo),
    ChannelDelete {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
    },
    MessageCreate {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        author_id: Id<UserMarker>,
        author: String,
        content: Option<String>,
    },
    MessageHistoryLoaded {
        channel_id: Id<ChannelMarker>,
        messages: Vec<MessageInfo>,
    },
    MessageHistoryLoadFailed {
        channel_id: Id<ChannelMarker>,
        message: String,
    },
    MessageUpdate {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        content: Option<String>,
    },
    MessageDelete {
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    },
    GuildMemberUpsert {
        guild_id: Id<GuildMarker>,
        member: MemberInfo,
    },
    GuildMemberRemove {
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    },
    PresenceUpdate {
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
        status: PresenceStatus,
    },
    GuildFoldersUpdate {
        folders: Vec<GuildFolder>,
    },
    GatewayError {
        message: String,
    },
    GatewayClosed,
}

impl AppEvent {
    pub fn from_message(message: Message) -> Self {
        let message = MessageInfo::from_message(message);
        Self::MessageCreate {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.message_id,
            author_id: message.author_id,
            author: message.author,
            content: message.content,
        }
    }
}

impl MessageInfo {
    pub fn from_message(message: Message) -> Self {
        Self {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
            author_id: message.author.id,
            author: message.author.name,
            content: Some(message.content),
        }
    }
}

pub fn map_event(event: Event, message_content_enabled: bool) -> Option<AppEvent> {
    match event {
        Event::Ready(ready) => Some(AppEvent::Ready {
            user: ready.user.name,
        }),
        Event::GuildCreate(guild) => map_guild_create(*guild),
        Event::GuildDelete(guild) => Some(AppEvent::GuildDelete { guild_id: guild.id }),
        Event::GuildUpdate(guild) => Some(AppEvent::GuildUpdate {
            guild_id: guild.id,
            name: guild.name.clone(),
        }),
        Event::ChannelCreate(channel) => Some(AppEvent::ChannelUpsert(channel_info(&channel.0))),
        Event::ChannelUpdate(channel) => Some(AppEvent::ChannelUpsert(channel_info(&channel.0))),
        Event::ChannelDelete(channel) => Some(AppEvent::ChannelDelete {
            guild_id: channel.guild_id,
            channel_id: channel.id,
        }),
        Event::MessageCreate(message) => Some(AppEvent::MessageCreate {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
            author_id: message.author.id,
            author: message.author.name.clone(),
            content: map_message_content(&message.content, message_content_enabled),
        }),
        Event::MessageUpdate(message) => Some(AppEvent::MessageUpdate {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
            content: map_message_content(&message.content, message_content_enabled),
        }),
        Event::MessageDelete(message) => Some(AppEvent::MessageDelete {
            guild_id: message.guild_id,
            channel_id: message.channel_id,
            message_id: message.id,
        }),
        Event::MemberAdd(member_add) => Some(member_upsert_from_add(&member_add)),
        Event::MemberUpdate(update) => Some(member_upsert_from_update(&update)),
        Event::MemberRemove(remove) => Some(AppEvent::GuildMemberRemove {
            guild_id: remove.guild_id,
            user_id: remove.user.id,
        }),
        Event::PresenceUpdate(presence) => Some(presence_update(&presence)),
        _ => None,
    }
}

fn map_guild_create(guild: GuildCreatePayload) -> Option<AppEvent> {
    let guild = match guild {
        GuildCreatePayload::Available(guild) => guild,
        GuildCreatePayload::Unavailable(_) => return None,
    };

    let channels = guild.channels.iter().map(channel_info).collect();
    let members = guild.members.iter().map(member_info).collect();
    let presences = guild
        .presences
        .iter()
        .map(|presence| (presence.user.id(), map_status(presence.status)))
        .collect();

    Some(AppEvent::GuildCreate {
        guild_id: guild.id,
        name: guild.name,
        channels,
        members,
        presences,
    })
}

fn channel_info(channel: &Channel) -> ChannelInfo {
    ChannelInfo {
        guild_id: channel.guild_id,
        channel_id: channel.id,
        parent_id: channel.parent_id,
        position: channel.position,
        last_message_id: channel.last_message_id.map(|id| Id::new(id.get())),
        name: channel
            .name
            .clone()
            .unwrap_or_else(|| format!("channel-{}", channel.id.get())),
        kind: format!("{:?}", channel.kind),
    }
}

fn member_info(member: &TwilightMember) -> MemberInfo {
    MemberInfo {
        user_id: member.user.id,
        display_name: display_name(member.nick.as_deref(), &member.user),
        is_bot: member.user.bot,
    }
}

fn member_upsert_from_add(payload: &MemberAdd) -> AppEvent {
    AppEvent::GuildMemberUpsert {
        guild_id: payload.guild_id,
        member: member_info(&payload.member),
    }
}

fn member_upsert_from_update(update: &MemberUpdate) -> AppEvent {
    AppEvent::GuildMemberUpsert {
        guild_id: update.guild_id,
        member: MemberInfo {
            user_id: update.user.id,
            display_name: display_name(update.nick.as_deref(), &update.user),
            is_bot: update.user.bot,
        },
    }
}

fn presence_update(payload: &PresenceUpdatePayload) -> AppEvent {
    AppEvent::PresenceUpdate {
        guild_id: payload.0.guild_id,
        user_id: match &payload.0.user {
            UserOrId::User(user) => user.id,
            UserOrId::UserId { id } => *id,
        },
        status: map_status(payload.0.status),
    }
}

fn map_status(status: TwilightStatus) -> PresenceStatus {
    match status {
        TwilightStatus::Online => PresenceStatus::Online,
        TwilightStatus::Idle => PresenceStatus::Idle,
        TwilightStatus::DoNotDisturb => PresenceStatus::DoNotDisturb,
        TwilightStatus::Offline | TwilightStatus::Invisible => PresenceStatus::Offline,
    }
}

fn display_name(nick: Option<&str>, user: &TwilightUser) -> String {
    if let Some(nick) = nick.filter(|value| !value.is_empty()) {
        return nick.to_owned();
    }
    if let Some(global) = user
        .global_name
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return global.to_owned();
    }
    user.name.clone()
}

fn map_message_content(content: &str, message_content_enabled: bool) -> Option<String> {
    if message_content_enabled || !content.is_empty() {
        return Some(content.to_owned());
    }

    None
}
