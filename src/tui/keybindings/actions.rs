use crate::discord::password_auth::MfaMethod;
use crate::tui::state::{FocusPane, MessageActionKind};

use super::KeyChord;

macro_rules! define_ui_actions {
    ($($variant:ident => ($name:literal, $label:literal),)*) => {
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
                    $(Self::$variant => $name,)*
                }
            }

            pub(in crate::tui) fn label(self) -> &'static str {
                match self {
                    $(Self::$variant => $label,)*
                }
            }
        }
    };
}

define_ui_actions! {
    StartComposer => ("StartComposer", "start composer"),
    OpenPaneFilter => ("OpenPaneFilter", "filter/search pane"),
    ClosePopup => ("ClosePopup", "close popup"),
    FocusGuildPane => ("FocusGuildPane", "focus Servers"),
    FocusChannelPane => ("FocusChannelPane", "focus Channels"),
    FocusMessagePane => ("FocusMessagePane", "focus Messages"),
    FocusMemberPane => ("FocusMemberPane", "focus Members"),
    SelectNext => ("SelectNext", "select next"),
    SelectPrevious => ("SelectPrevious", "select previous"),
    CycleFocusNext => ("CycleFocusNext", "focus next"),
    CycleFocusPrevious => ("CycleFocusPrevious", "focus previous"),
    HalfPageDown => ("HalfPageDown", "half page down"),
    HalfPageUp => ("HalfPageUp", "half page up"),
    ScrollViewportDown => ("ScrollViewportDown", "scroll viewport down"),
    ScrollViewportUp => ("ScrollViewportUp", "scroll viewport up"),
    JumpTop => ("JumpTop", "jump top"),
    JumpBottom => ("JumpBottom", "jump bottom"),
    ScrollHorizontalLeft => ("ScrollHorizontalLeft", "scroll left"),
    ScrollHorizontalRight => ("ScrollHorizontalRight", "scroll right"),
    ResizePaneLeft => ("ResizePaneLeft", "resize pane left"),
    ResizePaneRight => ("ResizePaneRight", "resize pane right"),
    Quit => ("Quit", "quit"),
    CopyMessage => ("CopyMessage", "copy message"),
    ReactMessage => ("ReactMessage", "react"),
    ReplyMessage => ("ReplyMessage", "reply"),
    DeleteMessage => ("DeleteMessage", "delete message"),
    EditMessage => ("EditMessage", "edit message"),
    OpenMessageUrl => ("OpenMessageUrl", "open URL"),
    RemoveMessageEmbeds => ("RemoveMessageEmbeds", "remove embeds"),
    PlayMedia => ("PlayMedia", "play media"),
    ViewMessageAttachment => ("ViewMessageAttachment", "view attachment"),
    ShowMessageProfile => ("ShowMessageProfile", "show message sender profile"),
    PinMessage => ("PinMessage", "pin message"),
    OpenThread => ("OpenThread", "open thread"),
    ShowReactionUsers => ("ShowReactionUsers", "show reacted users"),
    OpenPollVotePicker => ("OpenPollVotePicker", "choose poll votes"),
    GoToReferencedMessage => ("GoToReferencedMessage", "go to referenced message"),
    ToggleGuildPane => ("ToggleGuildPane", "toggle Servers"),
    ToggleChannelPane => ("ToggleChannelPane", "toggle Channels"),
    ToggleMemberPane => ("ToggleMemberPane", "toggle Members"),
    OpenFocusedPaneAction => ("OpenFocusedPaneAction", "Actions"),
    OpenCurrentUserProfile => ("OpenCurrentUserProfile", "My profile"),
    OpenOptions => ("OpenOptions", "Options"),
    ChannelSwitcher => ("ChannelSwitcher", "Switch channels"),
    OpenNotificationInbox => ("OpenNotificationInbox", "Notification inbox"),
    OpenDisplayOptions => ("OpenDisplayOptions", "Display options"),
    OpenComposerOptions => ("OpenComposerOptions", "Composer options"),
    OpenNotificationOptions => ("OpenNotificationOptions", "Notification options"),
    OpenVoiceOptions => ("OpenVoiceOptions", "Voice options"),
    VoiceDeafen => ("VoiceDeafen", "deafen voice"),
    VoiceMute => ("VoiceMute", "mute voice"),
    VoiceLeave => ("VoiceLeave", "leave voice"),
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
pub(in crate::tui) enum MessageConfirmationAction {
    Confirm,
    Cancel,
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
    DeleteChar,
    DeletePreviousWord,
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorWordLeft,
    MoveCursorWordRight,
    MoveCursorHome,
    MoveCursorEnd,
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
    Scroll(ScrollAction),
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
