// Portions of the SPS VUI policy are derived from the WebRTC project.
// Copyright (c) 2016 The WebRTC project authors. All Rights Reserved.
// WebRTC distributes this code under its BSD-style license.

use std::borrow::Cow;

const SPS_NAL_TYPE: u8 = 7;
const HIGH_PROFILE_IDS: [u8; 12] = [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134];

pub(super) fn normalize_annex_b_for_webrtc(frame: &mut Vec<u8>) -> Result<(), String> {
    let nals = crate::discord::voice::media::annex_b_nals(frame);
    if nals.is_empty() {
        return Err("H264 access unit contains no NAL units".to_owned());
    }

    let mut changed = false;
    let mut normalized = Vec::with_capacity(nals.len());
    for nal in nals {
        if nal
            .first()
            .is_some_and(|header| header & 0x1f == SPS_NAL_TYPE)
        {
            match rewrite_sps_for_webrtc(nal)? {
                Some(rewritten) => {
                    changed = true;
                    normalized.push(Cow::Owned(rewritten));
                }
                None => normalized.push(Cow::Borrowed(nal)),
            }
        } else {
            normalized.push(Cow::Borrowed(nal));
        }
    }

    if changed {
        let required = normalized
            .iter()
            .try_fold(0usize, |length, nal| {
                length.checked_add(4)?.checked_add(nal.len())
            })
            .ok_or_else(|| "normalized H264 access unit length overflowed".to_owned())?;
        let mut output = Vec::with_capacity(required);
        for nal in normalized {
            output.extend_from_slice(&[0, 0, 0, 1]);
            output.extend_from_slice(&nal);
        }
        *frame = output;
    }

    Ok(())
}

fn rewrite_sps_for_webrtc(nal: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let (&header, ebsp) = nal
        .split_first()
        .ok_or_else(|| "H264 SPS NAL unit is empty".to_owned())?;
    let rbsp = decode_rbsp(ebsp);
    let mut reader = BitReader::new(&rbsp);
    let state = parse_sps_up_to_vui(&mut reader)?;

    // Discord's receiver applies WebRTC's low-latency SPS rewrite before DAVE
    // verification. SPS bytes are authenticated, so the sender must make the
    // same change before DAVE sees the encoded frame.
    let mut writer = BitWriter::new();
    writer.copy_from(&rbsp, state.vui_flag_position)?;
    writer.write_bit(true);

    let rewritten = if state.vui_present {
        copy_and_rewrite_vui(&mut reader, &mut writer, state.max_num_ref_frames)?
    } else {
        write_low_latency_vui(&mut writer, state.max_num_ref_frames);
        true
    };

    if !rewritten {
        return Ok(None);
    }

    while reader.remaining_bits() > 0 {
        writer.write_bit(reader.read_bit()?);
    }

    let mut output = Vec::with_capacity(nal.len().saturating_add(64));
    output.push(header);
    encode_rbsp(&writer.into_bytes(), &mut output);
    Ok(Some(output))
}

struct SpsState {
    max_num_ref_frames: u32,
    vui_flag_position: usize,
    vui_present: bool,
}

