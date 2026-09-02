use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AppError, Result,
    discord::{GuildFolder, PresenceStatus},
};

use super::DiscordRest;

const PRELOADED_SETTINGS_URL: &str = "https://discord.com/api/v9/users/@me/settings-proto/1";
const VERSIONS_FIELD: u32 = 1;
const DATA_VERSION_FIELD: u32 = 3;
const STATUS_FIELD: u32 = 11;
const STATUS_VALUE_FIELD: u32 = 1;
const GUILD_FOLDERS_FIELD: u32 = 14;

#[derive(Debug, Deserialize)]
struct SettingsProtoResponse {
    settings: String,
    #[serde(default)]
    out_of_date: bool,
}

impl DiscordRest {
    pub async fn update_guild_folders(&self, folders: &[GuildFolder]) -> Result<()> {
        let guild_folders = encode_guild_folders(folders);
        self.update_preloaded_settings_field(
            GUILD_FOLDERS_FIELD,
            move |_| Ok(guild_folders.clone()),
            "guild folders settings update",
        )
        .await
    }

    pub(super) async fn update_status_settings(&self, status: PresenceStatus) -> Result<()> {
        self.update_preloaded_settings_field(
            STATUS_FIELD,
            move |current_status| {
                replace_len_field(
                    current_status,
                    STATUS_VALUE_FIELD,
                    &encode_string_wrapper(status.gateway_status()),
                )
            },
            "status settings update",
        )
        .await
    }

    async fn update_preloaded_settings_field(
        &self,
        field: u32,
        update: impl Fn(&[u8]) -> Result<Vec<u8>>,
        label: &str,
    ) -> Result<()> {
        // Serialize local edits so each one starts from the canonical response
        // produced by the previous edit. Discord rejects stale versions, and the
        // returned canonical proto is the source of truth for a retry.
        let mut cached_settings = self.preloaded_settings.lock().await;
        if cached_settings.is_none() {
            let response: SettingsProtoResponse = self
                .send_json(
                    self.raw_http.get(PRELOADED_SETTINGS_URL),
                    "user settings fetch",
                )
                .await?;
            *cached_settings = Some(decode_settings(&response.settings, "user settings fetch")?);
        }

        for _ in 0..2 {
            let current = cached_settings
                .as_deref()
                .expect("preloaded settings are fetched before update");
            let current_field = len_field_value(current, field)?.unwrap_or_default();
            let updated_field = update(current_field)?;
            let mut partial_settings = Vec::new();
            write_len_field(&mut partial_settings, field, &updated_field);
            let required_data_version = preloaded_data_version(current)?;
            let body =
                settings_proto_request_body_with_version(partial_settings, required_data_version);
            let response: SettingsProtoResponse = self
                .send_json(
                    self.raw_http.patch(PRELOADED_SETTINGS_URL).json(&body),
                    label,
                )
                .await?;
            let canonical = decode_settings(&response.settings, label)?;
            *cached_settings = Some(canonical);
            if !response.out_of_date {
                return Ok(());
            }
        }

        Err(AppError::DiscordRequest(format!(
            "{label} was rejected because the settings changed on another client"
        )))
    }
}

#[cfg(test)]
pub(super) fn settings_proto_request_body(folders: &[GuildFolder]) -> Value {
    settings_proto_request_body_with_version(encode_preloaded_guild_folders(folders), None)
}

fn settings_proto_request_body_with_version(
    settings: Vec<u8>,
    required_data_version: Option<u64>,
) -> Value {
    let mut body = json!({
        "settings": BASE64_STANDARD.encode(settings),
    });
    if let Some(version) = required_data_version {
        body["required_data_version"] = Value::from(version);
    }
    body
}

#[cfg(test)]
fn encode_preloaded_guild_folders(folders: &[GuildFolder]) -> Vec<u8> {
    let mut settings = Vec::new();
    write_len_field(
        &mut settings,
        GUILD_FOLDERS_FIELD,
        &encode_guild_folders(folders),
    );
    settings
}

