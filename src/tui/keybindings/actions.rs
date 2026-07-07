use crossterm::event::{KeyCode, KeyModifiers};

use crate::discord::password_auth::MfaMethod;
use crate::tui::state::{FocusPane, MessageActionKind};
use crate::tui::text_input::TextEditAction;

use self::DefaultKeymapChord::{Char, Ctrl, Key, Leader, ModifiedKey};
use super::KeyChord;

/// Chord alphabet for default key bindings. `Leader` is a placeholder that
/// resolves to the configured leader chord when the keymap is built.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DefaultKeymapChord {
    Leader,
    Char(char),
    Ctrl(char),
    Key(KeyCode),
    ModifiedKey(KeyCode, KeyModifiers),
}

// Single source of truth for every UI action: its keymap name is the variant
// identifier itself, and the default key sequences plus the dashboard mapping
// live in the same row, so adding an action is a one-line change.
macro_rules! define_ui_actions {
    ($($variant:ident => ($label:literal, $sequences:expr, $dashboard:expr),)*) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub(in crate::tui) enum UiAction {
            $($variant,)*
        }

        impl UiAction {
            pub(in crate::tui) const ALL: &'static [Self] = &[$(Self::$variant,)*];

            pub(in crate::tui) fn from_name(name: &str) -> Option<Self> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|action| action.name() == name)
            }

            pub(in crate::tui) fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant),)*
                }
            }

            pub(in crate::tui) fn label(self) -> &'static str {
                match self {
                    $(Self::$variant => $label,)*
                }
            }

            /// Default key sequences before the leader placeholder is
            /// resolved. Empty when the action ships unbound.
            pub(super) fn default_sequences(self) -> &'static [&'static [DefaultKeymapChord]] {
                match self {
                    $(Self::$variant => $sequences,)*
                }
            }

            /// Focus-independent `DashboardAction` this action dispatches
            /// to. Returns `None` for actions handled by focus-specific
            /// handlers.
            pub(super) fn global_dashboard_action(self) -> Option<DashboardAction> {
                match self {
                    $(Self::$variant => $dashboard,)*
                }
            }
        }
    };
}