fn parse_sps_up_to_vui(reader: &mut BitReader<'_>) -> Result<SpsState, String> {
    let profile_idc = reader.read_bits(8)? as u8;
    reader.skip_bits(16)?;
    reader.read_ue()?;

    if HIGH_PROFILE_IDS.contains(&profile_idc) {
        let chroma_format_idc = reader.read_ue()?;
        if chroma_format_idc > 3 {
            return Err(format!(
                "H264 SPS has invalid chroma_format_idc {chroma_format_idc}"
            ));
        }
        if chroma_format_idc == 3 {
            reader.skip_bits(1)?;
        }
        reader.read_ue()?;
        reader.read_ue()?;
        reader.skip_bits(1)?;
        if reader.read_bit()? {
            let scaling_list_count = if chroma_format_idc == 3 { 12 } else { 8 };
            for index in 0..scaling_list_count {
                if reader.read_bit()? {
                    skip_scaling_list(reader, if index < 6 { 16 } else { 64 })?;
                }
            }
        }
    }

    let log2_max_frame_num_minus4 = reader.read_ue()?;
    if log2_max_frame_num_minus4 > 12 {
        return Err("H264 SPS frame number width is too large".to_owned());
    }

    match reader.read_ue()? {
        0 => {
            if reader.read_ue()? > 12 {
                return Err("H264 SPS picture order count width is too large".to_owned());
            }
        }
        1 => {
            reader.skip_bits(1)?;
            reader.read_ue()?;
            reader.read_ue()?;
            let count = reader.read_ue()?;
            if count > 255 {
                return Err("H264 SPS picture order count cycle is too large".to_owned());
            }
            for _ in 0..count {
                reader.read_ue()?;
            }
        }
        _ => {}
    }

    let max_num_ref_frames = reader.read_ue()?;
    reader.skip_bits(1)?;
    reader.read_ue()?;
    reader.read_ue()?;
    let frame_mbs_only = reader.read_bit()?;
    if !frame_mbs_only {
        reader.skip_bits(1)?;
    }
    reader.skip_bits(1)?;
    if reader.read_bit()? {
        for _ in 0..4 {
            reader.read_ue()?;
        }
    }

    let vui_flag_position = reader.position();
    let vui_present = reader.read_bit()?;
    Ok(SpsState {
        max_num_ref_frames,
        vui_flag_position,
        vui_present,
    })
}

fn skip_scaling_list(reader: &mut BitReader<'_>, size: usize) -> Result<(), String> {
    let mut last_scale = 8i32;
    let mut next_scale = 8i32;
    for _ in 0..size {
        if next_scale != 0 {
            let delta_scale = reader.read_se()?;
            if !(-128..=127).contains(&delta_scale) {
                return Err("H264 SPS scaling-list delta is out of range".to_owned());
            }
            next_scale = (last_scale + delta_scale + 256) % 256;
        }
        if next_scale != 0 {
            last_scale = next_scale;
        }
    }
    Ok(())
}

fn copy_and_rewrite_vui(
    reader: &mut BitReader<'_>,
    writer: &mut BitWriter,
    max_num_ref_frames: u32,
) -> Result<bool, String> {
    // Preserve every VUI field except the final decoder-buffer restrictions.
    // This matches the receiver policy without changing color or timing data.
    if copy_bit(reader, writer)? && copy_bits(reader, writer, 8)? == 255 {
        copy_bits(reader, writer, 32)?;
    }
    if copy_bit(reader, writer)? {
        copy_bit(reader, writer)?;
    }
    if copy_bit(reader, writer)? {
        copy_bits(reader, writer, 3)?;
        copy_bit(reader, writer)?;
        if copy_bit(reader, writer)? {
            copy_bits(reader, writer, 24)?;
        }
    }
    if copy_bit(reader, writer)? {
        copy_ue(reader, writer)?;
        copy_ue(reader, writer)?;
    }
    if copy_bit(reader, writer)? {
        copy_bits(reader, writer, 32)?;
        copy_bits(reader, writer, 32)?;
        copy_bit(reader, writer)?;
    }

    let nal_hrd_present = copy_bit(reader, writer)?;
    if nal_hrd_present {
        copy_hrd_parameters(reader, writer)?;
    }
    let vcl_hrd_present = copy_bit(reader, writer)?;
    if vcl_hrd_present {
        copy_hrd_parameters(reader, writer)?;
    }
    if nal_hrd_present || vcl_hrd_present {
        copy_bit(reader, writer)?;
    }
    copy_bit(reader, writer)?;

    let restriction_present = reader.read_bit()?;
    writer.write_bit(true);
    if !restriction_present {
        write_bitstream_restriction(writer, max_num_ref_frames);
        return Ok(true);
    }

    copy_bit(reader, writer)?;
    for _ in 0..4 {
        copy_ue(reader, writer)?;
    }
    let max_num_reorder_frames = reader.read_ue()?;
    let max_dec_frame_buffering = reader.read_ue()?;
    writer.write_ue(0);
    writer.write_ue(max_num_ref_frames);

    Ok(max_num_reorder_frames != 0 || max_dec_frame_buffering > max_num_ref_frames)
}

