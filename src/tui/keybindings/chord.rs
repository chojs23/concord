use std::str::FromStr;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MediaKeyCode};

use super::is_reserved_keymap_chord;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::tui) struct KeyChord {
    pub(super) code: KeyCode,
    pub(super) modifiers: KeyModifiers,
}

pub(super) struct KeySequence(pub(super) Vec<KeyChord>);

impl KeySequence {
    pub(super) fn parse(value: &str, leader: KeyChord) -> std::result::Result<Self, String> {
        let mut keys = Vec::new();
        for token in value.split_whitespace() {
            for key in parse_sequence_token(token, leader)? {
                if is_reserved_keymap_chord(key) {
                    return Err(format!("{} is reserved", key.label()));
                }
                keys.push(key.canonical());
            }
        }
        if keys.is_empty() {
            return Err("keymap sequence cannot be empty".to_owned());
        }
        Ok(Self(keys))
    }
}

pub(super) fn parse_sequence_token(
    token: &str,
    leader: KeyChord,
) -> std::result::Result<Vec<KeyChord>, String> {
    let token = token.trim();
    if token.is_empty() {
        return Ok(Vec::new());
    }
    if token.contains('+') {
        return Err(format!(
            "unsupported key `{token}`; use Vim-style angle modifiers like `<C-w>`"
        ));
    }
    if !token.contains('<') {
        return parse_plain_sequence_token(token);
    }

    let mut keys = Vec::new();
    let mut rest = token;
    while !rest.is_empty() {
        if let Some(after_open) = rest.strip_prefix('<') {
            let Some(close_index) = after_open.find('>') else {
                return Err(format!("unsupported key `{rest}`"));
            };
            let inner = &after_open[..close_index];
            if inner.eq_ignore_ascii_case("leader") {
                keys.push(leader);
            } else {
                keys.push(parse_angle_key(inner)?);
            }
            rest = &after_open[close_index + 1..];
        } else {
            let next_angle = rest.find('<').unwrap_or(rest.len());
            let segment = &rest[..next_angle];
            if looks_like_bare_modifier_key(segment) {
                return Err(format!(
                    "unsupported key `{segment}`; use Vim-style angle modifiers like `<C-w>`"
                ));
            }
            keys.extend(segment.chars().map(char_chord));
            rest = &rest[next_angle..];
        }
    }
    Ok(keys)
}

pub(super) fn parse_plain_sequence_token(
    token: &str,
) -> std::result::Result<Vec<KeyChord>, String> {
    if looks_like_bare_modifier_key(token) {
        return Err(format!(
            "unsupported key `{token}`; use Vim-style angle modifiers like `<C-w>`"
        ));
    }
    match KeyChord::from_str(token) {
        Ok(key) => Ok(vec![key]),
        Err(error) => {
            if token.contains('+') {
                return Err(error);
            }
            Ok(token.chars().map(char_chord).collect())
        }
    }
}

pub(super) fn looks_like_bare_modifier_key(value: &str) -> bool {
    let Some((modifier, key)) = value.split_once('-') else {
        return false;
    };
    if key.is_empty() {
        return false;
    }
    matches!(
        modifier,
        "C" | "S"
            | "A"
            | "M"
            | "c"
            | "s"
            | "a"
            | "m"
            | "ctrl"
            | "control"
            | "shift"
            | "alt"
            | "meta"
    )
}

pub(super) fn parse_angle_key(value: &str) -> std::result::Result<KeyChord, String> {
    if value.contains('+') {
        return Err(format!(
            "unsupported angle key `{value}`; use Vim-style hyphen modifiers like `C-w`"
        ));
    }

    let parts = value.split('-').map(str::trim).collect::<Vec<_>>();
    let Some((key, modifier_parts)) = parts.split_last() else {
        return KeyChord::from_str(value);
    };
    if modifier_parts.is_empty() {
        return KeyChord::from_str(value);
    }

    let mut modifiers = KeyModifiers::empty();
    for modifier in modifier_parts {
        match *modifier {
            "C" => modifiers.insert(KeyModifiers::CONTROL),
            "S" => modifiers.insert(KeyModifiers::SHIFT),
            "A" | "M" => modifiers.insert(KeyModifiers::ALT),
            unknown => return Err(format!("unsupported angle key modifier `{unknown}`")),
        }
    }

    let code = parse_key_code(key)?;
    Ok(KeyChord {
        code,
        modifiers: normalized_modifiers(modifiers),
    })
}

