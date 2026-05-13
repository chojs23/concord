use super::*;
use crate::tui::keybinding::Action;

pub(in crate::tui::ui) fn render_keymap_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_keymap_popup_open() {
        return;
    }

    let lines = keymap_popup_lines(state);
    let width: u16 = 52;
    let height = (lines.len() as u16).saturating_add(2);
    let popup = centered_rect(area, width, height);
    frame.render_widget(Clear, popup);
    let kb = state.key_bindings();
    let title = format!(
        "Default Keymaps  [{}/{}/Esc to close]",
        kb.label(Action::OpenKeymap),
        kb.label(Action::Quit),
    );
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block_owned(title, true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub fn keymap_popup_lines(state: &DashboardState) -> Vec<Line<'static>> {
    let kb = state.key_bindings();
    let move_down = kb.label(Action::MoveDown);
    let move_up = kb.label(Action::MoveUp);
    let jump_top = kb.label(Action::JumpTop);
    let jump_bottom = kb.label(Action::JumpBottom);
    let half_page_down = kb.label(Action::HalfPageDown);
    let half_page_up = kb.label(Action::HalfPageUp);
    let scroll_pane_left = kb.label(Action::ScrollPaneLeft);
    let scroll_pane_right = kb.label(Action::ScrollPaneRight);
    let quit = kb.label(Action::Quit);
    let open_leader = kb.label(Action::OpenLeader);
    let open_composer = kb.label(Action::OpenComposer);
    let open_in_editor = kb.label(Action::OpenInEditor);
    let scroll_viewport_down = kb.label(Action::ScrollViewportDown);
    let scroll_viewport_up = kb.label(Action::ScrollViewportUp);
    let open_keymap = kb.label(Action::OpenKeymap);

    fn row(key: String, desc: &'static str) -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("{key:<18}"), Style::default().fg(ACCENT)),
            Span::raw(desc),
        ])
    }
    fn section(label: String) -> Line<'static> {
        Line::from(Span::styled(
            label,
            Style::default().fg(DIM).add_modifier(Modifier::BOLD),
        ))
    }

    vec![
        section("Navigation".into()),
        row("1 / 2 / 3 / 4".into(), "focus pane"),
        row("Tab / Shift+Tab".into(), "cycle focus"),
        row(format!("{move_down} / {move_up} / ↑↓"), "move selection"),
        row(format!("{jump_top} / {jump_bottom}"), "jump top / bottom"),
        row(
            format!("{half_page_down} / {half_page_up}"),
            "half page down / up",
        ),
        row(
            format!("{scroll_pane_left} / {scroll_pane_right}"),
            "scroll pane left / right",
        ),
        row("Alt+←→".into(), "resize pane"),
        row("Enter".into(), "open channel / message"),
        row("Esc".into(), "go back"),
        row(format!("{quit} / Ctrl+C"), "quit"),
        Line::from(""),
        section(format!("Leader  [{open_leader}]")),
        row(format!("{open_leader} → 1/2/4"), "toggle pane visibility"),
        row(format!("{open_leader} → a"), "actions"),
        row(format!("{open_leader} → o"), "options"),
        row(format!("{open_leader} → {open_leader}"), "channel switcher"),
        Line::from(""),
        section("Composer".into()),
        row(open_composer, "open composer"),
        row("Enter".into(), "send message"),
        row("Shift+Enter".into(), "newline"),
        row(open_in_editor, "edit in $EDITOR"),
        row("Ctrl+←→".into(), "jump by word"),
        row("Esc".into(), "cancel"),
        Line::from(""),
        section("Messages".into()),
        row("Space → a".into(), "message actions"),
        row("r".into(), "react with emoji"),
        row(
            format!("{scroll_viewport_down} / {scroll_viewport_up}"),
            "scroll viewport",
        ),
        row(open_keymap, "show this help"),
    ]
}
