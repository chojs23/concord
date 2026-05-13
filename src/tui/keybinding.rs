use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::KeyBindingsConfig;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl KeyBinding {
    /// Normalise a raw key-event into a canonical `KeyBinding` for HashMap lookup.
    ///
    /// Crossterm / terminals report Shift+letter in three different styles:
    ///   (a) Char('j') + SHIFT     — crossterm canonical
    ///   (b) Char('J') + empty     — legacy no-modifier uppercase
    ///   (c) Char('J') + SHIFT     — uppercase WITH shift still set
    ///
    /// All three are normalised to form (a): lowercase char + SHIFT.
    /// `BackTab` (Shift+Tab) is normalised to `Tab + SHIFT`.
    fn from_event(key: KeyEvent) -> Self {
        if key.code == KeyCode::BackTab {
            return Self {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::SHIFT,
            };
        }
        if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
            if let KeyCode::Char(c) = key.code {
                if c.is_uppercase() {
                    if let Some(lower) = c.to_lowercase().next() {
                        if lower != c {
                            return Self {
                                code: KeyCode::Char(lower),
                                modifiers: KeyModifiers::SHIFT,
                            };
                        }
                    }
                }
            }
        }
        Self {
            code: key.code,
            modifiers: key.modifiers,
        }
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

    /// Parse a key spec like `"ctrl+e"`, `"alt+shift+f1"`, `"backtab"`, `"enter"`.
    pub fn parse(spec: &str) -> Option<Self> {
        let spec = spec.trim().to_lowercase();
        let parts: Vec<&str> = spec.split('+').collect();
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
            // "backtab" and "shift+tab" both normalise to Tab+SHIFT so they
            // hash-match the BackTab event produced by from_event.
            "backtab" => {
                modifiers |= KeyModifiers::SHIFT;
                KeyCode::Tab
            }
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

/// Every action that a key-press can trigger in normal (non-composer) mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Quit,
    Return,
    ToggleDebugLog,
    FocusGuilds,
    FocusChannels,
    FocusMessages,
    FocusMembers,
    CycleFocusForward,
    CycleFocusBackward,
    OpenComposer,
    OpenInEditor,
    OpenKeymap,
    OpenLeader,
    PaneSearch,
    MoveDown,
    MoveUp,
    HalfPageDown,
    HalfPageUp,
    JumpTop,
    JumpBottom,
    ScrollViewportDown,
    ScrollViewportUp,
    ScrollPaneLeft,
    ScrollPaneRight,
    NarrowPane,
    WidenPane,
    Confirm,
    ExpandRight,
    CollapseLeft,
    /// Home key: scroll to top of viewport (Messages) or jump to first item.
    ScrollTop,
    /// End key: scroll to bottom of viewport (Messages) or jump to last item.
    ScrollBottom,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Self::Quit => "quit",
            Self::Return => "return",
            Self::ToggleDebugLog => "toggle_debug_log",
            Self::FocusGuilds => "focus_guilds",
            Self::FocusChannels => "focus_channels",
            Self::FocusMessages => "focus_messages",
            Self::FocusMembers => "focus_members",
            Self::CycleFocusForward => "cycle_focus_forward",
            Self::CycleFocusBackward => "cycle_focus_backward",
            Self::OpenComposer => "open_composer",
            Self::OpenInEditor => "open_in_editor",
            Self::OpenKeymap => "open_keymap",
            Self::OpenLeader => "open_leader",
            Self::PaneSearch => "pane_search",
            Self::MoveDown => "move_down",
            Self::MoveUp => "move_up",
            Self::HalfPageDown => "half_page_down",
            Self::HalfPageUp => "half_page_up",
            Self::JumpTop => "jump_top",
            Self::JumpBottom => "jump_bottom",
            Self::ScrollViewportDown => "scroll_viewport_down",
            Self::ScrollViewportUp => "scroll_viewport_up",
            Self::ScrollPaneLeft => "scroll_pane_left",
            Self::ScrollPaneRight => "scroll_pane_right",
            Self::NarrowPane => "narrow_pane",
            Self::WidenPane => "widen_pane",
            Self::Confirm => "confirm",
            Self::ExpandRight => "expand_right",
            Self::CollapseLeft => "collapse_left",
            Self::ScrollTop => "scroll_top",
            Self::ScrollBottom => "scroll_bottom",
        }
    }

    #[allow(dead_code)]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "quit" => Some(Self::Quit),
            "return" => Some(Self::Return),
            "toggle_debug_log" => Some(Self::ToggleDebugLog),
            "focus_guilds" => Some(Self::FocusGuilds),
            "focus_channels" => Some(Self::FocusChannels),
            "focus_messages" => Some(Self::FocusMessages),
            "focus_members" => Some(Self::FocusMembers),
            "cycle_focus_forward" => Some(Self::CycleFocusForward),
            "cycle_focus_backward" => Some(Self::CycleFocusBackward),
            "open_composer" => Some(Self::OpenComposer),
            "open_in_editor" => Some(Self::OpenInEditor),
            "open_keymap" => Some(Self::OpenKeymap),
            "open_leader" => Some(Self::OpenLeader),
            "pane_search" => Some(Self::PaneSearch),
            "move_down" => Some(Self::MoveDown),
            "move_up" => Some(Self::MoveUp),
            "half_page_down" => Some(Self::HalfPageDown),
            "half_page_up" => Some(Self::HalfPageUp),
            "jump_top" => Some(Self::JumpTop),
            "jump_bottom" => Some(Self::JumpBottom),
            "scroll_viewport_down" => Some(Self::ScrollViewportDown),
            "scroll_viewport_up" => Some(Self::ScrollViewportUp),
            "scroll_pane_left" => Some(Self::ScrollPaneLeft),
            "scroll_pane_right" => Some(Self::ScrollPaneRight),
            "narrow_pane" => Some(Self::NarrowPane),
            "widen_pane" => Some(Self::WidenPane),
            "confirm" => Some(Self::Confirm),
            "expand_right" => Some(Self::ExpandRight),
            "collapse_left" => Some(Self::CollapseLeft),
            "scroll_top" => Some(Self::ScrollTop),
            "scroll_bottom" => Some(Self::ScrollBottom),
            _ => None,
        }
    }

    /// Default key spec for user-configurable actions; `None` for fixed-only actions.
    pub fn default_binding(self) -> Option<&'static str> {
        match self {
            Self::Quit => Some("q"),
            Self::Return => Some("esc"),
            Self::ToggleDebugLog => Some("`"),
            Self::FocusGuilds => Some("1"),
            Self::FocusChannels => Some("2"),
            Self::FocusMessages => Some("3"),
            Self::FocusMembers => Some("4"),
            Self::CycleFocusForward => Some("tab"),
            Self::CycleFocusBackward => Some("backtab"),
            Self::OpenComposer => Some("i"),
            Self::OpenInEditor => Some("ctrl+e"),
            Self::OpenKeymap => Some("?"),
            Self::OpenLeader => Some("space"),
            Self::PaneSearch => Some("/"),
            Self::MoveDown => Some("j"),
            Self::MoveUp => Some("k"),
            Self::HalfPageDown => Some("ctrl+d"),
            Self::HalfPageUp => Some("ctrl+u"),
            Self::JumpTop => Some("g"),
            Self::JumpBottom => Some("shift+g"),
            Self::ScrollViewportDown => Some("shift+j"),
            Self::ScrollViewportUp => Some("shift+k"),
            Self::ScrollPaneLeft => Some("shift+h"),
            Self::ScrollPaneRight => Some("shift+l"),
            Self::NarrowPane => Some("alt+h"),
            Self::WidenPane => Some("alt+l"),
            Self::Confirm => Some("enter"),
            Self::ExpandRight => Some("l"),
            Self::CollapseLeft => Some("h"),
            Self::ScrollTop | Self::ScrollBottom => None,
        }
    }
}

