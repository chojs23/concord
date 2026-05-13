use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::KeyBindingsConfig;

#[derive(Clone, Debug)]
pub struct KeyBinding {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl KeyBinding {
    pub fn matches(&self, key: KeyEvent) -> bool {
        if key.code == self.code && key.modifiers == self.modifiers {
            return true;
        }
        // Many terminals report Shift+<letter> as uppercase letter with either
        // no SHIFT modifier or with SHIFT still set. Accept all three forms so
        // bindings like "shift+j" match capital-J from any terminal style:
        //   (a) Char('j') + SHIFT  — crossterm canonical
        //   (b) Char('J') + empty  — legacy no-modifier uppercase
        //   (c) Char('J') + SHIFT  — uppercase WITH shift still set
        if self.modifiers == KeyModifiers::SHIFT {
            if let KeyCode::Char(c) = self.code {
                let upper = c.to_uppercase().next().unwrap_or(c);
                if upper != c
                    && key.code == KeyCode::Char(upper)
                    && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT)
                {
                    return true;
                }
            }
        }
        false
    }

    /// Human-readable label for display in the keymap popup, e.g. `"Ctrl+E"`.
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("Ctrl");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            parts.push("Alt");
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            parts.push("Shift");
        }
        let key_buf;
        let key: &str = match self.code {
            KeyCode::Char(' ') => "Space",
            KeyCode::Char(c) => {
                // Bare characters (no modifier) display as-is so "j" shows as
                // "j" not "J". Characters with any modifier display uppercase
                // ("Ctrl+E", "Shift+J") matching conventional notation.
                key_buf = if self.modifiers.is_empty() {
                    c.to_string()
                } else {
                    c.to_uppercase().to_string()
                };
                &key_buf
            }
            KeyCode::Enter => "Enter",
            KeyCode::Esc => "Esc",
            KeyCode::Backspace => "Backspace",
            KeyCode::Delete => "Delete",
            KeyCode::Tab => "Tab",
            KeyCode::Home => "Home",
            KeyCode::End => "End",
            KeyCode::PageUp => "PageUp",
            KeyCode::PageDown => "PageDown",
            KeyCode::Up => "Up",
            KeyCode::Down => "Down",
            KeyCode::Left => "Left",
            KeyCode::Right => "Right",
            KeyCode::F(n) => {
                key_buf = format!("F{n}");
                &key_buf
            }
            _ => "?",
        };
        parts.push(key);
        parts.join("+")
    }

    /// Parse a key spec like `"ctrl+e"`, `"alt+shift+f1"`, `"enter"`.
    pub fn parse(spec: &str) -> Option<Self> {
        let spec = spec.trim().to_lowercase();
        let parts: Vec<&str> = spec.split('+').collect();
        // split_last returns (last_element, rest_of_slice)
        let (key_part, modifier_parts) = parts.split_last()?;

        let mut modifiers = KeyModifiers::empty();
        for part in modifier_parts {
            match *part {
                "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
                "alt" | "meta" | "option" => modifiers |= KeyModifiers::ALT,
                "shift" => modifiers |= KeyModifiers::SHIFT,
                _ => return None,
            }
        }

        let code = match *key_part {
            "enter" | "return" => KeyCode::Enter,
            "esc" | "escape" => KeyCode::Esc,
            "backspace" => KeyCode::Backspace,
            "delete" | "del" => KeyCode::Delete,
            "tab" => KeyCode::Tab,
            "space" | "spc" => KeyCode::Char(' '),
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            s if s.starts_with('f') && s.len() > 1 => {
                let n: u8 = s[1..].parse().ok()?;
                KeyCode::F(n)
            }
            s if s.len() == 1 => KeyCode::Char(s.chars().next()?),
            _ => return None,
        };

        Some(Self { code, modifiers })
    }
}

