use std::{
    collections::VecDeque,
    ffi::c_void,
    ptr::{self, NonNull},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFNumberType, CFRetained, CFType, kCFBooleanFalse,
    kCFBooleanTrue, kCFTypeArrayCallBacks, kCFTypeDictionaryKeyCallBacks,
    kCFTypeDictionaryValueCallBacks,
};
use objc2_core_media::{
    CMSampleBuffer, CMTime, CMVideoFormatDescriptionGetH264ParameterSetAtIndex, kCMTimeInvalid,
    kCMVideoCodecType_H264,
};
use objc2_core_video::{
    CVAttachmentMode, CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane,
    CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetPlaneCount, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferPool, CVPixelBufferUnlockBaseAddress,
    kCVImageBufferColorPrimaries_ITU_R_709_2, kCVImageBufferColorPrimariesKey,
    kCVImageBufferTransferFunction_ITU_R_709_2, kCVImageBufferTransferFunctionKey,
    kCVImageBufferYCbCrMatrix_ITU_R_709_2, kCVImageBufferYCbCrMatrixKey, kCVPixelBufferHeightKey,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelBufferWidthKey,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVReturnSuccess,
};
use objc2_video_toolbox::{
    VTCompressionSession, VTEncodeInfoFlags, VTSessionCopyProperty, VTSessionSetProperty,
    kVTCompressionPropertyKey_AllowFrameReordering, kVTCompressionPropertyKey_AverageBitRate,
    kVTCompressionPropertyKey_DataRateLimits, kVTCompressionPropertyKey_ExpectedFrameRate,
    kVTCompressionPropertyKey_MaxKeyFrameInterval, kVTCompressionPropertyKey_ProfileLevel,
    kVTCompressionPropertyKey_RealTime,
    kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder,
    kVTEncodeFrameOptionKey_ForceKeyFrame, kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel,
    kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder,
};

#[cfg(test)]
use super::STREAM_INTRA_FRAME_PERIOD_FRAMES;
use super::{
    EncodedH264Frame, I420Frame, STREAM_CAPTURE_FPS, STREAM_CAPTURE_HEIGHT, STREAM_CAPTURE_WIDTH,
    STREAM_ENCODER_BITRATE, annex_b_contains_idr, copy_i420_to_nv12,
    length_prefixed_h264_to_annex_b, validate_parameterized_h264_idr,
};
use crate::logging;

type CallbackResult = Result<Option<EncodedH264Frame>, String>;

struct CallbackState {
    completed: Mutex<VecDeque<CallbackResult>>,
    panicked: AtomicBool,
}

pub(in crate::discord::voice::capture) struct VideoToolboxEncoder {
    session: CFRetained<VTCompressionSession>,
    pixel_buffer_pool: CFRetained<CVPixelBufferPool>,
    callback_state: Box<CallbackState>,
    frame_index: i64,
}

impl VideoToolboxEncoder {
    pub(super) fn new(keyframe_interval_frames: u32) -> Result<Self, String> {
        // A disposable session proves the hardware path without making the
        // black probe a reference picture for the first frame sent to Discord.
        let mut probe_encoder = Self::create_configured(keyframe_interval_frames)?;
        probe_encoder.run_startup_probe()?;
        drop(probe_encoder);

        Self::create_configured(keyframe_interval_frames)
    }

