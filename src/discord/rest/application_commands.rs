use serde_json::{Value, json};

use crate::discord::ids::{Id, marker::GuildMarker};
use crate::{
    Result,
    discord::{
        ApplicationCommandChoiceInfo, ApplicationCommandInfo, ApplicationCommandInteraction,
        ApplicationCommandInteractionOption, ApplicationCommandOptionInfo,
    },
};

use super::{DiscordRest, clone_array, extra_fields};

impl DiscordRest {
    pub async fn load_application_commands(
        &self,
        guild_id: Option<Id<GuildMarker>>,
    ) -> Result<Vec<ApplicationCommandInfo>> {
        let endpoint = match guild_id {
            Some(guild_id) => format!(
                "https://discord.com/api/v9/guilds/{}/application-command-index",
                guild_id.get()
            ),
            None => "https://discord.com/api/v9/users/@me/application-command-index".to_owned(),
        };
        let raw: Value = self
            .send_json(self.raw_http.get(endpoint), "application command index")
            .await?;
        Ok(parse_application_command_index(&raw))
    }

    pub async fn run_application_command(
        &self,
        interaction: &ApplicationCommandInteraction,
        session_id: &str,
    ) -> Result<()> {
        let body = application_command_interaction_body(interaction, session_id);
        self.send_unit(
            self.raw_http
                .post("https://discord.com/api/v9/interactions")
                .json(&body),
            "application command",
        )
        .await
    }
}

pub(super) fn parse_application_command_index(raw: &Value) -> Vec<ApplicationCommandInfo> {
    parse_application_command_index_response(raw).commands
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ApplicationCommandIndexResponse {
    pub(super) commands: Vec<ApplicationCommandInfo>,
    pub(super) applications: Vec<Value>,
    pub(super) extra_fields: std::collections::BTreeMap<String, Value>,
}

pub(super) fn parse_application_command_index_response(
    raw: &Value,
) -> ApplicationCommandIndexResponse {
    let applications = parse_application_command_applications(raw);
    let commands = raw
        .get("application_commands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|command| parse_application_command_info(command, &applications))
        .collect();
    ApplicationCommandIndexResponse {
        commands,
        applications: clone_array(raw.get("applications")),
        extra_fields: extra_fields(raw, &["applications", "application_commands"]),
    }
}

fn parse_application_command_applications(
    raw: &Value,
) -> std::collections::HashMap<String, &Value> {
    raw.get("applications")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|application| Some((application.get("id")?.as_str()?.to_owned(), application)))
        .collect()
}

fn parse_application_command_info(
    raw: &Value,
    applications: &std::collections::HashMap<String, &Value>,
) -> Option<ApplicationCommandInfo> {
    let id = raw
        .get("id")?
        .as_str()?
        .parse::<u64>()
        .ok()
        .and_then(Id::new_checked)?;
    let application_id_raw = raw.get("application_id")?.as_str()?;
    let application_id = application_id_raw
        .parse::<u64>()
        .ok()
        .and_then(Id::new_checked)?;
    let name = raw.get("name")?.as_str()?.to_owned();
    Some(ApplicationCommandInfo {
        id,
        application_id,
        version: raw.get("version")?.as_str()?.to_owned(),
        name,
        application_name: parse_application_command_application_name(
            raw,
            applications.get(application_id_raw).copied(),
        ),
        description: raw
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        options: raw
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(parse_application_command_option_info)
            .collect(),
        raw: raw.clone(),
    })
}

fn parse_application_command_application_name(
    raw: &Value,
    application: Option<&Value>,
) -> Option<String> {
    [
        raw.get("application").and_then(|value| value.get("name")),
        application.and_then(|value| value.get("name")),
        raw.get("bot").and_then(|value| value.get("global_name")),
        raw.get("bot").and_then(|value| value.get("username")),
        application
            .and_then(|value| value.get("bot"))
            .and_then(|value| value.get("global_name")),
        application
            .and_then(|value| value.get("bot"))
            .and_then(|value| value.get("username")),
        raw.get("user").and_then(|value| value.get("global_name")),
        raw.get("user").and_then(|value| value.get("username")),
        raw.get("display_name"),
        raw.get("application_name"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .find(|value| !value.trim().is_empty())
    .map(str::to_owned)
}

fn parse_application_command_option_info(raw: &Value) -> Option<ApplicationCommandOptionInfo> {
    Some(ApplicationCommandOptionInfo {
        kind: raw.get("type")?.as_u64()?,
        name: raw.get("name")?.as_str()?.to_owned(),
        description: raw
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        required: raw
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        autocomplete: raw
            .get("autocomplete")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        choices: raw
            .get("choices")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|choice| {
                Some(ApplicationCommandChoiceInfo {
                    name: choice.get("name")?.as_str()?.to_owned(),
                    value: choice.get("value")?.clone(),
                })
            })
            .collect(),
        options: raw
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(parse_application_command_option_info)
            .collect(),
    })
}

pub(super) fn application_command_interaction_body(
    interaction: &ApplicationCommandInteraction,
    session_id: &str,
) -> Value {
    let mut body = json!({
        "type": 2,
        "application_id": interaction.command.application_id.to_string(),
        "guild_id": interaction.guild_id.map(|guild_id| guild_id.to_string()),
        "channel_id": interaction.channel_id.to_string(),
        "session_id": session_id,
        "data": {
            "version": interaction.command.version,
            "id": interaction.command.id.to_string(),
            "name": interaction.command.name,
            "type": 1,
            "options": interaction.options.iter().map(application_command_option_body).collect::<Vec<_>>(),
            "application_command": interaction.command.raw,
            "attachments": [],
        },
        "nonce": interaction_nonce(),
        "analytics_location": "slash_ui",
    });
    if let Some(command_guild_id) = interaction
        .command
        .raw
        .get("guild_id")
        .and_then(Value::as_str)
    {
        body["data"]["guild_id"] = Value::String(command_guild_id.to_owned());
    }
    body
}

pub(super) fn application_command_option_body(
    option: &ApplicationCommandInteractionOption,
) -> Value {
    let mut body = json!({
        "type": option.kind,
        "name": option.name,
    });
    if let Some(value) = &option.value {
        body["value"] = value.clone();
    } else if !option.options.is_empty() {
        body["options"] = Value::Array(
            option
                .options
                .iter()
                .map(application_command_option_body)
                .collect(),
        );
    }
    body
}

fn interaction_nonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    (millis << 22).to_string()
}