#[derive(Clone, Debug)]
pub struct ActiveKeyBindings {
    pub open_in_editor: KeyBinding,
    pub quit: KeyBinding,
    pub open_composer: KeyBinding,
    pub open_leader: KeyBinding,
    pub open_keymap: KeyBinding,
    pub pane_search: KeyBinding,
    pub move_down: KeyBinding,
    pub move_up: KeyBinding,
    pub jump_top: KeyBinding,
    pub jump_bottom: KeyBinding,
    pub half_page_down: KeyBinding,
    pub half_page_up: KeyBinding,
    pub scroll_viewport_down: KeyBinding,
    pub scroll_viewport_up: KeyBinding,
    pub scroll_pane_left: KeyBinding,
    pub scroll_pane_right: KeyBinding,
}

fn parse_binding(spec: &str, fallback: &str) -> KeyBinding {
    KeyBinding::parse(spec).unwrap_or_else(|| {
        KeyBinding::parse(fallback).expect("fallback binding is always valid")
    })
}

impl Default for ActiveKeyBindings {
    fn default() -> Self {
        Self {
            open_in_editor: KeyBinding::parse("ctrl+e").expect("default binding is valid"),
            quit: KeyBinding::parse("q").expect("default binding is valid"),
            open_composer: KeyBinding::parse("i").expect("default binding is valid"),
            open_leader: KeyBinding::parse("space").expect("default binding is valid"),
            open_keymap: KeyBinding::parse("?").expect("default binding is valid"),
            pane_search: KeyBinding::parse("/").expect("default binding is valid"),
            move_down: KeyBinding::parse("j").expect("default binding is valid"),
            move_up: KeyBinding::parse("k").expect("default binding is valid"),
            jump_top: KeyBinding::parse("g").expect("default binding is valid"),
            jump_bottom: KeyBinding::parse("shift+g").expect("default binding is valid"),
            half_page_down: KeyBinding::parse("ctrl+d").expect("default binding is valid"),
            half_page_up: KeyBinding::parse("ctrl+u").expect("default binding is valid"),
            scroll_viewport_down: KeyBinding::parse("shift+j").expect("default binding is valid"),
            scroll_viewport_up: KeyBinding::parse("shift+k").expect("default binding is valid"),
            scroll_pane_left: KeyBinding::parse("shift+h").expect("default binding is valid"),
            scroll_pane_right: KeyBinding::parse("shift+l").expect("default binding is valid"),
        }
    }
}