    fn create_configured(keyframe_interval_frames: u32) -> Result<Self, String> {
        let mut callback_state = Box::new(CallbackState {
            completed: Mutex::new(VecDeque::new()),
            panicked: AtomicBool::new(false),
        });
        let callback_refcon = (&mut *callback_state as *mut CallbackState).cast::<c_void>();
        let encoder_specification = hardware_encoder_specification()?;
        let source_pixel_buffer_attributes = source_pixel_buffer_attributes()?;
        let mut raw_session = ptr::null_mut();

        // VideoToolbox retains the specification and the callback refcon remains
        // valid because `callback_state` has a stable allocation until teardown.
        let status = unsafe {
            VTCompressionSession::create(
                None,
                STREAM_CAPTURE_WIDTH as i32,
                STREAM_CAPTURE_HEIGHT as i32,
                kCMVideoCodecType_H264,
                Some(&encoder_specification),
                Some(&source_pixel_buffer_attributes),
                None,
                Some(compression_output_callback),
                callback_refcon,
                NonNull::from(&mut raw_session),
            )
        };
        status_result(status, "VideoToolbox H264 session creation")?;
        let raw_session = NonNull::new(raw_session)
            .ok_or_else(|| "VideoToolbox returned a null H264 session".to_owned())?;
        // `VTCompressionSessionCreate` returns a +1 retained Core Foundation object.
        let session = unsafe { CFRetained::from_raw(raw_session) };

        configure_session(&session, keyframe_interval_frames)?;
        let status = unsafe { session.prepare_to_encode_frames() };
        status_result(status, "VideoToolbox H264 encoder preparation")?;
        verify_hardware_encoder(&session)?;
        // The compression session pool matches the hardware encoder's preferred
        // layout and returns released buffers instead of allocating every frame.
        let pixel_buffer_pool = unsafe { session.pixel_buffer_pool() }
            .ok_or_else(|| "VideoToolbox returned no input pixel buffer pool".to_owned())?;

        Ok(Self {
            session,
            pixel_buffer_pool,
            callback_state,
            frame_index: 0,
        })
    }

    pub(super) fn encode(
        &mut self,
        frame: I420Frame<'_>,
        force_keyframe: bool,
    ) -> Result<Option<EncodedH264Frame>, String> {
        let pixel_buffer = create_pixel_buffer(&self.pixel_buffer_pool, frame)?;
        let presentation_time = unsafe { CMTime::new(self.frame_index, STREAM_CAPTURE_FPS as i32) };
        let duration = unsafe { CMTime::new(1, STREAM_CAPTURE_FPS as i32) };
        let force_keyframe = force_keyframe || self.frame_index == 0;
        self.frame_index = self
            .frame_index
            .checked_add(1)
            .ok_or_else(|| "VideoToolbox frame timestamp overflowed".to_owned())?;

        let frame_properties = force_keyframe
            .then(hardware_force_keyframe_properties)
            .transpose()?;
        let mut info_flags = VTEncodeInfoFlags::empty();
        let status = unsafe {
            self.session.encode_frame(
                &pixel_buffer,
                presentation_time,
                duration,
                frame_properties.as_deref(),
                ptr::null_mut(),
                &mut info_flags,
            )
        };
        status_result(status, "VideoToolbox H264 frame submission")?;

        // Completing through this timestamp gives the existing synchronous
        // encoder interface one result while the callback remains thread-safe.
        let status = unsafe { self.session.complete_frames(presentation_time) };
        status_result(status, "VideoToolbox H264 frame completion")?;
        if self.callback_state.panicked.swap(false, Ordering::AcqRel) {
            return Err("VideoToolbox H264 output callback panicked".to_owned());
        }

        self.callback_state
            .completed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .unwrap_or(Ok(None))
    }

    fn run_startup_probe(&mut self) -> Result<(), String> {
        let width = STREAM_CAPTURE_WIDTH as usize;
        let height = STREAM_CAPTURE_HEIGHT as usize;
        let y = vec![16; width * height];
        let u = vec![128; width * height / 4];
        let v = vec![128; width * height / 4];
        let probe = I420Frame::new(&y, &u, &v, width, height, width, width / 2, width / 2);

        let frame = self
            .encode(probe, true)?
            .ok_or_else(|| "VideoToolbox startup probe produced no H264 frame".to_owned())?;
        validate_parameterized_h264_idr(&frame, "VideoToolbox startup probe")?;
        validate_discord_h264_profile(&frame.annex_b)
    }
}

impl Drop for VideoToolboxEncoder {
    fn drop(&mut self) {
        // Completing and invalidating before the callback state is freed makes
        // teardown deterministic even when VideoToolbox used another thread.
        unsafe {
            let _ = self.session.complete_frames(kCMTimeInvalid);
            self.session.invalidate();
        }
    }
}

