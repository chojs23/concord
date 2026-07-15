use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::discord::ids::{
    Id,
    marker::{GuildMarker, RoleMarker, UserMarker},
};
use crate::{
    AppError, Result,
    discord::{
        FriendStatus, GlobalUserProfileUpdate, GuildUserProfileUpdate, MutualGuildInfo,
        UserProfileInfo, UserProfileUpdate, avatar::member_avatar_url, read_profile_avatar_image,
    },
};

use super::{DiscordRest, clone_array, extra_fields};

impl DiscordRest {
    pub async fn load_user_profile(
        &self,
        user_id: Id<UserMarker>,
        guild_id: Option<Id<crate::discord::ids::marker::GuildMarker>>,
        is_self: bool,
    ) -> Result<UserProfileInfo> {
        let mut url = format!(
            "https://discord.com/api/v9/users/{}/profile?",
            user_id.get(),
        );
        if !is_self {
            url.push_str("with_mutual_guilds=true&with_mutual_friends_count=true&");
        }
        if let Some(guild_id) = guild_id {
            url.push_str(&format!("guild_id={}", guild_id.get()));
        }
        let body: Value = self
            .send_json(self.raw_http.get(url), "user profile")
            .await?;

        Ok(parse_user_profile_response(user_id, guild_id, &body, None))
    }

    /// Returns the user's saved note, or `None` if Discord responds 404
    /// (no note set). Other errors propagate.
    pub(in crate::discord) async fn load_user_note(
        &self,
        user_id: Id<UserMarker>,
    ) -> Result<Option<String>> {
        let url = format!(
            "https://discord.com/api/v9/users/@me/notes/{}",
            user_id.get()
        );
        let response = self
            .execute_authenticated(self.raw_http.get(url), "user note")
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if let Err(error) = response.error_for_status_ref() {
            return Err(super::request_error(error, response, "user note").await);
        }
        let body: Value = response.json().await.map_err(|error| {
            AppError::DiscordRequest(format!("user note decode failed: {error}"))
        })?;
        Ok(UserNoteResponse::parse(body).note)
    }

    pub async fn update_user_profile(&self, update: &UserProfileUpdate) -> Result<()> {
        if !update.global.is_empty() {
            self.update_global_user_profile(&update.global).await?;
        }
        if let Some(guild) = update.guild.as_ref().filter(|guild| !guild.is_empty()) {
            self.update_guild_user_profile(guild).await?;
        }
        Ok(())
    }

    async fn update_global_user_profile(&self, update: &GlobalUserProfileUpdate) -> Result<()> {
        let mut user_body = serde_json::Map::new();
        if let Some(display_name) = &update.display_name {
            user_body.insert("global_name".to_owned(), nullable_text_value(display_name));
        }
        if let Some(avatar) = &update.avatar {
            user_body.insert(
                "avatar".to_owned(),
                Value::String(profile_avatar_data_uri(avatar).await?),
            );
        }
        if !user_body.is_empty() {
            self.send_unit(
                self.raw_http
                    .patch("https://discord.com/api/v9/users/@me")
                    .json(&Value::Object(user_body)),
                "global profile update",
            )
            .await?;
        }
        if let Some(pronouns) = &update.pronouns {
            self.send_unit(
                self.raw_http
                    .patch("https://discord.com/api/v9/users/@me/profile")
                    .json(&json!({ "pronouns": pronouns })),
                "profile pronouns update",
            )
            .await?;
        }
        Ok(())
    }

    async fn update_guild_user_profile(&self, update: &GuildUserProfileUpdate) -> Result<()> {
        if let Some(nickname) = &update.nickname {
            self.send_unit(
                self.raw_http
                    .patch(format!(
                        "https://discord.com/api/v9/guilds/{}/members/@me",
                        update.guild_id.get()
                    ))
                    .json(&json!({ "nick": nullable_text_value(nickname) })),
                "guild nickname update",
            )
            .await?;
        }
        if let Some(pronouns) = &update.pronouns {
            self.send_unit(
                self.raw_http
                    .patch(format!(
                        "https://discord.com/api/v9/guilds/{}/profile/@me",
                        update.guild_id.get()
                    ))
                    .json(&json!({ "pronouns": pronouns })),
                "guild profile update",
            )
            .await?;
        }
        Ok(())
    }
}

fn nullable_text_value(value: &str) -> Value {
    if value.trim().is_empty() {
        Value::Null
    } else {
        Value::String(value.to_owned())
    }
}

