use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::{
    ChannelActionItem, ChannelActionKind, EmojiReactionItem, FocusPane, GuildActionItem,
    GuildActionKind, MemberActionItem, MemberActionKind, MessageActionItem, MessageActionKind,
    ThreadActionItem, ThreadActionKind,
};
use crate::{
    config::{KeymapBinding, KeymapOptions},
    discord::{ReactionEmoji, password_auth::MfaMethod},
};

mod actions;
mod chord;
mod composer;
mod runtime;

pub use actions::OptionsCategoryShortcut;
pub(in crate::tui) use actions::{
    AttachmentViewerAction, ChannelSwitcherAction, ComposerAction, ComposerCompletionAction,
    DashboardAction, DebugLogPopupAction, EmojiReactionPickerAction, GlobalAction,
    LeaderActionMenuAction, LoginBusyAction, LoginGlobalAction, LoginMfaSelectAction,
    LoginModeSelectAction, LoginPasswordInputAction, LoginTextInputAction,
    MessageConfirmationAction, NotificationInboxAction, OptionsPopupAction, PaneFilterAction,
    PollVotePickerAction, PopupListAction, ProfilePopupAction, ProfilePopupTabAction,
    ReactionUsersPopupAction, ScrollAction, SearchPopupAction, SelectionAction, SelectionKeySet,
    UiAction,
};
pub(in crate::tui) use chord::KeyChord;
#[cfg(test)]
use chord::parse_angle_key;
use chord::{
    KeySequence, char_chord, ctrl_chord, key_chord, key_chords_match_same_event,
    modified_key_chord, parse_sequence_token,
};
use composer::ComposerKeyBindings;

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
    message: Vec<ActionShortcutBinding<MessageActionKind>>,
    member: Vec<ActionShortcutBinding<MemberActionKind>>,
    thread: Vec<ActionShortcutBinding<ThreadActionKind>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActionShortcutBinding<K> {
    kind: K,
    shortcuts: Vec<KeyChord>,
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

impl KeyBindings {
    pub fn from_options(keymap_options: &KeymapOptions) -> Self {
        Self {
            keymap: KeyMap::from_options_lossy(keymap_options),
            action_shortcuts: ActionShortcutBindings::from_options_lossy(keymap_options),
            composer: ComposerKeyBindings::from_options_lossy(keymap_options),
        }
    }

    pub(in crate::tui) fn try_from_options(
        keymap_options: &KeymapOptions,
    ) -> std::result::Result<Self, String> {
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
            message: parse_action_scope_lossy(
                &options.message_actions,
                MessageActionKind::from_keymap_name,
            ),
            member: parse_action_scope_lossy(
                &options.member_actions,
                MemberActionKind::from_keymap_name,
            ),
            thread: parse_action_scope_lossy(
                &options.thread_actions,
                ThreadActionKind::from_keymap_name,
            ),
        }
    }

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
            message: parse_action_scope(
                "keymap.message_actions",
                &options.message_actions,
                MessageActionKind::from_keymap_name,
            )?,
            member: parse_action_scope(
                "keymap.member_actions",
                &options.member_actions,
                MemberActionKind::from_keymap_name,
            )?,
            thread: parse_action_scope(
                "keymap.thread_actions",
                &options.thread_actions,
                ThreadActionKind::from_keymap_name,
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
        summaries.extend(self.message.iter().map(|binding| KeymapBindingSummary {
            scope: "keymap.message_actions",
            action: binding.kind.name().to_owned(),
            keys: key_labels(&binding.shortcuts),
        }));
        summaries.extend(self.member.iter().map(|binding| KeymapBindingSummary {
            scope: "keymap.member_actions",
            action: binding.kind.name().to_owned(),
            keys: key_labels(&binding.shortcuts),
        }));
        summaries.extend(self.thread.iter().map(|binding| KeymapBindingSummary {
            scope: "keymap.thread_actions",
            action: binding.kind.name().to_owned(),
            keys: key_labels(&binding.shortcuts),
        }));
        summaries
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
            if *action == UiAction::ClosePopup {
                continue;
            }
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
        .filter(|sequence| action != UiAction::ClosePopup || sequence.len() == 1)
        .collect::<Vec<_>>();
    (!sequences.is_empty()).then(|| KeyMapActionSpec {
        sequences,
        label: binding
            .description
            .clone()
            .unwrap_or_else(|| action.label().to_owned()),
    })
}

fn parse_keymap_binding(
    action_name: &str,
    action: UiAction,
    binding: &KeymapBinding,
    leader: KeyChord,
) -> std::result::Result<KeyMapActionSpec, String> {
    let mut sequences = Vec::new();
    for sequence in &binding.keys {
        let sequence = parse_keymap_sequence(action_name, sequence, leader)?.0;
        if action == UiAction::ClosePopup && sequence.len() != 1 {
            return Err(format!(
                "{action_name}: popup close key must be a single key"
            ));
        }
        sequences.push(sequence);
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

impl UiAction {
    fn from_keymap_name(name: &str) -> Option<Self> {
        match name {
            "ScrollMessageViewportDown" => Some(Self::ScrollViewportDown),
            "ScrollMessageViewportUp" => Some(Self::ScrollViewportUp),
            _ => Self::from_name(name),
        }
    }
}

impl GuildActionKind {
    fn from_keymap_name(name: &str) -> Option<Self> {
        match name {
            "MarkAsRead" => Some(Self::MarkAsRead),
            "MuteServer" | "ToggleMute" => Some(Self::ToggleMute),
            "LeaveServer" => Some(Self::LeaveServer),
            "FolderSettings" => Some(Self::FolderSettings),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::NoActionsYet => "NoActionsYet",
            Self::MarkAsRead => "MarkAsRead",
            Self::ToggleMute => "ToggleMute",
            Self::LeaveServer => "LeaveServer",
            Self::FolderSettings => "FolderSettings",
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

impl ThreadActionKind {
    fn from_keymap_name(name: &str) -> Option<Self> {
        match name {
            "MarkAsRead" => Some(Self::MarkAsRead),
            "ToggleFollow" => Some(Self::ToggleFollow),
            "Close" => Some(Self::Close),
            "Lock" => Some(Self::Lock),
            "Edit" => Some(Self::Edit),
            "CopyLink" => Some(Self::CopyLink),
            "ToggleMute" => Some(Self::ToggleMute),
            "NotificationSettings" => Some(Self::NotificationSettings),
            "Pin" => Some(Self::Pin),
            "Delete" => Some(Self::Delete),
            "CopyId" => Some(Self::CopyId),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::MarkAsRead => "MarkAsRead",
            Self::ToggleFollow => "ToggleFollow",
            Self::Close => "Close",
            Self::Lock => "Lock",
            Self::Edit => "Edit",
            Self::CopyLink => "CopyLink",
            Self::ToggleMute => "ToggleMute",
            Self::NotificationSettings => "NotificationSettings",
            Self::Pin => "Pin",
            Self::Delete => "Delete",
            Self::CopyId => "CopyId",
        }
    }
}

fn is_reserved_keymap_chord(chord: KeyChord) -> bool {
    matches!(
        chord.code,
        KeyCode::Enter | KeyCode::Esc | KeyCode::Backspace | KeyCode::Delete
    ) || matches!((chord.code, chord.modifiers), (KeyCode::Char(value), KeyModifiers::CONTROL) if matches!(value.to_ascii_lowercase(), 'c' | 'n' | 'p'))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DefaultKeymapChord {
    Leader,
    Char(char),
    Ctrl(char),
    Key(KeyCode),
    ModifiedKey(KeyCode, KeyModifiers),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DefaultKeymapDescriptor {
    action: UiAction,
    sequences: &'static [&'static [DefaultKeymapChord]],
}

const DEFAULT_KEYMAP_DESCRIPTORS: &[DefaultKeymapDescriptor] = &[
    DefaultKeymapDescriptor {
        action: UiAction::StartComposer,
        sequences: &[&[DefaultKeymapChord::Char('i')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::OpenPaneFilter,
        sequences: &[&[DefaultKeymapChord::Char('/')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ClosePopup,
        sequences: &[&[DefaultKeymapChord::Char('q')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::FocusGuildPane,
        sequences: &[&[DefaultKeymapChord::Char('1')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::FocusChannelPane,
        sequences: &[&[DefaultKeymapChord::Char('2')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::FocusMessagePane,
        sequences: &[&[DefaultKeymapChord::Char('3')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::FocusMemberPane,
        sequences: &[&[DefaultKeymapChord::Char('4')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::SelectNext,
        sequences: &[&[DefaultKeymapChord::Char('j')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::SelectPrevious,
        sequences: &[&[DefaultKeymapChord::Char('k')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::CycleFocusNext,
        sequences: &[
            &[DefaultKeymapChord::Key(KeyCode::Tab)],
            &[DefaultKeymapChord::Char('l')],
            &[DefaultKeymapChord::Key(KeyCode::Right)],
        ],
    },
    DefaultKeymapDescriptor {
        action: UiAction::CycleFocusPrevious,
        sequences: &[
            &[DefaultKeymapChord::ModifiedKey(
                KeyCode::Tab,
                KeyModifiers::SHIFT,
            )],
            &[DefaultKeymapChord::Char('h')],
            &[DefaultKeymapChord::Key(KeyCode::Left)],
        ],
    },
    DefaultKeymapDescriptor {
        action: UiAction::HalfPageDown,
        sequences: &[&[DefaultKeymapChord::Ctrl('d')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::HalfPageUp,
        sequences: &[&[DefaultKeymapChord::Ctrl('u')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ScrollViewportDown,
        sequences: &[&[DefaultKeymapChord::Char('J')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ScrollViewportUp,
        sequences: &[&[DefaultKeymapChord::Char('K')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::JumpTop,
        sequences: &[&[DefaultKeymapChord::Char('g'), DefaultKeymapChord::Char('g')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::JumpBottom,
        sequences: &[&[DefaultKeymapChord::Char('G')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ScrollHorizontalLeft,
        sequences: &[&[DefaultKeymapChord::Char('H')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ScrollHorizontalRight,
        sequences: &[&[DefaultKeymapChord::Char('L')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ResizePaneLeft,
        sequences: &[
            &[DefaultKeymapChord::ModifiedKey(
                KeyCode::Char('h'),
                KeyModifiers::ALT,
            )],
            &[DefaultKeymapChord::ModifiedKey(
                KeyCode::Left,
                KeyModifiers::ALT,
            )],
        ],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ResizePaneRight,
        sequences: &[
            &[DefaultKeymapChord::ModifiedKey(
                KeyCode::Char('l'),
                KeyModifiers::ALT,
            )],
            &[DefaultKeymapChord::ModifiedKey(
                KeyCode::Right,
                KeyModifiers::ALT,
            )],
        ],
    },
    DefaultKeymapDescriptor {
        action: UiAction::Quit,
        sequences: &[&[DefaultKeymapChord::Char('q')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::CopyMessage,
        sequences: &[&[DefaultKeymapChord::Char('y')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ReactMessage,
        sequences: &[&[DefaultKeymapChord::Char('r')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ReplyMessage,
        sequences: &[&[DefaultKeymapChord::Char('R')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::DeleteMessage,
        sequences: &[&[DefaultKeymapChord::Char('d')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::EditMessage,
        sequences: &[&[DefaultKeymapChord::Char('e')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::OpenMessageUrl,
        sequences: &[&[DefaultKeymapChord::Char('o')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::PlayMedia,
        sequences: &[&[DefaultKeymapChord::Char('x')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ViewMessageAttachment,
        sequences: &[&[DefaultKeymapChord::Char('v')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ToggleGuildPane,
        sequences: &[&[DefaultKeymapChord::Leader, DefaultKeymapChord::Char('1')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ToggleChannelPane,
        sequences: &[&[DefaultKeymapChord::Leader, DefaultKeymapChord::Char('2')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ToggleMemberPane,
        sequences: &[&[DefaultKeymapChord::Leader, DefaultKeymapChord::Char('4')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::OpenFocusedPaneAction,
        sequences: &[&[DefaultKeymapChord::Leader, DefaultKeymapChord::Char('a')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::OpenCurrentUserProfile,
        sequences: &[&[DefaultKeymapChord::Leader, DefaultKeymapChord::Char('p')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::OpenOptions,
        sequences: &[&[DefaultKeymapChord::Leader, DefaultKeymapChord::Char('o')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::ChannelSwitcher,
        sequences: &[&[DefaultKeymapChord::Leader, DefaultKeymapChord::Leader]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::OpenNotificationInbox,
        sequences: &[&[DefaultKeymapChord::Leader, DefaultKeymapChord::Char('n')]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::VoiceDeafen,
        sequences: &[&[
            DefaultKeymapChord::Leader,
            DefaultKeymapChord::Char('v'),
            DefaultKeymapChord::Char('d'),
        ]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::VoiceMute,
        sequences: &[&[
            DefaultKeymapChord::Leader,
            DefaultKeymapChord::Char('v'),
            DefaultKeymapChord::Char('m'),
        ]],
    },
    DefaultKeymapDescriptor {
        action: UiAction::VoiceLeave,
        sequences: &[&[
            DefaultKeymapChord::Leader,
            DefaultKeymapChord::Char('v'),
            DefaultKeymapChord::Char('l'),
        ]],
    },
];

fn default_keymap_specs(leader: KeyChord) -> BTreeMap<UiAction, KeyMapActionSpec> {
    DEFAULT_KEYMAP_DESCRIPTORS
        .iter()
        .map(|descriptor| {
            (
                descriptor.action,
                KeyMapActionSpec {
                    sequences: default_keymap_sequences(leader, descriptor.sequences),
                    label: descriptor.action.label().to_owned(),
                },
            )
        })
        .collect()
}

fn default_keymap_sequences(
    leader: KeyChord,
    sequences: &[&[DefaultKeymapChord]],
) -> Vec<Vec<KeyChord>> {
    sequences
        .iter()
        .map(|sequence| {
            sequence
                .iter()
                .map(|chord| default_keymap_chord(leader, *chord))
                .collect()
        })
        .collect()
}

fn default_keymap_chord(leader: KeyChord, chord: DefaultKeymapChord) -> KeyChord {
    match chord {
        DefaultKeymapChord::Leader => leader,
        DefaultKeymapChord::Char(value) => char_chord(value),
        DefaultKeymapChord::Ctrl(value) => ctrl_chord(value),
        DefaultKeymapChord::Key(code) => key_chord(code),
        DefaultKeymapChord::ModifiedKey(code, modifiers) => modified_key_chord(code, modifiers),
    }
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
            !configured
                .iter()
                .filter(|(configured_action, _)| **configured_action != UiAction::ClosePopup)
                .any(|(_, configured_spec)| {
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

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
            UiAction::from_name("ClosePopup"),
            Some(UiAction::ClosePopup)
        );
        assert_eq!(
            UiAction::from_name("ScrollViewportDown"),
            Some(UiAction::ScrollViewportDown)
        );
        assert_eq!(
            UiAction::from_name("ScrollViewportUp"),
            Some(UiAction::ScrollViewportUp)
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
    fn all_ui_action_names_round_trip() {
        for action in UiAction::ALL {
            assert_eq!(UiAction::from_name(action.name()), Some(*action));
            assert!(!action.label().is_empty());
        }
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
    fn close_popup_defaults_to_esc_and_q_and_can_be_remapped() {
        let key_bindings = KeyBindings::default();

        assert!(key_bindings.is_popup_close_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(
            key_bindings.is_popup_close_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
        );

        let documented_keymap = KeymapOptions {
            mappings: [("ClosePopup".to_owned(), KeymapBinding::one("q"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let documented_key_bindings = KeyBindings::try_from_options(&documented_keymap)
            .expect("documented close popup keymap parses");
        assert!(
            documented_key_bindings
                .is_popup_close_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        );
        assert!(
            documented_key_bindings
                .is_popup_close_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
        );

        let keymap = KeymapOptions {
            mappings: [(
                "ClosePopup".to_owned(),
                KeymapBinding {
                    keys: vec!["x".to_owned(), "<C-g>".to_owned()],
                    description: None,
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings =
            KeyBindings::try_from_options(&keymap).expect("close popup keymap parses");

        assert!(key_bindings.is_popup_close_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(
            !key_bindings.is_popup_close_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
        );
        assert!(
            key_bindings.is_popup_close_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
        );
        assert!(
            key_bindings
                .is_popup_close_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL))
        );
    }

    #[test]
    fn default_keymap_uses_g_prefix() {
        let key_bindings = KeyBindings::default();
        let prefix = [KeyChord::from_str("g").expect("g should parse")];

        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
            Some(KeyMapLookup::Pending)
        );
        assert_eq!(key_bindings.keymap_prefix_title(&prefix), "g");
        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &prefix,
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Action(UiAction::JumpTop))
        );

        let children = key_bindings.leader_keymap_children(&prefix);
        assert!(
            children
                .iter()
                .any(|item| item.key == "g" && item.label == "jump top")
        );

        for menu_only_key in ['p', 't', 'u', 'c', 'P'] {
            assert_eq!(
                key_bindings.keymap_lookup_direct_key(KeyEvent::new(
                    KeyCode::Char(menu_only_key),
                    KeyModifiers::NONE
                )),
                None,
                "{menu_only_key} should not be a default direct message action binding"
            );
        }
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
            message_actions: [("GoToReferencedMessage".to_owned(), KeymapBinding::one("g"))]
                .into_iter()
                .collect(),
            member_actions: [("ShowProfile".to_owned(), KeymapBinding::one("s"))]
                .into_iter()
                .collect(),
            thread_actions: [(
                "Close".to_owned(),
                KeymapBinding {
                    keys: vec!["x".to_owned()],
                    description: Some("close post".to_owned()),
                },
            )]
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

        let message_actions = [MessageActionItem {
            kind: MessageActionKind::GoToReferencedMessage,
            label: "Go to referenced message".to_owned(),
            enabled: true,
        }];
        assert_eq!(
            key_bindings.message_action_shortcuts(&message_actions, 0),
            char_chords(&['g'])
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

        let thread_actions = [ThreadActionItem {
            kind: ThreadActionKind::Close,
            label: "Close post".to_owned(),
            enabled: true,
        }];
        assert_eq!(
            key_bindings.thread_action_shortcuts(&thread_actions, 0),
            char_chords(&['x'])
        );
        assert_eq!(
            key_bindings.thread_action_label(&thread_actions[0]),
            "close post"
        );
    }

    #[test]
    fn thread_action_shortcuts_default_to_mnemonic_keys() {
        let key_bindings = KeyBindings::default();
        let actions = [
            ThreadActionItem {
                kind: ThreadActionKind::MarkAsRead,
                label: "Mark as read".to_owned(),
                enabled: true,
            },
            ThreadActionItem {
                kind: ThreadActionKind::Delete,
                label: "Delete post".to_owned(),
                enabled: true,
            },
        ];

        assert_eq!(key_bindings.thread_action_shortcut_label(&actions, 0), "m");
        assert_eq!(key_bindings.thread_action_shortcut_label(&actions, 1), "d");
    }

    #[test]
    fn message_action_menu_shortcuts_follow_message_action_scope() {
        let keymap = KeymapOptions {
            mappings: [
                ("ReplyMessage".to_owned(), KeymapBinding::one("n")),
                ("OpenThread".to_owned(), KeymapBinding::one("gt")),
            ]
            .into_iter()
            .collect(),
            message_actions: [
                (
                    "ReplyMessage".to_owned(),
                    KeymapBinding {
                        keys: vec!["m".to_owned()],
                        description: Some("reply from menu".to_owned()),
                    },
                ),
                ("OpenThread".to_owned(), KeymapBinding::one("T")),
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

        assert_eq!(key_bindings.message_action_shortcut_label(&actions, 0), "m");
        assert_eq!(
            key_bindings.message_action_label(&actions[0]),
            "reply from menu"
        );
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
        let direct_thread_prefix = [KeyChord::from_str("g").expect("g should parse")];
        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &direct_thread_prefix,
                KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE)
            ),
            Some(KeyMapLookup::Action(UiAction::OpenThread))
        );
        assert_eq!(
            key_bindings.dashboard_action_for_ui_action(UiAction::OpenThread, FocusPane::Messages),
            Some(DashboardAction::MessageShortcut(
                MessageActionKind::OpenThread
            ))
        );
        assert_eq!(
            key_bindings.message_action_label(&thread_actions[0]),
            "open thread"
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
    fn profile_popup_editing_uses_configured_composer_text_keys() {
        let keymap = KeymapOptions {
            composer: [
                ("PasteClipboard".to_owned(), KeymapBinding::one("<C-y>")),
                ("Submit".to_owned(), KeymapBinding::one("<C-s>")),
                ("Close".to_owned(), KeymapBinding::one("<C-q>")),
                (
                    "DeletePreviousWord".to_owned(),
                    KeymapBinding::one("<A-backspace>"),
                ),
                ("MoveCursorLeft".to_owned(), KeymapBinding::one("<A-left>")),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("composer keymap parses");

        assert_eq!(
            key_bindings.profile_popup_action(
                KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
                true,
            ),
            Some(ProfilePopupAction::PasteClipboard)
        );
        assert_eq!(
            key_bindings.profile_popup_action(
                KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
                true,
            ),
            Some(ProfilePopupAction::StartOrCommitEdit)
        );
        assert_eq!(
            key_bindings.profile_popup_action(
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL),
                true,
            ),
            Some(ProfilePopupAction::Close)
        );
        assert_eq!(
            key_bindings
                .profile_popup_action(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT), true),
            Some(ProfilePopupAction::DeletePreviousWord)
        );
        assert_eq!(
            key_bindings
                .profile_popup_action(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT), true),
            Some(ProfilePopupAction::MoveCursorLeft)
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
            key_bindings.options_category_shortcut_label(OptionsCategoryShortcut::Composer),
            "c"
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
    fn default_leader_p_opens_current_user_profile() {
        let key_bindings = KeyBindings::default();
        let leader_prefix = key_bindings.leader_keymap_prefix();

        assert_eq!(
            key_bindings.keymap_lookup_with_key(
                &leader_prefix,
                KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
            ),
            Some(KeyMapLookup::Action(UiAction::OpenCurrentUserProfile))
        );
        assert!(
            key_bindings
                .leader_keymap_children(&leader_prefix)
                .iter()
                .any(|item| item.key == "p" && item.label == "My profile")
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
    fn keymap_can_remap_viewport_scroll_actions() {
        let keymap = KeymapOptions {
            mappings: [
                ("ScrollViewportDown".to_owned(), KeymapBinding::one("N")),
                ("ScrollViewportUp".to_owned(), KeymapBinding::one("P")),
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
            Some(KeyMapLookup::Action(UiAction::ScrollViewportDown))
        );
        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE)),
            Some(KeyMapLookup::Action(UiAction::ScrollViewportUp))
        );
        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            key_bindings
                .dashboard_action_for_ui_action(UiAction::ScrollViewportDown, FocusPane::Messages,),
            Some(DashboardAction::ScrollViewportDown)
        );
        assert_eq!(
            key_bindings
                .dashboard_action_for_ui_action(UiAction::ScrollViewportUp, FocusPane::Messages),
            Some(DashboardAction::ScrollViewportUp)
        );
        assert_eq!(
            key_bindings
                .dashboard_action_for_ui_action(UiAction::ScrollViewportDown, FocusPane::Channels,),
            Some(DashboardAction::ScrollViewportDown)
        );
    }

    #[test]
    fn keymap_accepts_legacy_message_viewport_scroll_action_names() {
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
            KeyBindings::try_from_options(&keymap).expect("legacy scroll keys should parse");

        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::NONE)),
            Some(KeyMapLookup::Action(UiAction::ScrollViewportDown))
        );
        assert_eq!(
            key_bindings
                .keymap_lookup_root_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE)),
            Some(KeyMapLookup::Action(UiAction::ScrollViewportUp))
        );
    }

    #[test]
    fn keymap_maps_message_shortcuts_to_message_actions() {
        let key_bindings = KeyBindings::default();

        for binding in MessageActionKind::KEYMAP_BINDINGS {
            assert_eq!(
                key_bindings.dashboard_action_for_ui_action(binding.ui_action, FocusPane::Messages),
                Some(DashboardAction::MessageShortcut(binding.message_action))
            );
            assert_eq!(
                key_bindings.dashboard_action_for_ui_action(binding.ui_action, FocusPane::Channels),
                None
            );
            assert_eq!(
                MessageActionKind::from_keymap_name(binding.keymap_name),
                Some(binding.message_action)
            );
            assert_eq!(binding.message_action.name(), binding.keymap_name);
        }
    }

    #[test]
    fn close_popup_rejects_multi_key_sequences() {
        let keymap = KeymapOptions {
            mappings: [("ClosePopup".to_owned(), KeymapBinding::one("zz"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };

        assert!(KeyBindings::try_from_options(&keymap).is_err());
    }

    #[test]
    fn keymap_rejects_fixed_control_selection_keys() {
        for key in ["<C-n>", "<C-p>", "<C-N>", "<C-P>"] {
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
    fn keymap_uses_default_group_title() {
        let key_bindings = KeyBindings::default();
        let prefix = [key_bindings.keymap.leader, char_chord('v')];

        assert_eq!(key_bindings.keymap_prefix_title(&prefix), "Voice");
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
    fn profile_popup_uses_configured_selection_and_scroll_keys() {
        let keymap = KeymapOptions {
            mappings: [
                ("SelectNext".to_owned(), KeymapBinding::one("n")),
                ("SelectPrevious".to_owned(), KeymapBinding::one("p")),
                ("ScrollViewportDown".to_owned(), KeymapBinding::one("N")),
                ("ScrollViewportUp".to_owned(), KeymapBinding::one("P")),
            ]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let key_bindings = KeyBindings::try_from_options(&keymap).expect("keymap should parse");

        assert_eq!(
            key_bindings.profile_popup_action(
                KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
                false,
            ),
            Some(ProfilePopupAction::NextField)
        );
        assert_eq!(
            key_bindings
                .profile_popup_action(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), false),
            Some(ProfilePopupAction::StartOrCommitEdit)
        );
        assert_eq!(
            key_bindings.profile_popup_action(
                KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE),
                false,
            ),
            Some(ProfilePopupAction::SignOut)
        );
        assert_eq!(
            key_bindings.profile_popup_action(
                KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
                false,
            ),
            Some(ProfilePopupAction::PreviousField)
        );
        assert_eq!(
            key_bindings
                .profile_popup_action(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), false),
            None
        );
        assert_eq!(
            key_bindings.profile_popup_action(
                KeyEvent::new(KeyCode::Char('N'), KeyModifiers::NONE),
                false,
            ),
            Some(ProfilePopupAction::Scroll(ScrollAction::Down))
        );
        assert_eq!(
            key_bindings.profile_popup_action(
                KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE),
                false,
            ),
            Some(ProfilePopupAction::Scroll(ScrollAction::Up))
        );
        assert_eq!(
            key_bindings.profile_popup_action(
                KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE),
                false,
            ),
            None
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