define_ui_actions! {
    // variant => (label, default key sequences, dashboard action)
    StartComposer => ("start composer", &[&[Char('i')]], Some(DashboardAction::StartComposer)),
    OpenPaneFilter => ("filter/search pane", &[&[Char('/')]], Some(DashboardAction::OpenFocusedPaneFilter)),
    ClosePopup => ("close popup", &[&[Char('q')]], None),
    FocusGuildPane => ("focus Servers", &[&[Char('1')]], Some(DashboardAction::FocusPane(FocusPane::Guilds))),
    FocusChannelPane => ("focus Channels", &[&[Char('2')]], Some(DashboardAction::FocusPane(FocusPane::Channels))),
    FocusMessagePane => ("focus Messages", &[&[Char('3')]], Some(DashboardAction::FocusPane(FocusPane::Messages))),
    FocusMemberPane => ("focus Members", &[&[Char('4')]], Some(DashboardAction::FocusPane(FocusPane::Members))),
    SelectNext => ("select next", &[&[Char('j')]], Some(DashboardAction::Select(SelectionAction::Next))),
    SelectPrevious => ("select previous", &[&[Char('k')]], Some(DashboardAction::Select(SelectionAction::Previous))),
    CycleFocusNext => ("focus next", &[&[Key(KeyCode::Tab)], &[Char('l')], &[Key(KeyCode::Right)]], Some(DashboardAction::CycleFocusForward)),
    CycleFocusPrevious => ("focus previous", &[&[ModifiedKey(KeyCode::Tab, KeyModifiers::SHIFT)], &[Char('h')], &[Key(KeyCode::Left)]], Some(DashboardAction::CycleFocusBackward)),
    HalfPageDown => ("half page down", &[&[Ctrl('d')]], Some(DashboardAction::HalfPageDown)),
    HalfPageUp => ("half page up", &[&[Ctrl('u')]], Some(DashboardAction::HalfPageUp)),
    ScrollViewportDown => ("scroll viewport down", &[&[Char('J')]], Some(DashboardAction::ScrollViewportDown)),
    ScrollViewportUp => ("scroll viewport up", &[&[Char('K')]], Some(DashboardAction::ScrollViewportUp)),
    JumpTop => ("jump top", &[&[Char('g'), Char('g')]], Some(DashboardAction::JumpTop)),
    JumpBottom => ("jump bottom", &[&[Char('G')]], Some(DashboardAction::JumpBottom)),
    ScrollHorizontalLeft => ("scroll left", &[&[Char('H')]], Some(DashboardAction::ScrollHorizontalLeft)),
    ScrollHorizontalRight => ("scroll right", &[&[Char('L')]], Some(DashboardAction::ScrollHorizontalRight)),
    ResizePaneLeft => ("resize pane left", &[&[ModifiedKey(KeyCode::Char('h'), KeyModifiers::ALT)], &[ModifiedKey(KeyCode::Left, KeyModifiers::ALT)]], Some(DashboardAction::ResizePaneLeft)),
    ResizePaneRight => ("resize pane right", &[&[ModifiedKey(KeyCode::Char('l'), KeyModifiers::ALT)], &[ModifiedKey(KeyCode::Right, KeyModifiers::ALT)]], Some(DashboardAction::ResizePaneRight)),
    Quit => ("quit", &[&[Char('q')]], Some(DashboardAction::Quit)),
    CopyMessage => ("copy message", &[&[Char('y')]], None),
    ReactMessage => ("react", &[&[Char('r')]], None),
    ReplyMessage => ("reply", &[&[Char('R')]], None),
    DeleteMessage => ("delete message", &[&[Char('d')]], None),
    EditMessage => ("edit message", &[&[Char('e')]], None),
    OpenMessageUrl => ("open URL", &[&[Char('o')]], None),
    RemoveMessageEmbeds => ("remove embeds", &[], None),
    PlayMedia => ("play media", &[&[Char('x')]], None),
    ViewMessageAttachment => ("view attachment", &[&[Char('v')]], None),
    ShowMessageProfile => ("show message sender profile", &[], None),
    PinMessage => ("pin message", &[], None),
    OpenThread => ("open thread", &[], None),
    ShowReactionUsers => ("show reacted users", &[], None),
    OpenPollVotePicker => ("choose poll votes", &[], None),
    GoToReferencedMessage => ("go to referenced message", &[], None),
    ToggleGuildPane => ("toggle Servers", &[&[Leader, Char('1')]], None),
    ToggleChannelPane => ("toggle Channels", &[&[Leader, Char('2')]], None),
    ToggleMemberPane => ("toggle Members", &[&[Leader, Char('4')]], None),
    OpenFocusedPaneAction => ("Actions", &[&[Leader, Char('a')]], None),
    OpenCurrentUserProfile => ("My profile", &[&[Leader, Char('p')]], None),
    OpenOptions => ("Options", &[&[Leader, Char('o')]], None),
    ChannelSwitcher => ("Switch channels", &[&[Leader, Leader]], None),
    OpenNotificationInbox => ("Notification inbox", &[&[Leader, Char('n')]], None),
    OpenDisplayOptions => ("Display options", &[], None),
    OpenComposerOptions => ("Composer options", &[], None),
    OpenNotificationOptions => ("Notification options", &[], None),
    OpenVoiceOptions => ("Voice options", &[], None),
    VoiceDeafen => ("deafen voice", &[&[Leader, Char('v'), Char('d')]], None),
    VoiceMute => ("mute voice", &[&[Leader, Char('v'), Char('m')]], None),
    VoiceLeave => ("leave voice", &[&[Leader, Char('v'), Char('l')]], None),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) struct MessageActionBinding {
    pub ui_action: UiAction,
    pub message_action: MessageActionKind,
    pub keymap_name: &'static str,
}

