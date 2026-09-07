use super::*;
use crate::tui::keybindings::UiAction;
use crate::tui::media::MediaCacheStats;
use crate::tui::state::DebugLogLine;
use crate::tui::ui::panes::{render_pane_filter_bar_with_cursor, split_pane_filter_area};

const DEBUG_PANEL_WIDTH: u16 = 112;
const DEBUG_PANEL_HEIGHT: u16 = 36;

/// Drawing, wrapping, hit testing and media occlusion use the same stable bounds.
pub(in crate::tui::ui) fn debug_panel_area(area: Rect) -> Rect {
    centered_rect(area, DEBUG_PANEL_WIDTH, DEBUG_PANEL_HEIGHT).intersection(area)
}

struct DebugPanelLayout {
    popup: Rect,
    summary: Rect,
    logs: Rect,
    filter: Option<Rect>,
    footer: Rect,
}

impl DebugPanelLayout {
    fn new(area: Rect, filtering: bool) -> Self {
        let popup = debug_panel_area(area);
        let inner = panel_block("", true).inner(popup);
        // Keep history usable on short terminals instead of letting metrics
        // push the entire log viewport below the fold.
        let summary_height = match inner.height {
            22.. => 10,
            15.. => 7,
            9.. => 1,
            _ => 0,
        };
        let footer_height = u16::from(inner.height >= 4);
        let summary = Rect {
            height: summary_height,
            ..inner
        };
        let logs = Rect {
            y: inner.y + summary_height,
            height: inner.height.saturating_sub(summary_height + footer_height),
            ..inner
        };
        let footer = Rect {
            y: logs.bottom(),
            height: footer_height,
            ..inner
        };
        let (logs, filter) =
            split_pane_filter_area(logs, filtering && logs.height >= 3 && logs.width > 0);
        Self {
            popup,
            summary,
            logs,
            filter,
            footer,
        }
    }

    fn log_content(&self) -> Rect {
        Block::default().borders(Borders::TOP).inner(self.logs)
    }
}

pub(in crate::tui::ui) fn sync_debug_panel(area: Rect, state: &mut DashboardState) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::DebugLog) {
        return;
    }
    let query = state
        .debug_log_filter_query()
        .unwrap_or_default()
        .to_lowercase();
    let content =
        DebugPanelLayout::new(area, state.debug_log_filter_query().is_some()).log_content();
    let lines = wrap_debug_entries(
        state
            .debug_log_entries()
            .iter()
            .filter(|entry| query.is_empty() || entry.text.to_lowercase().contains(&query))
            .map(|entry| (entry.offset, entry.text.as_str())),
        usize::from(content.width.saturating_sub(1)),
    );
    state.sync_debug_log_lines(lines, usize::from(content.height));
}

