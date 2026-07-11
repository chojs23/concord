use cpal::traits::DeviceTrait;

pub(crate) trait F32OutputSource {
    fn fill<T>(&mut self, output: &mut [T], channels: usize, convert: fn(f32) -> T)
    where
        T: Default + Copy;
}

pub(crate) fn build_f32_output_stream<S, E>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    source: S,
    error_callback: E,
    unsupported_context: &str,
    build_context: &str,
) -> Result<cpal::Stream, String>
where
    S: F32OutputSource + Send + 'static,
    E: FnMut(cpal::Error) + Send + 'static,
{
    match sample_format {
        cpal::SampleFormat::F32 => build_f32_output_stream_as::<f32, _, _>(
            device,
            config,
            source,
            error_callback,
            clamp_f32_sample,
            build_context,
        ),
        cpal::SampleFormat::U8 => build_f32_output_stream_as::<u8, _, _>(
            device,
            config,
            source,
            error_callback,
            f32_sample_to_u8,
            build_context,
        ),
        cpal::SampleFormat::I16 => build_f32_output_stream_as::<i16, _, _>(
            device,
            config,
            source,
            error_callback,
            f32_sample_to_i16,
            build_context,
        ),
        cpal::SampleFormat::U16 => build_f32_output_stream_as::<u16, _, _>(
            device,
            config,
            source,
            error_callback,
            f32_sample_to_u16,
            build_context,
        ),
        other => Err(format!(
            "unsupported {unsupported_context} sample format: {other:?}"
        )),
    }
}

fn build_f32_output_stream_as<T, S, E>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut source: S,
    error_callback: E,
    convert: fn(f32) -> T,
    build_context: &str,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + Default + Copy + 'static,
    S: F32OutputSource + Send + 'static,
    E: FnMut(cpal::Error) + Send + 'static,
{
    let channels = usize::from(config.channels.max(1));
    device
        .build_output_stream(
            *config,
            move |output: &mut [T], _| source.fill(output, channels, convert),
            error_callback,
            None,
        )
        .map_err(|error| format!("{build_context} output stream build failed: {error}"))
}

pub(crate) fn clamp_f32_sample(sample: f32) -> f32 {
    sample.clamp(-1.0, 1.0)
}

pub(crate) fn f32_sample_to_u8(sample: f32) -> u8 {
    ((clamp_f32_sample(sample) + 1.0) * 0.5 * f32::from(u8::MAX)).round() as u8
}

pub(crate) fn f32_sample_to_i16(sample: f32) -> i16 {
    (clamp_f32_sample(sample) * f32::from(i16::MAX)).round() as i16
}

pub(crate) fn f32_sample_to_u16(sample: f32) -> u16 {
    ((clamp_f32_sample(sample) + 1.0) * 0.5 * f32::from(u16::MAX)).round() as u16
}

#[cfg(test)]
mod tests {
    use super::{f32_sample_to_i16, f32_sample_to_u8, f32_sample_to_u16};

    #[test]
    fn converts_f32_samples_to_integer_output_ranges() {
        assert_eq!(f32_sample_to_u8(1.0), u8::MAX);
        assert_eq!(f32_sample_to_u8(-1.0), 0);
        assert_eq!(f32_sample_to_i16(1.0), i16::MAX);
        assert_eq!(f32_sample_to_i16(-1.0), -i16::MAX);
        assert_eq!(f32_sample_to_u16(1.0), u16::MAX);
        assert_eq!(f32_sample_to_u16(-1.0), 0);
    }
}