/// All configurable actions in the order they appear in the config schema.
const CONFIGURABLE_ACTIONS: &[Action] = &[
    Action::Quit,
    Action::Return,
    Action::ToggleDebugLog,
    Action::FocusGuilds,
    Action::FocusChannels,
    Action::FocusMessages,
    Action::FocusMembers,
    Action::CycleFocusForward,
    Action::CycleFocusBackward,
    Action::OpenComposer,
    Action::OpenInEditor,
    Action::OpenKeymap,
    Action::OpenLeader,
    Action::PaneSearch,
    Action::MoveDown,
    Action::MoveUp,
    Action::HalfPageDown,
    Action::HalfPageUp,
    Action::JumpTop,
    Action::JumpBottom,
    Action::ScrollViewportDown,
    Action::ScrollViewportUp,
    Action::ScrollPaneLeft,
    Action::ScrollPaneRight,
    Action::NarrowPane,
    Action::WidenPane,
    Action::Confirm,
    Action::ExpandRight,
    Action::CollapseLeft,
];

/// Fixed aliases that are always present and cannot be overridden by config.
/// These cover hardware-conventional keys (arrow keys, Enter, Esc, Tab, …).
fn fixed_table() -> HashMap<KeyBinding, Action> {
    let mut m = HashMap::new();
    let entries: &[(&str, Action)] = &[
        ("ctrl+c", Action::Quit),
        ("down", Action::MoveDown),
        ("up", Action::MoveUp),
        ("pagedown", Action::HalfPageDown),
        ("pageup", Action::HalfPageUp),
        ("home", Action::ScrollTop),
        ("end", Action::ScrollBottom),
        ("right", Action::ExpandRight),
        ("left", Action::CollapseLeft),
        ("alt+left", Action::NarrowPane),
        ("alt+right", Action::WidenPane),
        ("tab", Action::CycleFocusForward),
        ("backtab", Action::CycleFocusBackward),
        ("enter", Action::Confirm),
        ("esc", Action::Return),
    ];
    for (spec, action) in entries {
        if let Some(binding) = KeyBinding::parse(spec) {
            m.insert(binding, *action);
        }
    }
    m
}

