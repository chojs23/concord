use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::{
    ChannelActionItem, ChannelActionKind, EmojiReactionItem, FocusPane, GuildActionItem,
    GuildActionKind, MemberActionItem, MemberActionKind, MessageActionItem, MessageActionKind,
    NotificationInboxTab, ThreadActionItem, ThreadActionKind,
};
use crate::{
    config::{KeymapBinding, KeymapOptions},
    discord::password_auth::MfaMethod,
};

mod actions;
mod chord;
mod composer;
mod runtime;

use actions::DefaultKeymapChord;
pub use actions::OptionsCategoryShortcut;
pub(in crate::tui) use actions::{
    AttachmentViewerAction, ChannelSwitcherAction, ComposerAction, ComposerCompletionAction,
    DashboardAction, DebugLogPopupAction, EmojiReactionPickerAction, GlobalAction, LoginBusyAction,
    LoginGlobalAction, LoginMfaSelectAction, LoginModeSelectAction, LoginPasswordInputAction,
    LoginTextInputAction, NotificationInboxAction, NotificationInboxActionKind, OptionsPopupAction,
    PaneFilterAction, PollVotePickerAction, PopupListAction, ProfilePopupAction,
    ProfilePopupTabAction, ReactionUsersPopupAction, ScrollAction, SearchPopupAction,
    SelectionAction, SelectionKeySet, UiAction,
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
    notification_inbox: Vec<ActionShortcutBinding<NotificationInboxActionKind>>,
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
            notification_inbox: parse_action_scope_lossy(
                &options.notification_inbox_actions,
                NotificationInboxActionKind::from_keymap_name,
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
            notification_inbox: parse_action_scope(
                "keymap.notification_inbox_actions",
                &options.notification_inbox_actions,
                NotificationInboxActionKind::from_keymap_name,
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
        summaries.extend(
            self.notification_inbox
                .iter()
                .map(|binding| KeymapBindingSummary {
                    scope: "keymap.notification_inbox_actions",
                    action: binding.kind.name().to_owned(),
                    keys: key_labels(&binding.shortcuts),
                }),
        );
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
            let Some(action) = UiAction::from_name(action_name) else {
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
            let action = UiAction::from_name(action_name)
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
    if binding.is_disabled() {
        return Some(KeyMapActionSpec {
            sequences: Vec::new(),
            label: action.label().to_owned(),
        });
    }

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
    if binding.is_disabled() {
        return Ok(KeyMapActionSpec {
            sequences: Vec::new(),
            label: action.label().to_owned(),
        });
    }

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
    if binding.is_disabled() {
        return Some((Vec::new(), None));
    }

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
    if binding.is_disabled() {
        return Ok((Vec::new(), None));
    }

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

impl NotificationInboxActionKind {
    fn from_keymap_name(name: &str) -> Option<Self> {
        match name {
            "MarkRead" => Some(Self::MarkRead),
            "MarkAllRead" => Some(Self::MarkAllRead),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::MarkRead => "MarkRead",
            Self::MarkAllRead => "MarkAllRead",
        }
    }
}

impl GuildActionKind {
    fn from_keymap_name(name: &str) -> Option<Self> {
        match name {
            "MarkAsRead" => Some(Self::MarkAsRead),
            "ToggleMute" => Some(Self::ToggleMute),
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
            "ShowPinnedMessages" => Some(Self::ShowPinnedMessages),
            "ShowThreads" => Some(Self::ShowThreads),
            "MarkAsRead" => Some(Self::MarkAsRead),
            "ToggleMute" => Some(Self::ToggleMute),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::JoinVoice => "JoinVoice",
            Self::LeaveVoice => "LeaveVoice",
            Self::ShowPinnedMessages => "ShowPinnedMessages",
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

fn default_keymap_specs(leader: KeyChord) -> BTreeMap<UiAction, KeyMapActionSpec> {
    UiAction::ALL
        .iter()
        .copied()
        .filter(|action| !action.default_sequences().is_empty())
        .map(|action| {
            (
                action,
                KeyMapActionSpec {
                    sequences: default_keymap_sequences(leader, action.default_sequences()),
                    label: action.label().to_owned(),
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
mod tests;