fn configure_session(
    session: &VTCompressionSession,
    keyframe_interval_frames: u32,
) -> Result<(), String> {
    // VideoToolbox exports these process-lifetime property keys and values.
    let (real_time, frame_reordering, bitrate, frame_rate, gop, profile, constrained_baseline) = unsafe {
        (
            kVTCompressionPropertyKey_RealTime,
            kVTCompressionPropertyKey_AllowFrameReordering,
            kVTCompressionPropertyKey_AverageBitRate,
            kVTCompressionPropertyKey_ExpectedFrameRate,
            kVTCompressionPropertyKey_MaxKeyFrameInterval,
            kVTCompressionPropertyKey_ProfileLevel,
            kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel,
        )
    };
    set_boolean_property(session, real_time, true, "real-time mode")?;
    set_boolean_property(session, frame_reordering, false, "frame reordering")?;
    set_number_property(
        session,
        bitrate,
        STREAM_ENCODER_BITRATE as i32,
        "average bitrate",
    )?;
    if let Err(error) = set_data_rate_limits(session) {
        // Apple documents this property as optional because not every encoder
        // supports it. Average bitrate and RTP pacing still keep the stream
        // usable, so an unsupported cap must not disable hardware encoding.
        logging::debug(
            "stream",
            format!(
                "VideoToolbox data rate limits are unavailable; continuing without the optional encoder cap: {error}"
            ),
        );
    }
    set_number_property(
        session,
        frame_rate,
        STREAM_CAPTURE_FPS as i32,
        "expected frame rate",
    )?;
    set_number_property(
        session,
        gop,
        i32::try_from(keyframe_interval_frames).expect("keyframe interval fits i32"),
        "keyframe interval",
    )?;
    set_property(
        session,
        profile,
        constrained_baseline,
        "H264 constrained baseline profile",
    )
}

fn validate_discord_h264_profile(annex_b: &[u8]) -> Result<(), String> {
    let sps = crate::discord::voice::media::annex_b_nals(annex_b)
        .into_iter()
        .find(|nal| nal.first().is_some_and(|header| header & 0x1f == 7))
        .ok_or_else(|| "VideoToolbox startup keyframe did not contain an SPS".to_owned())?;
    if sps.len() < 4 {
        return Err("VideoToolbox startup SPS is too short".to_owned());
    }

    let profile_idc = sps[1];
    let profile_iop = sps[2];
    let level_idc = sps[3];
    if profile_idc != 66 || profile_iop & 0x40 == 0 || level_idc != 31 {
        return Err(format!(
            "VideoToolbox produced incompatible H264 profile-level-id {profile_idc:02x}{profile_iop:02x}{level_idc:02x}; expected constrained baseline Level 3.1"
        ));
    }
    Ok(())
}

fn set_boolean_property(
    session: &VTCompressionSession,
    key: &objc2_core_foundation::CFString,
    value: bool,
    name: &str,
) -> Result<(), String> {
    let value = cf_boolean(value)?;
    set_property(session, key, value, name)
}

fn set_number_property(
    session: &VTCompressionSession,
    key: &objc2_core_foundation::CFString,
    value: i32,
    name: &str,
) -> Result<(), String> {
    // The pointed-to scalar is valid for the duration of `CFNumberCreate`.
    let value = unsafe {
        CFNumber::new(
            None,
            CFNumberType::SInt32Type,
            (&value as *const i32).cast(),
        )
    }
    .ok_or_else(|| format!("Core Foundation number creation failed for {name}"))?;
    set_property(session, key, &value, name)
}

