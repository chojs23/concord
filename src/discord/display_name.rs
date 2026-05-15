pub(crate) fn display_name_from_parts<'a>(
    nick: Option<&'a str>,
    global_name: Option<&'a str>,
    username: Option<&'a str>,
) -> Option<&'a str> {
    nick.and_then(non_empty)
        .or_else(|| global_name.and_then(non_empty))
        .or_else(|| username.and_then(non_empty))
}

pub(crate) fn display_name_from_parts_or_unknown(
    nick: Option<&str>,
    global_name: Option<&str>,
    username: Option<&str>,
) -> String {
    display_name_from_parts(nick, global_name, username)
        .unwrap_or("unknown")
        .to_owned()
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}
