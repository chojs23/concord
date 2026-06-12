#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinSlashCommand {
    Gif,
    Tenor,
    Tts,
    Me,
    Tableflip,
    Unflip,
    Shrug,
    Spoiler,
    Nick,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinSlashCommandInfo {
    pub kind: BuiltinSlashCommand,
    pub name: &'static str,
    pub description: &'static str,
    pub replacement: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuiltinSlashCommandSubmit {
    Message { content: String, tts: bool },
    Nickname { nickname: String },
    Unsupported { message: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuiltinSlashCommandParse {
    Ready(BuiltinSlashCommandSubmit),
    Incomplete,
    NotBuiltin,
}

const BUILTIN_SLASH_COMMANDS: &[BuiltinSlashCommandInfo] = &[
    BuiltinSlashCommandInfo {
        kind: BuiltinSlashCommand::Gif,
        name: "gif",
        description: "Search for a GIF",
        replacement: "/gif ",
    },
    BuiltinSlashCommandInfo {
        kind: BuiltinSlashCommand::Tenor,
        name: "tenor",
        description: "Search Tenor GIFs",
        replacement: "/tenor ",
    },
    BuiltinSlashCommandInfo {
        kind: BuiltinSlashCommand::Tts,
        name: "tts",
        description: "Send a text-to-speech message",
        replacement: "/tts ",
    },
    BuiltinSlashCommandInfo {
        kind: BuiltinSlashCommand::Me,
        name: "me",
        description: "Send an italic action message",
        replacement: "/me ",
    },
    BuiltinSlashCommandInfo {
        kind: BuiltinSlashCommand::Tableflip,
        name: "tableflip",
        description: "Add a table flip face ((╯°□°）╯︵ ┻━┻)",
        replacement: "/tableflip ",
    },
    BuiltinSlashCommandInfo {
        kind: BuiltinSlashCommand::Unflip,
        name: "unflip",
        description: "Add a table unflip face (┬─┬ ノ( ゜-゜ノ))",
        replacement: "/unflip ",
    },
    BuiltinSlashCommandInfo {
        kind: BuiltinSlashCommand::Shrug,
        name: "shrug",
        description: r"Add a shrug face (¯\_(ツ)_/¯)",
        replacement: "/shrug ",
    },
    BuiltinSlashCommandInfo {
        kind: BuiltinSlashCommand::Spoiler,
        name: "spoiler",
        description: "Send spoiler text",
        replacement: "/spoiler ",
    },
    BuiltinSlashCommandInfo {
        kind: BuiltinSlashCommand::Nick,
        name: "nick",
        description: "Change or clear your server nickname",
        replacement: "/nick ",
    },
];

pub fn builtin_slash_commands() -> &'static [BuiltinSlashCommandInfo] {
    BUILTIN_SLASH_COMMANDS
}

pub fn parse_builtin_slash_command(content: &str) -> BuiltinSlashCommandParse {
    let Some(rest) = content.strip_prefix('/') else {
        return BuiltinSlashCommandParse::NotBuiltin;
    };
    let Some((name, argument)) = split_command_name(rest) else {
        return BuiltinSlashCommandParse::NotBuiltin;
    };
    let Some(command) = BUILTIN_SLASH_COMMANDS
        .iter()
        .find(|command| command.name == name)
    else {
        return BuiltinSlashCommandParse::NotBuiltin;
    };

    match command.kind {
        BuiltinSlashCommand::Gif | BuiltinSlashCommand::Tenor => required_argument(argument)
            .map_or(BuiltinSlashCommandParse::Incomplete, |_| {
                BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Unsupported {
                    message: "GIF slash commands are not supported in Concord yet".to_owned(),
                })
            }),
        BuiltinSlashCommand::Tts => {
            required_argument(argument).map_or(BuiltinSlashCommandParse::Incomplete, |message| {
                BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Message {
                    content: message.to_owned(),
                    tts: true,
                })
            })
        }
        BuiltinSlashCommand::Me => {
            required_argument(argument).map_or(BuiltinSlashCommandParse::Incomplete, |message| {
                BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Message {
                    content: format!("_{message}_"),
                    tts: false,
                })
            })
        }
        BuiltinSlashCommand::Tableflip => {
            BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Message {
                content: append_optional_message(argument, "(╯°□°）╯︵ ┻━┻"),
                tts: false,
            })
        }
        BuiltinSlashCommand::Unflip => {
            BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Message {
                content: append_optional_message(argument, "┬─┬ ノ( ゜-゜ノ)"),
                tts: false,
            })
        }
        BuiltinSlashCommand::Shrug => {
            BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Message {
                content: append_optional_message(argument, r"¯\_(ツ)_/¯"),
                tts: false,
            })
        }
        BuiltinSlashCommand::Spoiler => {
            required_argument(argument).map_or(BuiltinSlashCommandParse::Incomplete, |message| {
                BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Message {
                    content: format!("||{message}||"),
                    tts: false,
                })
            })
        }
        BuiltinSlashCommand::Nick => {
            BuiltinSlashCommandParse::Ready(BuiltinSlashCommandSubmit::Nickname {
                nickname: argument.to_owned(),
            })
        }
    }
}

fn split_command_name(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let name_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let name = &trimmed[..name_end];
    let argument = trimmed[name_end..].trim_start();
    Some((name, argument))
}

fn required_argument(argument: &str) -> Option<&str> {
    (!argument.trim().is_empty()).then_some(argument)
}

fn append_optional_message(message: &str, suffix: &str) -> String {
    if message.is_empty() {
        suffix.to_owned()
    } else {
        format!("{message} {suffix}")
    }
}
