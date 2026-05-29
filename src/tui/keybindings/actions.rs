use crate::discord::password_auth::MfaMethod;
use crate::tui::state::{FocusPane, MessageActionKind};

use super::KeyChord;

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
    GoToReferencedMessage,
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
    ScrollMessageViewportDown,
    ScrollMessageViewportUp,
    ScrollHorizontalLeft,
    ScrollHorizontalRight,
    ActivateFocused,
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
    DownloadSelected,
    ToggleZoom,
    ZoomIn,
    ZoomOut,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum DebugLogPopupAction {
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionsCategoryShortcut {
    Display,
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
