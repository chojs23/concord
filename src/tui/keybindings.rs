use std::{collections::BTreeMap, str::FromStr};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::{
    ChannelActionItem, ChannelActionKind, EmojiReactionItem, FocusPane, GuildActionItem,
    GuildActionKind, MemberActionItem, MemberActionKind, MessageActionItem, MessageActionKind,
};
use crate::{
    config::{KeymapBinding, KeymapOptions},
    discord::{ReactionEmoji, password_auth::MfaMethod},
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KeyBindings {
    keymap: KeyMap,
    action_shortcuts: ActionShortcutBindings,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::tui) struct KeyChord {
    code: KeyCode,
    modifiers: KeyModifiers,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct KeyMap {
    leader: KeyChord,
    root: KeyMapNode,
    specs: BTreeMap<UiAction, KeyMapActionSpec>,
    group_titles: Vec<(Vec<KeyChord>, String)>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct KeyMapNode {
    action: Option<KeyMapAction>,
    children: Vec<KeyMapChild>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct KeyMapAction {
    action: UiAction,
    label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct KeyMapActionSpec {
    sequences: Vec<Vec<KeyChord>>,
    label: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ActionShortcutBindings {
    guild: Vec<ActionShortcutBinding<GuildActionKind>>,
    channel: Vec<ActionShortcutBinding<ChannelActionKind>>,
    member: Vec<ActionShortcutBinding<MemberActionKind>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActionShortcutBinding<K> {
    kind: K,
    shortcuts: Vec<char>,
    description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct KeyMapChild {
    key: KeyChord,
    node: KeyMapNode,
}

const MAX_KEYMAP_MAPPINGS: usize = 128;
const MAX_KEYMAP_SEQUENCE_CHORDS: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct LeaderShortcutItem {
    pub key: String,
    pub label: String,
    pub has_children: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum KeyMapLookup {
    Pending,
    Action(UiAction),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(in crate::tui) enum UiAction {
    StartComposer,
    OpenPaneFilter,
    FocusGuildPane,
    FocusChannelPane,
    FocusMessagePane,
    FocusMemberPane,
    CycleFocusNext,
    CycleFocusPrevious,
    HalfPageDown,
    HalfPageUp,
    JumpTop,
    JumpBottom,
    ScrollHorizontalLeft,
    ScrollHorizontalRight,
    CopyMessage,
    ReactMessage,
    ReplyMessage,
    DeleteMessage,
    EditMessage,
    OpenMessageUrl,
    ViewMessageImage,
    ShowMessageProfile,
    PinMessage,
    ToggleGuildPane,
    ToggleChannelPane,
    ToggleMemberPane,
    OpenFocusedPaneAction,
    OpenOptions,
    ChannelSwitcher,
    OpenDisplayOptions,
    OpenNotificationOptions,
    OpenVoiceOptions,
    VoiceDeafen,
    VoiceMute,
    VoiceLeave,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum SelectionAction {
    Next,
    Previous,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum SelectionKeySet {
    TextSafe,
    Navigation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ScrollAction {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum GlobalAction {
    ToggleDebugLog,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum DashboardAction {
    Select(SelectionAction),
    MessageShortcut(MessageShortcutAction),
    Back,
    Quit,
    StartComposer,
    FocusPane(FocusPane),
    CycleFocusForward,
    CycleFocusBackward,
    OpenFocusedPaneFilter,
    ResizePaneLeft,
    ResizePaneRight,
    HalfPageDown,
    HalfPageUp,
    JumpTop,
    JumpBottom,
    ScrollMessageViewportDown,
    ScrollMessageViewportUp,
    ScrollHorizontalLeft,
    ScrollHorizontalRight,
    ActivateFocused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MessageShortcutAction {
    CopyContent,
    OpenReactionPicker,
    Reply,
    OpenDeleteConfirmation,
    Edit,
    OpenUrl,
    ViewImage,
    ShowProfile,
    OpenPinConfirmation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ChannelSwitcherAction {
    Select(SelectionAction),
    Close,
    ActivateSelected,
    MoveQueryCursorLeft,
    MoveQueryCursorRight,
    DeleteQueryChar,
    InsertQueryChar(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LeaderActionMenuAction {
    BackOrClose,
    Close,
    ActivateShortcut(char),
    UnknownClose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MessageActionMenuAction {
    Close,
    Select(SelectionAction),
    ActivateSelected,
    ActivateShortcut(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MessageConfirmationAction {
    Confirm,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ImageViewerAction {
    Close,
    Previous,
    Next,
    DownloadSelected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ProfilePopupAction {
    Close,
    Scroll(ScrollAction),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum PaneFilterAction {
    Select(SelectionAction),
    Close,
    Confirm,
    DeleteChar,
    MoveCursorLeft,
    MoveCursorRight,
    Ignore,
    InsertChar(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum EmojiReactionPickerAction {
    Select(SelectionAction),
    Close,
    StartFilter,
    DeleteFilterChar,
    InsertFilterChar(char),
    ActivateSelected,
    ActivateShortcut(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum PollVotePickerAction {
    Close,
    Select(SelectionAction),
    ToggleSelected,
    Submit,
    ToggleShortcut(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ReactionUsersPopupAction {
    Close,
    Scroll(ScrollAction),
    PageDown,
    PageUp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum DebugLogPopupAction {
    Close,
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum OptionsPopupAction {
    Close,
    OpenCategory(OptionsCategoryShortcut),
    Select(SelectionAction),
    ToggleSelected,
    AdjustSelected(i8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ComposerAction {
    OpenInEditor,
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
    InsertChar(char),
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ComposerCompletionAction {
    Select(SelectionAction),
    Confirm,
    Cancel,
    FallThrough,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LoginGlobalAction {
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LoginModeSelectAction {
    StartToken,
    StartPassword,
    StartQr,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LoginTextInputAction {
    Submit,
    Back,
    DeletePreviousChar,
    InsertChar(char),
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LoginPasswordInputAction {
    Submit,
    SwitchField,
    Back,
    DeletePreviousChar,
    InsertChar(char),
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LoginMfaSelectAction {
    Choose(MfaMethod),
    Back,
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum LoginBusyAction {
    Cancel,
    Ignore,
}

impl KeyBindings {
    pub fn from_options(keymap_options: &KeymapOptions) -> Self {
        Self {
            keymap: KeyMap::from_options_lossy(keymap_options),
            action_shortcuts: ActionShortcutBindings::from_options_lossy(keymap_options),
        }
    }

    #[cfg(test)]
    fn try_from_options(keymap_options: &KeymapOptions) -> std::result::Result<Self, String> {
        let keymap = KeyMap::try_from_options(keymap_options)?;
        let action_shortcuts = ActionShortcutBindings::try_from_options(keymap_options)?;
        Ok(Self {
            keymap,
            action_shortcuts,
        })
    }
}

impl ActionShortcutBindings {
    fn from_options_lossy(options: &KeymapOptions) -> Self {
        Self {
            guild: parse_action_scope_lossy(
                &options.guild_actions,
                GuildActionKind::from_keymap_name,
            ),
            channel: parse_action_scope_lossy(
                &options.channel_actions,
                ChannelActionKind::from_keymap_name,
            ),
            member: parse_action_scope_lossy(
                &options.member_actions,
                MemberActionKind::from_keymap_name,
            ),
        }
    }

    #[cfg(test)]
    fn try_from_options(options: &KeymapOptions) -> std::result::Result<Self, String> {
        Ok(Self {
            guild: parse_action_scope(
                "keymap.guild_actions",
                &options.guild_actions,
                GuildActionKind::from_keymap_name,
            )?,
            channel: parse_action_scope(
                "keymap.channel_actions",
                &options.channel_actions,
                ChannelActionKind::from_keymap_name,
            )?,
            member: parse_action_scope(
                "keymap.member_actions",
                &options.member_actions,
                MemberActionKind::from_keymap_name,
            )?,
        })
    }
}

impl Default for KeyMap {
    fn default() -> Self {
        let leader = char_chord(' ');
        let specs = default_keymap_specs(leader);
        Self::from_specs(leader, &specs, default_keymap_group_titles(leader))
            .expect("default keymap has no conflicts")
    }
}

impl KeyMap {
    fn from_options_lossy(options: &KeymapOptions) -> Self {
        let Ok(leader) = keymap_leader(options) else {
            return Self::default();
        };
        let group_titles = keymap_group_titles_with_defaults(
            leader,
            parse_keymap_groups_lossy(&options.groups, leader),
        );
        let mut configured_specs = BTreeMap::new();

        for (action_name, binding) in options.mappings.iter().take(MAX_KEYMAP_MAPPINGS) {
            let Some(action) = UiAction::from_keymap_name(action_name) else {
                continue;
            };
            let Some(spec) = parse_keymap_binding_lossy(action_name, action, binding, leader)
            else {
                continue;
            };
            let previous = configured_specs.insert(action, spec);
            if Self::from_specs(leader, &configured_specs, group_titles.clone()).is_err() {
                if let Some(previous) = previous {
                    configured_specs.insert(action, previous);
                } else {
                    configured_specs.remove(&action);
                }
            }
        }

        let mut specs = default_keymap_specs(leader);
        remove_default_keymap_conflicts(&mut specs, &configured_specs);
        specs.extend(configured_specs);
        Self::from_specs(leader, &specs, group_titles).expect("default keymap has no conflicts")
    }

    #[cfg(test)]
    fn try_from_options(options: &KeymapOptions) -> std::result::Result<Self, String> {
        let leader = keymap_leader(options)?;
        if options.mappings.len() > MAX_KEYMAP_MAPPINGS {
            return Err(format!(
                "keymap supports at most {MAX_KEYMAP_MAPPINGS} mappings"
            ));
        }

        let group_titles = keymap_group_titles_with_defaults(
            leader,
            parse_keymap_groups(&options.groups, leader)?,
        );
        let mut configured_specs = BTreeMap::new();
        for (action_name, binding) in &options.mappings {
            let action = UiAction::from_keymap_name(action_name)
                .ok_or_else(|| format!("unknown keymap action `{action_name}`"))?;
            let spec = parse_keymap_binding(action_name, action, binding, leader)?;
            configured_specs.insert(action, spec);
        }
        Self::from_specs(leader, &configured_specs, group_titles.clone())?;

        let mut specs = default_keymap_specs(leader);
        remove_default_keymap_conflicts(&mut specs, &configured_specs);
        specs.extend(configured_specs);
        Self::from_specs(leader, &specs, group_titles)
    }

    fn from_specs(
        leader: KeyChord,
        specs: &BTreeMap<UiAction, KeyMapActionSpec>,
        group_titles: Vec<(Vec<KeyChord>, String)>,
    ) -> std::result::Result<Self, String> {
        let mut keymap = Self {
            leader,
            root: KeyMapNode::default(),
            specs: specs.clone(),
            group_titles,
        };
        for (action, spec) in specs {
            for sequence in &spec.sequences {
                keymap.insert(sequence, *action, spec.label.clone())?;
            }
        }
        Ok(keymap)
    }

    fn insert(
        &mut self,
        sequence: &[KeyChord],
        action: UiAction,
        label: String,
    ) -> std::result::Result<(), String> {
        if sequence.is_empty() {
            return Err(format!("{} keymap cannot be empty", action.name()));
        }

        let mut node = &mut self.root;
        for (position, key) in sequence.iter().enumerate() {
            if node.action.is_some() {
                return Err(format!(
                    "{} conflicts with an existing shorter keymap prefix",
                    action.name()
                ));
            }
            let index = match node.children.iter().position(|child| child.key == *key) {
                Some(index) => index,
                None => {
                    node.children.push(KeyMapChild {
                        key: *key,
                        node: KeyMapNode::default(),
                    });
                    node.children.len() - 1
                }
            };
            node = &mut node.children[index].node;
            if position + 1 == sequence.len() && !node.children.is_empty() {
                return Err(format!(
                    "{} conflicts with an existing longer keymap prefix",
                    action.name()
                ));
            }
        }
        let new_action = KeyMapAction { action, label };
        if let Some(previous) = node.action.replace(new_action)
            && previous.action != action
        {
            return Err(format!(
                "{} conflicts with {}",
                action.name(),
                previous.action.name()
            ));
        }
        Ok(())
    }

    fn lookup(&self, sequence: &[KeyChord]) -> Option<KeyMapLookup> {
        let node = self.node(sequence)?;
        node.action
            .as_ref()
            .map(|action| KeyMapLookup::Action(action.action))
            .or_else(|| (!node.children.is_empty()).then_some(KeyMapLookup::Pending))
    }

    fn children(&self, sequence: &[KeyChord]) -> Vec<LeaderShortcutItem> {
        self.node(sequence)
            .map(|node| {
                node.children
                    .iter()
                    .map(|child| {
                        let mut child_sequence = sequence.to_vec();
                        child_sequence.push(child.key);
                        LeaderShortcutItem {
                            key: child.key.label(),
                            label: child
                                .node
                                .action
                                .as_ref()
                                .map(|action| action.label.clone())
                                .or_else(|| self.group_title(&child_sequence))
                                .unwrap_or_else(|| "prefix".to_owned()),
                            has_children: !child.node.children.is_empty(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn group_title(&self, sequence: &[KeyChord]) -> Option<String> {
        self.group_titles
            .iter()
            .find(|(candidate, _)| candidate.as_slice() == sequence)
            .map(|(_, title)| title.clone())
    }

    fn first_sequence_label(&self, action: UiAction) -> String {
        self.specs
            .get(&action)
            .and_then(|spec| spec.sequences.first())
            .and_then(|sequence| sequence.last())
            .map(|chord| chord.label())
            .unwrap_or_default()
    }

    fn node(&self, sequence: &[KeyChord]) -> Option<&KeyMapNode> {
        let mut node = &self.root;
        for key in sequence {
            let event = KeyEvent::new(key.code, key.modifiers);
            node = &node
                .children
                .iter()
                .find(|child| child.key.matches(event))?
                .node;
        }
        Some(node)
    }
}

fn keymap_leader(options: &KeymapOptions) -> std::result::Result<KeyChord, String> {
    let leader = options
        .leader
        .as_deref()
        .map(KeyChord::from_str)
        .transpose()?
        .unwrap_or_else(|| char_chord(' '))
        .canonical();
    if is_reserved_keymap_chord(leader) {
        return Err(format!("leader: {} is reserved", leader.label()));
    }
    Ok(leader)
}

fn parse_keymap_sequence(
    action_name: &str,
    sequence: &str,
    leader: KeyChord,
) -> std::result::Result<KeySequence, String> {
    let sequence =
        KeySequence::parse(sequence, leader).map_err(|error| format!("{action_name}: {error}"))?;
    if sequence.0.len() > MAX_KEYMAP_SEQUENCE_CHORDS {
        return Err(format!(
            "{action_name}: keymap sequence supports at most {MAX_KEYMAP_SEQUENCE_CHORDS} keys"
        ));
    }
    if sequence.0.len() == 1 {
        if sequence.0.first().copied() == Some(leader) {
            return Err(format!(
                "{action_name}: single-key keymap sequences cannot use <leader>"
            ));
        }
        return Ok(sequence);
    }
    Ok(sequence)
}

fn parse_keymap_binding_lossy(
    action_name: &str,
    action: UiAction,
    binding: &KeymapBinding,
    leader: KeyChord,
) -> Option<KeyMapActionSpec> {
    let sequences = binding
        .keys
        .iter()
        .filter_map(|sequence| parse_keymap_sequence(action_name, sequence, leader).ok())
        .map(|sequence| sequence.0)
        .collect::<Vec<_>>();
    (!sequences.is_empty()).then(|| KeyMapActionSpec {
        sequences,
        label: binding
            .description
            .clone()
            .unwrap_or_else(|| action.label().to_owned()),
    })
}

#[cfg(test)]
fn parse_keymap_binding(
    action_name: &str,
    action: UiAction,
    binding: &KeymapBinding,
    leader: KeyChord,
) -> std::result::Result<KeyMapActionSpec, String> {
    let mut sequences = Vec::new();
    for sequence in &binding.keys {
        sequences.push(parse_keymap_sequence(action_name, sequence, leader)?.0);
    }
    if sequences.is_empty() {
        return Err(format!(
            "{action_name}: keymap entry must include at least one key"
        ));
    }
    Ok(KeyMapActionSpec {
        sequences,
        label: binding
            .description
            .clone()
            .unwrap_or_else(|| action.label().to_owned()),
    })
}

fn parse_keymap_groups_lossy(
    groups: &BTreeMap<String, String>,
    leader: KeyChord,
) -> Vec<(Vec<KeyChord>, String)> {
    groups
        .iter()
        .filter_map(|(sequence, title)| {
            parse_keymap_group(sequence, title, leader)
                .ok()
                .map(|(sequence, title)| (sequence.0, title))
        })
        .collect()
}

fn default_keymap_group_titles(leader: KeyChord) -> Vec<(Vec<KeyChord>, String)> {
    vec![(vec![leader, char_chord('v')], "Voice".to_owned())]
}

fn keymap_group_titles_with_defaults(
    leader: KeyChord,
    configured: Vec<(Vec<KeyChord>, String)>,
) -> Vec<(Vec<KeyChord>, String)> {
    let mut titles = default_keymap_group_titles(leader);
    for (sequence, title) in configured {
        if let Some((_, existing)) = titles
            .iter_mut()
            .find(|(candidate, _)| candidate == &sequence)
        {
            *existing = title;
        } else {
            titles.push((sequence, title));
        }
    }
    titles
}

#[cfg(test)]
fn parse_keymap_groups(
    groups: &BTreeMap<String, String>,
    leader: KeyChord,
) -> std::result::Result<Vec<(Vec<KeyChord>, String)>, String> {
    let mut parsed = Vec::new();
    for (sequence, title) in groups {
        let (sequence, title) = parse_keymap_group(sequence, title, leader)?;
        parsed.push((sequence.0, title));
    }
    Ok(parsed)
}

fn parse_keymap_group(
    sequence: &str,
    title: &str,
    leader: KeyChord,
) -> std::result::Result<(KeySequence, String), String> {
    let sequence = KeySequence::parse(sequence, leader)
        .map_err(|error| format!("keymap group `{sequence}`: {error}"))?;
    if sequence.0.is_empty() {
        return Err("keymap group cannot be empty".to_owned());
    }
    Ok((sequence, title.to_owned()))
}

fn parse_action_scope_lossy<K: Copy + Eq>(
    bindings: &BTreeMap<String, KeymapBinding>,
    parse_kind: fn(&str) -> Option<K>,
) -> Vec<ActionShortcutBinding<K>> {
    let mut parsed = Vec::new();
    for (action_name, binding) in bindings.iter().take(MAX_KEYMAP_MAPPINGS) {
        let Some(kind) = parse_kind(action_name) else {
            continue;
        };
        let Some(binding) = parse_action_shortcut_binding_lossy(binding) else {
            continue;
        };
        parsed.retain(|item: &ActionShortcutBinding<K>| item.kind != kind);
        parsed.push(ActionShortcutBinding {
            kind,
            shortcuts: binding.0,
            description: binding.1,
        });
    }
    parsed
}

#[cfg(test)]
fn parse_action_scope<K: Copy + Eq>(
    scope_name: &str,
    bindings: &BTreeMap<String, KeymapBinding>,
    parse_kind: fn(&str) -> Option<K>,
) -> std::result::Result<Vec<ActionShortcutBinding<K>>, String> {
    if bindings.len() > MAX_KEYMAP_MAPPINGS {
        return Err(format!(
            "{scope_name} supports at most {MAX_KEYMAP_MAPPINGS} mappings"
        ));
    }
    let mut parsed = Vec::new();
    for (action_name, binding) in bindings {
        let kind = parse_kind(action_name)
            .ok_or_else(|| format!("unknown {scope_name} action `{action_name}`"))?;
        let (shortcuts, description) = parse_action_shortcut_binding(action_name, binding)?;
        parsed.retain(|item: &ActionShortcutBinding<K>| item.kind != kind);
        parsed.push(ActionShortcutBinding {
            kind,
            shortcuts,
            description,
        });
    }
    Ok(parsed)
}

fn parse_action_shortcut_binding_lossy(
    binding: &KeymapBinding,
) -> Option<(Vec<char>, Option<String>)> {
    let shortcuts = binding
        .keys
        .iter()
        .filter_map(|key| parse_action_shortcut_key(key).ok())
        .collect::<Vec<_>>();
    (!shortcuts.is_empty()).then(|| (shortcuts, binding.description.clone()))
}

#[cfg(test)]
fn parse_action_shortcut_binding(
    action_name: &str,
    binding: &KeymapBinding,
) -> std::result::Result<(Vec<char>, Option<String>), String> {
    let mut shortcuts = Vec::new();
    for key in &binding.keys {
        shortcuts.push(
            parse_action_shortcut_key(key).map_err(|error| format!("{action_name}: {error}"))?,
        );
    }
    if shortcuts.is_empty() {
        return Err(format!(
            "{action_name}: action shortcut entry must include at least one key"
        ));
    }
    Ok((shortcuts, binding.description.clone()))
}

fn parse_action_shortcut_key(value: &str) -> std::result::Result<char, String> {
    let sequence = KeySequence::parse(value, char_chord(' '))?;
    let [key] = sequence.0.as_slice() else {
        return Err("action shortcut must be a single key".to_owned());
    };
    if !key.modifiers.is_empty() {
        return Err("action shortcut cannot use modifiers".to_owned());
    }
    match key.code {
        KeyCode::Char(value) if !value.is_whitespace() => Ok(value.to_ascii_lowercase()),
        _ => Err("action shortcut must be a character key".to_owned()),
    }
}

struct KeySequence(Vec<KeyChord>);

impl KeySequence {
    fn parse(value: &str, leader: KeyChord) -> std::result::Result<Self, String> {
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

fn parse_sequence_token(
    token: &str,
    leader: KeyChord,
) -> std::result::Result<Vec<KeyChord>, String> {
    let token = token.trim();
    if token.is_empty() {
        return Ok(Vec::new());
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
            keys.extend(segment.chars().map(char_chord));
            rest = &rest[next_angle..];
        }
    }
    Ok(keys)
}

fn parse_plain_sequence_token(token: &str) -> std::result::Result<Vec<KeyChord>, String> {
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

fn parse_angle_key(value: &str) -> std::result::Result<KeyChord, String> {
    let parts = value.split('-').map(str::trim).collect::<Vec<_>>();
    let Some((key, modifier_parts)) = parts.split_last() else {
        return KeyChord::from_str(value);
    };
    if modifier_parts.is_empty() {
        return KeyChord::from_str(value);
    }

    let mut modifiers = KeyModifiers::empty();
    for modifier in modifier_parts {
        match modifier.to_ascii_lowercase().as_str() {
            "c" | "ctrl" | "control" => modifiers.insert(KeyModifiers::CONTROL),
            "s" | "shift" => modifiers.insert(KeyModifiers::SHIFT),
            "a" | "alt" | "m" | "meta" => modifiers.insert(KeyModifiers::ALT),
            _ => return KeyChord::from_str(value),
        }
    }

    let code = parse_key_code(key)?;
    Ok(KeyChord {
        code,
        modifiers: normalized_modifiers(modifiers),
    })
}

impl UiAction {
    pub(in crate::tui) fn from_name(name: &str) -> Option<Self> {
        all_ui_actions()
            .iter()
            .copied()
            .find(|action| action.name() == name)
    }

    fn from_keymap_name(name: &str) -> Option<Self> {
        Self::from_name(name)
    }

    pub(in crate::tui) fn name(self) -> &'static str {
        match self {
            UiAction::StartComposer => "StartComposer",
            UiAction::OpenPaneFilter => "OpenPaneFilter",
            UiAction::FocusGuildPane => "FocusGuildPane",
            UiAction::FocusChannelPane => "FocusChannelPane",
            UiAction::FocusMessagePane => "FocusMessagePane",
            UiAction::FocusMemberPane => "FocusMemberPane",
            UiAction::CycleFocusNext => "CycleFocusNext",
            UiAction::CycleFocusPrevious => "CycleFocusPrevious",
            UiAction::HalfPageDown => "HalfPageDown",
            UiAction::HalfPageUp => "HalfPageUp",
            UiAction::JumpTop => "JumpTop",
            UiAction::JumpBottom => "JumpBottom",
            UiAction::ScrollHorizontalLeft => "ScrollHorizontalLeft",
            UiAction::ScrollHorizontalRight => "ScrollHorizontalRight",
            UiAction::CopyMessage => "CopyMessage",
            UiAction::ReactMessage => "ReactMessage",
            UiAction::ReplyMessage => "ReplyMessage",
            UiAction::DeleteMessage => "DeleteMessage",
            UiAction::EditMessage => "EditMessage",
            UiAction::OpenMessageUrl => "OpenMessageUrl",
            UiAction::ViewMessageImage => "ViewMessageImage",
            UiAction::ShowMessageProfile => "ShowMessageProfile",
            UiAction::PinMessage => "PinMessage",
            UiAction::ToggleGuildPane => "ToggleGuildPane",
            UiAction::ToggleChannelPane => "ToggleChannelPane",
            UiAction::ToggleMemberPane => "ToggleMemberPane",
            UiAction::OpenFocusedPaneAction => "OpenFocusedPaneAction",
            UiAction::OpenOptions => "OpenOptions",
            UiAction::ChannelSwitcher => "ChannelSwitcher",
            UiAction::OpenDisplayOptions => "OpenDisplayOptions",
            UiAction::OpenNotificationOptions => "OpenNotificationOptions",
            UiAction::OpenVoiceOptions => "OpenVoiceOptions",
            UiAction::VoiceDeafen => "VoiceDeafen",
            UiAction::VoiceMute => "VoiceMute",
            UiAction::VoiceLeave => "VoiceLeave",
        }
    }

    fn label(self) -> &'static str {
        match self {
            UiAction::StartComposer => "start composer",
            UiAction::OpenPaneFilter => "filter pane",
            UiAction::FocusGuildPane => "focus Servers",
            UiAction::FocusChannelPane => "focus Channels",
            UiAction::FocusMessagePane => "focus Messages",
            UiAction::FocusMemberPane => "focus Members",
            UiAction::CycleFocusNext => "focus next",
            UiAction::CycleFocusPrevious => "focus previous",
            UiAction::HalfPageDown => "half page down",
            UiAction::HalfPageUp => "half page up",
            UiAction::JumpTop => "jump top",
            UiAction::JumpBottom => "jump bottom",
            UiAction::ScrollHorizontalLeft => "scroll left",
            UiAction::ScrollHorizontalRight => "scroll right",
            UiAction::CopyMessage => "copy message",
            UiAction::ReactMessage => "react",
            UiAction::ReplyMessage => "reply",
            UiAction::DeleteMessage => "delete message",
            UiAction::EditMessage => "edit message",
            UiAction::OpenMessageUrl => "open URL",
            UiAction::ViewMessageImage => "view image",
            UiAction::ShowMessageProfile => "show profile",
            UiAction::PinMessage => "pin message",
            UiAction::ToggleGuildPane => "toggle Servers",
            UiAction::ToggleChannelPane => "toggle Channels",
            UiAction::ToggleMemberPane => "toggle Members",
            UiAction::OpenFocusedPaneAction => "Actions",
            UiAction::OpenOptions => "Options",
            UiAction::ChannelSwitcher => "Switch channels",
            UiAction::OpenDisplayOptions => "Display options",
            UiAction::OpenNotificationOptions => "Notification options",
            UiAction::OpenVoiceOptions => "Voice options",
            UiAction::VoiceDeafen => "deafen voice",
            UiAction::VoiceMute => "mute voice",
            UiAction::VoiceLeave => "leave voice",
        }
    }
}

impl GuildActionKind {
    fn from_keymap_name(name: &str) -> Option<Self> {
        match name {
            "MarkAsRead" => Some(Self::MarkAsRead),
            "MuteServer" | "ToggleMute" => Some(Self::ToggleMute),
            _ => None,
        }
    }
}

impl ChannelActionKind {
    fn from_keymap_name(name: &str) -> Option<Self> {
        match name {
            "JoinVoice" => Some(Self::JoinVoice),
            "LeaveVoice" => Some(Self::LeaveVoice),
            "ShowPinnedMessages" | "LoadPinnedMessages" => Some(Self::LoadPinnedMessages),
            "ShowThreads" => Some(Self::ShowThreads),
            "MarkAsRead" => Some(Self::MarkAsRead),
            "MuteChannel" | "ToggleMute" => Some(Self::ToggleMute),
            _ => None,
        }
    }
}

impl MemberActionKind {
    fn from_keymap_name(name: &str) -> Option<Self> {
        match name {
            "ShowProfile" => Some(Self::ShowProfile),
            _ => None,
        }
    }
}

fn all_ui_actions() -> &'static [UiAction] {
    &[
        UiAction::StartComposer,
        UiAction::OpenPaneFilter,
        UiAction::FocusGuildPane,
        UiAction::FocusChannelPane,
        UiAction::FocusMessagePane,
        UiAction::FocusMemberPane,
        UiAction::CycleFocusNext,
        UiAction::CycleFocusPrevious,
        UiAction::HalfPageDown,
        UiAction::HalfPageUp,
        UiAction::JumpTop,
        UiAction::JumpBottom,
        UiAction::ScrollHorizontalLeft,
        UiAction::ScrollHorizontalRight,
        UiAction::CopyMessage,
        UiAction::ReactMessage,
        UiAction::ReplyMessage,
        UiAction::DeleteMessage,
        UiAction::EditMessage,
        UiAction::OpenMessageUrl,
        UiAction::ViewMessageImage,
        UiAction::ShowMessageProfile,
        UiAction::PinMessage,
        UiAction::ToggleGuildPane,
        UiAction::ToggleChannelPane,
        UiAction::ToggleMemberPane,
        UiAction::OpenFocusedPaneAction,
        UiAction::OpenOptions,
        UiAction::ChannelSwitcher,
        UiAction::OpenDisplayOptions,
        UiAction::OpenNotificationOptions,
        UiAction::OpenVoiceOptions,
        UiAction::VoiceDeafen,
        UiAction::VoiceMute,
        UiAction::VoiceLeave,
    ]
}

impl KeyChord {
    fn matches(self, key: KeyEvent) -> bool {
        let expected = self.canonical();
        let actual = Self {
            code: key.code,
            modifiers: key.modifiers,
        }
        .canonical();

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

    fn canonical(self) -> Self {
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

    fn label(self) -> String {
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

    fn title_label(self) -> String {
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

impl FromStr for KeyChord {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err("keybinding cannot be empty".to_owned());
        }

        let mut modifiers = KeyModifiers::empty();
        let parts = value.split('+').map(str::trim).collect::<Vec<_>>();
        let Some((key, modifier_parts)) = parts.split_last() else {
            return Err("keybinding cannot be empty".to_owned());
        };
        for modifier in modifier_parts {
            match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.insert(KeyModifiers::CONTROL),
                "alt" => modifiers.insert(KeyModifiers::ALT),
                "shift" => modifiers.insert(KeyModifiers::SHIFT),
                unknown => return Err(format!("unsupported key modifier `{unknown}`")),
            }
        }

        let code = parse_key_code(key)?;
        Ok(Self {
            code,
            modifiers: normalized_modifiers(modifiers),
        })
    }
}

fn parse_key_code(value: &str) -> std::result::Result<KeyCode, String> {
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

fn normalized_modifiers(modifiers: KeyModifiers) -> KeyModifiers {
    modifiers & (KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT)
}

fn is_reserved_keymap_chord(chord: KeyChord) -> bool {
    matches!(
        chord.code,
        KeyCode::Enter | KeyCode::Esc | KeyCode::Backspace | KeyCode::Delete
    ) || matches!(
        (chord.code, chord.modifiers),
        (KeyCode::Char('c'), KeyModifiers::CONTROL)
    )
}

fn default_keymap_specs(leader: KeyChord) -> BTreeMap<UiAction, KeyMapActionSpec> {
    let mut specs = BTreeMap::new();
    for action in all_ui_actions() {
        let action_sequences = match *action {
            UiAction::StartComposer => vec![vec![char_chord('i')]],
            UiAction::OpenPaneFilter => vec![vec![char_chord('/')]],
            UiAction::FocusGuildPane => vec![vec![char_chord('1')]],
            UiAction::FocusChannelPane => vec![vec![char_chord('2')]],
            UiAction::FocusMessagePane => vec![vec![char_chord('3')]],
            UiAction::FocusMemberPane => vec![vec![char_chord('4')]],
            UiAction::CycleFocusNext => vec![
                vec![key_chord(KeyCode::Tab)],
                vec![char_chord('l')],
                vec![key_chord(KeyCode::Right)],
            ],
            UiAction::CycleFocusPrevious => vec![
                vec![KeyChord {
                    code: KeyCode::Tab,
                    modifiers: KeyModifiers::SHIFT,
                }],
                vec![char_chord('h')],
                vec![key_chord(KeyCode::Left)],
            ],
            UiAction::HalfPageDown => vec![vec![ctrl_chord('d')]],
            UiAction::HalfPageUp => vec![vec![ctrl_chord('u')]],
            UiAction::JumpTop => vec![vec![char_chord('g')]],
            UiAction::JumpBottom => vec![vec![char_chord('G')]],
            UiAction::ScrollHorizontalLeft => vec![vec![char_chord('H')]],
            UiAction::ScrollHorizontalRight => vec![vec![char_chord('L')]],
            UiAction::CopyMessage => vec![vec![char_chord('y')]],
            UiAction::ReactMessage => vec![vec![char_chord('r')]],
            UiAction::ReplyMessage => vec![vec![char_chord('R')]],
            UiAction::DeleteMessage => vec![vec![char_chord('d')]],
            UiAction::EditMessage => vec![vec![char_chord('e')]],
            UiAction::OpenMessageUrl => vec![vec![char_chord('o')]],
            UiAction::ViewMessageImage => vec![vec![char_chord('v')]],
            UiAction::ShowMessageProfile => vec![vec![char_chord('p')]],
            UiAction::PinMessage => vec![vec![char_chord('P')]],
            UiAction::ToggleGuildPane => vec![vec![leader, char_chord('1')]],
            UiAction::ToggleChannelPane => vec![vec![leader, char_chord('2')]],
            UiAction::ToggleMemberPane => vec![vec![leader, char_chord('4')]],
            UiAction::OpenFocusedPaneAction => vec![vec![leader, char_chord('a')]],
            UiAction::OpenOptions => vec![vec![leader, char_chord('o')]],
            UiAction::ChannelSwitcher => vec![vec![leader, leader]],
            UiAction::OpenDisplayOptions
            | UiAction::OpenNotificationOptions
            | UiAction::OpenVoiceOptions => Vec::new(),
            UiAction::VoiceDeafen => vec![vec![leader, char_chord('v'), char_chord('d')]],
            UiAction::VoiceMute => vec![vec![leader, char_chord('v'), char_chord('m')]],
            UiAction::VoiceLeave => vec![vec![leader, char_chord('v'), char_chord('l')]],
        };
        if !action_sequences.is_empty() {
            specs.insert(
                *action,
                KeyMapActionSpec {
                    sequences: action_sequences,
                    label: action.label().to_owned(),
                },
            );
        }
    }
    specs
}

fn remove_default_keymap_conflicts(
    defaults: &mut BTreeMap<UiAction, KeyMapActionSpec>,
    configured: &BTreeMap<UiAction, KeyMapActionSpec>,
) {
    defaults.retain(|default_action, default_spec| {
        if configured.contains_key(default_action) {
            return false;
        }
        default_spec.sequences.retain(|default_sequence| {
            !configured.values().any(|configured_spec| {
                configured_spec.sequences.iter().any(|configured_sequence| {
                    keymap_sequences_conflict(default_sequence, configured_sequence)
                })
            })
        });
        !default_spec.sequences.is_empty()
    });
}

fn keymap_sequences_conflict(left: &[KeyChord], right: &[KeyChord]) -> bool {
    let left = canonical_keymap_sequence(left);
    let right = canonical_keymap_sequence(right);
    left.starts_with(&right) || right.starts_with(&left)
}

fn canonical_keymap_sequence(sequence: &[KeyChord]) -> Vec<KeyChord> {
    sequence.iter().map(|chord| chord.canonical()).collect()
}

fn key_chord(code: KeyCode) -> KeyChord {
    KeyChord {
        code,
        modifiers: KeyModifiers::NONE,
    }
}

fn char_chord(value: char) -> KeyChord {
    key_chord(KeyCode::Char(value))
}

fn ctrl_chord(value: char) -> KeyChord {
    KeyChord {
        code: KeyCode::Char(value),
        modifiers: KeyModifiers::CONTROL,
    }
}

fn key_code_label(code: KeyCode) -> String {
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

impl KeyBindings {
    fn binding_label(&self, action: UiAction) -> String {
        self.keymap.first_sequence_label(action)
    }

    pub(in crate::tui) fn leader_keymap_prefix(&self) -> Vec<KeyChord> {
        vec![self.keymap.leader]
    }

    pub(in crate::tui) fn is_leader_key(&self, key: KeyEvent) -> bool {
        self.keymap.leader.matches(key)
    }

    #[cfg(test)]
    pub(in crate::tui) fn keymap_lookup_direct_key(&self, key: KeyEvent) -> Option<UiAction> {
        let sequence = [self.keymap_chord_for_event(key)];
        match self.keymap.lookup(&sequence) {
            Some(KeyMapLookup::Action(action)) => Some(action),
            _ => None,
        }
    }

    pub(in crate::tui) fn keymap_lookup_root_key(&self, key: KeyEvent) -> Option<KeyMapLookup> {
        let sequence = [self.keymap_chord_for_event(key)];
        self.keymap.lookup(&sequence)
    }

    pub(in crate::tui) fn keymap_lookup_with_key(
        &self,
        prefix: &[KeyChord],
        key: KeyEvent,
    ) -> Option<KeyMapLookup> {
        let mut sequence = prefix.to_vec();
        sequence.push(
            KeyChord {
                code: key.code,
                modifiers: key.modifiers,
            }
            .canonical(),
        );
        self.keymap.lookup(&sequence)
    }

    pub(in crate::tui) fn keymap_chord_for_event(&self, key: KeyEvent) -> KeyChord {
        KeyChord {
            code: key.code,
            modifiers: key.modifiers,
        }
        .canonical()
    }

    pub(in crate::tui) fn keymap_prefix_title(&self, prefix: &[KeyChord]) -> String {
        if let Some((_, title)) = self
            .keymap
            .group_titles
            .iter()
            .find(|(sequence, _)| sequence.as_slice() == prefix)
        {
            return title.clone();
        }
        if prefix == self.leader_keymap_prefix() {
            return "Leader".to_owned();
        }
        prefix.iter().map(|chord| chord.title_label()).collect()
    }

    pub(in crate::tui) fn leader_keymap_children(
        &self,
        prefix: &[KeyChord],
    ) -> Vec<LeaderShortcutItem> {
        self.keymap.children(prefix)
    }

    pub(in crate::tui) fn dashboard_action_for_ui_action(
        &self,
        action: UiAction,
        focus: FocusPane,
    ) -> Option<DashboardAction> {
        match action {
            UiAction::StartComposer => Some(DashboardAction::StartComposer),
            UiAction::OpenPaneFilter => Some(DashboardAction::OpenFocusedPaneFilter),
            UiAction::FocusGuildPane => Some(DashboardAction::FocusPane(FocusPane::Guilds)),
            UiAction::FocusChannelPane => Some(DashboardAction::FocusPane(FocusPane::Channels)),
            UiAction::FocusMessagePane => Some(DashboardAction::FocusPane(FocusPane::Messages)),
            UiAction::FocusMemberPane => Some(DashboardAction::FocusPane(FocusPane::Members)),
            UiAction::CycleFocusNext => Some(DashboardAction::CycleFocusForward),
            UiAction::CycleFocusPrevious => Some(DashboardAction::CycleFocusBackward),
            UiAction::HalfPageDown => Some(DashboardAction::HalfPageDown),
            UiAction::HalfPageUp => Some(DashboardAction::HalfPageUp),
            UiAction::JumpTop => Some(DashboardAction::JumpTop),
            UiAction::JumpBottom => Some(DashboardAction::JumpBottom),
            UiAction::ScrollHorizontalLeft => Some(DashboardAction::ScrollHorizontalLeft),
            UiAction::ScrollHorizontalRight => Some(DashboardAction::ScrollHorizontalRight),
            UiAction::CopyMessage if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::CopyContent),
            ),
            UiAction::ReactMessage if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::OpenReactionPicker),
            ),
            UiAction::ReplyMessage if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::Reply),
            ),
            UiAction::DeleteMessage if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::OpenDeleteConfirmation),
            ),
            UiAction::EditMessage if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::Edit),
            ),
            UiAction::OpenMessageUrl if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::OpenUrl),
            ),
            UiAction::ViewMessageImage if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::ViewImage),
            ),
            UiAction::ShowMessageProfile if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::ShowProfile),
            ),
            UiAction::PinMessage if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::OpenPinConfirmation),
            ),
            _ => None,
        }
    }

    pub(in crate::tui) fn dashboard_action(
        &self,
        key: KeyEvent,
        focus: FocusPane,
    ) -> Option<DashboardAction> {
        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            return Some(DashboardAction::Select(action));
        }

        match key.code {
            KeyCode::Esc => Some(DashboardAction::Back),
            KeyCode::Char('q') => Some(DashboardAction::Quit),
            KeyCode::Char('h') | KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(DashboardAction::ResizePaneLeft)
            }
            KeyCode::Char('l') | KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(DashboardAction::ResizePaneRight)
            }
            KeyCode::Char('J') if focus == FocusPane::Messages => {
                Some(DashboardAction::ScrollMessageViewportDown)
            }
            KeyCode::Char('K') if focus == FocusPane::Messages => {
                Some(DashboardAction::ScrollMessageViewportUp)
            }
            KeyCode::Enter => Some(DashboardAction::ActivateFocused),
            _ => None,
        }
    }

    pub(in crate::tui) fn global_action(&self, key: KeyEvent) -> Option<GlobalAction> {
        match key.code {
            KeyCode::Char('`') => Some(GlobalAction::ToggleDebugLog),
            _ => None,
        }
    }

    pub(in crate::tui) fn channel_switcher_action(
        &self,
        key: KeyEvent,
    ) -> Option<ChannelSwitcherAction> {
        if let Some(action) = self.selection_action(key, SelectionKeySet::TextSafe) {
            return Some(ChannelSwitcherAction::Select(action));
        }

        match key.code {
            KeyCode::Esc => Some(ChannelSwitcherAction::Close),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(ChannelSwitcherAction::Close)
            }
            KeyCode::Enter => Some(ChannelSwitcherAction::ActivateSelected),
            KeyCode::Left => Some(ChannelSwitcherAction::MoveQueryCursorLeft),
            KeyCode::Right => Some(ChannelSwitcherAction::MoveQueryCursorRight),
            KeyCode::Backspace => Some(ChannelSwitcherAction::DeleteQueryChar),
            KeyCode::Char(value) if is_shortcut_key(key) => {
                Some(ChannelSwitcherAction::InsertQueryChar(value))
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn leader_action_menu_action(
        &self,
        key: KeyEvent,
    ) -> LeaderActionMenuAction {
        match key.code {
            KeyCode::Esc => LeaderActionMenuAction::BackOrClose,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                LeaderActionMenuAction::Close
            }
            KeyCode::Char(shortcut) if is_shortcut_key(key) => {
                LeaderActionMenuAction::ActivateShortcut(shortcut)
            }
            code if is_left_key(code) => LeaderActionMenuAction::BackOrClose,
            _ => LeaderActionMenuAction::UnknownClose,
        }
    }

    pub(in crate::tui) fn message_action_menu_action(
        &self,
        key: KeyEvent,
    ) -> Option<MessageActionMenuAction> {
        if key.code == KeyCode::Esc {
            return Some(MessageActionMenuAction::Close);
        }
        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            return Some(MessageActionMenuAction::Select(action));
        }

        match key.code {
            code if is_confirm_key(code) => Some(MessageActionMenuAction::ActivateSelected),
            KeyCode::Char(shortcut) if is_shortcut_key(key) => {
                Some(MessageActionMenuAction::ActivateShortcut(shortcut))
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn message_confirmation_action(
        &self,
        key: KeyEvent,
    ) -> Option<MessageConfirmationAction> {
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') if is_shortcut_key(key) => {
                Some(MessageConfirmationAction::Confirm)
            }
            KeyCode::Esc | KeyCode::Char('n') if is_shortcut_key(key) => {
                Some(MessageConfirmationAction::Cancel)
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn image_viewer_action(&self, key: KeyEvent) -> Option<ImageViewerAction> {
        match key.code {
            KeyCode::Esc => Some(ImageViewerAction::Close),
            code if is_left_key(code) => Some(ImageViewerAction::Previous),
            code if is_right_key(code) => Some(ImageViewerAction::Next),
            KeyCode::Char('d') if is_shortcut_key(key) => Some(ImageViewerAction::DownloadSelected),
            _ => None,
        }
    }

    pub(in crate::tui) fn profile_popup_action(&self, key: KeyEvent) -> Option<ProfilePopupAction> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Some(ProfilePopupAction::Close),
            _ => self.scroll_action(key).map(ProfilePopupAction::Scroll),
        }
    }

    pub(in crate::tui) fn pane_filter_action(&self, key: KeyEvent) -> Option<PaneFilterAction> {
        if let Some(action) = self.selection_action(key, SelectionKeySet::TextSafe) {
            return Some(PaneFilterAction::Select(action));
        }

        match key.code {
            KeyCode::Esc => Some(PaneFilterAction::Close),
            KeyCode::Enter => Some(PaneFilterAction::Confirm),
            KeyCode::Backspace => Some(PaneFilterAction::DeleteChar),
            KeyCode::Left => Some(PaneFilterAction::MoveCursorLeft),
            KeyCode::Right => Some(PaneFilterAction::MoveCursorRight),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(PaneFilterAction::Ignore)
            }
            KeyCode::Char(value) if is_shortcut_key(key) => {
                Some(PaneFilterAction::InsertChar(value))
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn emoji_reaction_picker_action(
        &self,
        key: KeyEvent,
        filtering: bool,
    ) -> Option<EmojiReactionPickerAction> {
        let key_set = if filtering {
            SelectionKeySet::TextSafe
        } else {
            SelectionKeySet::Navigation
        };
        if let Some(action) = self.selection_action(key, key_set) {
            return Some(EmojiReactionPickerAction::Select(action));
        }

        match key.code {
            KeyCode::Esc => Some(EmojiReactionPickerAction::Close),
            KeyCode::Backspace if filtering => Some(EmojiReactionPickerAction::DeleteFilterChar),
            KeyCode::Char('/') if is_shortcut_key(key) && !filtering => {
                Some(EmojiReactionPickerAction::StartFilter)
            }
            KeyCode::Char(value) if is_shortcut_key(key) && filtering => {
                Some(EmojiReactionPickerAction::InsertFilterChar(value))
            }
            code if is_confirm_key(code) => Some(EmojiReactionPickerAction::ActivateSelected),
            KeyCode::Char(shortcut) if is_shortcut_key(key) => {
                Some(EmojiReactionPickerAction::ActivateShortcut(shortcut))
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn poll_vote_picker_action(
        &self,
        key: KeyEvent,
    ) -> Option<PollVotePickerAction> {
        if key.code == KeyCode::Esc {
            return Some(PollVotePickerAction::Close);
        }
        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            return Some(PollVotePickerAction::Select(action));
        }

        match key.code {
            KeyCode::Char(' ') => Some(PollVotePickerAction::ToggleSelected),
            KeyCode::Enter => Some(PollVotePickerAction::Submit),
            KeyCode::Char(shortcut) if is_shortcut_key(key) => {
                Some(PollVotePickerAction::ToggleShortcut(shortcut))
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn reaction_users_popup_action(
        &self,
        key: KeyEvent,
    ) -> Option<ReactionUsersPopupAction> {
        if key.code == KeyCode::Esc {
            return Some(ReactionUsersPopupAction::Close);
        }
        if let Some(action) = self.scroll_action(key) {
            return Some(ReactionUsersPopupAction::Scroll(action));
        }

        match key.code {
            KeyCode::PageDown => Some(ReactionUsersPopupAction::PageDown),
            KeyCode::PageUp => Some(ReactionUsersPopupAction::PageUp),
            _ => None,
        }
    }

    pub(in crate::tui) fn debug_log_popup_action(&self, key: KeyEvent) -> DebugLogPopupAction {
        match key.code {
            KeyCode::Esc | KeyCode::Char('`') => DebugLogPopupAction::Close,
            _ => DebugLogPopupAction::Ignore,
        }
    }

    pub(in crate::tui) fn options_popup_action(
        &self,
        key: KeyEvent,
        category_picker_open: bool,
    ) -> Option<OptionsPopupAction> {
        if matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('o')
        ) {
            return Some(OptionsPopupAction::Close);
        }
        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            return Some(OptionsPopupAction::Select(action));
        }
        match key.code {
            KeyCode::Char(shortcut @ ('d' | 'D' | 'n' | 'N' | 'v' | 'V'))
                if is_shortcut_key(key) && category_picker_open =>
            {
                self.options_category_shortcut(shortcut)
                    .map(OptionsPopupAction::OpenCategory)
            }
            KeyCode::Char('h') | KeyCode::Char('H') if is_shortcut_key(key) => Some(
                OptionsPopupAction::AdjustSelected(if key.code == KeyCode::Char('H') {
                    -10
                } else {
                    -1
                }),
            ),
            KeyCode::Char('l') | KeyCode::Char('L') if is_shortcut_key(key) => Some(
                OptionsPopupAction::AdjustSelected(if key.code == KeyCode::Char('L') {
                    10
                } else {
                    1
                }),
            ),
            code if is_confirm_key(code) => Some(OptionsPopupAction::ToggleSelected),
            _ => None,
        }
    }

    pub(in crate::tui) fn composer_action(&self, key: KeyEvent) -> ComposerAction {
        match key.code {
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ComposerAction::OpenInEditor
            }
            KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ComposerAction::PasteClipboard
            }
            _ if is_composer_newline_key(key) => ComposerAction::InsertNewline,
            KeyCode::Enter => ComposerAction::Submit,
            KeyCode::Esc => ComposerAction::Close,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ComposerAction::ClearInput
            }
            KeyCode::Backspace if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ComposerAction::DeletePreviousWord
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ComposerAction::DeletePreviousWord
            }
            KeyCode::Backspace => ComposerAction::DeletePreviousChar,
            KeyCode::Delete => ComposerAction::RemoveLastAttachment,
            KeyCode::Up => ComposerAction::MoveCursorUp,
            KeyCode::Down => ComposerAction::MoveCursorDown,
            KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ComposerAction::MoveCursorWordLeft
            }
            KeyCode::Left => ComposerAction::MoveCursorLeft,
            KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
                ComposerAction::MoveCursorWordRight
            }
            KeyCode::Right => ComposerAction::MoveCursorRight,
            KeyCode::Home => ComposerAction::MoveCursorHome,
            KeyCode::End => ComposerAction::MoveCursorEnd,
            KeyCode::Char(value) if is_shortcut_key(key) => ComposerAction::InsertChar(value),
            _ => ComposerAction::Ignore,
        }
    }

    pub(in crate::tui) fn composer_completion_action(
        &self,
        key: KeyEvent,
    ) -> ComposerCompletionAction {
        if let Some(action) = self.selection_action(key, SelectionKeySet::TextSafe) {
            return ComposerCompletionAction::Select(action);
        }

        match key.code {
            _ if is_composer_newline_key(key) => ComposerCompletionAction::FallThrough,
            KeyCode::Tab | KeyCode::Enter => ComposerCompletionAction::Confirm,
            KeyCode::Esc => ComposerCompletionAction::Cancel,
            _ => ComposerCompletionAction::FallThrough,
        }
    }

    pub(in crate::tui) fn login_global_action(&self, key: KeyEvent) -> Option<LoginGlobalAction> {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(LoginGlobalAction::Cancel)
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn login_mode_select_action(
        &self,
        key: KeyEvent,
    ) -> Option<LoginModeSelectAction> {
        match key.code {
            KeyCode::Char(value) if value.eq_ignore_ascii_case(&'t') => {
                Some(LoginModeSelectAction::StartToken)
            }
            KeyCode::Char(value) if value.eq_ignore_ascii_case(&'e') => {
                Some(LoginModeSelectAction::StartPassword)
            }
            KeyCode::Char(value) if value.eq_ignore_ascii_case(&'q') => {
                Some(LoginModeSelectAction::StartQr)
            }
            KeyCode::Esc => Some(LoginModeSelectAction::Cancel),
            _ => None,
        }
    }

    pub(in crate::tui) fn login_text_input_action(&self, key: KeyEvent) -> LoginTextInputAction {
        match key.code {
            KeyCode::Enter => LoginTextInputAction::Submit,
            KeyCode::Esc => LoginTextInputAction::Back,
            KeyCode::Backspace => LoginTextInputAction::DeletePreviousChar,
            KeyCode::Char(value) => LoginTextInputAction::InsertChar(value),
            _ => LoginTextInputAction::Ignore,
        }
    }

    pub(in crate::tui) fn login_password_input_action(
        &self,
        key: KeyEvent,
    ) -> LoginPasswordInputAction {
        match key.code {
            KeyCode::Enter => LoginPasswordInputAction::Submit,
            KeyCode::Tab | KeyCode::Down | KeyCode::Up => LoginPasswordInputAction::SwitchField,
            KeyCode::Esc => LoginPasswordInputAction::Back,
            KeyCode::Backspace => LoginPasswordInputAction::DeletePreviousChar,
            KeyCode::Char(value) => LoginPasswordInputAction::InsertChar(value),
            _ => LoginPasswordInputAction::Ignore,
        }
    }

    pub(in crate::tui) fn login_mfa_select_action(&self, key: KeyEvent) -> LoginMfaSelectAction {
        match key.code {
            KeyCode::Char(value) if value.eq_ignore_ascii_case(&'t') => {
                LoginMfaSelectAction::Choose(MfaMethod::Totp)
            }
            KeyCode::Char(value) if value.eq_ignore_ascii_case(&'s') => {
                LoginMfaSelectAction::Choose(MfaMethod::Sms)
            }
            KeyCode::Esc => LoginMfaSelectAction::Back,
            _ => LoginMfaSelectAction::Ignore,
        }
    }

    pub(in crate::tui) fn login_busy_action(&self, key: KeyEvent) -> LoginBusyAction {
        match key.code {
            KeyCode::Esc => LoginBusyAction::Cancel,
            _ => LoginBusyAction::Ignore,
        }
    }

    pub(in crate::tui) fn selection_action(
        &self,
        key: KeyEvent,
        key_set: SelectionKeySet,
    ) -> Option<SelectionAction> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Down => Some(SelectionAction::Next),
            KeyCode::Up => Some(SelectionAction::Previous),
            KeyCode::Char('n') if ctrl => Some(SelectionAction::Next),
            KeyCode::Char('p') if ctrl => Some(SelectionAction::Previous),
            KeyCode::Char('j')
                if key_set == SelectionKeySet::Navigation && is_shortcut_key(key) =>
            {
                Some(SelectionAction::Next)
            }
            KeyCode::Char('k')
                if key_set == SelectionKeySet::Navigation && is_shortcut_key(key) =>
            {
                Some(SelectionAction::Previous)
            }
            _ => None,
        }
    }

    pub(in crate::tui) fn scroll_action(&self, key: KeyEvent) -> Option<ScrollAction> {
        match key.code {
            KeyCode::Char('j') if is_shortcut_key(key) => Some(ScrollAction::Down),
            KeyCode::Char('k') if is_shortcut_key(key) => Some(ScrollAction::Up),
            KeyCode::Down => Some(ScrollAction::Down),
            KeyCode::Up => Some(ScrollAction::Up),
            _ => None,
        }
    }

    pub fn message_confirmation_confirm_label(&self) -> &'static str {
        "Enter/y"
    }

    pub fn message_confirmation_cancel_label(&self) -> &'static str {
        "Esc/n"
    }

    pub fn image_viewer_download_hint(&self) -> &'static str {
        "[d] download image"
    }

    pub fn unread_mark_as_read_hint(&self) -> &'static str {
        "channel action (a) to mark as read "
    }

    pub fn start_composer_key_label(&self) -> String {
        self.binding_label(UiAction::StartComposer)
    }

    pub fn emoji_reaction_filter_prefix(&self) -> &'static str {
        "/"
    }

    pub fn login_token_choice_prefix(&self) -> &'static str {
        "[t] "
    }

    pub fn login_password_choice_prefix(&self) -> &'static str {
        "[e] "
    }

    pub fn login_qr_choice_prefix(&self) -> &'static str {
        "[q] "
    }

    pub fn login_totp_choice_prefix(&self) -> &'static str {
        "[t] "
    }

    pub fn login_sms_choice_prefix(&self) -> &'static str {
        "[s] "
    }

    pub fn login_cancel_quit_label(&self) -> &'static str {
        "Esc cancel | Ctrl-C quit"
    }

    pub fn login_token_input_label(&self) -> &'static str {
        "Enter save | Esc back | Ctrl-C quit"
    }

    pub fn login_password_input_label(&self) -> &'static str {
        "Tab switch field | Enter login | Esc back | Ctrl-C quit"
    }

    pub fn login_back_quit_label(&self) -> &'static str {
        "Esc back | Ctrl-C quit"
    }

    pub fn login_mfa_code_label(&self) -> &'static str {
        "Enter verify | Esc choose method | Ctrl-C quit"
    }

    pub fn options_category_shortcut(&self, shortcut: char) -> Option<OptionsCategoryShortcut> {
        match shortcut {
            'd' | 'D' => Some(OptionsCategoryShortcut::Display),
            'n' | 'N' => Some(OptionsCategoryShortcut::Notifications),
            'v' | 'V' => Some(OptionsCategoryShortcut::Voice),
            _ => None,
        }
    }

    pub fn options_category_shortcut_label(&self, category: OptionsCategoryShortcut) -> String {
        let action = match category {
            OptionsCategoryShortcut::Display => UiAction::OpenDisplayOptions,
            OptionsCategoryShortcut::Notifications => UiAction::OpenNotificationOptions,
            OptionsCategoryShortcut::Voice => UiAction::OpenVoiceOptions,
        };
        let label = self.binding_label(action);
        if label.is_empty() {
            match category {
                OptionsCategoryShortcut::Display => "d",
                OptionsCategoryShortcut::Notifications => "n",
                OptionsCategoryShortcut::Voice => "v",
            }
            .to_owned()
        } else {
            label
        }
    }

    pub fn channel_action_shortcuts(
        &self,
        actions: &[ChannelActionItem],
        index: usize,
    ) -> Vec<char> {
        if index >= actions.len() {
            return Vec::new();
        }
        action_shortcuts(
            index,
            actions
                .iter()
                .map(|item| self.channel_action_shortcut_candidates(item.kind)),
        )
    }

    pub fn channel_action_label(&self, action: &ChannelActionItem) -> String {
        self.action_shortcuts
            .channel
            .iter()
            .find(|binding| binding.kind == action.kind)
            .and_then(|binding| binding.description.clone())
            .unwrap_or_else(|| action.label.clone())
    }

    fn channel_action_shortcut_candidates(&self, kind: ChannelActionKind) -> Vec<char> {
        self.action_shortcuts
            .channel
            .iter()
            .find(|binding| binding.kind == kind)
            .map(|binding| binding.shortcuts.clone())
            .unwrap_or_else(|| vec![self.default_channel_action_shortcut(kind)])
    }

    fn default_channel_action_shortcut(&self, kind: ChannelActionKind) -> char {
        match kind {
            ChannelActionKind::JoinVoice => 'j',
            ChannelActionKind::LeaveVoice => 'l',
            ChannelActionKind::LoadPinnedMessages => 'p',
            ChannelActionKind::ShowThreads => 't',
            ChannelActionKind::MarkAsRead => 'm',
            ChannelActionKind::ToggleMute => 'u',
        }
    }

    pub fn guild_action_shortcuts(&self, actions: &[GuildActionItem], index: usize) -> Vec<char> {
        if index >= actions.len() {
            return Vec::new();
        }
        action_shortcuts(
            index,
            actions
                .iter()
                .map(|item| self.guild_action_shortcut_candidates(item.kind)),
        )
    }

    pub fn guild_action_label(&self, action: &GuildActionItem) -> String {
        self.action_shortcuts
            .guild
            .iter()
            .find(|binding| binding.kind == action.kind)
            .and_then(|binding| binding.description.clone())
            .unwrap_or_else(|| action.label.clone())
    }

    fn guild_action_shortcut_candidates(&self, kind: GuildActionKind) -> Vec<char> {
        if let Some(binding) = self
            .action_shortcuts
            .guild
            .iter()
            .find(|binding| binding.kind == kind)
        {
            return binding.shortcuts.clone();
        }
        self.default_guild_action_shortcut(kind)
            .into_iter()
            .collect()
    }

    fn default_guild_action_shortcut(&self, kind: GuildActionKind) -> Option<char> {
        match kind {
            GuildActionKind::MarkAsRead => Some('m'),
            GuildActionKind::ToggleMute => Some('u'),
            GuildActionKind::NoActionsYet => None,
        }
    }

    pub fn member_action_shortcuts(&self, actions: &[MemberActionItem], index: usize) -> Vec<char> {
        if index >= actions.len() {
            return Vec::new();
        }
        action_shortcuts(
            index,
            actions
                .iter()
                .map(|item| self.member_action_shortcut_candidates(item.kind)),
        )
    }

    pub fn member_action_label(&self, action: &MemberActionItem) -> String {
        self.action_shortcuts
            .member
            .iter()
            .find(|binding| binding.kind == action.kind)
            .and_then(|binding| binding.description.clone())
            .unwrap_or_else(|| action.label.clone())
    }

    fn member_action_shortcut_candidates(&self, kind: MemberActionKind) -> Vec<char> {
        self.action_shortcuts
            .member
            .iter()
            .find(|binding| binding.kind == kind)
            .map(|binding| binding.shortcuts.clone())
            .unwrap_or_else(|| vec![self.default_member_action_shortcut(kind)])
    }

    fn default_member_action_shortcut(&self, kind: MemberActionKind) -> char {
        match kind {
            MemberActionKind::ShowProfile => 'p',
        }
    }

    pub fn message_action_shortcut(
        &self,
        actions: &[MessageActionItem],
        index: usize,
    ) -> Option<char> {
        let action = actions.get(index)?;
        unique_preferred_shortcut(
            self.message_action_preferred_shortcut(action.kind),
            actions
                .iter()
                .map(|item| self.message_action_preferred_shortcut(item.kind)),
        )
        .or_else(|| self.indexed_shortcut(index))
    }

    fn message_action_preferred_shortcut(&self, kind: MessageActionKind) -> Option<char> {
        match kind {
            MessageActionKind::Reply => Some('R'),
            MessageActionKind::Edit => Some('e'),
            MessageActionKind::Delete => Some('d'),
            MessageActionKind::OpenThread => Some('t'),
            MessageActionKind::ViewImage => Some('v'),
            MessageActionKind::OpenUrl => Some('o'),
            MessageActionKind::DownloadAttachment(_) => Some('f'),
            MessageActionKind::AddReaction => Some('r'),
            MessageActionKind::RemoveReaction(_) => Some('x'),
            MessageActionKind::ShowReactionUsers => Some('u'),
            MessageActionKind::ShowProfile => Some('p'),
            MessageActionKind::SetPinned(_) => Some('P'),
            MessageActionKind::VotePollAnswer(_) => None,
            MessageActionKind::OpenPollVotePicker => Some('c'),
        }
    }

    pub fn indexed_shortcut(&self, index: usize) -> Option<char> {
        indexed_shortcut(index)
    }

    pub fn emoji_reaction_shortcut(
        &self,
        reactions: &[EmojiReactionItem],
        existing_reactions: &[ReactionEmoji],
        index: usize,
    ) -> Option<char> {
        let reaction = reactions.get(index)?;
        if let Some(existing_index) = existing_reactions
            .iter()
            .position(|existing| existing == &reaction.emoji)
        {
            return self.qwerty_shortcut(existing_index);
        }

        let regular_index = reactions[..index]
            .iter()
            .filter(|item| !existing_reactions.contains(&item.emoji))
            .count();
        self.indexed_shortcut(regular_index)
    }

    fn qwerty_shortcut(&self, index: usize) -> Option<char> {
        const SHORTCUTS: &[u8] = b"qwertyuiop";
        SHORTCUTS.get(index).map(|shortcut| char::from(*shortcut))
    }
}

fn indexed_shortcut(index: usize) -> Option<char> {
    match index {
        0..=8 => char::from_digit(u32::try_from(index + 1).ok()?, 10),
        9 => Some('0'),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionsCategoryShortcut {
    Display,
    Notifications,
    Voice,
}

fn is_left_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('h') | KeyCode::Left)
}

fn is_right_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('l') | KeyCode::Right)
}

fn is_confirm_key(code: KeyCode) -> bool {
    matches!(code, KeyCode::Enter | KeyCode::Char(' '))
}

fn is_shortcut_key(key: KeyEvent) -> bool {
    key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT
}

fn is_composer_newline_key(key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Enter => key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT),
        _ => false,
    }
}

fn unique_preferred_shortcut(
    preferred: Option<char>,
    shortcuts: impl IntoIterator<Item = Option<char>>,
) -> Option<char> {
    let preferred = preferred?;
    let matches = shortcuts
        .into_iter()
        .filter(|shortcut| shortcut.is_some_and(|shortcut| shortcut == preferred))
        .count();
    (matches == 1).then_some(preferred)
}

fn action_shortcuts(index: usize, shortcut_sets: impl IntoIterator<Item = Vec<char>>) -> Vec<char> {
    let shortcut_sets = shortcut_sets.into_iter().collect::<Vec<_>>();
    let Some(preferred) = shortcut_sets.get(index) else {
        return Vec::new();
    };
    let shortcuts = unique_action_shortcuts(preferred, shortcut_sets.clone());
    if !shortcuts.is_empty() {
        return shortcuts;
    }

    let mut used = shortcut_sets.iter().flatten().copied().collect::<Vec<_>>();
    for fallback_index in 0..=index {
        let Some(preferred) = shortcut_sets.get(fallback_index) else {
            return Vec::new();
        };
        if !unique_action_shortcuts(preferred, shortcut_sets.clone()).is_empty() {
            continue;
        }
        let Some(fallback) = first_unused_indexed_shortcut(&used) else {
            return Vec::new();
        };
        if fallback_index == index {
            return vec![fallback];
        }
        used.push(fallback);
    }
    Vec::new()
}

fn first_unused_indexed_shortcut(used: &[char]) -> Option<char> {
    (0..10)
        .filter_map(indexed_shortcut)
        .find(|shortcut| !used.contains(shortcut))
}

fn unique_action_shortcuts(
    preferred: &[char],
    shortcut_sets: impl IntoIterator<Item = Vec<char>>,
) -> Vec<char> {
    let shortcut_sets = shortcut_sets.into_iter().collect::<Vec<_>>();
    let mut unique = Vec::new();
    for candidate in preferred.iter().copied() {
        if unique.contains(&candidate) {
            continue;
        }
        let matches = shortcut_sets
            .iter()
            .filter(|shortcuts| shortcuts.contains(&candidate))
            .count();
        if matches == 1 {
            unique.push(candidate);
        }
    }
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_chord_parses_modifiers_and_labels() {
        let chord = KeyChord::from_str("ctrl+k").expect("ctrl key should parse");

        assert_eq!(chord.code, KeyCode::Char('k'));
        assert_eq!(chord.modifiers, KeyModifiers::CONTROL);
        assert_eq!(chord.label(), "Ctrl+k");
    }

    #[test]
    fn angle_key_parses_neovim_modifier_aliases() {
        let cases = [
            ("C-f", KeyCode::Char('f'), KeyModifiers::CONTROL, "Ctrl+f"),
            ("S-f", KeyCode::Char('f'), KeyModifiers::SHIFT, "Shift+f"),
            ("A-f", KeyCode::Char('f'), KeyModifiers::ALT, "Alt+f"),
            ("M-f", KeyCode::Char('f'), KeyModifiers::ALT, "Alt+f"),
            (
                "C-S-f",
                KeyCode::Char('f'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                "Ctrl+Shift+f",
            ),
        ];

        for (value, code, modifiers, label) in cases {
            let chord = parse_angle_key(value).expect("angle key should parse");
            assert_eq!(chord.code, code);
            assert_eq!(chord.modifiers, modifiers);
            assert_eq!(chord.label(), label);
        }
    }

    #[test]
    fn key_chord_preserves_uppercase_letter_keys() {
        let chord = KeyChord::from_str("R").expect("uppercase key should parse");

        assert_eq!(chord.code, KeyCode::Char('R'));
        assert!(chord.matches(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT)));
        assert!(!chord.matches(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)));
    }

    #[test]
    fn shifted_angle_letter_matches_shifted_terminal_event() {
        let chord = parse_angle_key("S-f").expect("shifted key should parse");

        assert!(chord.matches(KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT)));
        assert!(!chord.matches(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE)));
    }

    #[test]
    fn ui_action_names_match_future_colon_command_names() {
        assert_eq!(
            UiAction::from_name("ToggleGuildPane"),
            Some(UiAction::ToggleGuildPane)
        );
        assert_eq!(UiAction::from_name("VoiceMute"), Some(UiAction::VoiceMute));
        assert_eq!(
            UiAction::from_name("VoiceLeave"),
            Some(UiAction::VoiceLeave)
        );
        assert_eq!(
            UiAction::from_name("ChannelSwitcher"),
            Some(UiAction::ChannelSwitcher)
        );
        assert_eq!(
            UiAction::from_name("OpenFocusedPaneAction"),
            Some(UiAction::OpenFocusedPaneAction)
        );
        assert_eq!(UiAction::from_name("OpenVoiceActions"), None);
    }

    #[test]
    fn default_keymap_uses_leader_v_voice_group() {
        let key_bindings = KeyBindings::default();
        let mut prefix = key_bindings.leader_keymap_prefix();

        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &prefix,
                KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Pending)
        );
        prefix.push(KeyChord::from_str("v").expect("v should parse"));
        let children = key_bindings.leader_keymap_children(&prefix);

        assert!(
            children
                .iter()
                .any(|item| item.key == "m" && item.label == "mute voice")
        );
        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &prefix,
                KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Action(UiAction::VoiceMute))
        );
        assert!(
            key_bindings
                .leader_keymap_children(&key_bindings.leader_keymap_prefix())
                .iter()
                .any(|item| item.key == "v" && item.label == "Voice" && item.has_children)
        );
    }

    #[test]
    fn scoped_action_keymaps_override_pane_action_shortcuts_and_labels() {
        let keymap = KeymapOptions {
            guild_actions: [(
                "MuteServer".to_owned(),
                KeymapBinding {
                    keys: vec!["x".to_owned()],
                    description: Some("mute server".to_owned()),
                },
            )]
            .into_iter()
            .collect(),
            channel_actions: [("MuteChannel".to_owned(), KeymapBinding::one("x"))]
                .into_iter()
                .collect(),
            member_actions: [("ShowProfile".to_owned(), KeymapBinding::one("s"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let key_bindings =
            KeyBindings::try_from_options(&keymap).expect("scoped action keymaps should parse");

        let guild_actions = [GuildActionItem {
            kind: GuildActionKind::ToggleMute,
            label: "Mute server".to_owned(),
            enabled: true,
        }];
        assert_eq!(
            key_bindings.guild_action_shortcuts(&guild_actions, 0),
            vec!['x']
        );
        assert_eq!(
            key_bindings.guild_action_label(&guild_actions[0]),
            "mute server"
        );

        let channel_actions = [ChannelActionItem {
            kind: ChannelActionKind::ToggleMute,
            label: "Mute channel".to_owned(),
            enabled: true,
        }];
        assert_eq!(
            key_bindings.channel_action_shortcuts(&channel_actions, 0),
            vec!['x']
        );

        let member_actions = [MemberActionItem {
            kind: MemberActionKind::ShowProfile,
            label: "Show profile".to_owned(),
            enabled: true,
        }];
        assert_eq!(
            key_bindings.member_action_shortcuts(&member_actions, 0),
            vec!['s']
        );
    }

    #[test]
    fn scoped_action_keymaps_reject_actions_outside_their_scope() {
        let keymap = KeymapOptions {
            guild_actions: [("MuteChannel".to_owned(), KeymapBinding::one("x"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        assert!(KeyBindings::try_from_options(&keymap).is_err());
    }

    #[test]
    fn scoped_action_keymaps_try_later_keys_when_first_key_conflicts() {
        let keymap = KeymapOptions {
            channel_actions: [
                ("ShowPinnedMessages".to_owned(), KeymapBinding::one("x")),
                (
                    "MuteChannel".to_owned(),
                    KeymapBinding {
                        keys: vec!["x".to_owned(), "z".to_owned()],
                        description: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings =
            KeyBindings::try_from_options(&keymap).expect("scoped action keymaps should parse");
        let actions = [
            ChannelActionItem {
                kind: ChannelActionKind::LoadPinnedMessages,
                label: "Show pinned messages".to_owned(),
                enabled: true,
            },
            ChannelActionItem {
                kind: ChannelActionKind::ToggleMute,
                label: "Mute channel".to_owned(),
                enabled: true,
            },
        ];

        assert_eq!(
            key_bindings.channel_action_shortcuts(&actions, 0),
            vec!['1']
        );
        assert_eq!(
            key_bindings.channel_action_shortcuts(&actions, 1),
            vec!['z']
        );
    }

    #[test]
    fn scoped_action_keymaps_keep_multiple_unique_aliases() {
        let keymap = KeymapOptions {
            channel_actions: [(
                "MuteChannel".to_owned(),
                KeymapBinding {
                    keys: vec!["x".to_owned(), "u".to_owned()],
                    description: None,
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings =
            KeyBindings::try_from_options(&keymap).expect("scoped action keymaps should parse");
        let actions = [ChannelActionItem {
            kind: ChannelActionKind::ToggleMute,
            label: "Mute channel".to_owned(),
            enabled: true,
        }];

        assert_eq!(
            key_bindings.channel_action_shortcuts(&actions, 0),
            vec!['x', 'u']
        );
    }

    #[test]
    fn scoped_action_keymaps_do_not_reuse_conflicting_numeric_keys_as_fallbacks() {
        let keymap = KeymapOptions {
            channel_actions: [
                ("ShowPinnedMessages".to_owned(), KeymapBinding::one("1")),
                ("MuteChannel".to_owned(), KeymapBinding::one("1")),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings =
            KeyBindings::try_from_options(&keymap).expect("scoped action keymaps should parse");
        let actions = [
            ChannelActionItem {
                kind: ChannelActionKind::LoadPinnedMessages,
                label: "Show pinned messages".to_owned(),
                enabled: true,
            },
            ChannelActionItem {
                kind: ChannelActionKind::ToggleMute,
                label: "Mute channel".to_owned(),
                enabled: true,
            },
        ];

        assert_eq!(
            key_bindings.channel_action_shortcuts(&actions, 0),
            vec!['2']
        );
        assert_eq!(
            key_bindings.channel_action_shortcuts(&actions, 1),
            vec!['3']
        );
    }

    #[test]
    fn options_category_shortcut_labels_keep_contextual_defaults() {
        let key_bindings = KeyBindings::default();

        assert_eq!(
            key_bindings.options_category_shortcut_label(OptionsCategoryShortcut::Display),
            "d"
        );
        assert_eq!(
            key_bindings.options_category_shortcut_label(OptionsCategoryShortcut::Notifications),
            "n"
        );
        assert_eq!(
            key_bindings.options_category_shortcut_label(OptionsCategoryShortcut::Voice),
            "v"
        );
    }

    #[test]
    fn keymap_parses_leader_start_composer_sequence() {
        let keymap = KeymapOptions {
            mappings: [("StartComposer".to_owned(), KeymapBinding::one("<leader>e"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("keymap should parse");
        let leader_prefix = key_bindings.leader_keymap_prefix();

        assert!(
            key_bindings
                .leader_keymap_children(&leader_prefix)
                .iter()
                .any(|item| item.key == "e" && item.label == "start composer")
        );
        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &leader_prefix,
                KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Action(UiAction::StartComposer))
        );
    }

    #[test]
    fn keymap_parses_nested_leader_reply_sequence() {
        let keymap = KeymapOptions {
            mappings: [("ReplyMessage".to_owned(), KeymapBinding::one("<leader>m r"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("keymap should parse");
        let mut prefix = key_bindings.leader_keymap_prefix();

        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &prefix,
                KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Pending)
        );
        prefix.push(KeyChord::from_str("m").expect("m should parse"));
        let children = key_bindings.leader_keymap_children(&prefix);
        assert_eq!(children[0].key, "r");
        assert_eq!(children[0].label, "reply");
        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &prefix,
                KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Action(UiAction::ReplyMessage))
        );
    }

    #[test]
    fn keymap_allows_navigation_keys_after_leader_prefix() {
        let keymap = KeymapOptions {
            mappings: [("StartComposer".to_owned(), KeymapBinding::one("<leader>j"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("leader j should parse");
        let leader_prefix = key_bindings.leader_keymap_prefix();

        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &leader_prefix,
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Action(UiAction::StartComposer))
        );
    }

    #[test]
    fn keymap_parses_adjacent_angle_key_after_leader() {
        let keymap = KeymapOptions {
            mappings: [(
                "ChannelSwitcher".to_owned(),
                KeymapBinding::one("<leader><space>"),
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings =
            KeyBindings::try_from_options(&keymap).expect("leader space should parse");
        let leader_prefix = key_bindings.leader_keymap_prefix();

        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &leader_prefix,
                KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Action(UiAction::ChannelSwitcher))
        );
    }

    #[test]
    fn keymap_parses_adjacent_control_key_after_leader() {
        let keymap = KeymapOptions {
            mappings: [(
                "ChannelSwitcher".to_owned(),
                KeymapBinding::one("<leader><C-w>"),
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings =
            KeyBindings::try_from_options(&keymap).expect("leader ctrl-w should parse");
        let leader_prefix = key_bindings.leader_keymap_prefix();

        assert!(
            key_bindings
                .leader_keymap_children(&leader_prefix)
                .iter()
                .any(|item| item.key == "Ctrl+w" && item.label == "Switch channels")
        );
        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &leader_prefix,
                KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)
            ),
            Some(KeyMapLookup::Action(UiAction::ChannelSwitcher))
        );
    }

    #[test]
    fn keymap_parses_direct_sequence() {
        let keymap = KeymapOptions {
            mappings: [("ChannelSwitcher".to_owned(), KeymapBinding::one("<C-w>"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("direct key should parse");

        assert_eq!(
            key_bindings
                .keymap_lookup_direct_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            Some(UiAction::ChannelSwitcher)
        );
        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &key_bindings.leader_keymap_prefix(),
                KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)
            ),
            None
        );
    }

    #[test]
    fn keymap_parses_compact_non_leader_prefix_sequence() {
        let keymap = KeymapOptions {
            mappings: [("ChannelSwitcher".to_owned(), KeymapBinding::one("<C-w>f"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("prefix should parse");
        let prefix = [KeyChord {
            code: KeyCode::Char('w'),
            modifiers: KeyModifiers::CONTROL,
        }];

        assert_eq!(
            key_bindings.keymap.lookup(&prefix),
            Some(KeyMapLookup::Pending)
        );
        assert_eq!(key_bindings.keymap_prefix_title(&prefix), "<C-w>");
        assert_eq!(key_bindings.leader_keymap_children(&prefix)[0].key, "f");
        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &prefix,
                KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Action(UiAction::ChannelSwitcher))
        );
    }

    #[test]
    fn keymap_parses_plain_compact_prefix_sequence() {
        let keymap = KeymapOptions {
            mappings: [("VoiceDeafen".to_owned(), KeymapBinding::one("fd"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("prefix should parse");
        let prefix = [KeyChord::from_str("f").expect("f should parse")];

        assert_eq!(
            key_bindings.keymap.lookup(&prefix),
            Some(KeyMapLookup::Pending)
        );
        assert_eq!(key_bindings.keymap_prefix_title(&prefix), "f");
        assert_eq!(key_bindings.leader_keymap_children(&prefix)[0].key, "d");
        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &prefix,
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Action(UiAction::VoiceDeafen))
        );
    }

    #[test]
    fn keymap_configured_prefix_disables_conflicting_default_shortcut() {
        let keymap = KeymapOptions {
            mappings: [("VoiceDeafen".to_owned(), KeymapBinding::one("dd"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("prefix should parse");
        let prefix = [KeyChord::from_str("d").expect("d should parse")];

        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)),
            Some(KeyMapLookup::Pending)
        );
        assert_eq!(key_bindings.leader_keymap_children(&prefix)[0].key, "d");
        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &prefix,
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Action(UiAction::VoiceDeafen))
        );
    }

    #[test]
    fn keymap_configured_mapping_removes_canonical_default_alias_conflicts() {
        let keymap = KeymapOptions {
            mappings: [("VoiceDeafen".to_owned(), KeymapBinding::one("shift+tab d"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("prefix should parse");
        let prefix = [KeyChord::from_str("shift+tab").expect("shift tab should parse")];

        assert_eq!(
            key_bindings.keymap_lookup_root_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT)),
            Some(KeyMapLookup::Pending)
        );
        assert_eq!(key_bindings.leader_keymap_children(&prefix)[0].key, "d");
        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &prefix,
                KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Action(UiAction::VoiceDeafen))
        );
    }

    #[test]
    fn keymap_uses_configured_description_for_shortcut_label() {
        let keymap = KeymapOptions {
            mappings: [(
                "ChannelSwitcher".to_owned(),
                KeymapBinding {
                    keys: vec!["<C-w>f".to_owned()],
                    description: Some("find channel".to_owned()),
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings =
            KeyBindings::try_from_options(&keymap).expect("description should parse");
        let prefix = [KeyChord {
            code: KeyCode::Char('w'),
            modifiers: KeyModifiers::CONTROL,
        }];

        assert_eq!(
            key_bindings.leader_keymap_children(&prefix)[0].label,
            "find channel"
        );
    }

    #[test]
    fn keymap_uses_configured_group_title() {
        let keymap = KeymapOptions {
            groups: [("<C-w>".to_owned(), "Window".to_owned())]
                .into_iter()
                .collect(),
            mappings: [("ChannelSwitcher".to_owned(), KeymapBinding::one("<C-w>f"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("group should parse");
        let prefix = [KeyChord {
            code: KeyCode::Char('w'),
            modifiers: KeyModifiers::CONTROL,
        }];

        assert_eq!(key_bindings.keymap_prefix_title(&prefix), "Window");
    }

    #[test]
    fn lossy_keymap_keeps_valid_mapping_when_another_mapping_is_invalid() {
        let keymap = KeymapOptions {
            mappings: [
                ("StartComposer".to_owned(), KeymapBinding::one("<leader>e")),
                ("ReplyMessage".to_owned(), KeymapBinding::one("Enter")),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::from_options(&keymap);
        let leader_prefix = key_bindings.leader_keymap_prefix();

        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &leader_prefix,
                KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Action(UiAction::StartComposer))
        );
    }

    #[test]
    fn keymap_uses_custom_leader_key() {
        let keymap = KeymapOptions {
            leader: Some("ctrl+k".to_owned()),
            mappings: [("StartComposer".to_owned(), KeymapBinding::one("<leader>e"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let key_bindings =
            KeyBindings::try_from_options(&keymap).expect("custom leader should parse");

        assert!(
            key_bindings.is_leader_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL))
        );
    }

    #[test]
    fn keymap_rejects_overlong_sequences() {
        let long_sequence = std::iter::once("<leader>".to_owned())
            .chain((0..MAX_KEYMAP_SEQUENCE_CHORDS).map(|_| "x".to_owned()))
            .collect::<Vec<_>>()
            .join(" ");
        let keymap = KeymapOptions {
            mappings: [(
                "StartComposer".to_owned(),
                KeymapBinding::one(long_sequence),
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };

        assert!(KeyBindings::try_from_options(&keymap).is_err());
    }

    #[test]
    fn keymap_rejects_ambiguous_leaf_and_prefix_mappings() {
        let keymap = KeymapOptions {
            mappings: [
                ("StartComposer".to_owned(), KeymapBinding::one("<leader>m")),
                ("ReplyMessage".to_owned(), KeymapBinding::one("<leader>m r")),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };

        assert!(KeyBindings::try_from_options(&keymap).is_err());
    }
}
