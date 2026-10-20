use ratatui::{
    layout::Rect,
    widgets::{Block, Borders},
};

use super::super::state::{
    ActiveModalPopupKind, ConfirmationButton, DashboardState, FocusPane, FolderSettingsField,
    ForumPostComposerField, NotificationInboxTab, SelectablePopupTarget, ThreadEditField,
};
use super::{
    avatar_gutter_width, channel_pane_header_height,
    layout::{dashboard_areas, message_areas},
    panel_block, panel_block_owned,
    popups::{
        active_modal_popup_area, active_selectable_popup_layout, folder_settings_popup_area,
        forum_post_composer_field_at, forum_post_composer_popup_area, notification_inbox_tab_at,
        popup_form_areas, search_popup_field_at, thread_edit_field_at, thread_edit_popup_area,
        user_profile_control_at,
    },
    types::UserProfileControl,
};

/// A semantic UI target resolved from the geometry of the current frame.
///
/// Rendering remains immediate-mode, so this map derives hit regions from the
/// same layout helpers on demand. Input code never needs to know panel borders,
/// popup offsets, scroll positions, or variable-height row rules.
#[derive(Clone, Copy)]
pub(crate) struct InteractionMap<'a> {
    area: Rect,
    state: &'a DashboardState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InteractionTarget {
    Pane(FocusPane),
    PaneItem {
        pane: FocusPane,
        row: usize,
    },
    ReactionChip {
        row: usize,
        col: usize,
    },
    ImageThumbnail {
        row: usize,
        col: usize,
    },
    Composer,
    PopupItem {
        target: SelectablePopupTarget,
        row: usize,
    },
    UserProfileControl(UserProfileControl),
    ConfirmationButton(ConfirmationButton),
    FormButton(FormButton),
    FolderSettingsField(FolderSettingsField),
    ForumPostField(ForumPostComposerField),
    ThreadEditField(ThreadEditField),
    SearchField(usize),
    GifSearch,
    NotificationInboxTab(NotificationInboxTab),
    ModalSurface,
    ModalBackdrop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FormButton {
    ForumPostSubmit,
    ThreadEditSubmit,
    Cancel,
}

impl<'a> InteractionMap<'a> {
    pub(crate) fn new(area: Rect, state: &'a DashboardState) -> Self {
        Self { area, state }
    }

    pub(crate) fn target_at(self, column: u16, row: u16) -> Option<InteractionTarget> {
        let modal_kind = self.state.active_modal_popup_kind();
        if modal_kind == Some(ActiveModalPopupKind::GifPicker)
            && rect_contains(super::gif_picker_search_area(self.area), column, row)
        {
            return Some(InteractionTarget::GifSearch);
        }
        if modal_kind == Some(ActiveModalPopupKind::Search)
            && let Some(field) = search_popup_field_at(self.area, self.state, column, row)
        {
            return Some(InteractionTarget::SearchField(field));
        }
        if modal_kind == Some(ActiveModalPopupKind::NotificationInbox)
            && !self.state.notification_inbox_is_confirming_mark_all()
            && let Some(tab) = notification_inbox_tab_at(self.area, self.state, column, row)
        {
            return Some(InteractionTarget::NotificationInboxTab(tab));
        }
        if let Some(target) = self.selectable_popup_target_at(column, row) {
            return Some(target);
        }

        if let Some(kind) = modal_kind {
            if kind == ActiveModalPopupKind::UserProfile
                && let Some(control) = user_profile_control_at(self.area, self.state, column, row)
            {
                return Some(InteractionTarget::UserProfileControl(control));
            }
            if self.is_confirmation_open(kind)
                && let Some(button) = self.confirmation_button_at(column, row)
            {
                return Some(InteractionTarget::ConfirmationButton(button));
            }
            if kind == ActiveModalPopupKind::ForumPostComposer
                && let Some(field) =
                    forum_post_composer_field_at(self.area, self.state, column, row)
            {
                return Some(InteractionTarget::ForumPostField(field));
            }
            if kind == ActiveModalPopupKind::ThreadEdit
                && let Some(field) = thread_edit_field_at(self.area, self.state, column, row)
            {
                return Some(InteractionTarget::ThreadEditField(field));
            }
            if let Some(button) = self.form_button_at(kind, column, row) {
                return Some(InteractionTarget::FormButton(button));
            }

            return Some(
                active_modal_popup_area(self.area, self.state)
                    .filter(|popup| rect_contains(*popup, column, row))
                    .map_or(InteractionTarget::ModalBackdrop, |_| {
                        InteractionTarget::ModalSurface
                    }),
            );
        }

        if self.state.is_folder_settings_open() {
            return Some(self.folder_settings_target_at(column, row));
        }

        self.dashboard_target_at(column, row)
    }