impl KeyChord {
    pub(in crate::tui) fn matches_chord(self, other: Self) -> bool {
        key_chords_match_same_event(self, other)
    }

    pub(in crate::tui) fn matches_char(self, value: char) -> bool {
        self.matches_chord(char_chord(value))
    }

    pub(super) fn matches(self, key: KeyEvent) -> bool {
        let expected = self.canonical();
        let actual = Self {
            code: key.code,
            modifiers: key.modifiers,
        }
        .canonical();

        // Crossterm and terminals are not perfectly uniform for shifted letters:
        // Shift+r may arrive as `Char('r') + SHIFT`, `Char('R')`, or both.
        // Keep these forms equivalent so configured shortcuts and conflict checks
        // describe the user's physical key press, not one terminal's encoding.
        expected == actual
            || matches!(expected.code, KeyCode::Char(value) if value.is_ascii_lowercase())
                && expected.modifiers.contains(KeyModifiers::SHIFT)
                && actual.code
                    == KeyCode::Char(match expected.code {
                        KeyCode::Char(value) => value.to_ascii_uppercase(),
                        _ => unreachable!("expected code is matched as a char"),
                    })
                && actual.modifiers == expected.modifiers
            || matches!(expected.code, KeyCode::Char(_))
                && expected.modifiers.is_empty()
                && actual.code == expected.code
                && actual.modifiers == KeyModifiers::SHIFT
    }

    pub(super) fn canonical(self) -> Self {
        let modifiers = normalized_modifiers(self.modifiers);
        if self.code == KeyCode::BackTab {
            Self {
                code: KeyCode::Tab,
                modifiers: modifiers | KeyModifiers::SHIFT,
            }
        } else {
            Self {
                code: self.code,
                modifiers,
            }
        }
    }

    pub(in crate::tui) fn label(self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("Ctrl".to_owned());
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            parts.push("Alt".to_owned());
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            parts.push("Shift".to_owned());
        }
        parts.push(key_code_label(self.code));
        parts.join("+")
    }

    pub(super) fn title_label(self) -> String {
        if self.modifiers.is_empty()
            && let KeyCode::Char(value) = self.code
        {
            return value.to_string();
        }

        let mut value = String::from("<");
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            value.push_str("C-");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            value.push_str("A-");
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            value.push_str("S-");
        }
        value.push_str(&key_code_label(self.code));
        value.push('>');
        value
    }
}

pub(in crate::tui) fn push_to_talk_shortcut_from_key(
    key: KeyEvent,
) -> std::result::Result<String, String> {
    if key
        .modifiers
        .intersects(KeyModifiers::HYPER | KeyModifiers::META)
    {
        return Err("Hyper and Meta modifiers are not supported for push-to-talk".to_owned());
    }

    let mut modifiers = key.modifiers
        & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT | KeyModifiers::SUPER);
    let key_name = push_to_talk_key_name(key.code, &mut modifiers)?;
    let mut parts = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Control".to_owned());
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt".to_owned());
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift".to_owned());
    }
    if modifiers.contains(KeyModifiers::SUPER) {
        parts.push("Super".to_owned());
    }
    parts.push(key_name);
    Ok(parts.join("+"))
}