fn encode_guild_folders(folders: &[GuildFolder]) -> Vec<u8> {
    let mut guild_folders = Vec::new();
    for folder in folders {
        write_len_field(&mut guild_folders, 1, &encode_guild_folder(folder));
    }
    guild_folders
}

fn encode_guild_folder(folder: &GuildFolder) -> Vec<u8> {
    let mut bytes = Vec::new();
    if !folder.guild_ids.is_empty() {
        let mut guild_ids = Vec::with_capacity(folder.guild_ids.len() * 8);
        for guild_id in &folder.guild_ids {
            guild_ids.extend_from_slice(&guild_id.get().to_le_bytes());
        }
        write_len_field(&mut bytes, 1, &guild_ids);
    }

    if let Some(id) = folder.id {
        write_len_field(&mut bytes, 2, &encode_varint_wrapper(id));
    }

    if let Some(name) = &folder.name {
        write_len_field(&mut bytes, 3, &encode_string_wrapper(name));
    }

    if let Some(color) = folder.color {
        write_len_field(&mut bytes, 4, &encode_varint_wrapper(u64::from(color)));
    }
    bytes
}

fn encode_varint_wrapper(value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_varint_field(&mut bytes, 1, value);
    bytes
}

fn encode_string_wrapper(value: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_len_field(&mut bytes, 1, value.as_bytes());
    bytes
}

fn write_varint_field(bytes: &mut Vec<u8>, field: u32, value: u64) {
    write_varint(bytes, u64::from(field << 3));
    write_varint(bytes, value);
}

fn write_len_field(bytes: &mut Vec<u8>, field: u32, value: &[u8]) {
    write_varint(bytes, u64::from((field << 3) | 2));
    write_varint(bytes, value.len() as u64);
    bytes.extend_from_slice(value);
}

fn write_varint(bytes: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        bytes.push((value as u8) | 0x80);
        value >>= 7;
    }
    bytes.push(value as u8);
}

fn decode_settings(encoded: &str, label: &str) -> Result<Vec<u8>> {
    BASE64_STANDARD.decode(encoded).map_err(|error| {
        AppError::DiscordRequest(format!("{label} settings decode failed: {error}"))
    })
}

fn preloaded_data_version(settings: &[u8]) -> Result<Option<u64>> {
    let Some(versions) = len_field_value(settings, VERSIONS_FIELD)? else {
        return Ok(None);
    };
    varint_field_value(versions, DATA_VERSION_FIELD)
}

fn replace_len_field(message: &[u8], field: u32, value: &[u8]) -> Result<Vec<u8>> {
    let mut updated = Vec::with_capacity(message.len() + value.len());
    let mut cursor = 0;
    while cursor < message.len() {
        let parsed = parse_field(message, cursor)?;
        if parsed.number != field {
            updated.extend_from_slice(&message[cursor..parsed.end]);
        }
        cursor = parsed.end;
    }
    write_len_field(&mut updated, field, value);
    Ok(updated)
}

fn len_field_value(message: &[u8], field: u32) -> Result<Option<&[u8]>> {
    let mut cursor = 0;
    let mut value = None;
    while cursor < message.len() {
        let parsed = parse_field(message, cursor)?;
        if parsed.number == field && parsed.wire_type == 2 {
            value = Some(&message[parsed.value_start..parsed.value_end]);
        }
        cursor = parsed.end;
    }
    Ok(value)
}

fn varint_field_value(message: &[u8], field: u32) -> Result<Option<u64>> {
    let mut cursor = 0;
    let mut value = None;
    while cursor < message.len() {
        let parsed = parse_field(message, cursor)?;
        if parsed.number == field && parsed.wire_type == 0 {
            let mut value_cursor = parsed.value_start;
            value = Some(read_varint(message, &mut value_cursor)?);
        }
        cursor = parsed.end;
    }
    Ok(value)
}