    pub(crate) fn pane_at(self, column: u16, row: u16) -> Option<FocusPane> {
        let areas = dashboard_areas(self.area, self.state);
        [
            (areas.guilds, FocusPane::Guilds),
            (areas.channels, FocusPane::Channels),
            (areas.messages, FocusPane::Messages),
            (areas.members, FocusPane::Members),
        ]
        .into_iter()
        .filter(|(_, pane)| self.state.is_pane_visible(*pane))
        .find_map(|(area, pane)| rect_contains(area, column, row).then_some(pane))
    }

    fn selectable_popup_target_at(self, column: u16, row: u16) -> Option<InteractionTarget> {
        let layout = active_selectable_popup_layout(self.area, self.state)?;
        if !rect_contains(layout.popup, column, row) {
            return Some(InteractionTarget::ModalBackdrop);
        }
        Some(
            layout
                .item_at(column, row)
                .map(|row| InteractionTarget::PopupItem {
                    target: layout.target,
                    row,
                })
                .unwrap_or(InteractionTarget::ModalSurface),
        )
    }

    fn dashboard_target_at(self, column: u16, row: u16) -> Option<InteractionTarget> {
        let areas = dashboard_areas(self.area, self.state);
        if self.state.is_pane_visible(FocusPane::Guilds)
            && let Some(target) = pane_target_at(
                areas.guilds,
                FocusPane::Guilds,
                column,
                row,
                self.state.guild_pane_filter_query().is_some(),
                0,
            )
        {
            return Some(target);
        }
        if self.state.is_pane_visible(FocusPane::Channels)
            && let Some(target) = pane_target_at(
                areas.channels,
                FocusPane::Channels,
                column,
                row,
                self.state.channel_pane_filter_query().is_some(),
                channel_pane_header_height(self.state),
            )
        {
            return Some(target);
        }
        if let Some(target) = message_target_at(areas.messages, self.state, column, row) {
            return Some(target);
        }
        if self.state.is_pane_visible(FocusPane::Members)
            && let Some(target) =
                pane_target_at(areas.members, FocusPane::Members, column, row, false, 0)
        {
            return Some(target);
        }
        None
    }

    fn is_confirmation_open(self, kind: ActiveModalPopupKind) -> bool {
        matches!(
            kind,
            ActiveModalPopupKind::MessageConfirmation
                | ActiveModalPopupKind::LongMessageConfirmation
                | ActiveModalPopupKind::QuitConfirmation
                | ActiveModalPopupKind::GuildLeaveConfirmation
                | ActiveModalPopupKind::GroupLeaveConfirmation
                | ActiveModalPopupKind::ThreadDeleteConfirmation
        ) || (kind == ActiveModalPopupKind::NotificationInbox
            && self.state.notification_inbox_is_confirming_mark_all())
    }

    fn confirmation_button_at(self, column: u16, row: u16) -> Option<ConfirmationButton> {
        let popup = active_modal_popup_area(self.area, self.state)?;
        let inner = Block::default().borders(Borders::ALL).inner(popup);
        if !rect_contains(inner, column, row) || inner.height < 2 {
            return None;
        }
        let first_button_row = inner.y.saturating_add(inner.height.saturating_sub(2));
        match row.saturating_sub(first_button_row) {
            0 => Some(ConfirmationButton::Confirm),
            1 => Some(ConfirmationButton::Cancel),
            _ => None,
        }
    }