fn push_to_talk_key_name(
    code: KeyCode,
    modifiers: &mut KeyModifiers,
) -> std::result::Result<String, String> {
    let name = match code {
        KeyCode::Backspace => "Backspace".to_owned(),
        KeyCode::Enter => "Enter".to_owned(),
        KeyCode::Left => "Left".to_owned(),
        KeyCode::Right => "Right".to_owned(),
        KeyCode::Up => "Up".to_owned(),
        KeyCode::Down => "Down".to_owned(),
        KeyCode::Home => "Home".to_owned(),
        KeyCode::End => "End".to_owned(),
        KeyCode::PageUp => "PageUp".to_owned(),
        KeyCode::PageDown => "PageDown".to_owned(),
        KeyCode::Tab => "Tab".to_owned(),
        KeyCode::BackTab => {
            modifiers.insert(KeyModifiers::SHIFT);
            "Tab".to_owned()
        }
        KeyCode::Delete => "Delete".to_owned(),
        KeyCode::Insert => "Insert".to_owned(),
        KeyCode::F(value @ 1..=24) => format!("F{value}"),
        KeyCode::Char(value) => push_to_talk_character_name(value, modifiers)?,
        KeyCode::Esc => "Esc".to_owned(),
        KeyCode::CapsLock => "CapsLock".to_owned(),
        KeyCode::ScrollLock => "ScrollLock".to_owned(),
        KeyCode::NumLock => "NumLock".to_owned(),
        KeyCode::PrintScreen => "PrintScreen".to_owned(),
        KeyCode::Pause => "Pause".to_owned(),
        KeyCode::Media(media) => push_to_talk_media_key_name(media)?.to_owned(),
        KeyCode::F(_)
        | KeyCode::Null
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Modifier(_) => {
            return Err("That key cannot be used as a global push-to-talk shortcut".to_owned());
        }
    };
    Ok(name)
}

fn push_to_talk_character_name(
    value: char,
    modifiers: &mut KeyModifiers,
) -> std::result::Result<String, String> {
    if value == ' ' {
        return Ok("Space".to_owned());
    }
    if value.is_ascii_uppercase() {
        modifiers.insert(KeyModifiers::SHIFT);
        return Ok(value.to_string());
    }
    if value.is_ascii_lowercase() || value.is_ascii_digit() {
        return Ok(value.to_ascii_uppercase().to_string());
    }

    let (base, shifted) = match value {
        '`' => ('`', false),
        '-' => ('-', false),
        '=' => ('=', false),
        '[' => ('[', false),
        ']' => (']', false),
        '\\' => ('\\', false),
        ';' => (';', false),
        '\'' => ('\'', false),
        ',' => (',', false),
        '.' => ('.', false),
        '/' => ('/', false),
        '~' => ('`', true),
        '_' => ('-', true),
        '+' => ('=', true),
        '{' => ('[', true),
        '}' => (']', true),
        '|' => ('\\', true),
        ':' => (';', true),
        '"' => ('\'', true),
        '<' => (',', true),
        '>' => ('.', true),
        '?' => ('/', true),
        '!' => ('1', true),
        '@' => ('2', true),
        '#' => ('3', true),
        '$' => ('4', true),
        '%' => ('5', true),
        '^' => ('6', true),
        '&' => ('7', true),
        '*' => ('8', true),
        '(' => ('9', true),
        ')' => ('0', true),
        _ => {
            return Err(
                "Only portable keyboard keys can be used for global push-to-talk".to_owned(),
            );
        }
    };
    if shifted {
        modifiers.insert(KeyModifiers::SHIFT);
    }
    Ok(base.to_string())
}

fn push_to_talk_media_key_name(key: MediaKeyCode) -> std::result::Result<&'static str, String> {
    match key {
        MediaKeyCode::Play => Ok("MediaPlay"),
        MediaKeyCode::Pause => Ok("MediaPause"),
        MediaKeyCode::PlayPause => Ok("MediaPlayPause"),
        MediaKeyCode::Stop => Ok("MediaStop"),
        MediaKeyCode::TrackNext => Ok("MediaTrackNext"),
        MediaKeyCode::TrackPrevious => Ok("MediaTrackPrevious"),
        MediaKeyCode::LowerVolume => Ok("VolumeDown"),
        MediaKeyCode::RaiseVolume => Ok("VolumeUp"),
        MediaKeyCode::MuteVolume => Ok("VolumeMute"),
        MediaKeyCode::Reverse
        | MediaKeyCode::FastForward
        | MediaKeyCode::Rewind
        | MediaKeyCode::Record => {
            Err("That media key is not supported for global push-to-talk".to_owned())
        }
    }
}

impl FromStr for KeyChord {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err("keybinding cannot be empty".to_owned());
        }

        if value.contains('+') {
            return Err(format!(
                "unsupported key `{value}`; use Vim-style angle modifiers like `<C-w>`"
            ));
        }

        let code = parse_key_code(value)?;
        Ok(Self {
            code,
            modifiers: KeyModifiers::empty(),
        })
    }
}