fn set_data_rate_limits(session: &VTCompressionSession) -> Result<(), String> {
    // VideoToolbox treats AverageBitRate as a target rather than a ceiling.
    // Match WebRTC's 1.5x one-second cap to bound sustained encoder output.
    // The RTP pacer separately spreads short packet bursts on the wire.
    let peak_bytes_per_second = i32::try_from(
        u64::from(STREAM_ENCODER_BITRATE)
            .saturating_mul(3)
            .saturating_div(2)
            .saturating_div(8),
    )
    .map_err(|_| "VideoToolbox peak bitrate does not fit i32".to_owned())?;
    let peak_bytes = cf_number(peak_bytes_per_second, "peak bitrate")?;
    let window_seconds = cf_number(1, "peak bitrate window")?;
    let mut values = [
        (&*peak_bytes as *const CFNumber).cast::<c_void>(),
        (&*window_seconds as *const CFNumber).cast::<c_void>(),
    ];
    // The array callbacks retain both numbers before the local references end.
    let limits = unsafe {
        CFArray::new(
            None,
            values.as_mut_ptr(),
            values.len() as isize,
            &kCFTypeArrayCallBacks,
        )
    }
    .ok_or_else(|| "VideoToolbox data rate limit array creation failed".to_owned())?;
    let key = unsafe { kVTCompressionPropertyKey_DataRateLimits };
    set_property(session, key, &limits, "data rate limits")
}

fn cf_number(value: i32, name: &str) -> Result<CFRetained<CFNumber>, String> {
    // The pointed-to scalar is valid for the duration of `CFNumberCreate`.
    unsafe {
        CFNumber::new(
            None,
            CFNumberType::SInt32Type,
            (&value as *const i32).cast(),
        )
    }
    .ok_or_else(|| format!("Core Foundation number creation failed for {name}"))
}

fn set_property(
    session: &VTCompressionSession,
    key: &objc2_core_foundation::CFString,
    value: &CFType,
    name: &str,
) -> Result<(), String> {
    let status = unsafe { VTSessionSetProperty(session, key, Some(value)) };
    status_result(status, &format!("VideoToolbox {name} configuration"))
}

fn verify_hardware_encoder(session: &VTCompressionSession) -> Result<(), String> {
    let mut raw_value: *mut CFType = ptr::null_mut();
    // VideoToolbox exports this process-lifetime property key.
    let hardware_property =
        unsafe { kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder };
    let status = unsafe {
        VTSessionCopyProperty(
            session,
            hardware_property,
            None,
            (&mut raw_value as *mut *mut CFType).cast(),
        )
    };
    status_result(status, "VideoToolbox hardware encoder verification")?;
    let raw_value = NonNull::new(raw_value)
        .ok_or_else(|| "VideoToolbox hardware encoder property was null".to_owned())?;
    // `VTSessionCopyProperty` follows the Core Foundation Copy rule.
    let value = unsafe { CFRetained::from_raw(raw_value) };
    let is_hardware = value
        .downcast_ref::<CFBoolean>()
        .is_some_and(CFBoolean::value);
    is_hardware
        .then_some(())
        .ok_or_else(|| "VideoToolbox selected a software H264 encoder".to_owned())
}

fn hardware_encoder_specification() -> Result<CFRetained<CFDictionary>, String> {
    // VideoToolbox exports this process-lifetime specification key.
    let require_hardware =
        unsafe { kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder };
    cf_dictionary(&[(require_hardware, cf_boolean(true)?)])
}

fn source_pixel_buffer_attributes() -> Result<CFRetained<CFDictionary>, String> {
    let pixel_format = cf_number(
        i32::try_from(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange)
            .map_err(|_| "VideoToolbox NV12 pixel format does not fit i32".to_owned())?,
        "NV12 pixel format",
    )?;
    let width = cf_number(
        i32::try_from(STREAM_CAPTURE_WIDTH)
            .map_err(|_| "VideoToolbox input width does not fit i32".to_owned())?,
        "input width",
    )?;
    let height = cf_number(
        i32::try_from(STREAM_CAPTURE_HEIGHT)
            .map_err(|_| "VideoToolbox input height does not fit i32".to_owned())?,
        "input height",
    )?;
    // Core Video retains the values in the dictionary. VideoToolbox then uses
    // these attributes to expose a pool compatible with its selected encoder.
    cf_dictionary(&[
        (unsafe { kCVPixelBufferPixelFormatTypeKey }, &pixel_format),
        (unsafe { kCVPixelBufferWidthKey }, &width),
        (unsafe { kCVPixelBufferHeightKey }, &height),
    ])
}

fn hardware_force_keyframe_properties() -> Result<CFRetained<CFDictionary>, String> {
    // VideoToolbox exports this process-lifetime frame property key.
    let force_keyframe = unsafe { kVTEncodeFrameOptionKey_ForceKeyFrame };
    cf_dictionary(&[(force_keyframe, cf_boolean(true)?)])
}

