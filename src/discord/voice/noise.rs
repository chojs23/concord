use nnnoiseless::DenoiseState;

use super::DISCORD_OPUS_20MS_STEREO_SAMPLES;

const RNNOISE_FRAMES_PER_OPUS_FRAME: usize = 2;

pub(super) struct VoiceNoiseSuppressor {
    state: Box<DenoiseState<'static>>,
    input: [f32; DenoiseState::FRAME_SIZE],
    output: [f32; DenoiseState::FRAME_SIZE],
}

impl VoiceNoiseSuppressor {
    pub(super) fn new() -> Self {
        let mut suppressor = Self {
            state: DenoiseState::new(),
            input: [0.0; DenoiseState::FRAME_SIZE],
            output: [0.0; DenoiseState::FRAME_SIZE],
        };
        suppressor.prime();
        suppressor
    }

    pub(super) fn reset(&mut self) {
        self.state = DenoiseState::new();
        self.input.fill(0.0);
        self.output.fill(0.0);
        self.prime();
    }

    pub(super) fn process_20ms_stereo(&mut self, samples: &mut [i16]) -> bool {
        if samples.len() != DISCORD_OPUS_20MS_STEREO_SAMPLES {
            return false;
        }

        for frame_index in 0..RNNOISE_FRAMES_PER_OPUS_FRAME {
            let mono_start = frame_index * DenoiseState::FRAME_SIZE;
            for sample_index in 0..DenoiseState::FRAME_SIZE {
                let stereo_index = (mono_start + sample_index) * 2;
                self.input[sample_index] =
                    (f32::from(samples[stereo_index]) + f32::from(samples[stereo_index + 1])) * 0.5;
            }

            self.state.process_frame(&mut self.output, &self.input);

            // Voice capture is treated as mono even when the device supplies two
            // channels. Writing the same cleaned sample to both channels avoids
            // phase differences that can weaken speech during downmixing.
            for sample_index in 0..DenoiseState::FRAME_SIZE {
                let stereo_index = (mono_start + sample_index) * 2;
                let sample = self.output[sample_index]
                    .round()
                    .clamp(f32::from(i16::MIN), f32::from(i16::MAX))
                    as i16;
                samples[stereo_index] = sample;
                samples[stereo_index + 1] = sample;
            }
        }

        true
    }

    fn prime(&mut self) {
        // RNNoise documents a fade-in artifact on its first output frame.
        // Processing silence once keeps that artifact out of the first live frame.
        self.state.process_frame(&mut self.output, &self.input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_suppressor_reduces_stationary_broadband_noise() {
        let mut suppressor = VoiceNoiseSuppressor::new();
        let mut seed = 1_u32;
        let mut input_energy = 0_u128;
        let mut output_energy = 0_u128;

        for frame_index in 0..40 {
            let mut samples = vec![0_i16; DISCORD_OPUS_20MS_STEREO_SAMPLES];
            for stereo_sample in samples.as_chunks_mut::<2>().0 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let sample = ((seed >> 16) as i16) / 4;
                stereo_sample.fill(sample);
            }
            let before = samples.clone();

            assert!(suppressor.process_20ms_stereo(&mut samples));
            assert!(
                samples
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|stereo_sample| stereo_sample[0] == stereo_sample[1])
            );

            // Give the stateful model time to classify the stable broadband
            // input before comparing the energy it removes.
            if frame_index >= 10 {
                input_energy += pcm_energy(&before);
                output_energy += pcm_energy(&samples);
            }
        }

        assert!(output_energy < input_energy);
    }

    #[test]
    fn noise_suppressor_rejects_an_incomplete_opus_frame() {
        let mut suppressor = VoiceNoiseSuppressor::new();
        let mut samples = vec![42_i16; DISCORD_OPUS_20MS_STEREO_SAMPLES - 1];

        assert!(!suppressor.process_20ms_stereo(&mut samples));
        assert!(samples.iter().all(|sample| *sample == 42));
    }

    fn pcm_energy(samples: &[i16]) -> u128 {
        samples
            .iter()
            .map(|sample| i128::from(*sample).pow(2) as u128)
            .sum()
    }
}
