use yuv::{
    YuvBiPlanarImage, YuvConversionMode, YuvRange, YuvStandardMatrix, p010_to_rgba,
    yuv_nv12_to_rgba,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum CapturePixelFormat {
    Rgba,
    Rgbx,
    Bgra,
    Bgrx,
    Xrgb210Le,
    Xbgr210Le,
    Rgbx102Le,
    Bgrx102Le,
    Nv12,
    P010Le,
}

impl CapturePixelFormat {
    pub(super) fn plane_count(self) -> usize {
        match self {
            Self::Nv12 | Self::P010Le => 2,
            _ => 1,
        }
    }

    pub(super) fn packed_row_length(self, width: u32) -> Result<Option<usize>, String> {
        match self {
            Self::Rgba
            | Self::Rgbx
            | Self::Bgra
            | Self::Bgrx
            | Self::Xrgb210Le
            | Self::Xbgr210Le
            | Self::Rgbx102Le
            | Self::Bgrx102Le => width
                .checked_mul(4)
                .and_then(|length| usize::try_from(length).ok())
                .map(Some)
                .ok_or_else(|| "capture row length is too large".to_owned()),
            Self::Nv12 | Self::P010Le => Ok(None),
        }
    }

    pub(super) fn plane_row_lengths(self, width: u32) -> Result<Vec<usize>, String> {
        if let Some(row_length) = self.packed_row_length(width)? {
            return Ok(vec![row_length]);
        }

        match self {
            Self::Nv12 => width
                .div_ceil(2)
                .checked_mul(2)
                .and_then(|uv_length| {
                    Some(vec![
                        usize::try_from(width).ok()?,
                        usize::try_from(uv_length).ok()?,
                    ])
                })
                .ok_or_else(|| "capture row length is too large".to_owned()),
            Self::P010Le => width
                .checked_mul(2)
                .zip(width.div_ceil(2).checked_mul(4))
                .and_then(|(y_length, uv_length)| {
                    Some(vec![
                        usize::try_from(y_length).ok()?,
                        usize::try_from(uv_length).ok()?,
                    ])
                })
                .ok_or_else(|| "capture row length is too large".to_owned()),
            _ => unreachable!("packed formats return before planar layout"),
        }
    }

    pub(super) fn plane_heights(self, height: u32) -> Vec<u32> {
        match self {
            Self::Nv12 | Self::P010Le => vec![height, height.div_ceil(2)],
            _ => vec![height],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum CaptureColorRange {
    #[default]
    Unknown,
    Full,
    Limited,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum CaptureColorMatrix {
    #[default]
    Unknown,
    Bt601,
    Bt709,
    Bt2020,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum CaptureTransferFunction {
    #[default]
    Unknown,
    Srgb,
    Bt709,
    Bt2020Ten,
    Pq,
    Hlg,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum CaptureColorPrimaries {
    #[default]
    Unknown,
    Bt709,
    Bt2020,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct CaptureColorInfo {
    pub(super) range: CaptureColorRange,
    pub(super) matrix: CaptureColorMatrix,
    pub(super) transfer: CaptureTransferFunction,
    pub(super) primaries: CaptureColorPrimaries,
}

pub(super) struct CapturePlane<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) offset: isize,
    pub(super) stride: isize,
}

pub(super) fn convert_capture_frame(
    planes: &[CapturePlane<'_>],
    width: u32,
    height: u32,
    format: CapturePixelFormat,
    color: CaptureColorInfo,
    rgba: &mut [u8],
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("capture frame dimensions must be non-zero".to_owned());
    }
    if planes.len() != format.plane_count() {
        return Err(format!(
            "capture format {format:?} requires {} planes, received {}",
            format.plane_count(),
            planes.len()
        ));
    }
    let output_length = rgba_length(width, height)?;
    if rgba.len() != output_length {
        return Err(format!(
            "capture RGBA output has the wrong length: required={output_length} available={}",
            rgba.len()
        ));
    }

    match format {
        CapturePixelFormat::Nv12 => convert_nv12(planes, width, height, color, rgba)?,
        CapturePixelFormat::P010Le => convert_p010(planes, width, height, color, rgba)?,
        _ => convert_packed(planes, width, height, format, color, rgba)?,
    }

    if matches!(
        format,
        CapturePixelFormat::Nv12 | CapturePixelFormat::P010Le
    ) {
        normalize_rgba_color(rgba, color);
    }
    Ok(())
}

fn convert_packed(
    planes: &[CapturePlane<'_>],
    width: u32,
    height: u32,
    format: CapturePixelFormat,
    color: CaptureColorInfo,
    rgba: &mut [u8],
) -> Result<(), String> {
    let row_length = format
        .packed_row_length(width)?
        .expect("packed capture format has a row length");
    // Eight-bit packed input differs from the RGBA output only by channel
    // order, so without color normalization it is a byte swizzle. The float
    // path below costs ~40ms for one 1440p frame, which does not fit in a
    // capture frame interval, and the packed cases reach it on every frame.
    let normalize = requires_color_normalization(color);
    for row in 0..height as usize {
        let source = plane_row(&planes[0], row, row_length, "packed RGB")?;
        let destination = &mut rgba[row * row_length..(row + 1) * row_length];
        match format {
            CapturePixelFormat::Rgba if !normalize => destination.copy_from_slice(source),
            CapturePixelFormat::Rgbx if !normalize => {
                swizzle_packed_row(source, destination, |pixel| {
                    [pixel[0], pixel[1], pixel[2], 255]
                });
            }
            CapturePixelFormat::Bgra if !normalize => {
                swizzle_packed_row(source, destination, |pixel| {
                    [pixel[2], pixel[1], pixel[0], pixel[3]]
                });
            }
            CapturePixelFormat::Bgrx if !normalize => {
                swizzle_packed_row(source, destination, |pixel| {
                    [pixel[2], pixel[1], pixel[0], 255]
                });
            }
            _ => {
                for (source, destination) in
                    source.chunks_exact(4).zip(destination.chunks_exact_mut(4))
                {
                    let [encoded_red, encoded_green, encoded_blue, alpha] =
                        packed_rgba(source, format);
                    let [red, green, blue] =
                        normalize_rgb([encoded_red, encoded_green, encoded_blue], color);
                    destination.copy_from_slice(&[red, green, blue, to_u8(alpha)]);
                }
            }
        }
    }
    Ok(())
}

/// Monomorphized per channel order so the indices stay constant and the copy
/// vectorizes; a runtime index table does not.
fn swizzle_packed_row(source: &[u8], destination: &mut [u8], swizzle: impl Fn(&[u8]) -> [u8; 4]) {
    for (source, destination) in source.chunks_exact(4).zip(destination.chunks_exact_mut(4)) {
        destination.copy_from_slice(&swizzle(source));
    }
}

fn packed_rgba(source: &[u8], format: CapturePixelFormat) -> [f32; 4] {
    match format {
        CapturePixelFormat::Rgba => [
            f32::from(source[0]) / 255.0,
            f32::from(source[1]) / 255.0,
            f32::from(source[2]) / 255.0,
            f32::from(source[3]) / 255.0,
        ],
        CapturePixelFormat::Rgbx => [
            f32::from(source[0]) / 255.0,
            f32::from(source[1]) / 255.0,
            f32::from(source[2]) / 255.0,
            1.0,
        ],
        CapturePixelFormat::Bgra => [
            f32::from(source[2]) / 255.0,
            f32::from(source[1]) / 255.0,
            f32::from(source[0]) / 255.0,
            f32::from(source[3]) / 255.0,
        ],
        CapturePixelFormat::Bgrx => [
            f32::from(source[2]) / 255.0,
            f32::from(source[1]) / 255.0,
            f32::from(source[0]) / 255.0,
            1.0,
        ],
        CapturePixelFormat::Xrgb210Le
        | CapturePixelFormat::Xbgr210Le
        | CapturePixelFormat::Rgbx102Le
        | CapturePixelFormat::Bgrx102Le => {
            let value = u32::from_le_bytes(
                source
                    .try_into()
                    .expect("packed 10-bit pixel contains four bytes"),
            );
            let (red, green, blue) = match format {
                CapturePixelFormat::Xrgb210Le => {
                    ((value >> 20) & 0x3ff, (value >> 10) & 0x3ff, value & 0x3ff)
                }
                CapturePixelFormat::Xbgr210Le => {
                    (value & 0x3ff, (value >> 10) & 0x3ff, (value >> 20) & 0x3ff)
                }
                CapturePixelFormat::Rgbx102Le => (
                    (value >> 22) & 0x3ff,
                    (value >> 12) & 0x3ff,
                    (value >> 2) & 0x3ff,
                ),
                CapturePixelFormat::Bgrx102Le => (
                    (value >> 2) & 0x3ff,
                    (value >> 12) & 0x3ff,
                    (value >> 22) & 0x3ff,
                ),
                _ => unreachable!("10-bit match contains only packed 10-bit formats"),
            };
            [
                red as f32 / 1023.0,
                green as f32 / 1023.0,
                blue as f32 / 1023.0,
                1.0,
            ]
        }
        CapturePixelFormat::Nv12 | CapturePixelFormat::P010Le => {
            unreachable!("planar formats use their dedicated conversion path")
        }
    }
}

fn convert_nv12(
    planes: &[CapturePlane<'_>],
    width: u32,
    height: u32,
    color: CaptureColorInfo,
    rgba: &mut [u8],
) -> Result<(), String> {
    let y = copy_plane_rows(&planes[0], width as usize, height, "NV12 Y")?;
    let uv_stride = width
        .div_ceil(2)
        .checked_mul(2)
        .ok_or_else(|| "NV12 UV stride is too large".to_owned())?;
    let uv = copy_plane_rows(
        &planes[1],
        uv_stride as usize,
        height.div_ceil(2),
        "NV12 UV",
    )?;
    let image = YuvBiPlanarImage {
        y_plane: &y,
        y_stride: width,
        uv_plane: &uv,
        uv_stride,
        width,
        height,
    };
    yuv_nv12_to_rgba(
        &image,
        rgba,
        width
            .checked_mul(4)
            .ok_or_else(|| "capture RGBA stride is too large".to_owned())?,
        yuv_range(color.range),
        yuv_matrix(color.matrix),
        YuvConversionMode::Fast,
    )
    .map_err(|error| format!("NV12 to RGBA conversion failed: {error}"))
}

fn convert_p010(
    planes: &[CapturePlane<'_>],
    width: u32,
    height: u32,
    color: CaptureColorInfo,
    rgba: &mut [u8],
) -> Result<(), String> {
    let y_row_bytes = width
        .checked_mul(2)
        .and_then(|length| usize::try_from(length).ok())
        .ok_or_else(|| "P010 row length is too large".to_owned())?;
    let y = p010_samples(copy_plane_rows(&planes[0], y_row_bytes, height, "P010 Y")?);
    let uv_row_bytes = width
        .div_ceil(2)
        .checked_mul(4)
        .and_then(|length| usize::try_from(length).ok())
        .ok_or_else(|| "P010 UV row length is too large".to_owned())?;
    let uv = p010_samples(copy_plane_rows(
        &planes[1],
        uv_row_bytes,
        height.div_ceil(2),
        "P010 UV",
    )?);
    let image = YuvBiPlanarImage {
        y_plane: &y,
        y_stride: width,
        uv_plane: &uv,
        uv_stride: width.div_ceil(2) * 2,
        width,
        height,
    };
    p010_to_rgba(
        &image,
        rgba,
        width
            .checked_mul(4)
            .ok_or_else(|| "capture RGBA stride is too large".to_owned())?,
        yuv_range(color.range),
        yuv_matrix(color.matrix),
        YuvConversionMode::Fast,
    )
    .map_err(|error| format!("P010 to RGBA conversion failed: {error}"))
}

fn p010_samples(bytes: Vec<u8>) -> Vec<u16> {
    bytes
        .chunks_exact(2)
        .map(|sample| u16::from_le_bytes([sample[0], sample[1]]))
        .collect()
}

fn copy_plane_rows(
    plane: &CapturePlane<'_>,
    row_length: usize,
    rows: u32,
    name: &str,
) -> Result<Vec<u8>, String> {
    let output_length = row_length
        .checked_mul(rows as usize)
        .ok_or_else(|| format!("{name} output length overflowed"))?;
    let mut output = Vec::with_capacity(output_length);
    for row in 0..rows as usize {
        output.extend_from_slice(plane_row(plane, row, row_length, name)?);
    }
    Ok(output)
}

fn plane_row<'a>(
    plane: &'a CapturePlane<'_>,
    row: usize,
    row_length: usize,
    name: &str,
) -> Result<&'a [u8], String> {
    if plane.stride.unsigned_abs() < row_length {
        return Err(format!("{name} stride is shorter than its row"));
    }
    let offset = plane
        .stride
        .checked_mul(isize::try_from(row).map_err(|_| format!("{name} row index is too large"))?)
        .and_then(|row_offset| plane.offset.checked_add(row_offset))
        .ok_or_else(|| format!("{name} row offset overflowed"))?;
    let offset = usize::try_from(offset).map_err(|_| format!("{name} row offset is negative"))?;
    let end = offset
        .checked_add(row_length)
        .ok_or_else(|| format!("{name} row end overflowed"))?;
    plane.bytes.get(offset..end).ok_or_else(|| {
        format!(
            "{name} plane is too short: required={end} available={}",
            plane.bytes.len()
        )
    })
}

fn rgba_length(width: u32, height: u32) -> Result<usize, String> {
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|length| usize::try_from(length).ok())
        .ok_or_else(|| "capture RGBA output is too large".to_owned())
}

fn yuv_range(range: CaptureColorRange) -> YuvRange {
    match range {
        CaptureColorRange::Full => YuvRange::Full,
        CaptureColorRange::Unknown | CaptureColorRange::Limited => YuvRange::Limited,
    }
}

fn yuv_matrix(matrix: CaptureColorMatrix) -> YuvStandardMatrix {
    match matrix {
        CaptureColorMatrix::Bt601 => YuvStandardMatrix::Bt601,
        CaptureColorMatrix::Bt2020 => YuvStandardMatrix::Bt2020,
        CaptureColorMatrix::Unknown | CaptureColorMatrix::Bt709 => YuvStandardMatrix::Bt709,
    }
}

pub(super) fn normalize_rgba_color(rgba: &mut [u8], color: CaptureColorInfo) {
    if !requires_color_normalization(color) {
        return;
    }
    for pixel in rgba.chunks_exact_mut(4) {
        let encoded = [
            f32::from(pixel[0]) / 255.0,
            f32::from(pixel[1]) / 255.0,
            f32::from(pixel[2]) / 255.0,
        ];
        let [red, green, blue] = normalize_rgb(encoded, color);
        pixel.copy_from_slice(&[red, green, blue, 255]);
    }
}

fn normalize_rgb(encoded: [f32; 3], color: CaptureColorInfo) -> [u8; 3] {
    if !requires_color_normalization(color) {
        return encoded.map(to_u8);
    }

    let mut linear = encoded.map(|value| decode_transfer(value, color.transfer));
    if color.primaries == CaptureColorPrimaries::Bt2020 {
        linear = [
            1.660_491 * linear[0] - 0.587_641 * linear[1] - 0.072_85 * linear[2],
            -0.124_55 * linear[0] + 1.132_9 * linear[1] - 0.008_349 * linear[2],
            -0.018_151 * linear[0] - 0.100_579 * linear[1] + 1.118_73 * linear[2],
        ];
    }
    if matches!(
        color.transfer,
        CaptureTransferFunction::Pq | CaptureTransferFunction::Hlg
    ) {
        let scale = if color.transfer == CaptureTransferFunction::Pq {
            10_000.0 / 203.0
        } else {
            1_000.0 / 203.0
        };
        linear = linear.map(|value| {
            let exposed = value.max(0.0) * scale;
            exposed / (1.0 + exposed)
        });
    }

    linear.map(|value| to_u8(encode_bt709(value.max(0.0))))
}

fn requires_color_normalization(color: CaptureColorInfo) -> bool {
    color.primaries == CaptureColorPrimaries::Bt2020
        || matches!(
            color.transfer,
            CaptureTransferFunction::Pq | CaptureTransferFunction::Hlg
        )
}

fn decode_transfer(value: f32, transfer: CaptureTransferFunction) -> f32 {
    let value = value.clamp(0.0, 1.0);
    match transfer {
        CaptureTransferFunction::Srgb => {
            if value <= 0.040_45 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        CaptureTransferFunction::Pq => {
            let m1 = 2610.0 / 16_384.0;
            let m2 = 2523.0 / 32.0;
            let c1 = 3424.0 / 4096.0;
            let c2 = 2413.0 / 128.0;
            let c3 = 2392.0 / 128.0;
            let power = value.powf(1.0 / m2);
            ((power - c1).max(0.0) / (c2 - c3 * power).max(f32::EPSILON)).powf(1.0 / m1)
        }
        CaptureTransferFunction::Hlg => {
            const A: f32 = 0.178_832_77;
            const B: f32 = 0.284_668_92;
            const C: f32 = 0.559_910_7;
            if value <= 0.5 {
                value * value / 3.0
            } else {
                (((value - C) / A).exp() + B) / 12.0
            }
        }
        CaptureTransferFunction::Unknown
        | CaptureTransferFunction::Bt709
        | CaptureTransferFunction::Bt2020Ten => {
            if value < 0.081 {
                value / 4.5
            } else {
                ((value + 0.099) / 1.099).powf(1.0 / 0.45)
            }
        }
    }
}

fn encode_bt709(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    if value < 0.018 {
        value * 4.5
    } else {
        1.099 * value.powf(0.45) - 0.099
    }
}

fn to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_rgb_formats_preserve_channel_order_and_quantize_ten_bit_values() {
        let cases = [
            (CapturePixelFormat::Rgba, [255, 0, 0, 17], 17),
            (CapturePixelFormat::Rgbx, [255, 0, 0, 17], 255),
            (CapturePixelFormat::Bgra, [0, 0, 255, 17], 17),
            (CapturePixelFormat::Bgrx, [0, 0, 255, 17], 255),
            (
                CapturePixelFormat::Xrgb210Le,
                (0x3ff_u32 << 20).to_le_bytes(),
                255,
            ),
            (CapturePixelFormat::Xbgr210Le, 0x3ff_u32.to_le_bytes(), 255),
            (
                CapturePixelFormat::Rgbx102Le,
                (0x3ff_u32 << 22).to_le_bytes(),
                255,
            ),
            (
                CapturePixelFormat::Bgrx102Le,
                (0x3ff_u32 << 2).to_le_bytes(),
                255,
            ),
        ];

        for (format, source, expected_alpha) in cases {
            let mut rgba = [0; 4];
            convert_capture_frame(
                &[CapturePlane {
                    bytes: &source,
                    offset: 0,
                    stride: 4,
                }],
                1,
                1,
                format,
                CaptureColorInfo::default(),
                &mut rgba,
            )
            .expect("supported packed RGB should convert");
            assert_eq!(rgba, [255, 0, 0, expected_alpha], "format={format:?}");
        }
    }

    #[test]
    fn nv12_and_p010_convert_independent_planes_to_opaque_rgba() {
        let cases = [
            (
                CapturePixelFormat::Nv12,
                vec![16, 235, 16, 235],
                vec![128, 128],
                2,
            ),
            (
                CapturePixelFormat::P010Le,
                [64_u16 << 6, 940_u16 << 6, 64_u16 << 6, 940_u16 << 6]
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
                [512_u16 << 6, 512_u16 << 6]
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
                4,
            ),
        ];

        for (format, y, uv, stride) in cases {
            let mut rgba = [0; 16];
            convert_capture_frame(
                &[
                    CapturePlane {
                        bytes: &y,
                        offset: 0,
                        stride,
                    },
                    CapturePlane {
                        bytes: &uv,
                        offset: 0,
                        stride,
                    },
                ],
                2,
                2,
                format,
                CaptureColorInfo {
                    range: CaptureColorRange::Limited,
                    matrix: CaptureColorMatrix::Bt709,
                    ..CaptureColorInfo::default()
                },
                &mut rgba,
            )
            .expect("supported bi-planar YUV should convert");

            assert_eq!(&rgba[0..4], &[0, 0, 0, 255], "format={format:?}");
            assert_eq!(&rgba[4..8], &[255, 255, 255, 255], "format={format:?}");
        }
    }

    #[test]
    fn bi_planar_formats_round_up_chroma_rows_for_odd_dimensions() {
        let nv12_y = vec![16; 9];
        let nv12_uv = vec![128; 8];
        let p010_y = [64_u16 << 6; 9]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let p010_uv = [512_u16 << 6; 8]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let cases = [
            (
                CapturePixelFormat::Nv12,
                nv12_y.as_slice(),
                nv12_uv.as_slice(),
                3,
                4,
            ),
            (
                CapturePixelFormat::P010Le,
                p010_y.as_slice(),
                p010_uv.as_slice(),
                6,
                8,
            ),
        ];

        for (format, y, uv, y_stride, uv_stride) in cases {
            let mut rgba = [0; 36];
            convert_capture_frame(
                &[
                    CapturePlane {
                        bytes: y,
                        offset: 0,
                        stride: y_stride,
                    },
                    CapturePlane {
                        bytes: uv,
                        offset: 0,
                        stride: uv_stride,
                    },
                ],
                3,
                3,
                format,
                CaptureColorInfo {
                    range: CaptureColorRange::Limited,
                    matrix: CaptureColorMatrix::Bt709,
                    ..CaptureColorInfo::default()
                },
                &mut rgba,
            )
            .expect("odd bi-planar dimensions should round up chroma rows");
            assert!(
                rgba.chunks_exact(4).all(|pixel| pixel == [0, 0, 0, 255]),
                "format={format:?}"
            );
        }
    }

    #[test]
    fn packed_conversion_accepts_bottom_up_rows() {
        let source = [0, 0, 255, 0, 0, 255, 0, 0];
        let mut rgba = [0; 8];

        convert_capture_frame(
            &[CapturePlane {
                bytes: &source,
                offset: 4,
                stride: -4,
            }],
            1,
            2,
            CapturePixelFormat::Bgrx,
            CaptureColorInfo::default(),
            &mut rgba,
        )
        .expect("bottom-up packed RGB should convert");

        assert_eq!(rgba, [0, 255, 0, 255, 255, 0, 0, 255]);
    }

    #[test]
    fn packed_byte_swizzle_matches_the_float_path_across_a_row() {
        // Four pixels so the row copy and the chunked swizzle both run, and one
        // gray value that survives normalization unchanged only if it is skipped.
        let cases = [
            (CapturePixelFormat::Rgba, [10, 20, 30, 40], [10, 20, 30, 40]),
            (
                CapturePixelFormat::Rgbx,
                [10, 20, 30, 40],
                [10, 20, 30, 255],
            ),
            (CapturePixelFormat::Bgra, [10, 20, 30, 40], [30, 20, 10, 40]),
            (
                CapturePixelFormat::Bgrx,
                [10, 20, 30, 40],
                [30, 20, 10, 255],
            ),
        ];

        for (format, pixel, expected) in cases {
            let source = pixel.repeat(4);
            let mut rgba = [0; 16];
            convert_capture_frame(
                &[CapturePlane {
                    bytes: &source,
                    offset: 0,
                    stride: 8,
                }],
                2,
                2,
                format,
                CaptureColorInfo::default(),
                &mut rgba,
            )
            .expect("packed row should convert");
            assert!(
                rgba.chunks_exact(4).all(|converted| converted == expected),
                "format={format:?} rgba={rgba:?}"
            );
        }

        // HDR still needs the float path, so the swizzle must not shortcut it.
        let mut normalized = [0; 16];
        convert_capture_frame(
            &[CapturePlane {
                bytes: &[10, 20, 30, 40].repeat(4),
                offset: 0,
                stride: 8,
            }],
            2,
            2,
            CapturePixelFormat::Bgra,
            CaptureColorInfo {
                transfer: CaptureTransferFunction::Pq,
                primaries: CaptureColorPrimaries::Bt2020,
                ..CaptureColorInfo::default()
            },
            &mut normalized,
        )
        .expect("HDR packed row should convert");
        assert_ne!(&normalized[0..4], &[30, 20, 10, 40]);
    }

    #[test]
    fn hdr_normalization_maps_bt2020_pq_into_bounded_bt709() {
        let source = (0x3ff_u32 << 20 | 0x200_u32 << 10 | 0x100_u32).to_le_bytes();
        let mut rgba = [0; 4];

        convert_capture_frame(
            &[CapturePlane {
                bytes: &source,
                offset: 0,
                stride: 4,
            }],
            1,
            1,
            CapturePixelFormat::Xrgb210Le,
            CaptureColorInfo {
                transfer: CaptureTransferFunction::Pq,
                primaries: CaptureColorPrimaries::Bt2020,
                ..CaptureColorInfo::default()
            },
            &mut rgba,
        )
        .expect("HDR packed RGB should normalize");

        assert_eq!(rgba[3], 255);
        assert!(rgba[0] >= rgba[1]);
        assert!(rgba[1] >= rgba[2]);
    }
}