fn cf_dictionary(
    entries: &[(&objc2_core_foundation::CFString, &CFType)],
) -> Result<CFRetained<CFDictionary>, String> {
    let mut keys: Vec<*const c_void> = entries
        .iter()
        .map(|(key, _)| (*key as *const objc2_core_foundation::CFString).cast())
        .collect();
    let mut values: Vec<*const c_void> = entries
        .iter()
        .map(|(_, value)| (*value as *const CFType).cast())
        .collect();
    // The arrays are valid for the call and CF type callbacks retain entries.
    unsafe {
        CFDictionary::new(
            None,
            keys.as_mut_ptr(),
            values.as_mut_ptr(),
            entries.len() as isize,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        )
    }
    .ok_or_else(|| "Core Foundation dictionary creation failed".to_owned())
}

fn cf_boolean(value: bool) -> Result<&'static CFBoolean, String> {
    let value = if value {
        unsafe { kCFBooleanTrue }
    } else {
        unsafe { kCFBooleanFalse }
    };
    value.ok_or_else(|| "Core Foundation boolean constant was unavailable".to_owned())
}

fn create_pixel_buffer(
    pool: &CVPixelBufferPool,
    frame: I420Frame<'_>,
) -> Result<CFRetained<CVPixelBuffer>, String> {
    let width = STREAM_CAPTURE_WIDTH as usize;
    let height = STREAM_CAPTURE_HEIGHT as usize;
    if frame.width != width || frame.height != height {
        return Err(format!(
            "VideoToolbox input dimensions must be {width}x{height}, got {}x{}",
            frame.width, frame.height
        ));
    }
    let mut raw_buffer = ptr::null_mut();
    // The output pointer is valid and the retained pool owns its allocation policy.
    let status = unsafe {
        CVPixelBufferPool::create_pixel_buffer(None, pool, NonNull::from(&mut raw_buffer))
    };
    if status != kCVReturnSuccess {
        return Err(format!(
            "NV12 pixel buffer creation failed with status {status}"
        ));
    }
    let raw_buffer = NonNull::new(raw_buffer)
        .ok_or_else(|| "Core Video returned a null NV12 pixel buffer".to_owned())?;
    // `CVPixelBufferPoolCreatePixelBuffer` returns a +1 retained Core Foundation object.
    let pixel_buffer = unsafe { CFRetained::from_raw(raw_buffer) };

    attach_bt709_metadata(&pixel_buffer);
    fill_pixel_buffer(&pixel_buffer, frame)?;
    Ok(pixel_buffer)
}

fn attach_bt709_metadata(pixel_buffer: &CVPixelBuffer) {
    let mode = CVAttachmentMode::ShouldPropagate;
    // These well-known Core Video strings are valid CF values and describe
    // the BT.709 matrix, primaries, and transfer function used by capture.
    let (matrix_key, matrix, primaries_key, primaries, transfer_key, transfer) = unsafe {
        (
            kCVImageBufferYCbCrMatrixKey,
            kCVImageBufferYCbCrMatrix_ITU_R_709_2,
            kCVImageBufferColorPrimariesKey,
            kCVImageBufferColorPrimaries_ITU_R_709_2,
            kCVImageBufferTransferFunctionKey,
            kCVImageBufferTransferFunction_ITU_R_709_2,
        )
    };
    unsafe {
        pixel_buffer.set_attachment(matrix_key, matrix, mode);
        pixel_buffer.set_attachment(primaries_key, primaries, mode);
        pixel_buffer.set_attachment(transfer_key, transfer, mode);
    }
}

