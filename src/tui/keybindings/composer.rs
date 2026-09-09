use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::{KeymapBinding, KeymapOptions};
use crate::tui::text_input::TextEditAction;

use super::{
    ComposerAction, KeyChord, KeymapBindingSummary, MAX_KEYMAP_MAPPINGS, char_chord, ctrl_chord,
    key_chord, key_chords_match_same_event, key_labels, modified_key_chord, parse_sequence_token,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ComposerKeyBindings {
    bindings: Vec<ComposerKeyBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ComposerKeyBinding {
    action: ComposerShortcutAction,
    shortcuts: Vec<KeyChord>,
}

// Composer actions have four views: the configured name, aliases, runtime
// action, and default shortcuts. Keeping them in one declaration prevents a
// new action from being accepted by only part of the composer keymap.
macro_rules! define_composer_actions {
    (
        $(
            $variant:ident => (
                aliases: [$($alias:literal),* $(,)?],
                action: $action:expr,
                defaults: $defaults:expr
            )
        ),* $(,)?
    ) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        enum ComposerShortcutAction {
            $($variant),*
        }

        impl ComposerShortcutAction {
            fn from_keymap_name(name: &str) -> Option<Self> {
                match name {
                    $(stringify!($variant) $(| $alias)* => Some(Self::$variant),)*
                    _ => None,
                }
            }

            fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant),)*
                }
            }

            fn to_composer_action(self) -> ComposerAction {
                match self {
                    $(Self::$variant => $action,)*
                }
            }

            fn defaults() -> BTreeMap<Self, Vec<KeyChord>> {
                BTreeMap::from([$( (Self::$variant, $defaults), )*])
            }
        }
    };
}

define_composer_actions! {
    OpenEditor => (
        aliases: ["OpenInEditor"],
        action: ComposerAction::OpenInEditor,
        defaults: vec![ctrl_chord('e')]
    ),
    PasteClipboard => (
        aliases: [],
        action: ComposerAction::PasteClipboard,
        defaults: vec![ctrl_chord('v')]
    ),
    InsertNewline => (
        aliases: [],
        action: ComposerAction::InsertNewline,
        defaults: vec![
            ctrl_chord('j'),
            modified_key_chord(KeyCode::Enter, KeyModifiers::SHIFT),
            modified_key_chord(KeyCode::Enter, KeyModifiers::CONTROL),
            modified_key_chord(KeyCode::Enter, KeyModifiers::ALT),
        ]
    ),
    Submit => (
        aliases: [],
        action: ComposerAction::Submit,
        defaults: vec![key_chord(KeyCode::Enter)]
    ),
    Close => (
        aliases: [],
        action: ComposerAction::Close,
        defaults: vec![key_chord(KeyCode::Esc)]
    ),
    ClearInput => (
        aliases: [],
        action: ComposerAction::ClearInput,
        defaults: vec![ctrl_chord('c')]
    ),
    RemoveLastAttachment => (
        aliases: [],
        action: ComposerAction::RemoveLastAttachment,
        defaults: vec![key_chord(KeyCode::Delete)]
    ),
    DeletePreviousChar => (
        aliases: [],
        action: ComposerAction::EditText(TextEditAction::DeletePreviousChar),
        defaults: vec![key_chord(KeyCode::Backspace)]
    ),
    DeletePreviousWord => (
        aliases: [],
        action: ComposerAction::EditText(TextEditAction::DeletePreviousWord),
        defaults: vec![
            modified_key_chord(KeyCode::Backspace, KeyModifiers::ALT),
            modified_key_chord(KeyCode::Backspace, KeyModifiers::CONTROL),
            ctrl_chord('w'),
        ]
    ),
    DeleteToLineStart => (
        aliases: [],
        action: ComposerAction::EditText(TextEditAction::DeleteToLineStart),
        defaults: vec![ctrl_chord('u')]
    ),
    DeleteToLineEnd => (
        aliases: [],
        action: ComposerAction::EditText(TextEditAction::DeleteToLineEnd),
        defaults: vec![ctrl_chord('k')]
    ),
    MoveCursorUp => (
        aliases: [],
        action: ComposerAction::EditText(TextEditAction::MoveCursorUp),
        defaults: vec![key_chord(KeyCode::Up)]
    ),
    MoveCursorDown => (
        aliases: [],
        action: ComposerAction::EditText(TextEditAction::MoveCursorDown),
        defaults: vec![key_chord(KeyCode::Down)]
    ),
    MoveCursorWordLeft => (
        aliases: [],
        action: ComposerAction::EditText(TextEditAction::MoveCursorWordLeft),
        defaults: vec![modified_key_chord(KeyCode::Left, KeyModifiers::CONTROL)]
    ),
    MoveCursorLeft => (
        aliases: [],
        action: ComposerAction::EditText(TextEditAction::MoveCursorLeft),
        defaults: vec![key_chord(KeyCode::Left)]
    ),
    MoveCursorWordRight => (
        aliases: [],
        action: ComposerAction::EditText(TextEditAction::MoveCursorWordRight),
        defaults: vec![modified_key_chord(KeyCode::Right, KeyModifiers::CONTROL)]
    ),
    MoveCursorRight => (
        aliases: [],
        action: ComposerAction::EditText(TextEditAction::MoveCursorRight),
        defaults: vec![key_chord(KeyCode::Right)]
    ),
    MoveCursorHome => (
        aliases: [],
        action: ComposerAction::EditText(TextEditAction::MoveCursorHome),
        defaults: vec![key_chord(KeyCode::Home)]
    ),
    MoveCursorEnd => (
        aliases: [],
        action: ComposerAction::EditText(TextEditAction::MoveCursorEnd),
        defaults: vec![key_chord(KeyCode::End)]
    ),
    ToggleReplyPing => (
        aliases: [],
        action: ComposerAction::ToggleReplyPing,
        defaults: vec![modified_key_chord(KeyCode::Char('p'), KeyModifiers::ALT)]
    ),
}

