use super::*;
use crate::discord::VoiceParticipantVolumePercent;
use crate::tui::state::VoiceParticipantAudioField;

const VOICE_PARTICIPANT_AUDIO_POPUP_WIDTH: u16 = 52;
const VOICE_PARTICIPANT_AUDIO_POPUP_HEIGHT: u16 = 5;
const VOICE_PARTICIPANT_AUDIO_GAUGE_X_OFFSET: u16 = 7;

pub(in crate::tui::ui) fn render_voice_participant_audio_popup(
    frame: &mut Frame,
    area: Rect,
    state: &DashboardState,
) {
    let Some(view) = state.voice_participant_audio_popup_view() else {
        return;
    };
    let popup = voice_participant_audio_popup_area(area);
    let inner = render_modal_frame(frame, popup, format!("Audio: {}", view.display_name));
    frame.render_widget(
        Paragraph::new(voice_participant_audio_popup_lines(
            view.selected,
            view.settings.volume.label(),
            view.settings.muted,
        )),
        inner,
    );

    let gauge_style = theme::current().apply(
        theme::HighlightGroup::GaugeFill,
        theme::current().style(theme::HighlightGroup::Normal),
    );
    render_popup_gauge(
        frame,
        inner,
        PopupGauge {
            x_offset: VOICE_PARTICIPANT_AUDIO_GAUGE_X_OFFSET,
            width_margin: 12,
            y: inner.y.saturating_add(1),
            value: view.settings.volume.value(),
            maximum: VoiceParticipantVolumePercent::maximum(),
            style: gauge_style,
        },
    );
}

pub(in crate::tui::ui) fn voice_participant_audio_popup_area(area: Rect) -> Rect {
    centered_rect(
        area,
        VOICE_PARTICIPANT_AUDIO_POPUP_WIDTH,
        VOICE_PARTICIPANT_AUDIO_POPUP_HEIGHT,
    )
}

pub(in crate::tui::ui) fn voice_participant_audio_popup_lines(
    selected: VoiceParticipantAudioField,
    volume_label: String,
    muted: bool,
) -> Vec<Line<'static>> {
    let volume_selected = selected == VoiceParticipantAudioField::Volume;
    let muted_selected = selected == VoiceParticipantAudioField::Muted;
    let volume_style = selectable_popup_label_style(volume_selected, true);
    let detail_style = theme::current().style(theme::HighlightGroup::Description);
    let muted_style = selectable_popup_label_style(muted_selected, true);

    vec![
        selected_row_line(
            Line::from(vec![
                selectable_popup_marker(volume_selected),
                Span::styled(format!("[{volume_label}] "), volume_style),
                Span::styled("Volume", volume_style),
            ]),
            volume_selected,
        ),
        popup_gauge_line(
            VOICE_PARTICIPANT_AUDIO_GAUGE_X_OFFSET,
            "0%",
            format!("{}%", VoiceParticipantVolumePercent::maximum()),
            detail_style,
        ),
        selected_row_line(
            Line::from(vec![
                selectable_popup_marker(muted_selected),
                Span::styled(if muted { "[x] " } else { "[ ] " }, muted_style),
                Span::styled("Muted", muted_style),
            ]),
            muted_selected,
        ),
    ]
}