    fn form_button_at(
        self,
        kind: ActiveModalPopupKind,
        column: u16,
        row: u16,
    ) -> Option<FormButton> {
        let (popup, primary) = match kind {
            ActiveModalPopupKind::ForumPostComposer => (
                forum_post_composer_popup_area(self.area),
                FormButton::ForumPostSubmit,
            ),
            ActiveModalPopupKind::ThreadEdit => (
                thread_edit_popup_area(self.area),
                FormButton::ThreadEditSubmit,
            ),
            _ => return None,
        };
        let footer = popup_form_areas(popup).footer;
        let inner = Block::default().borders(Borders::TOP).inner(footer);
        if !rect_contains(inner, column, row) {
            return None;
        }
        match row.saturating_sub(inner.y) {
            0 => Some(primary),
            1 => Some(FormButton::Cancel),
            _ => None,
        }
    }

    fn folder_settings_target_at(self, column: u16, row: u16) -> InteractionTarget {
        let popup = folder_settings_popup_area(self.area);
        if !rect_contains(popup, column, row) {
            return InteractionTarget::ModalBackdrop;
        }
        let inner = Block::default().borders(Borders::ALL).inner(popup);
        if !rect_contains(inner, column, row) {
            return InteractionTarget::ModalSurface;
        }
        match row.saturating_sub(inner.y) {
            0 => InteractionTarget::FolderSettingsField(FolderSettingsField::Name),
            2 => InteractionTarget::FolderSettingsField(FolderSettingsField::Color),
            5 => InteractionTarget::FolderSettingsField(FolderSettingsField::Submit),
            6 => InteractionTarget::FolderSettingsField(FolderSettingsField::Cancel),
            _ => InteractionTarget::ModalSurface,
        }
    }
}

fn pane_target_at(
    area: Rect,
    pane: FocusPane,
    column: u16,
    row: u16,
    filter_active: bool,
    leading_rows: u16,
) -> Option<InteractionTarget> {
    if !rect_contains(area, column, row) {
        return None;
    }
    let inner = panel_block("", false).inner(area);
    let leading_rows = leading_rows.min(inner.height);
    let content_height = inner.height.saturating_sub(leading_rows);
    let list_height = if filter_active && content_height >= 2 {
        content_height - 1
    } else {
        content_height
    };
    let list = Rect {
        y: inner.y.saturating_add(leading_rows),
        height: list_height,
        ..inner
    };
    if rect_contains(list, column, row) {
        return Some(InteractionTarget::PaneItem {
            pane,
            row: usize::from(row.saturating_sub(list.y)),
        });
    }
    Some(InteractionTarget::Pane(pane))
}

fn message_target_at(
    area: Rect,
    state: &DashboardState,
    column: u16,
    row: u16,
) -> Option<InteractionTarget> {
    if !rect_contains(area, column, row) {
        return None;
    }
    let inner = panel_block_owned(String::new(), false).inner(area);
    let areas = message_areas(inner, state);
    if rect_contains(areas.composer, column, row) {
        return Some(InteractionTarget::Composer);
    }
    if rect_contains(areas.list, column, row) {
        let list_row = usize::from(row.saturating_sub(areas.list.y));
        let list_col = usize::from(column.saturating_sub(areas.list.x));
        let avatar_offset = avatar_gutter_width(state.show_avatars());
        if state
            .reaction_chip_at_viewport_position(list_row, list_col, avatar_offset)
            .is_some()
        {
            return Some(InteractionTarget::ReactionChip {
                row: list_row,
                col: list_col,
            });
        }
        if state
            .image_preview_at_viewport_position(list_row, list_col, avatar_offset)
            .is_some()
        {
            return Some(InteractionTarget::ImageThumbnail {
                row: list_row,
                col: list_col,
            });
        }
        return Some(InteractionTarget::PaneItem {
            pane: FocusPane::Messages,
            row: list_row,
        });
    }
    Some(InteractionTarget::Pane(FocusPane::Messages))
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}