/// Resolved keybindings used at runtime.
#[derive(Clone, Debug)]
pub struct ActiveKeyBindings {
    /// User-configurable bindings: config spec → action (with fallback to default).
    named: HashMap<KeyBinding, Action>,
    /// Reverse of `named`: action → its current key label (for display).
    reverse: HashMap<Action, KeyBinding>,
    /// Immutable hardware-conventional aliases (arrow keys, Enter, Esc, Tab, …).
    fixed: HashMap<KeyBinding, Action>,
}

impl ActiveKeyBindings {
    pub fn from_config(config: &KeyBindingsConfig) -> Self {
        let fixed = fixed_table();
        let mut named: HashMap<KeyBinding, Action> = HashMap::new();
        let mut reverse: HashMap<Action, KeyBinding> = HashMap::new();

        for &action in CONFIGURABLE_ACTIONS {
            let default_spec = match action.default_binding() {
                Some(s) => s,
                None => continue,
            };
            // Use the user's spec if present and parseable; fall back to default.
            let spec = config
                .0
                .get(action.name())
                .map(String::as_str)
                .unwrap_or(default_spec);
            let binding = KeyBinding::parse(spec).unwrap_or_else(|| {
                KeyBinding::parse(default_spec).expect("default is always valid")
            });
            named.insert(binding.clone(), action);
            reverse.insert(action, binding);
        }

        Self {
            named,
            reverse,
            fixed,
        }
    }

    /// Look up which action the given key event triggers, if any.
    /// Named (user-configurable) bindings take priority over fixed aliases.
    pub fn lookup(&self, key: KeyEvent) -> Option<Action> {
        let binding = KeyBinding::from_event(key);
        self.named
            .get(&binding)
            .or_else(|| self.fixed.get(&binding))
            .copied()
    }

    /// Human-readable key label for `action`, e.g. `"Ctrl+E"`.
    /// Returns the current user binding if one exists, otherwise the fixed alias,
    /// otherwise `"?"`.
    pub fn label(&self, action: Action) -> String {
        if let Some(binding) = self.reverse.get(&action) {
            return binding.label();
        }
        for (binding, a) in &self.fixed {
            if *a == action {
                return binding.label();
            }
        }
        "?".to_owned()
    }
}

