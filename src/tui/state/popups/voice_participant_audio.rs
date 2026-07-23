use super::*;

const VOICE_PARTICIPANT_AUDIO_FIELD_COUNT: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum VoiceParticipantAudioField {
    Volume,
    Muted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::tui) struct VoiceParticipantAudioPopupView {
    pub display_name: String,
    pub selected: VoiceParticipantAudioField,
    pub settings: VoiceParticipantPlaybackSettings,
}

#[derive(Debug)]
pub(in crate::tui::state) struct VoiceParticipantAudioPopupState {
    user_id: Id<UserMarker>,
    display_name: String,
    selection: SelectablePopupState,
}

impl VoiceParticipantAudioPopupState {
    fn selected_field(&self) -> VoiceParticipantAudioField {
        match self
            .selection
            .selected_for_len(VOICE_PARTICIPANT_AUDIO_FIELD_COUNT)
        {
            0 => VoiceParticipantAudioField::Volume,
            _ => VoiceParticipantAudioField::Muted,
        }
    }
}

impl DashboardState {
    pub(in crate::tui::state) fn open_voice_participant_audio_popup(
        &mut self,
        user_id: Id<UserMarker>,
        display_name: String,
    ) {
        self.popups.modal = Some(ModalPopup::VoiceParticipantAudio(
            VoiceParticipantAudioPopupState {
                user_id,
                display_name,
                selection: SelectablePopupState::default(),
            },
        ));
    }

    pub(in crate::tui) fn close_voice_participant_audio_popup(&mut self) {
        if self.is_active_modal_popup(ActiveModalPopupKind::VoiceParticipantAudio) {
            self.popups.clear_modal();
        }
    }

    pub(in crate::tui) fn voice_participant_audio_popup_view(
        &self,
    ) -> Option<VoiceParticipantAudioPopupView> {
        let popup = self.popups.voice_participant_audio()?;
        Some(VoiceParticipantAudioPopupView {
            display_name: popup.display_name.clone(),
            selected: popup.selected_field(),
            settings: self.voice_participant_playback_settings(popup.user_id),
        })
    }

    pub(in crate::tui) fn move_voice_participant_audio_selection(
        &mut self,
        action: SelectionAction,
    ) {
        let Some(popup) = self.popups.voice_participant_audio_mut() else {
            return;
        };
        match action {
            SelectionAction::Next => popup
                .selection
                .move_down(VOICE_PARTICIPANT_AUDIO_FIELD_COUNT),
            SelectionAction::Previous => popup.selection.move_up(),
        }
    }

    pub(in crate::tui) fn adjust_voice_participant_audio_volume(
        &mut self,
        delta: i8,
    ) -> Option<AppCommand> {
        let popup = self.popups.voice_participant_audio()?;
        if popup.selected_field() != VoiceParticipantAudioField::Volume {
            return None;
        }
        let user_id = popup.user_id;
        let mut settings = self.voice_participant_playback_settings(user_id);
        let adjusted_volume = settings.volume.adjust(i16::from(delta));
        if adjusted_volume == settings.volume {
            return None;
        }
        settings.volume = adjusted_volume;
        Some(self.update_voice_participant_playback_settings(user_id, settings))
    }

    pub(in crate::tui) fn activate_voice_participant_audio_field(&mut self) -> Option<AppCommand> {
        let popup = self.popups.voice_participant_audio()?;
        if popup.selected_field() != VoiceParticipantAudioField::Muted {
            return None;
        }
        let user_id = popup.user_id;
        let mut settings = self.voice_participant_playback_settings(user_id);
        settings.muted = !settings.muted;
        Some(self.update_voice_participant_playback_settings(user_id, settings))
    }
}