pub(super) fn key_chords_match_same_event(left: KeyChord, right: KeyChord) -> bool {
    // Compare by possible terminal events rather than raw fields. This keeps
    // `A`, `Shift+a`, and uppercase-with-Shift encodings from being treated as
    // independent shortcuts when they can be produced by the same key press.
    candidate_key_events(left)
        .into_iter()
        .chain(candidate_key_events(right))
        .any(|event| left.matches(event) && right.matches(event))
}

pub(super) fn candidate_key_events(chord: KeyChord) -> Vec<KeyEvent> {
    let chord = chord.canonical();
    let mut events = vec![KeyEvent::new(chord.code, chord.modifiers)];
    if let KeyCode::Char(value) = chord.code {
        events.push(KeyEvent::new(
            KeyCode::Char(value.to_ascii_uppercase()),
            KeyModifiers::SHIFT,
        ));
        events.push(KeyEvent::new(
            KeyCode::Char(value.to_ascii_lowercase()),
            KeyModifiers::NONE,
        ));
    }
    events
}

pub(super) fn parse_key_code(value: &str) -> std::result::Result<KeyCode, String> {
    if value.chars().count() == 1 {
        return Ok(KeyCode::Char(value.chars().next().expect("one char")));
    }

    let lower = value.to_ascii_lowercase();
    match lower.as_str() {
        "space" => Ok(KeyCode::Char(' ')),
        "tab" => Ok(KeyCode::Tab),
        "backtab" => Ok(KeyCode::BackTab),
        "enter" => Ok(KeyCode::Enter),
        "esc" | "escape" => Ok(KeyCode::Esc),
        "backspace" => Ok(KeyCode::Backspace),
        "delete" | "del" => Ok(KeyCode::Delete),
        "left" => Ok(KeyCode::Left),
        "right" => Ok(KeyCode::Right),
        "up" => Ok(KeyCode::Up),
        "down" => Ok(KeyCode::Down),
        "home" => Ok(KeyCode::Home),
        "end" => Ok(KeyCode::End),
        "pageup" | "page-up" => Ok(KeyCode::PageUp),
        "pagedown" | "page-down" => Ok(KeyCode::PageDown),
        value if value.starts_with('f') => value[1..]
            .parse::<u8>()
            .map(KeyCode::F)
            .map_err(|_| format!("unsupported key `{value}`")),
        _ => Err(format!("unsupported key `{value}`")),
    }
}

pub(super) fn normalized_modifiers(modifiers: KeyModifiers) -> KeyModifiers {
    modifiers & (KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT)
}

pub(super) fn key_chord(code: KeyCode) -> KeyChord {
    KeyChord {
        code,
        modifiers: KeyModifiers::NONE,
    }
}

pub(super) fn char_chord(value: char) -> KeyChord {
    key_chord(KeyCode::Char(value))
}

pub(super) fn ctrl_chord(value: char) -> KeyChord {
    modified_key_chord(KeyCode::Char(value), KeyModifiers::CONTROL)
}

pub(super) fn modified_key_chord(code: KeyCode, modifiers: KeyModifiers) -> KeyChord {
    KeyChord {
        code,
        modifiers: normalized_modifiers(modifiers),
    }
}

pub(super) fn key_code_label(code: KeyCode) -> String {
    match code {
        KeyCode::Char(' ') => "Space".to_owned(),
        KeyCode::Char(value) => value.to_string(),
        KeyCode::BackTab => "Shift+Tab".to_owned(),
        KeyCode::PageUp => "PageUp".to_owned(),
        KeyCode::PageDown => "PageDown".to_owned(),
        KeyCode::Left => "Left".to_owned(),
        KeyCode::Right => "Right".to_owned(),
        KeyCode::Up => "Up".to_owned(),
        KeyCode::Down => "Down".to_owned(),
        KeyCode::Enter => "Enter".to_owned(),
        KeyCode::Esc => "Esc".to_owned(),
        KeyCode::Backspace => "Backspace".to_owned(),
        KeyCode::Delete => "Delete".to_owned(),
        KeyCode::Home => "Home".to_owned(),
        KeyCode::End => "End".to_owned(),
        KeyCode::Tab => "Tab".to_owned(),
        KeyCode::F(value) => format!("F{value}"),
        _ => format!("{code:?}"),
    }
}
