use serde_json::Value;

use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, MessageMarker, UserMarker},
};
use crate::{
    Result,
    discord::{ReactionEmoji, ReactionUserInfo},
};

use super::DiscordRest;

const REACTION_USERS_PAGE_LIMIT: u16 = 100;

/// `next_after` is `Some` only when the page came back full, meaning more users
/// may exist.
#[derive(Clone, Debug, PartialEq)]
pub struct ReactionUsersPage {
    pub users: Vec<ReactionUserInfo>,
    pub next_after: Option<Id<UserMarker>>,
}

impl DiscordRest {
    pub async fn add_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http.put(format!(
                "https://discord.com/api/v9/channels/{}/messages/{}/reactions/{}/@me",
                channel_id.get(),
                message_id.get(),
                reaction_route_component(emoji)
            )),
            "add reaction",
        )
        .await
    }

    pub async fn remove_current_user_reaction(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http.delete(format!(
                "https://discord.com/api/v9/channels/{}/messages/{}/reactions/{}/@me",
                channel_id.get(),
                message_id.get(),
                reaction_route_component(emoji)
            )),
            "remove reaction",
        )
        .await
    }

    pub async fn load_reaction_users_page(
        &self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        emoji: &ReactionEmoji,
        after: Option<Id<UserMarker>>,
    ) -> Result<ReactionUsersPage> {
        let mut request = self
            .raw_http
            .get(format!(
                "https://discord.com/api/v9/channels/{}/messages/{}/reactions/{}",
                channel_id.get(),
                message_id.get(),
                reaction_route_component(emoji)
            ))
            .query(&[
                ("limit", REACTION_USERS_PAGE_LIMIT.to_string()),
                ("type", "0".to_owned()),
            ]);
        if let Some(user_id) = after {
            request = request.query(&[("after", user_id.to_string())]);
        }

        let raw_users: Vec<Value> = self.send_json(request, "reaction users").await?;
        let next_after = next_reaction_users_after(&raw_users);
        let response = parse_reaction_users_response(raw_users);
        Ok(ReactionUsersPage {
            users: response.users,
            next_after,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ReactionUsersResponse {
    pub(super) users: Vec<ReactionUserInfo>,
    pub(super) raw_users: Vec<Value>,
}

fn parse_reaction_users_response(raw_users: Vec<Value>) -> ReactionUsersResponse {
    let users = raw_users
        .iter()
        .filter_map(reaction_user_info_from_raw)
        .collect();
    ReactionUsersResponse { users, raw_users }
}

fn reaction_user_info_from_raw(value: &Value) -> Option<ReactionUserInfo> {
    let user_id = value
        .get("id")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<u64>().ok())
        .and_then(Id::<UserMarker>::new_checked)?;
    let display_name = value
        .get("global_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| value.get("username").and_then(Value::as_str))?
        .to_owned();

    Some(ReactionUserInfo {
        user_id,
        display_name,
    })
}

pub(super) fn reaction_route_component(emoji: &ReactionEmoji) -> String {
    emoji.route_component()
}

/// Read from the last raw entry rather than the parsed users, so a user we could
/// not fully parse (e.g. missing display name) still advances the cursor instead
/// of stalling pagination.
pub(super) fn next_reaction_users_after(raw_users: &[Value]) -> Option<Id<UserMarker>> {
    if raw_users.len() != usize::from(REACTION_USERS_PAGE_LIMIT) {
        return None;
    }
    raw_users
        .last()
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<u64>().ok())
        .and_then(Id::<UserMarker>::new_checked)
}
