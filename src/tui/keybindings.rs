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
    composer: ComposerKeyBindings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct KeymapBindingSummary {
    pub scope: &'static str,
    pub action: String,
    pub keys: Vec<String>,
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
    shortcuts: Vec<KeyChord>,
    description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ComposerKeyBindings {
    bindings: Vec<ComposerKeyBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ComposerKeyBinding {
    action: ComposerShortcutAction,
    shortcuts: Vec<KeyChord>,
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
    SelectNext,
    SelectPrevious,
    CycleFocusNext,
    CycleFocusPrevious,
    HalfPageDown,
    HalfPageUp,
    ScrollMessageViewportDown,
    ScrollMessageViewportUp,
    JumpTop,
    JumpBottom,
    ScrollHorizontalLeft,
    ScrollHorizontalRight,
    ResizePaneLeft,
    ResizePaneRight,
    Quit,
    CopyMessage,
    ReactMessage,
    ReplyMessage,
    DeleteMessage,
    EditMessage,
    OpenMessageUrl,
    ViewMessageAttachment,
    ShowMessageProfile,
    PinMessage,
    OpenThread,
    ShowReactionUsers,
    OpenPollVotePicker,
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
    ViewAttachment,
    ShowProfile,
    OpenPinConfirmation,
    OpenThread,
    ShowReactionUsers,
    OpenPollVotePicker,
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
    ActivateShortcut(KeyChord),
    UnknownClose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum PopupListAction {
    Close,
    Select(SelectionAction),
    Page(SelectionAction),
    ActivateSelected,
    ActivateShortcut(KeyChord),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum MessageConfirmationAction {
    Confirm,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum AttachmentViewerAction {
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
    CommitFilter,
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
            composer: ComposerKeyBindings::from_options_lossy(keymap_options),
        }
    }

    #[cfg(test)]
    fn try_from_options(keymap_options: &KeymapOptions) -> std::result::Result<Self, String> {
        let keymap = KeyMap::try_from_options(keymap_options)?;
        let action_shortcuts = ActionShortcutBindings::try_from_options(keymap_options)?;
        let composer = ComposerKeyBindings::try_from_options(keymap_options)?;
        Ok(Self {
            keymap,
            action_shortcuts,
            composer,
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

    fn binding_summaries(&self) -> Vec<KeymapBindingSummary> {
        let mut summaries = Vec::new();
        summaries.extend(self.guild.iter().map(|binding| KeymapBindingSummary {
            scope: "keymap.guild_actions",
            action: binding.kind.name().to_owned(),
            keys: key_labels(&binding.shortcuts),
        }));
        summaries.extend(self.channel.iter().map(|binding| KeymapBindingSummary {
            scope: "keymap.channel_actions",
            action: binding.kind.name().to_owned(),
            keys: key_labels(&binding.shortcuts),
        }));
        summaries.extend(self.member.iter().map(|binding| KeymapBindingSummary {
            scope: "keymap.member_actions",
            action: binding.kind.name().to_owned(),
            keys: key_labels(&binding.shortcuts),
        }));
        summaries
    }
}

impl Default for ComposerKeyBindings {
    fn default() -> Self {
        Self::from_specs(default_composer_key_bindings())
    }
}

impl ComposerKeyBindings {
    fn from_options_lossy(options: &KeymapOptions) -> Self {
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

    #[cfg(test)]
    fn try_from_options(options: &KeymapOptions) -> std::result::Result<Self, String> {
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

    fn action_for_key(&self, key: KeyEvent) -> Option<ComposerAction> {
        self.bindings.iter().find_map(|binding| {
            binding
                .shortcuts
                .iter()
                .any(|shortcut| shortcut.matches(key))
                .then(|| binding.action.to_composer_action())
        })
    }

    fn binding_summaries(&self) -> Vec<KeymapBindingSummary> {
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
    let leader = match options.leader.as_deref() {
        Some(value) => {
            let sequence = KeySequence::parse(value, char_chord(' '))?;
            let [leader] = sequence.0.as_slice() else {
                return Err("leader must be a single key".to_owned());
            };
            *leader
        }
        None => char_chord(' '),
    }
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
) -> Option<(Vec<KeyChord>, Option<String>)> {
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
) -> std::result::Result<(Vec<KeyChord>, Option<String>), String> {
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

fn parse_action_shortcut_key(value: &str) -> std::result::Result<KeyChord, String> {
    let sequence = KeySequence::parse(value, char_chord(' '))?;
    let [key] = sequence.0.as_slice() else {
        return Err("action shortcut must be a single key".to_owned());
    };
    match key.code {
        KeyCode::Char(value) if !value.is_whitespace() => Ok(key.canonical()),
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

fn parse_plain_sequence_token(token: &str) -> std::result::Result<Vec<KeyChord>, String> {
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

fn looks_like_bare_modifier_key(value: &str) -> bool {
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

fn parse_angle_key(value: &str) -> std::result::Result<KeyChord, String> {
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
            UiAction::SelectNext => "SelectNext",
            UiAction::SelectPrevious => "SelectPrevious",
            UiAction::CycleFocusNext => "CycleFocusNext",
            UiAction::CycleFocusPrevious => "CycleFocusPrevious",
            UiAction::HalfPageDown => "HalfPageDown",
            UiAction::HalfPageUp => "HalfPageUp",
            UiAction::ScrollMessageViewportDown => "ScrollMessageViewportDown",
            UiAction::ScrollMessageViewportUp => "ScrollMessageViewportUp",
            UiAction::JumpTop => "JumpTop",
            UiAction::JumpBottom => "JumpBottom",
            UiAction::ScrollHorizontalLeft => "ScrollHorizontalLeft",
            UiAction::ScrollHorizontalRight => "ScrollHorizontalRight",
            UiAction::ResizePaneLeft => "ResizePaneLeft",
            UiAction::ResizePaneRight => "ResizePaneRight",
            UiAction::Quit => "Quit",
            UiAction::CopyMessage => "CopyMessage",
            UiAction::ReactMessage => "ReactMessage",
            UiAction::ReplyMessage => "ReplyMessage",
            UiAction::DeleteMessage => "DeleteMessage",
            UiAction::EditMessage => "EditMessage",
            UiAction::OpenMessageUrl => "OpenMessageUrl",
            UiAction::ViewMessageAttachment => "ViewMessageAttachment",
            UiAction::ShowMessageProfile => "ShowMessageProfile",
            UiAction::PinMessage => "PinMessage",
            UiAction::OpenThread => "OpenThread",
            UiAction::ShowReactionUsers => "ShowReactionUsers",
            UiAction::OpenPollVotePicker => "OpenPollVotePicker",
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
            UiAction::SelectNext => "select next",
            UiAction::SelectPrevious => "select previous",
            UiAction::CycleFocusNext => "focus next",
            UiAction::CycleFocusPrevious => "focus previous",
            UiAction::HalfPageDown => "half page down",
            UiAction::HalfPageUp => "half page up",
            UiAction::ScrollMessageViewportDown => "scroll message viewport down",
            UiAction::ScrollMessageViewportUp => "scroll message viewport up",
            UiAction::JumpTop => "jump top",
            UiAction::JumpBottom => "jump bottom",
            UiAction::ScrollHorizontalLeft => "scroll left",
            UiAction::ScrollHorizontalRight => "scroll right",
            UiAction::ResizePaneLeft => "resize pane left",
            UiAction::ResizePaneRight => "resize pane right",
            UiAction::Quit => "quit",
            UiAction::CopyMessage => "copy message",
            UiAction::ReactMessage => "react",
            UiAction::ReplyMessage => "reply",
            UiAction::DeleteMessage => "delete message",
            UiAction::EditMessage => "edit message",
            UiAction::OpenMessageUrl => "open URL",
            UiAction::ViewMessageAttachment => "view attachment",
            UiAction::ShowMessageProfile => "show profile",
            UiAction::PinMessage => "pin message",
            UiAction::OpenThread => "open thread",
            UiAction::ShowReactionUsers => "show reacted users",
            UiAction::OpenPollVotePicker => "choose poll votes",
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
            "LeaveServer" => Some(Self::LeaveServer),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::NoActionsYet => "NoActionsYet",
            Self::MarkAsRead => "MarkAsRead",
            Self::ToggleMute => "ToggleMute",
            Self::LeaveServer => "LeaveServer",
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

    fn name(self) -> &'static str {
        match self {
            Self::JoinVoice => "JoinVoice",
            Self::LeaveVoice => "LeaveVoice",
            Self::LoadPinnedMessages => "ShowPinnedMessages",
            Self::ShowThreads => "ShowThreads",
            Self::MarkAsRead => "MarkAsRead",
            Self::ToggleMute => "ToggleMute",
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

    fn name(self) -> &'static str {
        match self {
            Self::ShowProfile => "ShowProfile",
        }
    }
}

impl MessageActionKind {
    fn ui_action(self) -> UiAction {
        match self {
            MessageActionKind::CopyContent => UiAction::CopyMessage,
            MessageActionKind::OpenReactionPicker => UiAction::ReactMessage,
            MessageActionKind::Reply => UiAction::ReplyMessage,
            MessageActionKind::OpenDeleteConfirmation => UiAction::DeleteMessage,
            MessageActionKind::Edit => UiAction::EditMessage,
            MessageActionKind::OpenUrl => UiAction::OpenMessageUrl,
            MessageActionKind::ViewAttachment => UiAction::ViewMessageAttachment,
            MessageActionKind::ShowProfile => UiAction::ShowMessageProfile,
            MessageActionKind::OpenPinConfirmation => UiAction::PinMessage,
            MessageActionKind::OpenThread => UiAction::OpenThread,
            MessageActionKind::ShowReactionUsers => UiAction::ShowReactionUsers,
            MessageActionKind::OpenPollVotePicker => UiAction::OpenPollVotePicker,
        }
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

fn all_ui_actions() -> &'static [UiAction] {
    &[
        UiAction::StartComposer,
        UiAction::OpenPaneFilter,
        UiAction::FocusGuildPane,
        UiAction::FocusChannelPane,
        UiAction::FocusMessagePane,
        UiAction::FocusMemberPane,
        UiAction::SelectNext,
        UiAction::SelectPrevious,
        UiAction::CycleFocusNext,
        UiAction::CycleFocusPrevious,
        UiAction::HalfPageDown,
        UiAction::HalfPageUp,
        UiAction::ScrollMessageViewportDown,
        UiAction::ScrollMessageViewportUp,
        UiAction::JumpTop,
        UiAction::JumpBottom,
        UiAction::ScrollHorizontalLeft,
        UiAction::ScrollHorizontalRight,
        UiAction::ResizePaneLeft,
        UiAction::ResizePaneRight,
        UiAction::Quit,
        UiAction::CopyMessage,
        UiAction::ReactMessage,
        UiAction::ReplyMessage,
        UiAction::DeleteMessage,
        UiAction::EditMessage,
        UiAction::OpenMessageUrl,
        UiAction::ViewMessageAttachment,
        UiAction::ShowMessageProfile,
        UiAction::PinMessage,
        UiAction::OpenThread,
        UiAction::ShowReactionUsers,
        UiAction::OpenPollVotePicker,
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
    pub(in crate::tui) fn matches_chord(self, other: Self) -> bool {
        key_chords_match_same_event(self, other)
    }

    pub(in crate::tui) fn matches_char(self, value: char) -> bool {
        self.matches_chord(char_chord(value))
    }

    fn matches(self, key: KeyEvent) -> bool {
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

fn parse_composer_binding_lossy(binding: &KeymapBinding) -> Option<Vec<KeyChord>> {
    let shortcuts = binding
        .keys
        .iter()
        .filter_map(|key| parse_composer_shortcut_key(key).ok())
        .collect::<Vec<_>>();
    (!shortcuts.is_empty()).then_some(shortcuts)
}

#[cfg(test)]
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

fn key_chords_match_same_event(left: KeyChord, right: KeyChord) -> bool {
    // Compare by possible terminal events rather than raw fields. This keeps
    // `A`, `Shift+a`, and uppercase-with-Shift encodings from being treated as
    // independent shortcuts when they can be produced by the same key press.
    candidate_key_events(left)
        .into_iter()
        .chain(candidate_key_events(right))
        .any(|event| left.matches(event) && right.matches(event))
}

fn candidate_key_events(chord: KeyChord) -> Vec<KeyEvent> {
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
        (KeyCode::Char('c' | 'n' | 'p'), KeyModifiers::CONTROL)
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
            UiAction::SelectNext => vec![vec![char_chord('j')]],
            UiAction::SelectPrevious => vec![vec![char_chord('k')]],
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
            UiAction::ScrollMessageViewportDown => vec![vec![char_chord('J')]],
            UiAction::ScrollMessageViewportUp => vec![vec![char_chord('K')]],
            UiAction::JumpTop => vec![vec![char_chord('g')]],
            UiAction::JumpBottom => vec![vec![char_chord('G')]],
            UiAction::ScrollHorizontalLeft => vec![vec![char_chord('H')]],
            UiAction::ScrollHorizontalRight => vec![vec![char_chord('L')]],
            UiAction::ResizePaneLeft => vec![
                vec![modified_key_chord(KeyCode::Char('h'), KeyModifiers::ALT)],
                vec![modified_key_chord(KeyCode::Left, KeyModifiers::ALT)],
            ],
            UiAction::ResizePaneRight => vec![
                vec![modified_key_chord(KeyCode::Char('l'), KeyModifiers::ALT)],
                vec![modified_key_chord(KeyCode::Right, KeyModifiers::ALT)],
            ],
            UiAction::Quit => vec![vec![char_chord('q')]],
            UiAction::CopyMessage => vec![vec![char_chord('y')]],
            UiAction::ReactMessage => vec![vec![char_chord('r')]],
            UiAction::ReplyMessage => vec![vec![char_chord('R')]],
            UiAction::DeleteMessage => vec![vec![char_chord('d')]],
            UiAction::EditMessage => vec![vec![char_chord('e')]],
            UiAction::OpenMessageUrl => vec![vec![char_chord('o')]],
            UiAction::ViewMessageAttachment => vec![vec![char_chord('v')]],
            UiAction::ShowMessageProfile => vec![vec![char_chord('p')]],
            UiAction::PinMessage => vec![vec![char_chord('P')]],
            UiAction::OpenThread => vec![vec![char_chord('t')]],
            UiAction::ShowReactionUsers => vec![vec![char_chord('u')]],
            UiAction::OpenPollVotePicker => vec![vec![char_chord('c')]],
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

fn keymap_sequence_label(sequence: &[KeyChord], leader: Option<KeyChord>) -> String {
    sequence
        .iter()
        .map(|chord| keymap_popup_key_label(*chord, leader))
        .collect::<Vec<_>>()
        .join(" ")
}

fn key_labels(keys: &[KeyChord]) -> Vec<String> {
    keys.iter()
        .map(|key| keymap_popup_key_label(*key, None))
        .collect()
}

fn keymap_popup_key_label(key: KeyChord, leader: Option<KeyChord>) -> String {
    if leader.is_some_and(|leader| key.matches_chord(leader)) {
        "<leader>".to_owned()
    } else {
        key.title_label()
    }
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
    modified_key_chord(KeyCode::Char(value), KeyModifiers::CONTROL)
}

fn modified_key_chord(code: KeyCode, modifiers: KeyModifiers) -> KeyChord {
    KeyChord {
        code,
        modifiers: normalized_modifiers(modifiers),
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
    pub(in crate::tui) fn binding_summaries(&self) -> Vec<KeymapBindingSummary> {
        let mut summaries = self
            .keymap
            .specs
            .iter()
            .map(|(action, spec)| KeymapBindingSummary {
                scope: "keymap",
                action: action.name().to_owned(),
                keys: spec
                    .sequences
                    .iter()
                    .map(|sequence| keymap_sequence_label(sequence, Some(self.keymap.leader)))
                    .collect(),
            })
            .collect::<Vec<_>>();

        summaries.extend(self.action_shortcuts.binding_summaries());
        summaries.extend(self.composer.binding_summaries());
        summaries
    }

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
            UiAction::SelectNext => Some(DashboardAction::Select(SelectionAction::Next)),
            UiAction::SelectPrevious => Some(DashboardAction::Select(SelectionAction::Previous)),
            UiAction::CycleFocusNext => Some(DashboardAction::CycleFocusForward),
            UiAction::CycleFocusPrevious => Some(DashboardAction::CycleFocusBackward),
            UiAction::HalfPageDown => Some(DashboardAction::HalfPageDown),
            UiAction::HalfPageUp => Some(DashboardAction::HalfPageUp),
            UiAction::ScrollMessageViewportDown if focus == FocusPane::Messages => {
                Some(DashboardAction::ScrollMessageViewportDown)
            }
            UiAction::ScrollMessageViewportUp if focus == FocusPane::Messages => {
                Some(DashboardAction::ScrollMessageViewportUp)
            }
            UiAction::JumpTop => Some(DashboardAction::JumpTop),
            UiAction::JumpBottom => Some(DashboardAction::JumpBottom),
            UiAction::ScrollHorizontalLeft => Some(DashboardAction::ScrollHorizontalLeft),
            UiAction::ScrollHorizontalRight => Some(DashboardAction::ScrollHorizontalRight),
            UiAction::ResizePaneLeft => Some(DashboardAction::ResizePaneLeft),
            UiAction::ResizePaneRight => Some(DashboardAction::ResizePaneRight),
            UiAction::Quit => Some(DashboardAction::Quit),
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
            UiAction::ViewMessageAttachment if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::ViewAttachment),
            ),
            UiAction::ShowMessageProfile if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::ShowProfile),
            ),
            UiAction::PinMessage if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::OpenPinConfirmation),
            ),
            UiAction::OpenThread if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::OpenThread),
            ),
            UiAction::ShowReactionUsers if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::ShowReactionUsers),
            ),
            UiAction::OpenPollVotePicker if focus == FocusPane::Messages => Some(
                DashboardAction::MessageShortcut(MessageShortcutAction::OpenPollVotePicker),
            ),
            _ => None,
        }
    }

    pub(in crate::tui) fn dashboard_action(
        &self,
        key: KeyEvent,
        _focus: FocusPane,
    ) -> Option<DashboardAction> {
        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            return Some(DashboardAction::Select(action));
        }

        match key.code {
            KeyCode::Esc => Some(DashboardAction::Back),
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
            KeyCode::Char(_) => {
                LeaderActionMenuAction::ActivateShortcut(self.keymap_chord_for_event(key))
            }
            code if is_left_key(code) => LeaderActionMenuAction::BackOrClose,
            _ => LeaderActionMenuAction::UnknownClose,
        }
    }

    pub(in crate::tui) fn popup_list_action(&self, key: KeyEvent) -> Option<PopupListAction> {
        if key.code == KeyCode::Esc {
            return Some(PopupListAction::Close);
        }
        if let Some(action) = self.popup_page_action(key) {
            return Some(PopupListAction::Page(action));
        }
        if let Some(action) = self.selection_action(key, SelectionKeySet::Navigation) {
            return Some(PopupListAction::Select(action));
        }

        match key.code {
            code if is_confirm_key(code) => Some(PopupListAction::ActivateSelected),
            KeyCode::Char(_) => Some(PopupListAction::ActivateShortcut(
                self.keymap_chord_for_event(key),
            )),
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

    pub(in crate::tui) fn attachment_viewer_action(
        &self,
        key: KeyEvent,
    ) -> Option<AttachmentViewerAction> {
        match key.code {
            KeyCode::Esc => Some(AttachmentViewerAction::Close),
            code if is_left_key(code) => Some(AttachmentViewerAction::Previous),
            code if is_right_key(code) => Some(AttachmentViewerAction::Next),
            KeyCode::Char('d') if is_shortcut_key(key) => {
                Some(AttachmentViewerAction::DownloadSelected)
            }
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
        filter_editing: bool,
    ) -> Option<EmojiReactionPickerAction> {
        let key_set = if filter_editing {
            SelectionKeySet::TextSafe
        } else {
            SelectionKeySet::Navigation
        };
        if let Some(action) = self.selection_action(key, key_set) {
            return Some(EmojiReactionPickerAction::Select(action));
        }

        match key.code {
            KeyCode::Esc => Some(EmojiReactionPickerAction::Close),
            KeyCode::Enter if filter_editing => Some(EmojiReactionPickerAction::CommitFilter),
            KeyCode::Backspace if filter_editing => {
                Some(EmojiReactionPickerAction::DeleteFilterChar)
            }
            KeyCode::Char('/') if is_shortcut_key(key) && !filter_editing => {
                Some(EmojiReactionPickerAction::StartFilter)
            }
            KeyCode::Char(value) if is_shortcut_key(key) && filter_editing => {
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
        if let Some(action) = self.popup_page_action(key) {
            return Some(match action {
                SelectionAction::Next => ReactionUsersPopupAction::PageDown,
                SelectionAction::Previous => ReactionUsersPopupAction::PageUp,
            });
        }
        if let Some(action) = self.scroll_action(key) {
            return Some(ReactionUsersPopupAction::Scroll(action));
        }

        None
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
        if let Some(action) = self.composer.action_for_key(key) {
            return action;
        }

        match key.code {
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
            _ if key_set == SelectionKeySet::Navigation => self.keymap_selection_action(key),
            _ => None,
        }
    }

    pub(in crate::tui) fn popup_page_action(&self, key: KeyEvent) -> Option<SelectionAction> {
        match key.code {
            KeyCode::PageDown => return Some(SelectionAction::Next),
            KeyCode::PageUp => return Some(SelectionAction::Previous),
            _ => {}
        }

        self.keymap_single_key_shortcuts(UiAction::HalfPageDown)
            .iter()
            .any(|shortcut| shortcut.matches(key))
            .then_some(SelectionAction::Next)
            .or_else(|| {
                self.keymap_single_key_shortcuts(UiAction::HalfPageUp)
                    .iter()
                    .any(|shortcut| shortcut.matches(key))
                    .then_some(SelectionAction::Previous)
            })
    }

    fn keymap_selection_action(&self, key: KeyEvent) -> Option<SelectionAction> {
        self.keymap_single_key_shortcuts(UiAction::SelectNext)
            .iter()
            .any(|shortcut| shortcut.matches(key))
            .then_some(SelectionAction::Next)
            .or_else(|| {
                self.keymap_single_key_shortcuts(UiAction::SelectPrevious)
                    .iter()
                    .any(|shortcut| shortcut.matches(key))
                    .then_some(SelectionAction::Previous)
            })
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

    pub fn attachment_viewer_download_hint(&self) -> &'static str {
        "[d] download attachment"
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
    ) -> Vec<KeyChord> {
        scoped_action_shortcuts(
            index,
            actions.iter().map(|item| item.kind),
            &self.action_shortcuts.channel,
            |kind| self.default_channel_action_shortcut(kind),
        )
    }

    pub fn channel_action_label(&self, action: &ChannelActionItem) -> String {
        action_label(&self.action_shortcuts.channel, action.kind, &action.label)
    }

    fn default_channel_action_shortcut(&self, kind: ChannelActionKind) -> Vec<KeyChord> {
        vec![char_chord(match kind {
            ChannelActionKind::JoinVoice => 'j',
            ChannelActionKind::LeaveVoice => 'l',
            ChannelActionKind::LoadPinnedMessages => 'p',
            ChannelActionKind::ShowThreads => 't',
            ChannelActionKind::MarkAsRead => 'm',
            ChannelActionKind::ToggleMute => 'u',
        })]
    }

    pub fn guild_action_shortcuts(
        &self,
        actions: &[GuildActionItem],
        index: usize,
    ) -> Vec<KeyChord> {
        scoped_action_shortcuts(
            index,
            actions.iter().map(|item| item.kind),
            &self.action_shortcuts.guild,
            |kind| self.default_guild_action_shortcut(kind),
        )
    }

    pub fn guild_action_label(&self, action: &GuildActionItem) -> String {
        action_label(&self.action_shortcuts.guild, action.kind, &action.label)
    }

    fn default_guild_action_shortcut(&self, kind: GuildActionKind) -> Vec<KeyChord> {
        match kind {
            GuildActionKind::MarkAsRead => vec![char_chord('m')],
            GuildActionKind::ToggleMute => vec![char_chord('u')],
            GuildActionKind::LeaveServer => vec![char_chord('l')],
            GuildActionKind::NoActionsYet => Vec::new(),
        }
    }

    pub fn member_action_shortcuts(
        &self,
        actions: &[MemberActionItem],
        index: usize,
    ) -> Vec<KeyChord> {
        scoped_action_shortcuts(
            index,
            actions.iter().map(|item| item.kind),
            &self.action_shortcuts.member,
            |kind| self.default_member_action_shortcut(kind),
        )
    }

    pub fn member_action_label(&self, action: &MemberActionItem) -> String {
        action_label(&self.action_shortcuts.member, action.kind, &action.label)
    }

    fn default_member_action_shortcut(&self, kind: MemberActionKind) -> Vec<KeyChord> {
        vec![char_chord(match kind {
            MemberActionKind::ShowProfile => 'p',
        })]
    }

    pub fn message_action_shortcuts(
        &self,
        actions: &[MessageActionItem],
        index: usize,
    ) -> Vec<KeyChord> {
        keymap_action_shortcuts(index, actions.iter().map(|item| item.kind), |kind| {
            self.keymap_single_key_shortcuts(kind.ui_action())
        })
    }

    pub fn message_action_label(&self, action: &MessageActionItem) -> String {
        self.keymap_action_label(action.kind.ui_action())
            .unwrap_or_else(|| action.label.clone())
    }

    pub fn message_action_shortcut_label(
        &self,
        actions: &[MessageActionItem],
        index: usize,
    ) -> String {
        let activation_shortcuts = self.message_action_shortcuts(actions, index);
        if !activation_shortcuts.is_empty() {
            return key_chord_list_label(&activation_shortcuts);
        }
        actions
            .get(index)
            .and_then(|action| self.keymap_action_sequence_label(action.kind.ui_action()))
            .unwrap_or_default()
    }

    fn keymap_action_label(&self, action: UiAction) -> Option<String> {
        self.keymap
            .specs
            .get(&action)
            .map(|spec| spec.label.clone())
    }

    fn keymap_single_key_shortcuts(&self, action: UiAction) -> Vec<KeyChord> {
        self.keymap
            .specs
            .get(&action)
            .map(|spec| {
                spec.sequences
                    .iter()
                    .filter_map(|sequence| match sequence.as_slice() {
                        [shortcut] => Some(*shortcut),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn keymap_action_sequence_label(&self, action: UiAction) -> Option<String> {
        self.keymap.specs.get(&action).map(|spec| {
            spec.sequences
                .iter()
                .map(|sequence| key_chord_sequence_label(sequence))
                .collect::<Vec<_>>()
                .join("/")
        })
    }

    pub(in crate::tui) fn matching_action_shortcut_index<A>(
        &self,
        actions: &[A],
        shortcut: KeyChord,
        shortcuts: impl Fn(&Self, &[A], usize) -> Vec<KeyChord>,
        is_enabled: impl Fn(&A) -> bool,
    ) -> Option<usize> {
        actions.iter().enumerate().position(|(index, action)| {
            is_enabled(action)
                && shortcuts(self, actions, index)
                    .iter()
                    .any(|candidate| candidate.matches_chord(shortcut))
        })
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

fn action_label<K>(bindings: &[ActionShortcutBinding<K>], kind: K, fallback: &str) -> String
where
    K: Copy + Eq,
{
    bindings
        .iter()
        .find(|binding| binding.kind == kind)
        .and_then(|binding| binding.description.clone())
        .unwrap_or_else(|| fallback.to_owned())
}

fn scoped_action_shortcuts<K>(
    index: usize,
    kinds: impl IntoIterator<Item = K>,
    bindings: &[ActionShortcutBinding<K>],
    default_shortcuts: impl Fn(K) -> Vec<KeyChord>,
) -> Vec<KeyChord>
where
    K: Copy + Eq,
{
    let shortcut_sets = kinds
        .into_iter()
        .map(|kind| action_shortcut_candidates(bindings, kind, &default_shortcuts))
        .collect::<Vec<_>>();
    if index >= shortcut_sets.len() {
        return Vec::new();
    }
    action_shortcuts(index, shortcut_sets)
}

fn action_shortcut_candidates<K>(
    bindings: &[ActionShortcutBinding<K>],
    kind: K,
    default_shortcuts: &impl Fn(K) -> Vec<KeyChord>,
) -> Vec<KeyChord>
where
    K: Copy + Eq,
{
    bindings
        .iter()
        .find(|binding| binding.kind == kind)
        .map(|binding| binding.shortcuts.clone())
        .unwrap_or_else(|| default_shortcuts(kind))
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

fn action_shortcuts(
    index: usize,
    shortcut_sets: impl IntoIterator<Item = Vec<KeyChord>>,
) -> Vec<KeyChord> {
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

fn keymap_action_shortcuts<K>(
    index: usize,
    kinds: impl IntoIterator<Item = K>,
    shortcuts: impl Fn(K) -> Vec<KeyChord>,
) -> Vec<KeyChord>
where
    K: Copy,
{
    let shortcut_sets = kinds.into_iter().map(shortcuts).collect::<Vec<_>>();
    let Some(preferred) = shortcut_sets.get(index) else {
        return Vec::new();
    };
    if preferred.is_empty() {
        return Vec::new();
    }
    action_shortcuts(index, shortcut_sets)
}

fn first_unused_indexed_shortcut(used: &[KeyChord]) -> Option<KeyChord> {
    (0..10)
        .filter_map(indexed_shortcut)
        .map(char_chord)
        .find(|shortcut| {
            !used
                .iter()
                .any(|used| key_chords_match_same_event(*used, *shortcut))
        })
}

fn unique_action_shortcuts(
    preferred: &[KeyChord],
    shortcut_sets: impl IntoIterator<Item = Vec<KeyChord>>,
) -> Vec<KeyChord> {
    let shortcut_sets = shortcut_sets.into_iter().collect::<Vec<_>>();
    let mut unique = Vec::new();
    for candidate in preferred.iter().copied() {
        if unique
            .iter()
            .any(|unique| key_chords_match_same_event(*unique, candidate))
        {
            continue;
        }
        let matches = shortcut_sets
            .iter()
            .filter(|shortcuts| {
                shortcuts
                    .iter()
                    .any(|shortcut| key_chords_match_same_event(*shortcut, candidate))
            })
            .count();
        if matches == 1 {
            unique.push(candidate);
        }
    }
    unique
}

fn key_chord_list_label(shortcuts: &[KeyChord]) -> String {
    shortcuts
        .iter()
        .map(|key| key.label())
        .collect::<Vec<_>>()
        .join("/")
}

fn key_chord_sequence_label(sequence: &[KeyChord]) -> String {
    sequence
        .iter()
        .map(|key| key.label())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn char_chords(values: &[char]) -> Vec<KeyChord> {
        values.iter().copied().map(char_chord).collect()
    }

    #[test]
    fn key_chord_parses_bare_keys_and_labels() {
        let chord = KeyChord::from_str("k").expect("key should parse");

        assert_eq!(chord.code, KeyCode::Char('k'));
        assert_eq!(chord.modifiers, KeyModifiers::NONE);
        assert_eq!(chord.label(), "k");
    }

    #[test]
    fn key_chord_rejects_legacy_plus_modifier_syntax() {
        let cases = ["ctrl+k", "control+k", "shift+tab", "alt+backspace"];

        for value in cases {
            assert!(
                KeyChord::from_str(value).is_err(),
                "{value} should not parse as a key chord"
            );
        }
    }

    #[test]
    fn angle_key_parses_neovim_modifier_aliases() {
        let cases = [
            ("C-f", KeyCode::Char('f'), KeyModifiers::CONTROL, "Ctrl+f"),
            ("C-w", KeyCode::Char('w'), KeyModifiers::CONTROL, "Ctrl+w"),
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
    fn angle_key_rejects_non_vim_modifier_spellings() {
        let cases = [
            "ctrl+w",
            "C+w",
            "ctrl-w",
            "control-w",
            "shift-f",
            "alt-f",
            "c-w",
        ];

        for value in cases {
            assert!(
                parse_angle_key(value).is_err(),
                "{value} should not parse as an angle key"
            );
        }
    }

    #[test]
    fn keymap_rejects_legacy_modifier_syntax_in_mixed_tokens() {
        let cases = [
            "ctrl+u<C-w>",
            "<C-w>ctrl+u",
            "ctrl-w",
            "C-w",
            "alt-backspace",
        ];

        for value in cases {
            let keymap = KeymapOptions {
                mappings: [("ChannelSwitcher".to_owned(), KeymapBinding::one(value))]
                    .into_iter()
                    .collect(),
                ..Default::default()
            };

            assert!(
                KeyBindings::try_from_options(&keymap).is_err(),
                "{value} should not parse as a keymap sequence"
            );
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
            UiAction::from_name("SelectNext"),
            Some(UiAction::SelectNext)
        );
        assert_eq!(
            UiAction::from_name("SelectPrevious"),
            Some(UiAction::SelectPrevious)
        );
        assert_eq!(
            UiAction::from_name("ScrollMessageViewportDown"),
            Some(UiAction::ScrollMessageViewportDown)
        );
        assert_eq!(
            UiAction::from_name("ScrollMessageViewportUp"),
            Some(UiAction::ScrollMessageViewportUp)
        );
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
        assert_eq!(UiAction::from_name("Quit"), Some(UiAction::Quit));
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
            char_chords(&['x'])
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
            char_chords(&['x'])
        );

        let member_actions = [MemberActionItem {
            kind: MemberActionKind::ShowProfile,
            label: "Show profile".to_owned(),
            enabled: true,
        }];
        assert_eq!(
            key_bindings.member_action_shortcuts(&member_actions, 0),
            char_chords(&['s'])
        );
    }

    #[test]
    fn message_action_menu_shortcuts_follow_direct_keymap_overrides() {
        let keymap = KeymapOptions {
            mappings: [
                ("ReplyMessage".to_owned(), KeymapBinding::one("n")),
                (
                    "OpenThread".to_owned(),
                    KeymapBinding {
                        keys: vec!["T".to_owned()],
                        description: Some("open message thread".to_owned()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings =
            KeyBindings::try_from_options(&keymap).expect("message keymap should parse");
        let actions = [MessageActionItem {
            kind: MessageActionKind::Reply,
            label: "reply".to_owned(),
            enabled: true,
        }];

        assert_eq!(key_bindings.message_action_shortcut_label(&actions, 0), "n");
        assert_eq!(key_bindings.message_action_label(&actions[0]), "reply");
        assert_eq!(
            key_bindings
                .keymap_lookup_direct_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
            Some(UiAction::ReplyMessage)
        );
        let thread_actions = [MessageActionItem {
            kind: MessageActionKind::OpenThread,
            label: "open thread".to_owned(),
            enabled: true,
        }];
        assert_eq!(
            key_bindings.message_action_shortcuts(&thread_actions, 0),
            char_chords(&['T'])
        );
        assert_eq!(
            key_bindings.message_action_label(&thread_actions[0]),
            "open message thread"
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
            char_chords(&['1'])
        );
        assert_eq!(
            key_bindings.channel_action_shortcuts(&actions, 1),
            char_chords(&['z'])
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
            char_chords(&['x', 'u'])
        );
    }

    #[test]
    fn scoped_action_keymaps_keep_modified_shortcuts_distinct() {
        let keymap = KeymapOptions {
            channel_actions: [(
                "MuteChannel".to_owned(),
                KeymapBinding {
                    keys: vec!["u".to_owned(), "<C-u>".to_owned()],
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
            vec![
                KeyChord::from_str("u").expect("u should parse"),
                parse_angle_key("C-u").expect("C-u should parse"),
            ]
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
            char_chords(&['2'])
        );
        assert_eq!(
            key_bindings.channel_action_shortcuts(&actions, 1),
            char_chords(&['3'])
        );
    }

    #[test]
    fn composer_keymaps_override_default_composer_shortcuts() {
        let keymap = KeymapOptions {
            composer: [
                ("OpenEditor".to_owned(), KeymapBinding::one("<C-o>")),
                (
                    "DeletePreviousWord".to_owned(),
                    KeymapBinding {
                        keys: vec!["<A-backspace>".to_owned()],
                        description: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("composer keymap parses");

        assert_eq!(
            key_bindings.composer_action(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)),
            ComposerAction::OpenInEditor
        );
        assert_eq!(
            key_bindings.composer_action(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL)),
            ComposerAction::Ignore
        );
        assert_eq!(
            key_bindings.composer_action(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT)),
            ComposerAction::DeletePreviousWord
        );
        assert_eq!(
            key_bindings.composer_action(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
            ComposerAction::Ignore
        );
    }

    #[test]
    fn composer_keymaps_reject_unknown_actions_and_conflicts() {
        let unknown = KeymapOptions {
            composer: [("MuteChannel".to_owned(), KeymapBinding::one("<C-m>"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        assert!(KeyBindings::try_from_options(&unknown).is_err());

        let conflicting = KeymapOptions {
            composer: [
                ("OpenEditor".to_owned(), KeymapBinding::one("<C-o>")),
                ("ClearInput".to_owned(), KeymapBinding::one("<C-o>")),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        assert!(KeyBindings::try_from_options(&conflicting).is_err());

        let shifted_printable_conflict = KeymapOptions {
            composer: [
                ("OpenEditor".to_owned(), KeymapBinding::one("A")),
                ("ClearInput".to_owned(), KeymapBinding::one("<S-a>")),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        assert!(KeyBindings::try_from_options(&shifted_printable_conflict).is_err());
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
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("leader C-w should parse");
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
            mappings: [("VoiceDeafen".to_owned(), KeymapBinding::one("<S-tab> d"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("prefix should parse");
        let prefix = [parse_angle_key("S-tab").expect("S-tab should parse")];

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
    fn keymap_can_remap_quit_action() {
        let keymap = KeymapOptions {
            mappings: [("Quit".to_owned(), KeymapBinding::one("x"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("quit should parse");

        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            Some(KeyMapLookup::Action(UiAction::Quit))
        );
        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            None
        );
    }

    #[test]
    fn keymap_can_remap_navigation_selection_actions() {
        let keymap = KeymapOptions {
            mappings: [
                ("SelectNext".to_owned(), KeymapBinding::one("n")),
                ("SelectPrevious".to_owned(), KeymapBinding::one("p")),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings =
            KeyBindings::try_from_options(&keymap).expect("selection keys should parse");

        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
            Some(KeyMapLookup::Action(UiAction::SelectNext))
        );
        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
            Some(KeyMapLookup::Action(UiAction::SelectPrevious))
        );
        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            key_bindings.selection_action(
                KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
                SelectionKeySet::Navigation,
            ),
            Some(SelectionAction::Next)
        );
        assert_eq!(
            key_bindings.selection_action(
                KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
                SelectionKeySet::Navigation,
            ),
            Some(SelectionAction::Previous)
        );
        assert_eq!(
            key_bindings.selection_action(
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
                SelectionKeySet::Navigation,
            ),
            None
        );
        assert_eq!(
            key_bindings.selection_action(
                KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
                SelectionKeySet::TextSafe,
            ),
            None
        );
        assert_eq!(
            key_bindings.selection_action(
                KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
                SelectionKeySet::Navigation,
            ),
            Some(SelectionAction::Next)
        );
        assert_eq!(
            key_bindings.selection_action(
                KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
                SelectionKeySet::Navigation,
            ),
            Some(SelectionAction::Previous)
        );
    }

    #[test]
    fn keymap_can_remap_message_viewport_scroll_actions() {
        let keymap = KeymapOptions {
            mappings: [
                (
                    "ScrollMessageViewportDown".to_owned(),
                    KeymapBinding::one("N"),
                ),
                (
                    "ScrollMessageViewportUp".to_owned(),
                    KeymapBinding::one("P"),
                ),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings =
            KeyBindings::try_from_options(&keymap).expect("viewport scroll keys should parse");

        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::NONE)),
            Some(KeyMapLookup::Action(UiAction::ScrollMessageViewportDown))
        );
        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE)),
            Some(KeyMapLookup::Action(UiAction::ScrollMessageViewportUp))
        );
        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            key_bindings.dashboard_action_for_ui_action(
                UiAction::ScrollMessageViewportDown,
                FocusPane::Messages,
            ),
            Some(DashboardAction::ScrollMessageViewportDown)
        );
        assert_eq!(
            key_bindings.dashboard_action_for_ui_action(
                UiAction::ScrollMessageViewportUp,
                FocusPane::Messages
            ),
            Some(DashboardAction::ScrollMessageViewportUp)
        );
        assert_eq!(
            key_bindings.dashboard_action_for_ui_action(
                UiAction::ScrollMessageViewportDown,
                FocusPane::Channels,
            ),
            None
        );
    }

    #[test]
    fn keymap_rejects_fixed_control_selection_keys() {
        for key in ["<C-n>", "<C-p>"] {
            let keymap = KeymapOptions {
                mappings: [("StartComposer".to_owned(), KeymapBinding::one(key))]
                    .into_iter()
                    .collect(),
                ..Default::default()
            };

            assert!(
                KeyBindings::try_from_options(&keymap).is_err(),
                "{key} should stay reserved for row movement"
            );
        }
    }

    #[test]
    fn default_keymap_maps_resize_shortcuts_to_dashboard_actions() {
        let key_bindings = KeyBindings::default();

        let cases = [
            (
                KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT),
                UiAction::ResizePaneLeft,
                DashboardAction::ResizePaneLeft,
            ),
            (
                KeyEvent::new(KeyCode::Left, KeyModifiers::ALT),
                UiAction::ResizePaneLeft,
                DashboardAction::ResizePaneLeft,
            ),
            (
                KeyEvent::new(KeyCode::Char('l'), KeyModifiers::ALT),
                UiAction::ResizePaneRight,
                DashboardAction::ResizePaneRight,
            ),
            (
                KeyEvent::new(KeyCode::Right, KeyModifiers::ALT),
                UiAction::ResizePaneRight,
                DashboardAction::ResizePaneRight,
            ),
        ];

        for (key, ui_action, dashboard_action) in cases {
            assert_eq!(key_bindings.keymap_lookup_direct_key(key), Some(ui_action));
            assert_eq!(
                key_bindings.dashboard_action_for_ui_action(ui_action, FocusPane::Messages),
                Some(dashboard_action)
            );
        }
    }

    #[test]
    fn keymap_can_remap_resize_actions() {
        let keymap = KeymapOptions {
            mappings: [
                ("ResizePaneLeft".to_owned(), KeymapBinding::one("<C-h>")),
                ("ResizePaneRight".to_owned(), KeymapBinding::one("<C-l>")),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings =
            KeyBindings::try_from_options(&keymap).expect("resize keys should parse");

        assert_eq!(
            key_bindings
                .keymap_lookup_direct_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL)),
            Some(UiAction::ResizePaneLeft)
        );
        assert_eq!(
            key_bindings
                .keymap_lookup_direct_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
            Some(UiAction::ResizePaneRight)
        );
        assert_eq!(
            key_bindings
                .keymap_lookup_direct_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT)),
            None
        );
        assert_eq!(
            key_bindings
                .keymap_lookup_direct_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::ALT)),
            None
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
    fn keymap_summaries_include_active_direct_and_composer_bindings() {
        let key_bindings = KeyBindings::default();
        let summaries = key_bindings.binding_summaries();

        assert!(summaries.iter().any(|summary| {
            summary.scope == "keymap"
                && summary.action == "StartComposer"
                && summary.keys.iter().any(|key| key == "i")
        }));
        assert!(summaries.iter().any(|summary| {
            summary.scope == "keymap.composer"
                && summary.action == "Submit"
                && summary.keys.iter().any(|key| key == "<Enter>")
        }));
        assert!(summaries.iter().any(|summary| {
            summary.scope == "keymap"
                && summary.action == "ToggleGuildPane"
                && summary.keys.iter().any(|key| key == "<leader> 1")
        }));
    }

    #[test]
    fn keymap_uses_custom_leader_key() {
        let keymap = KeymapOptions {
            leader: Some("<C-k>".to_owned()),
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