macro_rules! define_message_action_bindings {
    ($($message_action:ident => ($ui_action:ident, $keymap_name:literal),)*) => {
        const MESSAGE_ACTION_BINDINGS: &[MessageActionBinding] = &[
            $(MessageActionBinding {
                ui_action: UiAction::$ui_action,
                message_action: MessageActionKind::$message_action,
                keymap_name: $keymap_name,
            },)*
        ];

        impl MessageActionKind {
            #[cfg(test)]
            pub(in crate::tui) const KEYMAP_BINDINGS: &'static [MessageActionBinding] =
                MESSAGE_ACTION_BINDINGS;

            pub(in crate::tui) fn from_keymap_name(name: &str) -> Option<Self> {
                match name {
                    $($keymap_name => Some(Self::$message_action),)*
                    _ => None,
                }
            }

            pub(in crate::tui) fn name(self) -> &'static str {
                match self {
                    $(Self::$message_action => $keymap_name,)*
                }
            }
        }
    };
}

define_message_action_bindings! {
    CopyContent => (CopyMessage, "CopyMessage"),
    OpenReactionPicker => (ReactMessage, "ReactMessage"),
    Reply => (ReplyMessage, "ReplyMessage"),
    OpenDeleteConfirmation => (DeleteMessage, "DeleteMessage"),
    Edit => (EditMessage, "EditMessage"),
    OpenUrl => (OpenMessageUrl, "OpenMessageUrl"),
    RemoveEmbeds => (RemoveMessageEmbeds, "RemoveMessageEmbeds"),
    PlayMedia => (PlayMedia, "PlayMedia"),
    ViewAttachment => (ViewMessageAttachment, "ViewMessageAttachment"),
    ShowProfile => (ShowMessageProfile, "ShowMessageProfile"),
    OpenPinConfirmation => (PinMessage, "PinMessage"),
    OpenThread => (OpenThread, "OpenThread"),
    ShowReactionUsers => (ShowReactionUsers, "ShowReactionUsers"),
    OpenPollVotePicker => (OpenPollVotePicker, "OpenPollVotePicker"),
    GoToReferencedMessage => (GoToReferencedMessage, "GoToReferencedMessage"),
}

impl UiAction {
    pub(in crate::tui) fn message_action_kind(self) -> Option<MessageActionKind> {
        MESSAGE_ACTION_BINDINGS
            .iter()
            .find(|binding| binding.ui_action == self)
            .map(|binding| binding.message_action)
    }
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
    MessageShortcut(MessageActionKind),
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
    ScrollViewportDown,
    ScrollViewportUp,
    ScrollHorizontalLeft,
    ScrollHorizontalRight,
    ActivateFocused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ProfilePopupTabAction {
    Global,
    Guild,
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
pub(in crate::tui) enum NotificationInboxAction {
    Select(SelectionAction),
    SwitchTab(SelectionAction),
    Close,
    ActivateSelected,
    MarkSelectedRead,
    MarkAllRead,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum SearchPopupAction {
    Select(SelectionAction),
    Page(SelectionAction),
    Close,
    ActivateSelected,
    NextField,
    PreviousField,
    MoveCursorLeft,
    MoveCursorRight,
    DeleteChar,
    InsertChar(char),
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
    ActivateSelected,
    ActivateShortcut(KeyChord),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum AttachmentViewerAction {
    Close,
    Previous,
    Next,
    PlaySelected,
    DownloadSelected,
    ToggleZoom,
    ZoomIn,
    ZoomOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum ProfilePopupAction {
    Close,
    Scroll(ScrollAction),
    NextField,
    PreviousField,
    SwitchTab(ProfilePopupTabAction),
    StartOrCommitEdit,
    PasteClipboard,
    Save,
    SignOut,
    EditText(TextEditAction),
    InsertChar(char),
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
    Back,
    Activate,
    Navigate(SelectionAction),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum DebugLogPopupAction {
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionsCategoryShortcut {
    Display,
    Composer,
    Notifications,
    Voice,
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
    EditText(TextEditAction),
    ToggleReplyPing,
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