#[derive(Clone, Copy, Debug)]
struct ParsedField {
    number: u32,
    wire_type: u8,
    value_start: usize,
    value_end: usize,
    end: usize,
}

fn parse_field(message: &[u8], start: usize) -> Result<ParsedField> {
    let mut cursor = start;
    let key = read_varint(message, &mut cursor)?;
    let number =
        u32::try_from(key >> 3).map_err(|_| invalid_proto("field number does not fit in u32"))?;
    let wire_type = (key & 0x07) as u8;
    let value_start;
    let value_end = match wire_type {
        0 => {
            value_start = cursor;
            read_varint(message, &mut cursor)?;
            cursor
        }
        1 => {
            value_start = cursor;
            cursor = cursor
                .checked_add(8)
                .filter(|end| *end <= message.len())
                .ok_or_else(|| invalid_proto("fixed64 field is truncated"))?;
            cursor
        }
        2 => {
            let length = usize::try_from(read_varint(message, &mut cursor)?)
                .map_err(|_| invalid_proto("length does not fit in usize"))?;
            value_start = cursor;
            cursor = cursor
                .checked_add(length)
                .filter(|end| *end <= message.len())
                .ok_or_else(|| invalid_proto("length-delimited field is truncated"))?;
            cursor
        }
        5 => {
            value_start = cursor;
            cursor = cursor
                .checked_add(4)
                .filter(|end| *end <= message.len())
                .ok_or_else(|| invalid_proto("fixed32 field is truncated"))?;
            cursor
        }
        _ => return Err(invalid_proto("unsupported protobuf wire type")),
    };

    Ok(ParsedField {
        number,
        wire_type,
        value_start,
        value_end,
        end: cursor,
    })
}

fn read_varint(message: &[u8], cursor: &mut usize) -> Result<u64> {
    let mut value = 0_u64;
    for shift in (0..=63).step_by(7) {
        let byte = *message
            .get(*cursor)
            .ok_or_else(|| invalid_proto("varint is truncated"))?;
        *cursor += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(invalid_proto("varint is too long"))
}

fn invalid_proto(reason: &str) -> AppError {
    AppError::DiscordRequest(format!("user settings protobuf is invalid: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_update_preserves_sibling_settings_and_uses_data_version() {
        let mut versions = Vec::new();
        write_varint_field(&mut versions, DATA_VERSION_FIELD, 42);

        let mut status = Vec::new();
        write_len_field(
            &mut status,
            STATUS_VALUE_FIELD,
            &encode_string_wrapper("online"),
        );
        write_len_field(&mut status, 2, b"custom-status-sibling");

        let mut settings = Vec::new();
        write_len_field(&mut settings, VERSIONS_FIELD, &versions);
        write_len_field(&mut settings, STATUS_FIELD, &status);
        write_len_field(&mut settings, 99, b"unrelated-top-level-setting");

        let current_status = len_field_value(&settings, STATUS_FIELD)
            .expect("settings should parse")
            .expect("status field should exist");
        let updated_status = replace_len_field(
            current_status,
            STATUS_VALUE_FIELD,
            &encode_string_wrapper("idle"),
        )
        .expect("status should update");
        let body = settings_proto_request_body_with_version(
            {
                let mut partial = Vec::new();
                write_len_field(&mut partial, STATUS_FIELD, &updated_status);
                partial
            },
            preloaded_data_version(&settings).expect("version should parse"),
        );

        assert_eq!(body["required_data_version"].as_u64(), Some(42));
        assert_eq!(
            len_field_value(&updated_status, 2).expect("status should parse"),
            Some(b"custom-status-sibling".as_slice())
        );
        let encoded = body["settings"]
            .as_str()
            .expect("request should contain settings");
        let partial = BASE64_STANDARD
            .decode(encoded)
            .expect("request settings should decode");
        assert_eq!(
            len_field_value(&partial, STATUS_FIELD)
                .expect("partial settings should parse")
                .expect("partial settings should contain status"),
            updated_status
        );
    }
}