async fn profile_avatar_data_uri(avatar: &crate::discord::ProfileAvatarUpload) -> Result<String> {
    let image = read_profile_avatar_image(avatar)
        .await
        .map_err(AppError::DiscordRequest)?;
    Ok(format!(
        "data:{};base64,{}",
        image.content_type,
        BASE64_STANDARD.encode(image.bytes)
    ))
}

/// Builds the dashboard's `UserProfileInfo` from Discord's
/// `/users/{id}/profile` JSON. Friend status is left as `None` here because the
/// caller fills it in from cached relationship data.
pub(super) fn parse_user_profile_response(
    user_id: Id<UserMarker>,
    guild_id: Option<Id<GuildMarker>>,
    body: &Value,
    note: Option<String>,
) -> UserProfileInfo {
    UserProfileResponse::parse(user_id, guild_id, body, note).profile
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct UserProfileResponse {
    pub(super) profile: UserProfileInfo,
    pub(super) user: Option<Value>,
    pub(super) user_profile: Option<Value>,
    pub(super) guild_member: Option<Value>,
    pub(super) guild_member_profile: Option<Value>,
    pub(super) mutual_guilds: Vec<Value>,
    pub(super) extra_fields: std::collections::BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct UserNoteResponse {
    pub(super) note: Option<String>,
    pub(super) raw: Value,
}

impl UserNoteResponse {
    fn parse(raw: Value) -> Self {
        let note = raw
            .get("note")
            .and_then(Value::as_str)
            .filter(|note| !note.is_empty())
            .map(str::to_owned);
        Self { note, raw }
    }
}

impl UserProfileResponse {
    pub(super) fn parse(
        user_id: Id<UserMarker>,
        guild_id: Option<Id<GuildMarker>>,
        body: &Value,
        note: Option<String>,
    ) -> Self {
        let user = body.get("user");
        let username = user
            .and_then(|user| user.get("username"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let global_name = user
            .and_then(|user| user.get("global_name"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let avatar_url = member_avatar_url(guild_id, user_id, body.get("guild_member"), user);
        let user_profile = body.get("user_profile");
        let bio = user_profile
            .and_then(|profile| profile.get("bio"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let pronouns = user_profile
            .and_then(|profile| profile.get("pronouns"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let guild_pronouns = body
            .get("guild_member_profile")
            .and_then(|profile| profile.get("pronouns"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let mutual_guilds = body
            .get("mutual_guilds")
            .and_then(Value::as_array)
            .map(|array| {
                array
                    .iter()
                    .filter_map(|entry| {
                        let guild_id = entry
                            .get("id")
                            .and_then(Value::as_str)
                            .and_then(|raw| raw.parse::<u64>().ok())
                            .and_then(Id::<GuildMarker>::new_checked)?;
                        let nick = entry
                            .get("nick")
                            .and_then(Value::as_str)
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned);
                        Some(MutualGuildInfo { guild_id, nick })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mutual_friends_count = body
            .get("mutual_friends_count")
            .and_then(Value::as_u64)
            .map(|value| u32::try_from(value).unwrap_or(u32::MAX))
            .unwrap_or(0);
        let guild_nick = body
            .get("guild_member")
            .and_then(|member| member.get("nick"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let role_ids = body
            .get("guild_member")
            .and_then(|member| member.get("roles"))
            .and_then(Value::as_array)
            .map(|roles| roles.iter().filter_map(parse_profile_role_id).collect())
            .unwrap_or_default();

        Self {
            profile: UserProfileInfo {
                user_id,
                username,
                global_name,
                guild_nick,
                role_ids,
                avatar_url,
                bio,
                pronouns,
                guild_pronouns,
                mutual_guilds,
                mutual_friends_count,
                friend_status: FriendStatus::None,
                note,
            },
            user: body.get("user").cloned(),
            user_profile: body.get("user_profile").cloned(),
            guild_member: body.get("guild_member").cloned(),
            guild_member_profile: body.get("guild_member_profile").cloned(),
            mutual_guilds: clone_array(body.get("mutual_guilds")),
            extra_fields: extra_fields(
                body,
                &[
                    "user",
                    "user_profile",
                    "guild_member",
                    "guild_member_profile",
                    "mutual_guilds",
                    "mutual_friends_count",
                ],
            ),
        }
    }
}

fn parse_profile_role_id(value: &Value) -> Option<Id<RoleMarker>> {
    value
        .as_str()
        .and_then(|raw| raw.parse::<u64>().ok())
        .or_else(|| value.as_u64())
        .and_then(Id::new_checked)
}
