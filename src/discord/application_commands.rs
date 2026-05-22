use serde_json::{Number, Value};

use crate::discord::ids::{
    Id,
    marker::{ApplicationMarker, ChannelMarker, GuildMarker},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationCommandInfo {
    pub id: Id<ApplicationMarker>,
    pub application_id: Id<ApplicationMarker>,
    pub version: String,
    pub name: String,
    pub application_name: Option<String>,
    pub description: String,
    pub options: Vec<ApplicationCommandOptionInfo>,
    pub raw: Value,
}

impl ApplicationCommandInfo {
    pub fn without_raw(mut self) -> Self {
        self.raw = Value::Null;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationCommandOptionInfo {
    pub kind: u64,
    pub name: String,
    pub description: String,
    pub required: bool,
    pub autocomplete: bool,
    pub choices: Vec<ApplicationCommandChoiceInfo>,
    pub options: Vec<ApplicationCommandOptionInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationCommandChoiceInfo {
    pub name: String,
    pub value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationCommandInteraction {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub command: ApplicationCommandInfo,
    pub options: Vec<ApplicationCommandInteractionOption>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationCommandInvocation {
    pub guild_id: Option<Id<GuildMarker>>,
    pub channel_id: Id<ChannelMarker>,
    pub command_name: String,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationCommandInteractionOption {
    pub kind: u64,
    pub name: String,
    pub value: Option<Value>,
    pub options: Vec<ApplicationCommandInteractionOption>,
}

pub const APPLICATION_COMMAND_SUBCOMMAND_KIND: u64 = 1;
pub const APPLICATION_COMMAND_SUBCOMMAND_GROUP_KIND: u64 = 2;
const APPLICATION_COMMAND_INTEGER_KIND: u64 = 4;
const APPLICATION_COMMAND_BOOLEAN_KIND: u64 = 5;
const APPLICATION_COMMAND_USER_KIND: u64 = 6;
const APPLICATION_COMMAND_CHANNEL_KIND: u64 = 7;
const APPLICATION_COMMAND_ROLE_KIND: u64 = 8;
const APPLICATION_COMMAND_MENTIONABLE_KIND: u64 = 9;
const APPLICATION_COMMAND_NUMBER_KIND: u64 = 10;
const APPLICATION_COMMAND_ATTACHMENT_KIND: u64 = 11;

pub fn application_command_interaction_from_invocation(
    invocation: &ApplicationCommandInvocation,
    command: &ApplicationCommandInfo,
) -> Option<ApplicationCommandInteraction> {
    (invocation.command_name == command.name).then_some(())?;
    Some(ApplicationCommandInteraction {
        guild_id: invocation.guild_id,
        channel_id: invocation.channel_id,
        command: command.clone(),
        options: parsed_application_command_options(&invocation.content, command)?,
    })
}

pub fn application_command_content_is_complete(
    content: &str,
    command: &ApplicationCommandInfo,
) -> bool {
    parsed_application_command_options(content, command).is_some()
}

pub fn parsed_application_command_option_names(
    content: &str,
    command: &ApplicationCommandInfo,
    options: &[ApplicationCommandOptionInfo],
) -> std::collections::HashSet<String> {
    let Some(rest) = content.strip_prefix('/') else {
        return std::collections::HashSet::new();
    };
    let mut parts = rest.split_whitespace();
    if parts.next() != Some(command.name.as_str()) {
        return std::collections::HashSet::new();
    }
    let all_parts = parts.collect::<Vec<_>>();
    let leaf_parts = leaf_application_command_parts(&all_parts, command, options);
    parse_leaf_application_command_options(leaf_parts, options)
        .unwrap_or_default()
        .into_iter()
        .map(|option| option.name)
        .collect()
}

pub fn application_command_option_scope<'a>(
    command: &'a ApplicationCommandInfo,
    before_cursor: &str,
) -> Option<&'a [ApplicationCommandOptionInfo]> {
    let mut parts = before_cursor.strip_prefix('/')?.split_whitespace();
    if parts.next() != Some(command.name.as_str()) {
        return None;
    }
    let parts = parts.collect::<Vec<_>>();
    let Some(first) = parts.first().copied().filter(|part| !part.contains(':')) else {
        return Some(&command.options);
    };

    if let Some(group) = command.options.iter().find(|option| {
        option.kind == APPLICATION_COMMAND_SUBCOMMAND_GROUP_KIND && option.name == first
    }) {
        let Some(second) = parts.get(1).copied().filter(|part| !part.contains(':')) else {
            return Some(&group.options);
        };
        if let Some(subcommand) = group.options.iter().find(|option| {
            option.kind == APPLICATION_COMMAND_SUBCOMMAND_KIND && option.name == second
        }) {
            return Some(&subcommand.options);
        }
        return Some(&group.options);
    }

    if let Some(subcommand) = command
        .options
        .iter()
        .find(|option| option.kind == APPLICATION_COMMAND_SUBCOMMAND_KIND && option.name == first)
    {
        return Some(&subcommand.options);
    }

    Some(&command.options)
}

fn parsed_application_command_options(
    content: &str,
    command: &ApplicationCommandInfo,
) -> Option<Vec<ApplicationCommandInteractionOption>> {
    let rest = content.strip_prefix('/')?;
    let mut parts = rest.split_whitespace();
    if parts.next() != Some(command.name.as_str()) {
        return None;
    }

    let parts = parts.collect::<Vec<_>>();
    parsed_application_command_options_from_parts(&parts, command)
}

fn parsed_application_command_options_from_parts(
    parts: &[&str],
    command: &ApplicationCommandInfo,
) -> Option<Vec<ApplicationCommandInteractionOption>> {
    let has_structural_options = command.options.iter().any(is_structural_command_option);

    if let Some(first) = parts.first().copied()
        && let Some((subcommand_name, raw_value)) = first.split_once(':')
        && let Some(subcommand) = command.options.iter().find(|option| {
            option.kind == APPLICATION_COMMAND_SUBCOMMAND_KIND && option.name == subcommand_name
        })
    {
        let options = parse_single_leaf_application_command_option(
            &subcommand.options,
            raw_value,
            &parts[1..],
        )?;
        return Some(vec![structural_interaction_option(subcommand, options)]);
    }

    if let Some(first) = parts.first().copied().filter(|part| !part.contains(':')) {
        if let Some(group) = command.options.iter().find(|option| {
            option.kind == APPLICATION_COMMAND_SUBCOMMAND_GROUP_KIND && option.name == first
        }) {
            let subcommand_name = parts.get(1).copied().filter(|part| !part.contains(':'))?;
            let subcommand = group.options.iter().find(|option| {
                option.kind == APPLICATION_COMMAND_SUBCOMMAND_KIND && option.name == subcommand_name
            })?;
            let options = parse_leaf_application_command_options(&parts[2..], &subcommand.options)?;
            return Some(vec![structural_interaction_option(
                group,
                vec![structural_interaction_option(subcommand, options)],
            )]);
        }

        if let Some(subcommand) = command.options.iter().find(|option| {
            option.kind == APPLICATION_COMMAND_SUBCOMMAND_KIND && option.name == first
        }) {
            let options = parse_leaf_application_command_options(&parts[1..], &subcommand.options)?;
            return Some(vec![structural_interaction_option(subcommand, options)]);
        }
    }

    if has_structural_options {
        return None;
    }

    parse_leaf_application_command_options(parts, &command.options)
}

fn structural_interaction_option(
    option: &ApplicationCommandOptionInfo,
    options: Vec<ApplicationCommandInteractionOption>,
) -> ApplicationCommandInteractionOption {
    ApplicationCommandInteractionOption {
        kind: option.kind,
        name: option.name.clone(),
        value: None,
        options,
    }
}

fn parse_leaf_application_command_options(
    parts: &[&str],
    options: &[ApplicationCommandOptionInfo],
) -> Option<Vec<ApplicationCommandInteractionOption>> {
    let mut parsed = Vec::new();
    let mut current: Option<(&ApplicationCommandOptionInfo, String)> = None;

    for part in parts {
        if let Some((name, raw_value)) = part.split_once(':')
            && let Some(option) = options
                .iter()
                .find(|option| !is_structural_command_option(option) && option.name == name)
        {
            push_leaf_application_command_option(&mut parsed, current.take())?;
            current = Some((option, raw_value.to_owned()));
            continue;
        }

        if let Some((_, value)) = current.as_mut() {
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(part);
        } else if options
            .iter()
            .any(|option| !is_structural_command_option(option))
        {
            return None;
        }
    }

    push_leaf_application_command_option(&mut parsed, current.take())?;
    required_application_command_options_present(options, &parsed).then_some(parsed)
}

fn parse_single_leaf_application_command_option(
    options: &[ApplicationCommandOptionInfo],
    raw_value: &str,
    trailing_parts: &[&str],
) -> Option<Vec<ApplicationCommandInteractionOption>> {
    let leaf_options = options
        .iter()
        .filter(|option| !is_structural_command_option(option))
        .collect::<Vec<_>>();
    let required_options = leaf_options
        .iter()
        .copied()
        .filter(|option| option.required)
        .collect::<Vec<_>>();
    let option = if required_options.len() == 1 {
        required_options[0]
    } else if leaf_options.len() == 1 {
        leaf_options[0]
    } else {
        return None;
    };

    let mut value = raw_value.to_owned();
    for part in trailing_parts {
        if !value.is_empty() {
            value.push(' ');
        }
        value.push_str(part);
    }
    let mut parsed = Vec::new();
    push_leaf_application_command_option(&mut parsed, Some((option, value)))?;
    required_application_command_options_present(options, &parsed).then_some(parsed)
}

fn push_leaf_application_command_option(
    parsed: &mut Vec<ApplicationCommandInteractionOption>,
    current: Option<(&ApplicationCommandOptionInfo, String)>,
) -> Option<()> {
    let Some((option, value)) = current else {
        return Some(());
    };
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let value = application_command_option_value(option, value)?;
    parsed.push(ApplicationCommandInteractionOption {
        kind: option.kind,
        name: option.name.clone(),
        value: Some(value),
        options: Vec::new(),
    });
    Some(())
}

fn required_application_command_options_present(
    options: &[ApplicationCommandOptionInfo],
    parsed: &[ApplicationCommandInteractionOption],
) -> bool {
    options
        .iter()
        .filter(|option| option.required && !is_structural_command_option(option))
        .all(|option| parsed.iter().any(|parsed| parsed.name == option.name))
}

fn leaf_application_command_parts<'a>(
    parts: &'a [&'a str],
    command: &ApplicationCommandInfo,
    options: &[ApplicationCommandOptionInfo],
) -> &'a [&'a str] {
    if std::ptr::eq(options, command.options.as_slice()) {
        return parts;
    }
    let Some(first) = parts.first().copied().filter(|part| !part.contains(':')) else {
        return parts;
    };
    if let Some(group) = command.options.iter().find(|option| {
        option.kind == APPLICATION_COMMAND_SUBCOMMAND_GROUP_KIND && option.name == first
    }) {
        if std::ptr::eq(options, group.options.as_slice()) {
            return &parts[1..];
        }
        let Some(second) = parts.get(1).copied().filter(|part| !part.contains(':')) else {
            return parts;
        };
        if group.options.iter().any(|option| {
            option.kind == APPLICATION_COMMAND_SUBCOMMAND_KIND
                && option.name == second
                && std::ptr::eq(options, option.options.as_slice())
        }) {
            return &parts[2..];
        }
        return parts;
    }
    if command.options.iter().any(|option| {
        option.kind == APPLICATION_COMMAND_SUBCOMMAND_KIND
            && option.name == first
            && std::ptr::eq(options, option.options.as_slice())
    }) {
        return &parts[1..];
    }
    parts
}

fn is_structural_command_option(option: &ApplicationCommandOptionInfo) -> bool {
    matches!(
        option.kind,
        APPLICATION_COMMAND_SUBCOMMAND_KIND | APPLICATION_COMMAND_SUBCOMMAND_GROUP_KIND
    )
}

fn application_command_option_value(
    option: &ApplicationCommandOptionInfo,
    raw: &str,
) -> Option<Value> {
    match option.kind {
        APPLICATION_COMMAND_INTEGER_KIND => {
            raw.parse::<i64>().map(Number::from).map(Value::Number).ok()
        }
        APPLICATION_COMMAND_BOOLEAN_KIND => match raw {
            "true" | "yes" | "1" | "on" => Some(Value::Bool(true)),
            "false" | "no" | "0" | "off" => Some(Value::Bool(false)),
            _ => None,
        },
        APPLICATION_COMMAND_NUMBER_KIND => raw
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number),
        APPLICATION_COMMAND_USER_KIND
        | APPLICATION_COMMAND_CHANNEL_KIND
        | APPLICATION_COMMAND_ROLE_KIND
        | APPLICATION_COMMAND_MENTIONABLE_KIND => Some(Value::String(snowflake_option_value(raw))),
        APPLICATION_COMMAND_ATTACHMENT_KIND => None,
        _ => Some(Value::String(raw.to_owned())),
    }
}

fn snowflake_option_value(raw: &str) -> String {
    raw.trim_matches(|value| matches!(value, '<' | '>' | '@' | '!' | '#' | '&'))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::discord::ids::Id;

    use super::{
        ApplicationCommandInfo, ApplicationCommandInteractionOption, ApplicationCommandInvocation,
        ApplicationCommandOptionInfo, application_command_interaction_from_invocation,
    };

    fn application_command(
        name: &str,
        options: Vec<ApplicationCommandOptionInfo>,
    ) -> ApplicationCommandInfo {
        ApplicationCommandInfo {
            id: Id::new(100),
            application_id: Id::new(200),
            version: "1".to_owned(),
            name: name.to_owned(),
            application_name: Some("TestBot".to_owned()),
            description: format!("{name} command"),
            options,
            raw: json!({
                "id": "100",
                "application_id": "200",
                "version": "1",
                "name": name,
            }),
        }
    }

    fn application_command_option(
        kind: u64,
        name: &str,
        required: bool,
        options: Vec<ApplicationCommandOptionInfo>,
    ) -> ApplicationCommandOptionInfo {
        ApplicationCommandOptionInfo {
            kind,
            name: name.to_owned(),
            description: format!("{name} option"),
            required,
            autocomplete: false,
            choices: Vec::new(),
            options,
        }
    }

    fn invocation(command_name: &str, content: &str) -> ApplicationCommandInvocation {
        ApplicationCommandInvocation {
            guild_id: Some(Id::new(1)),
            channel_id: Id::new(2),
            command_name: command_name.to_owned(),
            content: content.to_owned(),
        }
    }

    #[test]
    fn invocation_builds_direct_interaction_options() {
        let command = application_command(
            "echo",
            vec![
                application_command_option(3, "text", true, Vec::new()),
                application_command_option(5, "loud", false, Vec::new()),
            ],
        );

        let interaction = application_command_interaction_from_invocation(
            &invocation("echo", "/echo text:hello world loud:true"),
            &command,
        )
        .expect("valid invocation should build interaction");

        assert_eq!(
            interaction.options,
            vec![
                ApplicationCommandInteractionOption {
                    kind: 3,
                    name: "text".to_owned(),
                    value: Some(Value::String("hello world".to_owned())),
                    options: Vec::new(),
                },
                ApplicationCommandInteractionOption {
                    kind: 5,
                    name: "loud".to_owned(),
                    value: Some(Value::Bool(true)),
                    options: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn invocation_builds_nested_subcommand_group_options() {
        let command = application_command(
            "mod",
            vec![application_command_option(
                2,
                "admin",
                false,
                vec![application_command_option(
                    1,
                    "ban",
                    false,
                    vec![
                        application_command_option(6, "user", true, Vec::new()),
                        application_command_option(3, "reason", false, Vec::new()),
                    ],
                )],
            )],
        );

        let interaction = application_command_interaction_from_invocation(
            &invocation("mod", "/mod admin ban user:<@123> reason:spam links"),
            &command,
        )
        .expect("valid invocation should build interaction");

        assert_eq!(
            interaction.options,
            vec![ApplicationCommandInteractionOption {
                kind: 2,
                name: "admin".to_owned(),
                value: None,
                options: vec![ApplicationCommandInteractionOption {
                    kind: 1,
                    name: "ban".to_owned(),
                    value: None,
                    options: vec![
                        ApplicationCommandInteractionOption {
                            kind: 6,
                            name: "user".to_owned(),
                            value: Some(Value::String("123".to_owned())),
                            options: Vec::new(),
                        },
                        ApplicationCommandInteractionOption {
                            kind: 3,
                            name: "reason".to_owned(),
                            value: Some(Value::String("spam links".to_owned())),
                            options: Vec::new(),
                        },
                    ],
                }],
            }]
        );
    }

    #[test]
    fn invocation_rejects_invalid_or_incomplete_options() {
        let command = application_command(
            "roll",
            vec![application_command_option(4, "sides", true, Vec::new())],
        );

        assert!(
            application_command_interaction_from_invocation(
                &invocation("roll", "/roll sides:many"),
                &command,
            )
            .is_none()
        );
        assert!(
            application_command_interaction_from_invocation(&invocation("roll", "/roll"), &command)
                .is_none()
        );
    }
}