fn fill_pixel_buffer(pixel_buffer: &CVPixelBuffer, frame: I420Frame<'_>) -> Result<(), String> {
    let lock_flags = CVPixelBufferLockFlags::empty();
    let status = unsafe { CVPixelBufferLockBaseAddress(pixel_buffer, lock_flags) };
    if status != kCVReturnSuccess {
        return Err(format!(
            "NV12 pixel buffer lock failed with status {status}"
        ));
    }

    let result = (|| {
        if CVPixelBufferGetPlaneCount(pixel_buffer) != 2 {
            return Err("NV12 pixel buffer does not expose two planes".to_owned());
        }
        let y_stride = CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, 0);
        let uv_stride = CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, 1);
        let y_ptr = CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, 0).cast::<u8>();
        let uv_ptr = CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, 1).cast::<u8>();
        if y_ptr.is_null() || uv_ptr.is_null() {
            return Err("NV12 pixel buffer exposed a null plane".to_owned());
        }

        // The locked Core Video planes contain at least stride times plane
        // height bytes, and remain valid until the matching unlock below.
        let y = unsafe { std::slice::from_raw_parts_mut(y_ptr, y_stride * frame.height) };
        let uv = unsafe { std::slice::from_raw_parts_mut(uv_ptr, uv_stride * (frame.height / 2)) };
        copy_i420_to_nv12(frame, y, uv, y_stride, uv_stride)
    })();

    let unlock_status = unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, lock_flags) };
    if unlock_status != kCVReturnSuccess {
        return Err(format!(
            "NV12 pixel buffer unlock failed with status {unlock_status}"
        ));
    }
    result
}

unsafe extern "C-unwind" fn compression_output_callback(
    output_callback_refcon: *mut c_void,
    _source_frame_refcon: *mut c_void,
    status: i32,
    _info_flags: VTEncodeInfoFlags,
    sample_buffer: *mut CMSampleBuffer,
) {
    if output_callback_refcon.is_null() {
        return;
    }
    // The refcon points to the boxed state for the full session lifetime.
    let state = unsafe { &*(output_callback_refcon.cast::<CallbackState>()) };
    let callback_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let result = if status != 0 {
            Err(format!(
                "VideoToolbox H264 output callback failed with status {status}"
            ))
        } else if let Some(sample_buffer) = unsafe { sample_buffer.as_ref() } {
            encode_sample_buffer(sample_buffer).map(Some)
        } else {
            Ok(None)
        };
        state
            .completed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_back(result);
    }));
    if callback_result.is_err() {
        state.panicked.store(true, Ordering::Release);
    }
}

fn encode_sample_buffer(sample_buffer: &CMSampleBuffer) -> Result<EncodedH264Frame, String> {
    if !unsafe { sample_buffer.data_is_ready() } {
        return Err("VideoToolbox returned an H264 sample whose data is not ready".to_owned());
    }
    let block_buffer = unsafe { sample_buffer.data_buffer() }
        .ok_or_else(|| "VideoToolbox H264 sample has no block buffer".to_owned())?;
    let data_length = unsafe { block_buffer.data_length() };
    let mut avcc = vec![0; data_length];
    if !avcc.is_empty() {
        let destination = NonNull::new(avcc.as_mut_ptr().cast::<c_void>())
            .ok_or_else(|| "H264 output allocation returned a null pointer".to_owned())?;
        let status = unsafe { block_buffer.copy_data_bytes(0, data_length, destination) };
        status_result(status, "VideoToolbox H264 output copy")?;
    }

    let format = unsafe { sample_buffer.format_description() }
        .ok_or_else(|| "VideoToolbox H264 sample has no format description".to_owned())?;
    let (_, _, parameter_set_count, nal_header_length) = h264_parameter_set(&format, 0)?;
    let mut annex_b = length_prefixed_h264_to_annex_b(&avcc, nal_header_length)?;
    let is_keyframe = annex_b_contains_idr(&annex_b);

    if is_keyframe {
        let mut with_parameter_sets = Vec::new();
        for index in 0..parameter_set_count {
            let (pointer, size, _, _) = h264_parameter_set(&format, index)?;
            with_parameter_sets.extend_from_slice(&[0, 0, 0, 1]);
            // The pointer belongs to the retained format description and is
            // valid for the duration of this copy.
            let parameter_set = unsafe { std::slice::from_raw_parts(pointer, size) };
            with_parameter_sets.extend_from_slice(parameter_set);
        }
        with_parameter_sets.append(&mut annex_b);
        annex_b = with_parameter_sets;
    }

    EncodedH264Frame::new(annex_b, is_keyframe)
}