impl Default for ActiveKeyBindings {
    fn default() -> Self {
        Self::from_config(&KeyBindingsConfig::default())
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
        assert!(b == KeyBinding::from_event(press(KeyCode::Char('e'), KeyModifiers::CONTROL)));
        assert!(b != KeyBinding::from_event(press(KeyCode::Char('e'), KeyModifiers::empty())));
        assert!(b != KeyBinding::from_event(press(KeyCode::Char('o'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn parse_is_case_insensitive() {
        let b = KeyBinding::parse("Ctrl+E").expect("should parse");
        assert_eq!(b, KeyBinding::parse("ctrl+e").unwrap());
    }

    #[test]
    fn parse_alt_modifier() {
        let b = KeyBinding::parse("alt+e").expect("should parse");
        assert_eq!(
            b,
            KeyBinding::from_event(press(KeyCode::Char('e'), KeyModifiers::ALT))
        );
    }

    #[test]
    fn parse_option_alias_for_alt() {
        let b = KeyBinding::parse("option+e").expect("should parse");
        assert_eq!(b, KeyBinding::parse("alt+e").unwrap());
    }

    #[test]
    fn parse_function_key() {
        let b = KeyBinding::parse("f12").expect("should parse");
        assert_eq!(
            b,
            KeyBinding::from_event(press(KeyCode::F(12), KeyModifiers::empty()))
        );
    }

    #[test]
    fn parse_special_keys() {
        assert!(KeyBinding::parse("enter").is_some());
        assert!(KeyBinding::parse("esc").is_some());
        assert!(KeyBinding::parse("ctrl+backspace").is_some());
    }

    #[test]
    fn parse_backtab_and_shift_tab_are_equivalent() {
        let backtab = KeyBinding::parse("backtab").expect("should parse");
        let shift_tab = KeyBinding::parse("shift+tab").expect("should parse");
        assert_eq!(backtab, shift_tab);
    }

    #[test]
    fn backtab_event_normalises_to_shift_tab() {
        let backtab_event = KeyBinding::from_event(press(KeyCode::BackTab, KeyModifiers::empty()));
        let shift_tab = KeyBinding::parse("shift+tab").expect("should parse");
        assert_eq!(backtab_event, shift_tab);
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
    fn label_backtab_is_shift_tab() {
        let b = KeyBinding::parse("backtab").expect("should parse");
        assert_eq!(b.label(), "Shift+Tab");
    }

    #[test]
    fn shift_binding_matches_all_three_terminal_styles() {
        let b = KeyBinding::parse("shift+j").expect("should parse");
        // (a) crossterm canonical: lowercase + SHIFT modifier
        assert_eq!(
            b,
            KeyBinding::from_event(press(KeyCode::Char('j'), KeyModifiers::SHIFT))
        );
        // (b) legacy: uppercase + no modifier
        assert_eq!(
            b,
            KeyBinding::from_event(press(KeyCode::Char('J'), KeyModifiers::empty()))
        );
        // (c) uppercase WITH SHIFT still set (some terminals)
        assert_eq!(
            b,
            KeyBinding::from_event(press(KeyCode::Char('J'), KeyModifiers::SHIFT))
        );
        // must NOT match plain lowercase
        assert_ne!(
            b,
            KeyBinding::from_event(press(KeyCode::Char('j'), KeyModifiers::empty()))
        );
    }

    #[test]
    fn jump_bottom_matches_capital_g() {
        let b = KeyBinding::parse("shift+g").expect("should parse");
        assert_eq!(
            b,
            KeyBinding::from_event(press(KeyCode::Char('g'), KeyModifiers::SHIFT))
        );
        assert_eq!(
            b,
            KeyBinding::from_event(press(KeyCode::Char('G'), KeyModifiers::empty()))
        );
        assert_eq!(
            b,
            KeyBinding::from_event(press(KeyCode::Char('G'), KeyModifiers::SHIFT))
        );
        assert_ne!(
            b,
            KeyBinding::from_event(press(KeyCode::Char('g'), KeyModifiers::empty()))
        );
    }

    #[test]
    fn default_bindings_look_up_correctly() {
        let kb = ActiveKeyBindings::default();
        assert_eq!(
            kb.lookup(press(KeyCode::Char('q'), KeyModifiers::empty())),
            Some(Action::Quit)
        );
        assert_eq!(
            kb.lookup(press(KeyCode::Char('j'), KeyModifiers::empty())),
            Some(Action::MoveDown)
        );
        assert_eq!(
            kb.lookup(press(KeyCode::Down, KeyModifiers::empty())),
            Some(Action::MoveDown)
        );
        assert_eq!(
            kb.lookup(press(KeyCode::Tab, KeyModifiers::empty())),
            Some(Action::CycleFocusForward)
        );
        assert_eq!(
            kb.lookup(press(KeyCode::BackTab, KeyModifiers::empty())),
            Some(Action::CycleFocusBackward)
        );
        assert_eq!(
            kb.lookup(press(KeyCode::Enter, KeyModifiers::empty())),
            Some(Action::Confirm)
        );
        assert_eq!(
            kb.lookup(press(KeyCode::Esc, KeyModifiers::empty())),
            Some(Action::Return)
        );
    }

    #[test]
    fn from_config_falls_back_on_bad_spec() {
        use crate::config::KeyBindingsConfig;
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert("open_in_editor".to_owned(), "notakey".to_owned());
        let cfg = KeyBindingsConfig(map);
        let bindings = ActiveKeyBindings::from_config(&cfg);
        // Should have fallen back to ctrl+e
        assert_eq!(
            bindings.lookup(press(KeyCode::Char('e'), KeyModifiers::CONTROL)),
            Some(Action::OpenInEditor)
        );
    }

    #[test]
    fn custom_binding_overrides_default() {
        use crate::config::KeyBindingsConfig;
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert("open_composer".to_owned(), "e".to_owned());
        let cfg = KeyBindingsConfig(map);
        let bindings = ActiveKeyBindings::from_config(&cfg);
        assert_eq!(
            bindings.lookup(press(KeyCode::Char('e'), KeyModifiers::empty())),
            Some(Action::OpenComposer)
        );
        // Old default 'i' should no longer be mapped
        assert_ne!(
            bindings.lookup(press(KeyCode::Char('i'), KeyModifiers::empty())),
            Some(Action::OpenComposer)
        );
    }

    #[test]
    fn label_returns_current_binding() {
        let kb = ActiveKeyBindings::default();
        assert_eq!(kb.label(Action::MoveDown), "j");
        assert_eq!(kb.label(Action::Quit), "q");
        assert_eq!(kb.label(Action::OpenInEditor), "Ctrl+E");
    }
}