impl ActiveKeyBindings {
    pub fn from_config(config: &KeyBindingsConfig) -> Self {
        Self {
            open_in_editor: parse_binding(&config.open_in_editor, "ctrl+e"),
            quit: parse_binding(&config.quit, "q"),
            open_composer: parse_binding(&config.open_composer, "i"),
            open_leader: parse_binding(&config.open_leader, "space"),
            open_keymap: parse_binding(&config.open_keymap, "?"),
            pane_search: parse_binding(&config.pane_search, "/"),
            move_down: parse_binding(&config.move_down, "j"),
            move_up: parse_binding(&config.move_up, "k"),
            jump_top: parse_binding(&config.jump_top, "g"),
            jump_bottom: parse_binding(&config.jump_bottom, "shift+g"),
            half_page_down: parse_binding(&config.half_page_down, "ctrl+d"),
            half_page_up: parse_binding(&config.half_page_up, "ctrl+u"),
            scroll_viewport_down: parse_binding(&config.scroll_viewport_down, "shift+j"),
            scroll_viewport_up: parse_binding(&config.scroll_viewport_up, "shift+k"),
            scroll_pane_left: parse_binding(&config.scroll_pane_left, "shift+h"),
            scroll_pane_right: parse_binding(&config.scroll_pane_right, "shift+l"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyEventKind, KeyEventState};

    use super::*;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    #[test]
    fn parse_ctrl_e() {
        let b = KeyBinding::parse("ctrl+e").expect("should parse");
        assert!(b.matches(press(KeyCode::Char('e'), KeyModifiers::CONTROL)));
        assert!(!b.matches(press(KeyCode::Char('e'), KeyModifiers::empty())));
        assert!(!b.matches(press(KeyCode::Char('o'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn parse_is_case_insensitive() {
        let b = KeyBinding::parse("Ctrl+E").expect("should parse");
        assert!(b.matches(press(KeyCode::Char('e'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn parse_alt_modifier() {
        let b = KeyBinding::parse("alt+e").expect("should parse");
        assert!(b.matches(press(KeyCode::Char('e'), KeyModifiers::ALT)));
    }

    #[test]
    fn parse_option_alias_for_alt() {
        let b = KeyBinding::parse("option+e").expect("should parse");
        assert!(b.matches(press(KeyCode::Char('e'), KeyModifiers::ALT)));
    }

    #[test]
    fn parse_function_key() {
        let b = KeyBinding::parse("f12").expect("should parse");
        assert!(b.matches(press(KeyCode::F(12), KeyModifiers::empty())));
    }

    #[test]
    fn parse_special_keys() {
        assert!(KeyBinding::parse("enter").is_some());
        assert!(KeyBinding::parse("esc").is_some());
        assert!(KeyBinding::parse("ctrl+backspace").is_some());
    }

    #[test]
    fn parse_invalid_returns_none() {
        assert!(KeyBinding::parse("").is_none());
        assert!(KeyBinding::parse("ctrl+badkey").is_none());
        assert!(KeyBinding::parse("supermod+e").is_none());
    }

    #[test]
    fn label_ctrl_e() {
        let b = KeyBinding::parse("ctrl+e").expect("should parse");
        assert_eq!(b.label(), "Ctrl+E");
    }

    #[test]
    fn label_bare_char_is_lowercase() {
        assert_eq!(KeyBinding::parse("j").unwrap().label(), "j");
        assert_eq!(KeyBinding::parse("g").unwrap().label(), "g");
        assert_eq!(KeyBinding::parse("q").unwrap().label(), "q");
        assert_eq!(KeyBinding::parse("?").unwrap().label(), "?");
    }

    #[test]
    fn label_alt_shift_f1() {
        let b = KeyBinding::parse("alt+shift+f1").expect("should parse");
        assert_eq!(b.label(), "Alt+Shift+F1");
    }

    #[test]
    fn shift_binding_matches_all_three_terminal_styles() {
        let b = KeyBinding::parse("shift+j").expect("should parse");
        // (a) crossterm canonical: lowercase + SHIFT modifier
        assert!(b.matches(press(KeyCode::Char('j'), KeyModifiers::SHIFT)));
        // (b) legacy: uppercase + no modifier
        assert!(b.matches(press(KeyCode::Char('J'), KeyModifiers::empty())));
        // (c) uppercase WITH SHIFT still set (some terminals)
        assert!(b.matches(press(KeyCode::Char('J'), KeyModifiers::SHIFT)));
        // must NOT match plain lowercase
        assert!(!b.matches(press(KeyCode::Char('j'), KeyModifiers::empty())));
    }

    #[test]
    fn jump_bottom_matches_capital_g() {
        let b = KeyBinding::parse("shift+g").expect("should parse");
        assert!(b.matches(press(KeyCode::Char('g'), KeyModifiers::SHIFT)));
        assert!(b.matches(press(KeyCode::Char('G'), KeyModifiers::empty())));
        assert!(b.matches(press(KeyCode::Char('G'), KeyModifiers::SHIFT)));
        assert!(!b.matches(press(KeyCode::Char('g'), KeyModifiers::empty())));
    }

    #[test]
    fn from_config_falls_back_on_bad_spec() {
        use crate::config::KeyBindingsConfig;
        let cfg = KeyBindingsConfig {
            open_in_editor: "notakey".to_owned(),
            ..KeyBindingsConfig::default()
        };
        let bindings = ActiveKeyBindings::from_config(&cfg);
        // Should have fallen back to ctrl+e
        assert!(bindings
            .open_in_editor
            .matches(press(KeyCode::Char('e'), KeyModifiers::CONTROL)));
    }
}