impl Default for ComposerKeyBindings {
    fn default() -> Self {
        Self::from_specs(ComposerShortcutAction::defaults())
    }
}

impl ComposerKeyBindings {
    pub(super) fn from_options_lossy(options: &KeymapOptions) -> Self {
        let mut configured = BTreeMap::new();
        for (action_name, binding) in options.composer.iter().take(MAX_KEYMAP_MAPPINGS) {
            let Some(action) = ComposerShortcutAction::from_keymap_name(action_name) else {
                continue;
            };
            let Some(shortcuts) = parse_composer_binding_lossy(binding) else {
                continue;
            };
            let previous = configured.insert(action, shortcuts);
            if composer_shortcuts_have_conflicts(&configured) {
                if let Some(previous) = previous {
                    configured.insert(action, previous);
                } else {
                    configured.remove(&action);
                }
            }
        }

        let mut specs = ComposerShortcutAction::defaults();
        remove_default_composer_conflicts(&mut specs, &configured);
        specs.extend(configured);
        Self::from_specs(specs)
    }

    pub(super) fn try_from_options(options: &KeymapOptions) -> std::result::Result<Self, String> {
        if options.composer.len() > MAX_KEYMAP_MAPPINGS {
            return Err(format!(
                "keymap.composer supports at most {MAX_KEYMAP_MAPPINGS} mappings"
            ));
        }

        let mut configured = BTreeMap::new();
        for (action_name, binding) in &options.composer {
            let action = ComposerShortcutAction::from_keymap_name(action_name)
                .ok_or_else(|| format!("unknown keymap.composer action `{action_name}`"))?;
            let shortcuts = parse_composer_binding(action_name, binding)?;
            configured.insert(action, shortcuts);
        }
        if composer_shortcuts_have_conflicts(&configured) {
            return Err("keymap.composer contains conflicting shortcuts".to_owned());
        }

        let mut specs = ComposerShortcutAction::defaults();
        remove_default_composer_conflicts(&mut specs, &configured);
        specs.extend(configured);
        Ok(Self::from_specs(specs))
    }

    fn from_specs(specs: BTreeMap<ComposerShortcutAction, Vec<KeyChord>>) -> Self {
        Self {
            bindings: specs
                .into_iter()
                .filter(|(_, shortcuts)| !shortcuts.is_empty())
                .map(|(action, shortcuts)| ComposerKeyBinding { action, shortcuts })
                .collect(),
        }
    }

    pub(super) fn action_for_key(&self, key: KeyEvent) -> Option<ComposerAction> {
        self.bindings.iter().find_map(|binding| {
            binding
                .shortcuts
                .iter()
                .any(|shortcut| shortcut.matches(key))
                .then(|| binding.action.to_composer_action())
        })
    }

    pub(super) fn binding_summaries(&self) -> Vec<KeymapBindingSummary> {
        self.bindings
            .iter()
            .map(|binding| KeymapBindingSummary {
                scope: "keymap.composer",
                action: binding.action.name().to_owned(),
                keys: key_labels(&binding.shortcuts),
            })
            .collect()
    }
}

fn parse_composer_binding_lossy(binding: &KeymapBinding) -> Option<Vec<KeyChord>> {
    let shortcuts = binding
        .keys
        .iter()
        .filter_map(|key| parse_composer_shortcut_key(key).ok())
        .collect::<Vec<_>>();
    (!shortcuts.is_empty()).then_some(shortcuts)
}

fn parse_composer_binding(
    action_name: &str,
    binding: &KeymapBinding,
) -> std::result::Result<Vec<KeyChord>, String> {
    let mut shortcuts = Vec::new();
    for key in &binding.keys {
        shortcuts.push(
            parse_composer_shortcut_key(key).map_err(|error| format!("{action_name}: {error}"))?,
        );
    }
    if shortcuts.is_empty() {
        return Err(format!(
            "{action_name}: composer keymap entry must include at least one key"
        ));
    }
    Ok(shortcuts)
}

fn parse_composer_shortcut_key(value: &str) -> std::result::Result<KeyChord, String> {
    let mut keys = Vec::new();
    for token in value.split_whitespace() {
        keys.extend(parse_sequence_token(token, char_chord(' '))?);
    }
    let [key] = keys.as_slice() else {
        return Err("composer shortcut must be a single key".to_owned());
    };
    Ok(key.canonical())
}

fn remove_default_composer_conflicts(
    defaults: &mut BTreeMap<ComposerShortcutAction, Vec<KeyChord>>,
    configured: &BTreeMap<ComposerShortcutAction, Vec<KeyChord>>,
) {
    defaults.retain(|default_action, default_shortcuts| {
        if configured.contains_key(default_action) {
            return false;
        }
        default_shortcuts.retain(|default_shortcut| {
            !configured.values().any(|configured_shortcuts| {
                configured_shortcuts.iter().any(|configured_shortcut| {
                    key_chords_match_same_event(*default_shortcut, *configured_shortcut)
                })
            })
        });
        !default_shortcuts.is_empty()
    });
}

fn composer_shortcuts_have_conflicts(
    bindings: &BTreeMap<ComposerShortcutAction, Vec<KeyChord>>,
) -> bool {
    let shortcuts = bindings
        .values()
        .flat_map(|binding| binding.iter().copied())
        .collect::<Vec<_>>();
    shortcuts.iter().enumerate().any(|(index, shortcut)| {
        shortcuts
            .iter()
            .skip(index + 1)
            .any(|other| key_chords_match_same_event(*shortcut, *other))
    })
}