fn write_low_latency_vui(writer: &mut BitWriter, max_num_ref_frames: u32) {
    writer.write_bits(0, 2);
    writer.write_bit(false);
    writer.write_bits(0, 5);
    writer.write_bit(true);
    write_bitstream_restriction(writer, max_num_ref_frames);
}

fn write_bitstream_restriction(writer: &mut BitWriter, max_num_ref_frames: u32) {
    writer.write_bit(true);
    writer.write_ue(2);
    writer.write_ue(1);
    writer.write_ue(16);
    writer.write_ue(16);
    writer.write_ue(0);
    writer.write_ue(max_num_ref_frames);
}

fn copy_hrd_parameters(reader: &mut BitReader<'_>, writer: &mut BitWriter) -> Result<(), String> {
    let cpb_count_minus1 = copy_ue(reader, writer)?;
    if cpb_count_minus1 > 31 {
        return Err("H264 SPS HRD entry count is too large".to_owned());
    }
    copy_bits(reader, writer, 8)?;
    for _ in 0..=cpb_count_minus1 {
        copy_ue(reader, writer)?;
        copy_ue(reader, writer)?;
        copy_bit(reader, writer)?;
    }
    copy_bits(reader, writer, 20)?;
    Ok(())
}

fn copy_bit(reader: &mut BitReader<'_>, writer: &mut BitWriter) -> Result<bool, String> {
    let value = reader.read_bit()?;
    writer.write_bit(value);
    Ok(value)
}

fn copy_bits(
    reader: &mut BitReader<'_>,
    writer: &mut BitWriter,
    count: usize,
) -> Result<u32, String> {
    let value = reader.read_bits(count)?;
    writer.write_bits(value, count);
    Ok(value)
}

fn copy_ue(reader: &mut BitReader<'_>, writer: &mut BitWriter) -> Result<u32, String> {
    let value = reader.read_ue()?;
    writer.write_ue(value);
    Ok(value)
}

fn decode_rbsp(ebsp: &[u8]) -> Vec<u8> {
    let mut rbsp = Vec::with_capacity(ebsp.len());
    let mut index = 0usize;
    while index < ebsp.len() {
        if ebsp.len() - index >= 3
            && ebsp[index] == 0
            && ebsp[index + 1] == 0
            && ebsp[index + 2] == 3
        {
            rbsp.extend_from_slice(&[0, 0]);
            index += 3;
        } else {
            rbsp.push(ebsp[index]);
            index += 1;
        }
    }
    rbsp
}

fn encode_rbsp(rbsp: &[u8], destination: &mut Vec<u8>) {
    let mut consecutive_zeros = 0usize;
    for &byte in rbsp {
        if byte <= 3 && consecutive_zeros >= 2 {
            destination.push(3);
            consecutive_zeros = 0;
        }
        destination.push(byte);
        if byte == 0 {
            consecutive_zeros += 1;
        } else {
            consecutive_zeros = 0;
        }
    }
}