fn wrap_debug_entries<S: AsRef<str>>(
    entries: impl IntoIterator<Item = (u64, S)>,
    width: usize,
) -> Vec<DebugLogLine> {
    entries
        .into_iter()
        .flat_map(|(entry_id, text)| {
            let text = text.as_ref();
            if text.is_empty() {
                return vec![DebugLogLine {
                    entry_id,
                    source_start: 0,
                    text: String::new(),
                }];
            }
            wrap_text_with_metadata(text, &[], &[], width.max(1))
                .into_iter()
                .map(move |line| DebugLogLine {
                    entry_id,
                    source_start: line.source_start,
                    text: line.text,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(in crate::tui::ui) fn render_debug_panel(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    if !state.is_active_modal_popup(ActiveModalPopupKind::DebugLog) {
        return;
    }
    let layout = DebugPanelLayout::new(area, state.debug_log_filter_query().is_some());
    render_modal_frame(frame, layout.popup, "Diagnostics");
    frame.render_widget(
        Paragraph::new(summary_lines(state, layout.summary)),
        layout.summary,
    );
    render_pane_filter_bar_with_cursor(
        frame,
        layout.filter,
        state.debug_log_filter_query(),
        state.debug_log_filter_cursor(),
        state.debug_log_filter_cursor().is_some(),
    );

    let theme = theme::current();
    let lines = state.debug_log_lines();
    let entry_count = lines.iter().filter(|line| line.source_start == 0).count();
    let mut title = vec![Span::styled(
        " Logs ",
        theme.style(theme::HighlightGroup::Heading),
    )];
    if !state.debug_log_following() {
        title.push(Span::styled(
            "PAUSED ",
            theme.style(theme::HighlightGroup::Warning),
        ));
    }
    let count = if state
        .debug_log_filter_query()
        .is_some_and(|query| !query.is_empty())
    {
        format!("· {entry_count}/{} lines ", state.debug_log_entries().len())
    } else {
        format!("· {entry_count} lines ")
    };
    title.push(Span::styled(
        count,
        theme.style(theme::HighlightGroup::Muted),
    ));
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(theme.style(theme::HighlightGroup::ModalBorder))
        .title(Line::from(title));
    frame.render_widget(block, layout.logs);

    let content = layout.log_content();
    let text_area = Rect {
        width: content.width.saturating_sub(1),
        ..content
    };
    let viewport = usize::from(content.height);
    let scroll = state.debug_log_scroll();
    if let Some(error) = state.debug_log_error() {
        frame.render_widget(
            Paragraph::new(error)
                .style(theme.style(theme::HighlightGroup::Error))
                .wrap(Wrap { trim: false }),
            text_area,
        );
    } else if lines.is_empty() {
        frame.render_widget(
            Paragraph::new("Empty").style(theme.style(theme::HighlightGroup::Placeholder)),
            text_area,
        );
    } else {
        let visible = lines
            .iter()
            .skip(scroll)
            .take(viewport)
            .map(log_line)
            .collect::<Vec<_>>();
        // Already wrapped to the exact text width. A second wrap would break
        // both line anchors and the one-row scroll contract.
        frame.render_widget(Paragraph::new(visible), text_area);
        render_vertical_scrollbar(frame, content, scroll, viewport, lines.len());
    }

    let range = if lines.is_empty() {
        "0/0".to_owned()
    } else {
        format!(
            "{}-{}/{}",
            scroll + 1,
            (scroll + viewport).min(lines.len()),
            lines.len()
        )
    };
    let range_width = (range.len() as u16).min(layout.footer.width);
    let help_area = Rect {
        width: layout.footer.width.saturating_sub(range_width + 1),
        ..layout.footer
    };
    let range_area = Rect {
        x: layout.footer.right().saturating_sub(range_width),
        width: range_width,
        ..layout.footer
    };
    let help = if state.debug_log_filter_cursor().is_some() {
        "Enter apply · Esc clear".to_owned()
    } else {
        let filter_key = state.key_bindings().binding_label(UiAction::OpenPaneFilter);
        let filter_hint = if filter_key.is_empty() {
            String::new()
        } else {
            format!("{filter_key} filter · ")
        };
        let jump_hints = [(UiAction::JumpTop, "first"), (UiAction::JumpBottom, "live")]
            .into_iter()
            .filter_map(|(action, label)| {
                let key = state.key_bindings().binding_label(action);
                (!key.is_empty()).then(|| format!("{key} {label} · "))
            })
            .collect::<String>();
        let close_hint = if state.debug_log_filter_query().is_some() {
            "Esc clear"
        } else {
            "Esc close"
        };
        if help_area.width >= 80 {
            format!("{filter_hint}↑↓ / wheel · PgUp/PgDn · {jump_hints}{close_hint}")
        } else if help_area.width >= 52 {
            format!("{filter_hint}↑↓ · {jump_hints}{close_hint}")
        } else {
            format!("{filter_hint}↑↓ · {close_hint}")
        }
    };
    let style = theme.style(theme::HighlightGroup::Hint);
    frame.render_widget(Paragraph::new(help).style(style), help_area);
    frame.render_widget(
        Paragraph::new(range)
            .alignment(Alignment::Right)
            .style(style),
        range_area,
    );
}

fn log_line(line: &DebugLogLine) -> Line<'_> {
    let theme = theme::current();
    if line.source_start == 0 {
        for (level, group) in [
            ("[ERROR] ", theme::HighlightGroup::Error),
            ("[DEBUG] ", theme::HighlightGroup::Muted),
        ] {
            if let Some((timestamp, message)) = line.text.split_once(level) {
                return Line::from(vec![
                    Span::styled(timestamp, theme.style(theme::HighlightGroup::Timestamp)),
                    Span::styled(level, theme.style(group)),
                    Span::raw(message),
                ]);
            }
        }
    }
    Line::from(line.text.as_str())
}

fn mib(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0))
}

fn cache_line(name: &str, stats: &MediaCacheStats, width: u16) -> Line<'static> {
    let theme = theme::current();
    let filled = stats
        .decoded_bytes
        .saturating_mul(8)
        .checked_div(stats.decoded_byte_budget)
        .unwrap_or(0)
        .min(8) as usize;
    let wide = width >= 76;
    let usage = if wide {
        format!("{}{}", "━".repeat(filled), "─".repeat(8 - filled))
    } else {
        String::new()
    };
    let mut spans = vec![
        Span::styled(
            format!("{name:<10}"),
            theme.style(theme::HighlightGroup::Strong),
        ),
        Span::raw(format!(
            " {:>5}  ",
            format!("{}/{}", stats.entries, stats.entry_limit)
        )),
        Span::styled(
            usage,
            theme.style(if stats.decoded_bytes > stats.decoded_byte_budget {
                theme::HighlightGroup::Warning
            } else {
                theme::HighlightGroup::Success
            }),
        ),
        Span::raw(format!(
            " {:>6}/{:<5} MiB",
            mib(stats.decoded_bytes),
            mib(stats.decoded_byte_budget)
        )),
    ];
    if width >= 56 {
        spans.push(Span::styled(
            format!("  {:>5}", stats.ready),
            theme.style(theme::HighlightGroup::Success),
        ));
        spans.push(Span::raw(format!(
            " {:>5} ",
            stats.loading + stats.decoding
        )));
        spans.push(Span::styled(
            format!("{:>5}", stats.failed),
            theme.style(if stats.failed == 0 {
                theme::HighlightGroup::Muted
            } else {
                theme::HighlightGroup::Error
            }),
        ));
    }
    if wide {
        spans.push(Span::raw(format!(
            "  {:>6} MiB",
            mib(stats.render_protocol_bytes)
        )));
    }
    Line::from(spans)
}

fn summary_lines(state: &DashboardState, area: Rect) -> Vec<Line<'static>> {
    let theme = theme::current();
    let Some(media) = state.debug_media_snapshot() else {
        return vec![Line::styled(
            "Media diagnostics unavailable",
            theme.style(theme::HighlightGroup::Placeholder),
        )];
    };
    let heading = Line::from(vec![
        Span::styled("MEDIA CACHE", theme.style(theme::HighlightGroup::Heading)),
        Span::styled(
            format!(
                "   {} · sources {}/{}",
                media.protocol.as_deref().unwrap_or("Images unavailable"),
                media.active_sources,
                media.source_limit
            ),
            theme.style(theme::HighlightGroup::Muted),
        ),
    ]);
    if area.height <= 1 {
        return vec![heading];
    }
    let header = if area.width >= 76 {
        format!(
            "{:<10} {:>5}  {:<25}  {:>5} {:>5} {:>5}  {:>10}",
            "Cache", "Items", "Decoded / budget", "Ready", "Wait", "Fail", "Render"
        )
    } else if area.width >= 56 {
        format!(
            "{:<10} {:>5}  {:<17}  {:>5} {:>5} {:>5}",
            "Cache", "Items", "Decoded / budget", "Ready", "Wait", "Fail"
        )
    } else {
        "Cache       Items   Decoded / budget".to_owned()
    };
    let mut lines = vec![
        heading,
        Line::styled(header, theme.style(theme::HighlightGroup::Muted)),
        cache_line("Previews", &media.previews, area.width),
        cache_line("Avatars", &media.avatars, area.width),
        cache_line("Emoji", &media.emojis, area.width),
        Line::from(format!(
            "Shared     {}/{} ready · {}/{} MiB · {} decoding",
            media.shared.ready,
            media.shared.ready_limit,
            mib(media.shared.ready_decoded_bytes),
            mib(media.shared.decoded_byte_budget),
            media.shared.decoding
        )),
        Line::styled(
            "Shared image references overlap. Values are not total process RAM.",
            theme.style(theme::HighlightGroup::Muted),
        ),
    ];
    if area.height > 7 {
        lines.extend([
            Line::from(format!(
                "Work       {} waiting consumers · {} decode retries · {} MiB source bytes",
                media.shared.pending_requests,
                media.shared.retry_pending,
                mib(media.shared.retained_source_bytes)
            )),
            Line::from(format!(
                "Fetch      {} loading · {} decoding · {} failed · {} retryable",
                media.previews.loading + media.avatars.loading + media.emojis.loading,
                media.previews.decoding + media.avatars.decoding + media.emojis.decoding,
                media.previews.failed + media.avatars.failed + media.emojis.failed,
                media.previews.retryable + media.avatars.retryable + media.emojis.retryable
            )),
            Line::default(),
        ]);
    }
    lines
}

#[cfg(test)]
#[path = "debug_panel_tests.rs"]
mod tests;