fn h264_parameter_set(
    format: &objc2_core_media::CMFormatDescription,
    index: usize,
) -> Result<(*const u8, usize, usize, usize), String> {
    let mut pointer = ptr::null();
    let mut size = 0;
    let mut count = 0;
    let mut nal_header_length = 0;
    // The output pointers are valid locals and `format` remains retained.
    let status = unsafe {
        CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            format,
            index,
            &mut pointer,
            &mut size,
            &mut count,
            &mut nal_header_length,
        )
    };
    status_result(status, "VideoToolbox H264 parameter set extraction")?;
    if pointer.is_null() || size == 0 {
        return Err("VideoToolbox returned an empty H264 parameter set".to_owned());
    }
    let nal_header_length = usize::try_from(nal_header_length)
        .map_err(|_| "VideoToolbox returned a negative H264 NAL header length".to_owned())?;
    Ok((pointer, size, count, nal_header_length))
}

fn status_result(status: i32, operation: &str) -> Result<(), String> {
    (status == 0)
        .then_some(())
        .ok_or_else(|| format!("{operation} failed with status {status}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a physical VideoToolbox H264 encoder"]
    fn hardware_encoder_produces_parameterized_idr() {
        let mut encoder = VideoToolboxEncoder::new(STREAM_INTRA_FRAME_PERIOD_FRAMES)
            .expect("VideoToolbox hardware encoder should pass its startup probe");
        let width = STREAM_CAPTURE_WIDTH as usize;
        let height = STREAM_CAPTURE_HEIGHT as usize;
        let y = vec![16; width * height];
        let u = vec![128; width * height / 4];
        let v = vec![128; width * height / 4];
        let frame = I420Frame::new(&y, &u, &v, width, height, width, width / 2, width / 2);

        let encoded = encoder
            .encode(frame, false)
            .expect("VideoToolbox should encode the first production frame")
            .expect("VideoToolbox should return the completed first frame");
        let nal_types: Vec<u8> = crate::discord::voice::media::annex_b_nals(&encoded.annex_b)
            .into_iter()
            .filter_map(|nal| nal.first().map(|header| header & 0x1f))
            .collect();
        assert!(encoded.is_keyframe);
        assert!(nal_types.contains(&7), "IDR output should contain SPS");
        assert!(nal_types.contains(&8), "IDR output should contain PPS");
        assert!(
            nal_types.contains(&5),
            "first frame should contain an IDR NAL"
        );
        validate_discord_h264_profile(&encoded.annex_b)
            .expect("VideoToolbox should produce constrained baseline Level 3.1");

        let mut decoder = openh264::decoder::Decoder::new()
            .expect("OpenH264 decoder should initialize for the hardware smoke test");
        let decoded = decoder
            .decode(&encoded.annex_b)
            .expect("the first VideoToolbox access unit should decode");
        assert!(
            decoded.is_some(),
            "the first access unit should contain a frame"
        );

        for luma in [32, 64, 96, 128] {
            let y = vec![luma; width * height];
            let frame = I420Frame::new(&y, &u, &v, width, height, width, width / 2, width / 2);
            let encoded = encoder
                .encode(frame, false)
                .expect("VideoToolbox should encode a follow-up frame")
                .expect("VideoToolbox should complete each low-latency frame");
            let decoded = decoder
                .decode(&encoded.annex_b)
                .expect("a follow-up VideoToolbox access unit should decode");
            assert!(decoded.is_some(), "each access unit should contain a frame");
        }
    }

    #[test]
    fn discord_profile_validation_requires_constrained_baseline_level_3_1() {
        validate_discord_h264_profile(&[0, 0, 0, 1, 0x67, 0x42, 0xc0, 0x1f])
            .expect("OpenH264's constrained baseline profile should be accepted");

        let error = validate_discord_h264_profile(&[0, 0, 0, 1, 0x27, 0x42, 0x00, 0x1f])
            .expect_err("unrestricted baseline should be rejected");
        assert!(error.contains("42001f"), "unexpected error: {error}");
    }
}
