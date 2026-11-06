use std::time::{Duration, Instant};

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::discord::AppCommand;

use super::{
    super::{
        state::{
            ActiveModalPopupKind, DashboardState, FocusPane, FolderSettingsField, ThreadEditField,
            UserProfileSettingsTab,
        },
        ui::{self, FormButton, InteractionTarget},
    },
    actions::activate_focused_target,
};

const DOUBLE_CLICK_MAX_DELAY: Duration = Duration::from_millis(500);

/// Stateful terminal mouse decoder. The UI map owns geometry and this type
/// owns only gesture history, so neither concern leaks into state actions.
#[derive(Default)]
pub struct MouseInputState {
    last_click: Option<TimedClick>,
}

#[derive(Clone, Copy)]
struct TimedClick {
    target: InteractionTarget,
    at: Instant,
}

#[derive(Default)]
pub struct MouseEventResult {
    pub handled: bool,
    pub command: Option<AppCommand>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MouseGesture {
    Click(InteractionTarget),
    DoubleClick(InteractionTarget),
    ContextMenu(InteractionTarget),
    Scroll {
        target: Option<InteractionTarget>,
        direction: ScrollDirection,
    },
    Release,
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScrollDirection {
    Down,
    Up,
}

impl MouseEventResult {
    fn ignored() -> Self {
        Self::default()
    }

    fn handled(command: Option<AppCommand>) -> Self {
        Self {
            handled: true,
            command,
        }
    }
}

#[cfg(test)]
pub fn handle_mouse(state: &mut DashboardState, mouse: MouseEvent, area: Rect) -> bool {
    let mut input = MouseInputState::default();
    handle_mouse_event(state, mouse, area, &mut input).handled
}

pub fn handle_mouse_event(
    state: &mut DashboardState,
    mouse: MouseEvent,
    area: Rect,
    input: &mut MouseInputState,
) -> MouseEventResult {
    if state.is_key_sequence_active() {
        state.close_key_sequence();
        input.clear_click_sequence();
    }

    let target = ui::InteractionMap::new(area, state).target_at(mouse.column, mouse.row);
    let gif_picker_open = state.gif_picker().is_some();
    let pressed_outside_composer = state.is_composing()
        && !gif_picker_open
        && target != Some(InteractionTarget::Composer)
        && matches!(
            mouse.kind,
            MouseEventKind::Down(MouseButton::Left | MouseButton::Right)
        );
    if state.is_composing()
        && !gif_picker_open
        && target != Some(InteractionTarget::Composer)
        && !pressed_outside_composer
    {
        return MouseEventResult::ignored();
    }
    if pressed_outside_composer {
        input.clear_click_sequence();
        state.close_composer();
    }

    let gesture = input.decode(mouse.kind, target);
    dispatch_gesture(
        state,
        area,
        mouse.column,
        mouse.row,
        gesture,
        pressed_outside_composer,
    )
}

impl MouseInputState {
    fn decode(&mut self, kind: MouseEventKind, target: Option<InteractionTarget>) -> MouseGesture {
        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(target) = target else {
                    self.clear_click_sequence();
                    return MouseGesture::Ignore;
                };
                if !supports_double_click(target) {
                    self.clear_click_sequence();
                    return MouseGesture::Click(target);
                }
                let now = Instant::now();
                let double_click = self.last_click.is_some_and(|click| {
                    click.target == target && now.duration_since(click.at) <= DOUBLE_CLICK_MAX_DELAY
                });
                self.last_click = if double_click {
                    None
                } else {
                    Some(TimedClick { target, at: now })
                };
                if double_click {
                    MouseGesture::DoubleClick(target)
                } else {
                    MouseGesture::Click(target)
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                self.clear_click_sequence();
                target.map_or(MouseGesture::Ignore, MouseGesture::ContextMenu)
            }
            MouseEventKind::ScrollDown => {
                self.clear_click_sequence();
                MouseGesture::Scroll {
                    target,
                    direction: ScrollDirection::Down,
                }
            }
            MouseEventKind::ScrollUp => {
                self.clear_click_sequence();
                MouseGesture::Scroll {
                    target,
                    direction: ScrollDirection::Up,
                }
            }
            MouseEventKind::Up(MouseButton::Left | MouseButton::Right) => MouseGesture::Release,
            _ => {
                self.clear_click_sequence();
                MouseGesture::Ignore
            }
        }
    }

    fn clear_click_sequence(&mut self) {
        self.last_click = None;
    }
}

fn dispatch_gesture(
    state: &mut DashboardState,
    area: Rect,
    column: u16,
    row: u16,
    gesture: MouseGesture,
    blurred_composer: bool,
) -> MouseEventResult {
    match gesture {
        MouseGesture::Click(target) => handle_click(state, target, false),
        MouseGesture::DoubleClick(target) => handle_click(state, target, true),
        MouseGesture::ContextMenu(target) => handle_context_menu(state, target),
        MouseGesture::Scroll { target, direction } => {
            handle_scroll(state, area, column, row, target, direction)
        }
        MouseGesture::Release => MouseEventResult::handled(None),
        MouseGesture::Ignore if blurred_composer => MouseEventResult::handled(None),
        MouseGesture::Ignore => MouseEventResult::ignored(),
    }
}

fn handle_click(
    state: &mut DashboardState,
    target: InteractionTarget,
    double_click: bool,
) -> MouseEventResult {
    match target {
        InteractionTarget::Composer => {
            state.start_composer();
            MouseEventResult::handled(None)
        }
        InteractionTarget::ModalSurface | InteractionTarget::ModalBackdrop => {
            MouseEventResult::handled(None)
        }
        InteractionTarget::PopupItem { target, row } => {
            if !state.select_active_popup_row(target, row) {
                return MouseEventResult::handled(None);
            }
            let command = double_click
                .then(|| state.activate_active_popup_row(target))
                .flatten();
            MouseEventResult::handled(command)
        }
        InteractionTarget::Pane(pane) => {
            state.focus_pane(pane);
            MouseEventResult::handled(None)
        }
        InteractionTarget::PaneItem { pane, row } => {
            state.focus_pane(pane);
            if !state.select_visible_pane_row(pane, row) {
                return MouseEventResult::handled(None);
            }
            // A single click on a message toggles its spoiler reveal; the
            // second click of a double-click activates the message instead,
            // so a double-click ends up revealed-and-activated.
            let command = if double_click {
                activate_focused_target(state)
            } else {
                if pane == FocusPane::Messages {
                    state.toggle_selected_message_spoilers();
                }
                None
            };
            MouseEventResult::handled(command)
        }
        InteractionTarget::ReactionChip { row, col } => {
            let avatar_offset = ui::avatar_gutter_width(state.show_avatars());
            let command = state.toggle_reaction_chip_at(row, col, avatar_offset);
            MouseEventResult::handled(command)
        }
        InteractionTarget::UserProfileControl(control) => {
            let command = match control {
                ui::UserProfileControl::Tab(UserProfileSettingsTab::Global) => {
                    state.switch_user_profile_settings_to_global();
                    None
                }
                ui::UserProfileControl::Tab(UserProfileSettingsTab::Guild) => {
                    state.switch_user_profile_settings_to_guild();
                    None
                }
                ui::UserProfileControl::Field(field) => {
                    let selected = state.select_user_profile_settings_field(field);
                    if selected && state.user_profile_settings_editing_field() != Some(field) {
                        state.start_or_commit_user_profile_edit()
                    } else {
                        None
                    }
                }
            };
            MouseEventResult::handled(command)
        }
        InteractionTarget::ConfirmationButton(button) => {
            MouseEventResult::handled(state.activate_confirmation_button(button))
        }
        InteractionTarget::FormButton(FormButton::ForumPostSubmit) => {
            MouseEventResult::handled(state.save_forum_post_composer())
        }
        InteractionTarget::FormButton(FormButton::ThreadEditSubmit) => {
            MouseEventResult::handled(state.submit_thread_edit())
        }
        InteractionTarget::FormButton(FormButton::Cancel) => {
            match state.active_modal_popup_kind() {
                Some(ActiveModalPopupKind::ForumPostComposer) => {
                    state.close_forum_post_composer();
                }
                Some(ActiveModalPopupKind::ThreadEdit) => {
                    state.close_thread_edit();
                }
                _ => {}
            }
            MouseEventResult::handled(None)
        }
        InteractionTarget::FolderSettingsField(field) => {
            if !state.select_folder_settings_field(field) {
                return MouseEventResult::handled(None);
            }
            let command = match field {
                FolderSettingsField::Name | FolderSettingsField::Color => {
                    state.start_or_commit_folder_settings_edit();
                    None
                }
                FolderSettingsField::Submit => state.commit_folder_settings_command(),
                FolderSettingsField::Cancel => {
                    state.close_folder_settings();
                    None
                }
            };
            MouseEventResult::handled(command)
        }
        InteractionTarget::ForumPostField(field) => {
            let already_editing = state
                .forum_post_composer_view()
                .is_some_and(|view| view.editing_field == Some(field));
            let command = if state.select_forum_post_field(field) && !already_editing {
                state.activate_forum_post_composer()
            } else {
                None
            };
            MouseEventResult::handled(command)
        }
        InteractionTarget::ThreadEditField(field) => {
            let already_editing = state.thread_edit_view().is_some_and(|view| {
                (field == ThreadEditField::Title && view.editing_title)
                    || (field == ThreadEditField::Tags && view.editing_tags)
            });
            let command = if state.select_thread_edit_field(field) && !already_editing {
                state.activate_thread_edit()
            } else {
                None
            };
            MouseEventResult::handled(command)
        }
        InteractionTarget::SearchField(field) => {
            state.select_search_field(field);
            MouseEventResult::handled(None)
        }
        InteractionTarget::GifSearch => {
            state.select_gif_query();
            MouseEventResult::handled(None)
        }
        InteractionTarget::NotificationInboxTab(tab) => {
            state.select_notification_inbox_tab(tab);
            MouseEventResult::handled(None)
        }
    }
}

fn handle_context_menu(state: &mut DashboardState, target: InteractionTarget) -> MouseEventResult {
    match target {
        InteractionTarget::PaneItem { pane, row } => {
            state.focus_pane(pane);
            if state.select_visible_pane_row(pane, row) {
                state.open_focused_pane_actions();
            }
            MouseEventResult::handled(None)
        }
        InteractionTarget::Pane(pane) => {
            state.focus_pane(pane);
            MouseEventResult::handled(None)
        }
        InteractionTarget::ModalSurface
        | InteractionTarget::ModalBackdrop
        | InteractionTarget::PopupItem { .. }
        | InteractionTarget::ReactionChip { .. }
        | InteractionTarget::UserProfileControl(_)
        | InteractionTarget::ConfirmationButton(_)
        | InteractionTarget::FormButton(_)
        | InteractionTarget::FolderSettingsField(_)
        | InteractionTarget::ForumPostField(_)
        | InteractionTarget::ThreadEditField(_)
        | InteractionTarget::SearchField(_)
        | InteractionTarget::GifSearch
        | InteractionTarget::NotificationInboxTab(_) => MouseEventResult::handled(None),
        InteractionTarget::Composer => MouseEventResult::ignored(),
    }
}

fn handle_scroll(
    state: &mut DashboardState,
    area: Rect,
    column: u16,
    row: u16,
    target: Option<InteractionTarget>,
    direction: ScrollDirection,
) -> MouseEventResult {
    if state.active_modal_popup_kind().is_some() {
        let command = match direction {
            ScrollDirection::Down => state.move_active_popup_down(),
            ScrollDirection::Up => state.move_active_popup_up(),
        };
        return MouseEventResult::handled(command);
    }
    if state.is_folder_settings_open() {
        return MouseEventResult::handled(None);
    }

    let pane = target
        .and_then(interaction_pane)
        .or_else(|| ui::InteractionMap::new(area, state).pane_at(column, row));
    let Some(pane) = pane else {
        return MouseEventResult::ignored();
    };
    state.focus_pane(pane);
    match direction {
        ScrollDirection::Down => {
            state.scroll_focused_pane_viewport_down();
            MouseEventResult::handled(None)
        }
        ScrollDirection::Up => {
            state.scroll_focused_pane_viewport_up();
            let command = (pane == FocusPane::Messages)
                .then(|| state.next_older_history_command_for_half_page_up())
                .flatten();
            MouseEventResult::handled(command)
        }
    }
}

fn supports_double_click(target: InteractionTarget) -> bool {
    matches!(
        target,
        InteractionTarget::PaneItem { .. } | InteractionTarget::PopupItem { .. }
    )
}

fn interaction_pane(target: InteractionTarget) -> Option<FocusPane> {
    match target {
        InteractionTarget::Pane(pane) | InteractionTarget::PaneItem { pane, .. } => Some(pane),
        InteractionTarget::Composer | InteractionTarget::ReactionChip { .. } => {
            Some(FocusPane::Messages)
        }
        InteractionTarget::PopupItem { .. }
        | InteractionTarget::UserProfileControl(_)
        | InteractionTarget::ConfirmationButton(_)
        | InteractionTarget::FormButton(_)
        | InteractionTarget::FolderSettingsField(_)
        | InteractionTarget::ForumPostField(_)
        | InteractionTarget::ThreadEditField(_)
        | InteractionTarget::SearchField(_)
        | InteractionTarget::GifSearch
        | InteractionTarget::NotificationInboxTab(_)
        | InteractionTarget::ModalSurface
        | InteractionTarget::ModalBackdrop => None,
    }
}
