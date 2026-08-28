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
        cpal::SampleFormat::F64 => build_f32_output_stream_as::<f64, _, _>(
            device,
            config,
            source,
            error_callback,
            f32_sample_to_f64,
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
        cpal::SampleFormat::I8 => build_f32_output_stream_as::<i8, _, _>(
            device,
            config,
            source,
            error_callback,
            f32_sample_to_i8,
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
        cpal::SampleFormat::I24 => build_f32_output_stream_as::<cpal::I24, _, _>(
            device,
            config,
            source,
            error_callback,
            f32_sample_to_i24,
            build_context,
        ),
        cpal::SampleFormat::I32 => build_f32_output_stream_as::<i32, _, _>(
            device,
            config,
            source,
            error_callback,
            f32_sample_to_i32,
            build_context,
        ),
        cpal::SampleFormat::I64 => build_f32_output_stream_as::<i64, _, _>(
            device,
            config,
            source,
            error_callback,
            f32_sample_to_i64,
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
        cpal::SampleFormat::U24 => build_f32_output_stream_as::<cpal::U24, _, _>(
            device,
            config,
            source,
            error_callback,
            f32_sample_to_u24,
            build_context,
        ),
        cpal::SampleFormat::U32 => build_f32_output_stream_as::<u32, _, _>(
            device,
            config,
            source,
            error_callback,
            f32_sample_to_u32,
            build_context,
        ),
        cpal::SampleFormat::U64 => build_f32_output_stream_as::<u64, _, _>(
            device,
            config,
            source,
            error_callback,
            f32_sample_to_u64,
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

pub(crate) fn f32_sample_to_f64(sample: f32) -> f64 {
    f64::from(clamp_f32_sample(sample))
}

pub(crate) fn f32_sample_to_u8(sample: f32) -> u8 {
    ((clamp_f32_sample(sample) + 1.0) * 0.5 * f32::from(u8::MAX)).round() as u8
}

pub(crate) fn f32_sample_to_i8(sample: f32) -> i8 {
    (clamp_f32_sample(sample) * f32::from(i8::MAX)).round() as i8
}

pub(crate) fn f32_sample_to_i16(sample: f32) -> i16 {
    (clamp_f32_sample(sample) * f32::from(i16::MAX)).round() as i16
}

pub(crate) fn f32_sample_to_i24(sample: f32) -> cpal::I24 {
    let value = (clamp_f32_sample(sample) * 8_388_607.0).round() as i32;
    cpal::I24::new(value).expect("clamped sample fits I24")
}

pub(crate) fn f32_sample_to_i32(sample: f32) -> i32 {
    (f64::from(clamp_f32_sample(sample)) * f64::from(i32::MAX)).round() as i32
}

pub(crate) fn f32_sample_to_i64(sample: f32) -> i64 {
    let sample = clamp_f32_sample(sample);
    if sample == 1.0 {
        i64::MAX
    } else if sample == -1.0 {
        -i64::MAX
    } else {
        (f64::from(sample) * i64::MAX as f64).round() as i64
    }
}

pub(crate) fn f32_sample_to_u16(sample: f32) -> u16 {
    ((clamp_f32_sample(sample) + 1.0) * 0.5 * f32::from(u16::MAX)).round() as u16
}

pub(crate) fn f32_sample_to_u24(sample: f32) -> cpal::U24 {
    let value = ((clamp_f32_sample(sample) + 1.0) * 0.5 * 16_777_215.0).round() as i32;
    cpal::U24::new(value).expect("clamped sample fits U24")
}

pub(crate) fn f32_sample_to_u32(sample: f32) -> u32 {
    ((f64::from(clamp_f32_sample(sample)) + 1.0) * 0.5 * f64::from(u32::MAX)).round() as u32
}

pub(crate) fn f32_sample_to_u64(sample: f32) -> u64 {
    let sample = clamp_f32_sample(sample);
    if sample == 1.0 {
        u64::MAX
    } else if sample == -1.0 {
        u64::MIN
    } else {
        ((f64::from(sample) + 1.0) * 0.5 * u64::MAX as f64).round() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::{
        f32_sample_to_f64, f32_sample_to_i8, f32_sample_to_i16, f32_sample_to_i24,
        f32_sample_to_i32, f32_sample_to_i64, f32_sample_to_u8, f32_sample_to_u16,
        f32_sample_to_u24, f32_sample_to_u32, f32_sample_to_u64,
    };

    #[test]
    fn converts_f32_samples_to_output_ranges() {
        assert_eq!(f32_sample_to_f64(1.0), 1.0);
        assert_eq!(f32_sample_to_f64(-1.0), -1.0);
        assert_eq!(f32_sample_to_f64(0.0), 0.0);
        assert_eq!(f32_sample_to_u8(1.0), u8::MAX);
        assert_eq!(f32_sample_to_u8(-1.0), 0);
        assert_eq!(f32_sample_to_i8(1.0), i8::MAX);
        assert_eq!(f32_sample_to_i8(-1.0), -i8::MAX);
        assert_eq!(f32_sample_to_i8(0.0), 0);
        assert_eq!(f32_sample_to_i16(1.0), i16::MAX);
        assert_eq!(f32_sample_to_i16(-1.0), -i16::MAX);
        assert_eq!(f32_sample_to_i24(1.0).inner(), 8_388_607);
        assert_eq!(f32_sample_to_i24(-1.0).inner(), -8_388_607);
        assert_eq!(f32_sample_to_i24(0.0).inner(), 0);
        assert_eq!(f32_sample_to_i32(1.0), i32::MAX);
        assert_eq!(f32_sample_to_i32(-1.0), -i32::MAX);
        assert_eq!(f32_sample_to_i32(0.0), 0);
        assert_eq!(f32_sample_to_i64(1.0), i64::MAX);
        assert_eq!(f32_sample_to_i64(-1.0), -i64::MAX);
        assert_eq!(f32_sample_to_i64(0.0), 0);
        assert_eq!(f32_sample_to_u16(1.0), u16::MAX);
        assert_eq!(f32_sample_to_u16(-1.0), 0);
        assert_eq!(f32_sample_to_u24(1.0).inner(), 16_777_215);
        assert_eq!(f32_sample_to_u24(-1.0).inner(), 0);
        assert_eq!(f32_sample_to_u24(0.0).inner(), 8_388_608);
        assert_eq!(f32_sample_to_u32(1.0), u32::MAX);
        assert_eq!(f32_sample_to_u32(-1.0), u32::MIN);
        assert_eq!(f32_sample_to_u32(0.0), 1 << 31);
        assert_eq!(f32_sample_to_u64(1.0), u64::MAX);
        assert_eq!(f32_sample_to_u64(-1.0), u64::MIN);
        assert_eq!(f32_sample_to_u64(0.0), 1 << 63);
    }
}
