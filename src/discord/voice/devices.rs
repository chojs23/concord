#[cfg(feature = "voice-playback")]
use std::str::FromStr;

#[cfg(feature = "voice-playback")]
use crate::logging;
#[cfg(feature = "voice-playback")]
use cpal::traits::{DeviceTrait, HostTrait};

pub(crate) const SYSTEM_DEFAULT_AUDIO_SOURCE: &str = "System default";
const UNKNOWN_AUDIO_SOURCE: &str = "Unknown audio device";
const MAX_AUDIO_SOURCE_LABEL_CHARS: usize = 120;
pub(crate) type VoiceAudioSourceList = Vec<(String, String)>;

/// CPAL device ids selected for voice capture and playback.
///
/// `None` keeps the system default. CPAL 0.18 device ids are preferred over
/// display names because names may be duplicated while ids are intended to
/// remain stable across application restarts.
///
/// A selected id is kept even when the device is not currently enumerated, so
/// that a headset which is merely powered off keeps its slot. Everything that
/// consumes a selection treats an unknown id as the system default:
/// `source_label` shows it as such, `adjust_source` cycles from that position,
/// and `resolve_device` opens the default device.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct VoiceAudioSources {
    pub input: Option<String>,
    pub output: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VoiceAudioSource {
    id: String,
    label: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct VoiceAudioSourceOptions {
    inputs: Vec<VoiceAudioSource>,
    outputs: Vec<VoiceAudioSource>,
}

impl VoiceAudioSourceOptions {
    pub(crate) fn input_label(&self, selected: Option<&str>) -> String {
        source_label(selected, &self.inputs)
    }

    pub(crate) fn output_label(&self, selected: Option<&str>) -> String {
        source_label(selected, &self.outputs)
    }

    pub(crate) fn adjust_input(&self, selected: &mut Option<String>, delta: i8) -> bool {
        adjust_source(selected, &self.inputs, delta)
    }

    pub(crate) fn adjust_output(&self, selected: &mut Option<String>, delta: i8) -> bool {
        adjust_source(selected, &self.outputs, delta)
    }

    pub(crate) fn into_parts(self) -> (VoiceAudioSourceList, VoiceAudioSourceList) {
        (source_parts(self.inputs), source_parts(self.outputs))
    }

    pub(crate) fn from_parts(inputs: VoiceAudioSourceList, outputs: VoiceAudioSourceList) -> Self {
        Self {
            inputs: sources_from_parts(inputs),
            outputs: sources_from_parts(outputs),
        }
    }

    #[cfg(test)]
    pub(crate) fn test(inputs: &[(&str, &str)], outputs: &[(&str, &str)]) -> Self {
        Self {
            inputs: test_sources(inputs),
            outputs: test_sources(outputs),
        }
    }
}

pub(crate) fn list_voice_audio_sources() -> Result<VoiceAudioSourceOptions, String> {
    #[cfg(feature = "voice-playback")]
    {
        #[cfg(target_os = "linux")]
        let alsa_error_output = alsa::Output::local_error_handler().ok();

        let result = (|| {
            let host = cpal::default_host();
            let inputs = host
                .input_devices()
                .map(collect_audio_sources)
                .map_err(|error| format!("voice input source enumeration failed: {error}"))?;
            let outputs = host
                .output_devices()
                .map(collect_audio_sources)
                .map_err(|error| format!("voice output source enumeration failed: {error}"))?;
            Ok(VoiceAudioSourceOptions { inputs, outputs })
        })();

        #[cfg(target_os = "linux")]
        super::microphone::log_captured_alsa_errors(&alsa_error_output);

        result
    }
    #[cfg(not(feature = "voice-playback"))]
    {
        Ok(VoiceAudioSourceOptions::default())
    }
}

#[cfg(feature = "voice-playback")]
pub(super) fn resolve_input_device(
    host: &cpal::Host,
    selected: Option<&str>,
) -> Result<cpal::Device, String> {
    resolve_device(
        host,
        selected,
        HostTrait::default_input_device,
        DeviceTrait::supports_input,
        "microphone input",
    )
}

#[cfg(feature = "voice-playback")]
pub(super) fn resolve_output_device(
    host: &cpal::Host,
    selected: Option<&str>,
) -> Result<cpal::Device, String> {
    resolve_device(
        host,
        selected,
        HostTrait::default_output_device,
        DeviceTrait::supports_output,
        "audio output",
    )
}

fn source_label(selected: Option<&str>, sources: &[VoiceAudioSource]) -> String {
    let Some(selected) = selected else {
        return SYSTEM_DEFAULT_AUDIO_SOURCE.to_owned();
    };
    sources
        .iter()
        .find(|source| source.id == selected)
        .map(|source| source.label.clone())
        .unwrap_or_else(|| SYSTEM_DEFAULT_AUDIO_SOURCE.to_owned())
}

fn adjust_source(selected: &mut Option<String>, sources: &[VoiceAudioSource], delta: i8) -> bool {
    if delta == 0 {
        return false;
    }
    // An empty list means enumeration saw nothing at all, which happens on
    // builds without the `voice-playback` feature and while an audio server is
    // restarting. There is nothing to cycle through, and clearing the saved id
    // would throw away a choice that works again once devices come back.
    if sources.is_empty() {
        return false;
    }
    let choice_count = sources.len() + 1;
    let current = selected
        .as_deref()
        .and_then(|selected| sources.iter().position(|source| source.id == selected))
        .map(|index| index + 1)
        .unwrap_or(0);
    let next = (current as isize + isize::from(delta)).rem_euclid(choice_count as isize) as usize;
    let next = next.checked_sub(1).map(|index| sources[index].id.clone());
    if *selected == next {
        return false;
    }
    *selected = next;
    true
}

#[cfg(feature = "voice-playback")]
fn collect_audio_sources(devices: impl Iterator<Item = cpal::Device>) -> Vec<VoiceAudioSource> {
    let mut sources = devices
        .filter_map(|device| {
            let id = device.id().ok()?.to_string();
            Some(VoiceAudioSource {
                id,
                label: sanitize_audio_source_label(&device.to_string()),
            })
        })
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| {
        left.label
            .to_lowercase()
            .cmp(&right.label.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    sources.dedup_by(|left, right| left.id == right.id);
    sources
}

fn sanitize_audio_source_label(label: &str) -> String {
    let mut sanitized = String::new();
    let mut pending_space = false;
    for character in label.chars() {
        if character.is_control() || character.is_whitespace() {
            pending_space = !sanitized.is_empty();
            continue;
        }
        if pending_space {
            sanitized.push(' ');
            pending_space = false;
        }
        sanitized.push(character);
    }
    if sanitized.is_empty() {
        return UNKNOWN_AUDIO_SOURCE.to_owned();
    }
    if sanitized.chars().count() <= MAX_AUDIO_SOURCE_LABEL_CHARS {
        return sanitized;
    }
    sanitized
        .chars()
        .take(MAX_AUDIO_SOURCE_LABEL_CHARS.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
}

fn source_parts(sources: Vec<VoiceAudioSource>) -> Vec<(String, String)> {
    sources
        .into_iter()
        .map(|source| (source.id, source.label))
        .collect()
}

fn sources_from_parts(parts: Vec<(String, String)>) -> Vec<VoiceAudioSource> {
    parts
        .into_iter()
        .map(|(id, label)| VoiceAudioSource {
            id,
            label: sanitize_audio_source_label(&label),
        })
        .collect()
}

#[cfg(feature = "voice-playback")]
fn resolve_device(
    host: &cpal::Host,
    selected: Option<&str>,
    default_device: impl FnOnce(&cpal::Host) -> Option<cpal::Device>,
    supports_direction: impl FnOnce(&cpal::Device) -> bool,
    direction: &str,
) -> Result<cpal::Device, String> {
    if let Some(selected) = selected {
        let device = cpal::DeviceId::from_str(selected)
            .ok()
            .and_then(|id| host.device_by_id(&id));
        if let Some(device) = device.filter(supports_direction) {
            return Ok(device);
        }
        logging::debug(
            "voice",
            format!("selected voice {direction} source is unavailable, using system default"),
        );
    }
    default_device(host).ok_or_else(|| format!("no default {direction} device is available"))
}

#[cfg(test)]
fn test_sources(sources: &[(&str, &str)]) -> Vec<VoiceAudioSource> {
    sources
        .iter()
        .map(|(id, label)| VoiceAudioSource {
            id: (*id).to_owned(),
            label: (*label).to_owned(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_source_options_cycle_through_default_and_available_devices() {
        let options = VoiceAudioSourceOptions::test(
            &[("mic-1", "Desk microphone"), ("mic-2", "Webcam")],
            &[("speaker-1", "Headphones")],
        );
        let mut input = None;
        let mut output = Some("missing".to_owned());

        assert!(options.adjust_input(&mut input, 1));
        assert_eq!(input.as_deref(), Some("mic-1"));
        assert_eq!(options.input_label(input.as_deref()), "Desk microphone");
        assert!(options.adjust_input(&mut input, -1));
        assert_eq!(input, None);
        assert_eq!(
            options.input_label(input.as_deref()),
            SYSTEM_DEFAULT_AUDIO_SOURCE
        );

        assert_eq!(
            options.output_label(output.as_deref()),
            SYSTEM_DEFAULT_AUDIO_SOURCE
        );
        assert!(options.adjust_output(&mut output, -1));
        assert_eq!(output.as_deref(), Some("speaker-1"));
        assert_eq!(options.output_label(output.as_deref()), "Headphones");
        assert!(options.adjust_output(&mut output, 1));
        assert_eq!(output, None);

        let no_sources = VoiceAudioSourceOptions::default();
        input = Some("disconnected".to_owned());
        assert!(!no_sources.adjust_input(&mut input, 1));
        assert_eq!(input.as_deref(), Some("disconnected"));
    }

    #[test]
    fn unknown_source_ids_read_as_the_system_default_and_labels_are_sanitized() {
        let options = VoiceAudioSourceOptions::from_parts(
            vec![(
                "mic-1".to_owned(),
                " Desk\n\u{1b}[31m microphone ".to_owned(),
            )],
            Vec::new(),
        );

        assert_eq!(
            options.input_label(Some("missing-mic")),
            SYSTEM_DEFAULT_AUDIO_SOURCE
        );
        assert_eq!(
            options.output_label(Some("missing-output")),
            SYSTEM_DEFAULT_AUDIO_SOURCE
        );
        assert_eq!(options.input_label(Some("mic-1")), "Desk [31m microphone");
        assert_eq!(sanitize_audio_source_label("\n\t"), UNKNOWN_AUDIO_SOURCE);
        let bounded = sanitize_audio_source_label(&"a".repeat(200));
        assert_eq!(bounded.chars().count(), MAX_AUDIO_SOURCE_LABEL_CHARS);
        assert!(bounded.ends_with('…'));
    }
}
