use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::{KeymapBinding, KeymapOptions};

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

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum ComposerShortcutAction {
    OpenEditor,
    PasteClipboard,
    InsertNewline,
    Submit,
    Close,
    ClearInput,
    RemoveLastAttachment,
    DeletePreviousChar,
    DeletePreviousWord,
    MoveCursorUp,
    MoveCursorDown,
    MoveCursorWordLeft,
    MoveCursorLeft,
    MoveCursorWordRight,
    MoveCursorRight,
    MoveCursorHome,
    MoveCursorEnd,
}

impl Default for ComposerKeyBindings {
    fn default() -> Self {
        Self::from_specs(default_composer_key_bindings())
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

        let mut specs = default_composer_key_bindings();
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

        let mut specs = default_composer_key_bindings();
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

impl ComposerShortcutAction {
    fn from_keymap_name(name: &str) -> Option<Self> {
        match name {
            "OpenEditor" | "OpenInEditor" => Some(Self::OpenEditor),
            "PasteClipboard" => Some(Self::PasteClipboard),
            "InsertNewline" => Some(Self::InsertNewline),
            "Submit" => Some(Self::Submit),
            "Close" => Some(Self::Close),
            "ClearInput" => Some(Self::ClearInput),
            "RemoveLastAttachment" => Some(Self::RemoveLastAttachment),
            "DeletePreviousChar" => Some(Self::DeletePreviousChar),
            "DeletePreviousWord" => Some(Self::DeletePreviousWord),
            "MoveCursorUp" => Some(Self::MoveCursorUp),
            "MoveCursorDown" => Some(Self::MoveCursorDown),
            "MoveCursorWordLeft" => Some(Self::MoveCursorWordLeft),
            "MoveCursorLeft" => Some(Self::MoveCursorLeft),
            "MoveCursorWordRight" => Some(Self::MoveCursorWordRight),
            "MoveCursorRight" => Some(Self::MoveCursorRight),
            "MoveCursorHome" => Some(Self::MoveCursorHome),
            "MoveCursorEnd" => Some(Self::MoveCursorEnd),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::OpenEditor => "OpenEditor",
            Self::PasteClipboard => "PasteClipboard",
            Self::InsertNewline => "InsertNewline",
            Self::Submit => "Submit",
            Self::Close => "Close",
            Self::ClearInput => "ClearInput",
            Self::RemoveLastAttachment => "RemoveLastAttachment",
            Self::DeletePreviousChar => "DeletePreviousChar",
            Self::DeletePreviousWord => "DeletePreviousWord",
            Self::MoveCursorUp => "MoveCursorUp",
            Self::MoveCursorDown => "MoveCursorDown",
            Self::MoveCursorWordLeft => "MoveCursorWordLeft",
            Self::MoveCursorLeft => "MoveCursorLeft",
            Self::MoveCursorWordRight => "MoveCursorWordRight",
            Self::MoveCursorRight => "MoveCursorRight",
            Self::MoveCursorHome => "MoveCursorHome",
            Self::MoveCursorEnd => "MoveCursorEnd",
        }
    }

    fn to_composer_action(self) -> ComposerAction {
        match self {
            Self::OpenEditor => ComposerAction::OpenInEditor,
            Self::PasteClipboard => ComposerAction::PasteClipboard,
            Self::InsertNewline => ComposerAction::InsertNewline,
            Self::Submit => ComposerAction::Submit,
            Self::Close => ComposerAction::Close,
            Self::ClearInput => ComposerAction::ClearInput,
            Self::RemoveLastAttachment => ComposerAction::RemoveLastAttachment,
            Self::DeletePreviousChar => ComposerAction::DeletePreviousChar,
            Self::DeletePreviousWord => ComposerAction::DeletePreviousWord,
            Self::MoveCursorUp => ComposerAction::MoveCursorUp,
            Self::MoveCursorDown => ComposerAction::MoveCursorDown,
            Self::MoveCursorWordLeft => ComposerAction::MoveCursorWordLeft,
            Self::MoveCursorLeft => ComposerAction::MoveCursorLeft,
            Self::MoveCursorWordRight => ComposerAction::MoveCursorWordRight,
            Self::MoveCursorRight => ComposerAction::MoveCursorRight,
            Self::MoveCursorHome => ComposerAction::MoveCursorHome,
            Self::MoveCursorEnd => ComposerAction::MoveCursorEnd,
        }
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

fn default_composer_key_bindings() -> BTreeMap<ComposerShortcutAction, Vec<KeyChord>> {
    BTreeMap::from([
        (ComposerShortcutAction::OpenEditor, vec![ctrl_chord('e')]),
        (
            ComposerShortcutAction::PasteClipboard,
            vec![ctrl_chord('v')],
        ),
        (
            ComposerShortcutAction::InsertNewline,
            vec![
                ctrl_chord('j'),
                modified_key_chord(KeyCode::Enter, KeyModifiers::SHIFT),
                modified_key_chord(KeyCode::Enter, KeyModifiers::CONTROL),
                modified_key_chord(KeyCode::Enter, KeyModifiers::ALT),
            ],
        ),
        (
            ComposerShortcutAction::Submit,
            vec![key_chord(KeyCode::Enter)],
        ),
        (ComposerShortcutAction::Close, vec![key_chord(KeyCode::Esc)]),
        (ComposerShortcutAction::ClearInput, vec![ctrl_chord('c')]),
        (
            ComposerShortcutAction::RemoveLastAttachment,
            vec![key_chord(KeyCode::Delete)],
        ),
        (
            ComposerShortcutAction::DeletePreviousChar,
            vec![key_chord(KeyCode::Backspace)],
        ),
        (
            ComposerShortcutAction::DeletePreviousWord,
            vec![
                modified_key_chord(KeyCode::Backspace, KeyModifiers::ALT),
                modified_key_chord(KeyCode::Backspace, KeyModifiers::CONTROL),
                ctrl_chord('w'),
            ],
        ),
        (
            ComposerShortcutAction::MoveCursorUp,
            vec![key_chord(KeyCode::Up)],
        ),
        (
            ComposerShortcutAction::MoveCursorDown,
            vec![key_chord(KeyCode::Down)],
        ),
        (
            ComposerShortcutAction::MoveCursorWordLeft,
            vec![modified_key_chord(KeyCode::Left, KeyModifiers::CONTROL)],
        ),
        (
            ComposerShortcutAction::MoveCursorLeft,
            vec![key_chord(KeyCode::Left)],
        ),
        (
            ComposerShortcutAction::MoveCursorWordRight,
            vec![modified_key_chord(KeyCode::Right, KeyModifiers::CONTROL)],
        ),
        (
            ComposerShortcutAction::MoveCursorRight,
            vec![key_chord(KeyCode::Right)],
        ),
        (
            ComposerShortcutAction::MoveCursorHome,
            vec![key_chord(KeyCode::Home)],
        ),
        (
            ComposerShortcutAction::MoveCursorEnd,
            vec![key_chord(KeyCode::End)],
        ),
    ])
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