struct BitReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn position(&self) -> usize {
        self.position
    }

    fn remaining_bits(&self) -> usize {
        self.bytes
            .len()
            .saturating_mul(8)
            .saturating_sub(self.position)
    }

    fn read_bit(&mut self) -> Result<bool, String> {
        if self.remaining_bits() == 0 {
            return Err("H264 SPS ends inside a bit field".to_owned());
        }
        let byte = self.bytes[self.position / 8];
        let bit = byte & (1 << (7 - self.position % 8)) != 0;
        self.position += 1;
        Ok(bit)
    }

    fn read_bits(&mut self, count: usize) -> Result<u32, String> {
        if count > 32 || self.remaining_bits() < count {
            return Err("H264 SPS ends inside a bit field".to_owned());
        }
        let mut value = 0u32;
        for _ in 0..count {
            value = (value << 1) | u32::from(self.read_bit()?);
        }
        Ok(value)
    }

    fn skip_bits(&mut self, count: usize) -> Result<(), String> {
        self.read_bits(count).map(|_| ())
    }

    fn read_ue(&mut self) -> Result<u32, String> {
        let mut leading_zeros = 0usize;
        while !self.read_bit()? {
            leading_zeros += 1;
            if leading_zeros > 31 {
                return Err("H264 SPS exponential-Golomb value is too large".to_owned());
            }
        }
        let suffix = self.read_bits(leading_zeros)?;
        Ok(((1u32 << leading_zeros) - 1) + suffix)
    }

    fn read_se(&mut self) -> Result<i32, String> {
        let code = self.read_ue()?;
        let magnitude = code.div_ceil(2) as i32;
        Ok(if code % 2 == 0 { -magnitude } else { magnitude })
    }
}

struct BitWriter {
    bytes: Vec<u8>,
    position: usize,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            position: 0,
        }
    }

    fn copy_from(&mut self, source: &[u8], bit_count: usize) -> Result<(), String> {
        if source.len().saturating_mul(8) < bit_count {
            return Err("H264 SPS copy exceeds its source".to_owned());
        }
        for position in 0..bit_count {
            let byte = source[position / 8];
            self.write_bit(byte & (1 << (7 - position % 8)) != 0);
        }
        Ok(())
    }

    fn write_bit(&mut self, value: bool) {
        if self.position.is_multiple_of(8) {
            self.bytes.push(0);
        }
        if value {
            let index = self.position / 8;
            self.bytes[index] |= 1 << (7 - self.position % 8);
        }
        self.position += 1;
    }

    fn write_bits(&mut self, value: u32, count: usize) {
        for shift in (0..count).rev() {
            self.write_bit(value & (1 << shift) != 0);
        }
    }

    fn write_ue(&mut self, value: u32) {
        let code = value + 1;
        let leading_zeros = 31 - code.leading_zeros();
        for _ in 0..leading_zeros {
            self.write_bit(false);
        }
        self.write_bits(code, leading_zeros as usize + 1);
    }

    fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn videotoolbox_sps_is_stable_for_webrtc_and_dave() {
        let mut access_unit = vec![
            0, 0, 0, 1, 0x27, 0x42, 0xc0, 0x1f, 0xab, 0x40, 0x28, 0x02, 0xdc, 0x80, 0, 0, 0, 1,
            0x28, 0xce, 0x3c, 0x80, 0, 0, 0, 1, 0x25, 0x88,
        ];

        normalize_annex_b_for_webrtc(&mut access_unit)
            .expect("VideoToolbox SPS should accept WebRTC VUI normalization");
        assert_eq!(
            crate::discord::voice::media::annex_b_nals(&access_unit)[0],
            [
                0x27, 0x42, 0xc0, 0x1f, 0xab, 0x40, 0x28, 0x02, 0xdd, 0x00, 0xda, 0x08, 0x84, 0x6a,
                0x00,
            ]
        );

        let normalized_once = access_unit.clone();
        normalize_annex_b_for_webrtc(&mut access_unit)
            .expect("a receiver-stable SPS should remain valid");
        assert_eq!(access_unit, normalized_once);
    }

    #[test]
    fn openh264_sps_with_low_latency_vui_is_preserved() {
        let mut access_unit = vec![
            0, 0, 0, 1, 0x67, 0x42, 0xc0, 0x1f, 0x8c, 0x68, 0x05, 0x00, 0x5b, 0xa6, 0xa0, 0x20,
            0x20, 0x20, 0xf0, 0x88, 0x46, 0xa0,
        ];
        let original = access_unit.clone();

        normalize_annex_b_for_webrtc(&mut access_unit)
            .expect("OpenH264 SPS should already satisfy WebRTC VUI rules");

        assert_eq!(access_unit, original);
    }
}
