use std::{
    mem::ManuallyDrop,
    ptr,
    rc::Rc,
    slice, thread,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::VARIANT_BOOL,
        Media::MediaFoundation::*,
        System::{
            Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
            Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_UI4},
        },
    },
    core::{GUID, HRESULT, IUnknown, Interface},
};

use super::{
    EncodedH264Frame, I420Frame, STREAM_CAPTURE_FPS, STREAM_CAPTURE_HEIGHT, STREAM_CAPTURE_WIDTH,
    STREAM_ENCODER_BITRATE, STREAM_INTRA_FRAME_PERIOD_FRAMES, annex_b_contains_idr,
    copy_i420_to_nv12, normalize_h264_access_unit, validate_parameterized_h264_idr,
};
use crate::logging;

const INPUT_STREAM_ID: u32 = 0;
const OUTPUT_STREAM_ID: u32 = 0;
const HNS_PER_SECOND: i64 = 10_000_000;
const MAX_EVENTS_PER_FRAME: usize = 32;
const EVENT_WAIT_TIMEOUT: Duration = Duration::from_secs(2);
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(1);

// ICodecAPI is declared in codecapi.h, but it is missing from the Windows
// metadata used to generate windows 0.62. Define only the interface methods we
// need so encoder settings do not depend on an untyped vtable cast.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct ICodecApi(IUnknown);

// SAFETY: ICodecApi is a transparent COM pointer wrapper and this IID and
// vtable layout are declared by codecapi.h.
unsafe impl Interface for ICodecApi {
    type Vtable = ICodecApi_Vtbl;
    const IID: GUID = GUID::from_u128(0x901db4c7_31ce_41a2_85dc_8fa0bf41b8da);
}

impl ICodecApi {
    unsafe fn set_value(&self, key: &GUID, value: &VARIANT) -> windows::core::Result<()> {
        // SAFETY: The interface was obtained by QueryInterface and both pointers
        // remain valid for the duration of this call.
        unsafe { (Interface::vtable(self).SetValue)(Interface::as_raw(self), key, value).ok() }
    }
}

#[repr(C)]
#[allow(non_snake_case)]
pub struct ICodecApi_Vtbl {
    base__: windows::core::IUnknown_Vtbl,
    IsSupported: usize,
    IsModifiable: usize,
    GetParameterRange: usize,
    GetParameterValues: usize,
    GetDefaultValue: usize,
    GetValue: usize,
    SetValue:
        unsafe extern "system" fn(*mut core::ffi::c_void, *const GUID, *const VARIANT) -> HRESULT,
    RegisterForEvent: usize,
    UnregisterForEvent: usize,
    SetAllDefaults: usize,
    SetValueWithNotify: usize,
    SetAllDefaultsWithNotify: usize,
    GetAllSettings: usize,
    SetAllSettings: usize,
    SetAllSettingsWithNotify: usize,
}

pub(in crate::discord::voice::capture) struct MediaFoundationEncoder {
    transform: IMFTransform,
    events: IMFMediaEventGenerator,
    codec_api: ICodecApi,
    activation: IMFActivate,
    reusable_input_sample: Option<IMFSample>,
    supplied_output_sample: Option<IMFSample>,
    frame_index: i64,
    need_input: bool,
    _media_foundation: Rc<MediaFoundationPlatform>,
    _com: Rc<ComApartment>,
}

impl MediaFoundationEncoder {
    pub(super) fn new() -> Result<Self, String> {
        let com = Rc::new(ComApartment::new()?);
        let media_foundation = Rc::new(MediaFoundationPlatform::new()?);
        let activations = enumerate_hardware_encoders()?;
        let mut failures = Vec::new();

        for activation in activations {
            match Self::from_activation(
                activation.clone(),
                Rc::clone(&media_foundation),
                Rc::clone(&com),
            ) {
                Ok(mut probe) => match probe.probe() {
                    Ok(()) => {
                        // Shutdown and drop every probe reference before activating
                        // a new transform for the real stream. A flush alone may
                        // retain hidden reference frames or sequence state.
                        drop(probe);
                        return Self::from_activation(
                            activation,
                            Rc::clone(&media_foundation),
                            Rc::clone(&com),
                        );
                    }
                    Err(error) => failures.push(error),
                },
                Err(error) => {
                    // SAFETY: This activation belongs to this attempted encoder.
                    let _ = unsafe { activation.ShutdownObject() };
                    failures.push(error);
                }
            }
        }

        Err(format!(
            "no compatible Media Foundation hardware H264 encoder was found ({})",
            failures.join("; ")
        ))
    }

    fn from_activation(
        activation: IMFActivate,
        media_foundation: Rc<MediaFoundationPlatform>,
        com: Rc<ComApartment>,
    ) -> Result<Self, String> {
        // SAFETY: COM and Media Foundation are initialized on this thread.
        let transform: IMFTransform = unsafe { activation.ActivateObject() }
            .map_err(|error| format!("hardware H264 MFT activation failed: {error}"))?;
        let events: IMFMediaEventGenerator = transform
            .cast()
            .map_err(|error| format!("hardware H264 MFT is not asynchronous: {error}"))?;
        let codec_api: ICodecApi = transform
            .cast()
            .map_err(|error| format!("hardware H264 MFT has no ICodecAPI: {error}"))?;

        configure_async_transform(&transform)?;
        configure_codec(&codec_api)?;
        configure_media_types(&transform)?;

        // SAFETY: Stream zero exists because type negotiation above succeeded.
        let output_info = unsafe { transform.GetOutputStreamInfo(OUTPUT_STREAM_ID) }
            .map_err(|error| format!("H264 MFT output stream query failed: {error}"))?;
        let output_provides_samples = output_info.dwFlags
            & ((MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0)
                as u32)
            != 0;
        // An input sample can only be overwritten after ProcessInput when the
        // transform explicitly promises that it does not retain the sample.
        let mut input_info = MFT_INPUT_STREAM_INFO::default();
        unsafe { transform.GetInputStreamInfo(INPUT_STREAM_ID, &mut input_info) }
            .map_err(|error| format!("H264 MFT input stream query failed: {error}"))?;
        let input_capacity = u32::try_from(nv12_frame_length()?)
            .map_err(|_| "Media Foundation NV12 frame size does not fit u32".to_owned())?;
        let reusable_input_sample =
            (input_info.dwFlags & MFT_INPUT_STREAM_DOES_NOT_ADDREF.0 as u32 != 0)
                .then(|| create_empty_sample(input_capacity))
                .transpose()?;
        let supplied_output_sample = (!output_provides_samples)
            .then(|| create_empty_sample(output_info.cbSize.max(1)))
            .transpose()?;

        let mut encoder = Self {
            transform,
            events,
            codec_api,
            activation,
            reusable_input_sample,
            supplied_output_sample,
            frame_index: 0,
            need_input: false,
            _media_foundation: media_foundation,
            _com: com,
        };
        encoder.start_stream()?;
        Ok(encoder)
    }

    fn probe(&mut self) -> Result<(), String> {
        let width = STREAM_CAPTURE_WIDTH as usize;
        let height = STREAM_CAPTURE_HEIGHT as usize;
        let y = vec![16; width * height];
        let u = vec![128; width * height / 4];
        let v = vec![128; width * height / 4];
        let black = I420Frame::new(&y, &u, &v, width, height, width, width / 2, width / 2);
        let mut encoded = None;
        for _ in 0..8 {
            if let Some(output) = self.encode(black, true)? {
                encoded = Some(output);
                break;
            }
        }
        let encoded = encoded
            .ok_or_else(|| "Media Foundation H264 probe produced no encoded frame".to_owned())?;
        validate_parameterized_h264_idr(&encoded, "Media Foundation H264 probe")
    }

    pub(super) fn encode(
        &mut self,
        input: I420Frame<'_>,
        force_keyframe: bool,
    ) -> Result<Option<EncodedH264Frame>, String> {
        self.wait_for_input_request()?;
        if force_keyframe || self.frame_index == 0 {
            set_codec_bool(&self.codec_api, &CODECAPI_AVEncVideoForceKeyFrame, true)
                .map_err(|error| format!("Media Foundation keyframe request failed: {error}"))?;
        }

        let sample = match self.reusable_input_sample.as_ref() {
            Some(sample) => sample.clone(),
            None => {
                let capacity = u32::try_from(nv12_frame_length()?)
                    .map_err(|_| "Media Foundation NV12 frame size does not fit u32".to_owned())?;
                create_empty_sample(capacity)?
            }
        };
        prepare_input_sample(&sample, input, self.frame_index)?;
        // SAFETY: The sample owns its buffer and the MFT requested input.
        unsafe { self.transform.ProcessInput(INPUT_STREAM_ID, &sample, 0) }
            .map_err(|error| format!("Media Foundation H264 frame submission failed: {error}"))?;
        self.need_input = false;
        self.frame_index = self
            .frame_index
            .checked_add(1)
            .ok_or_else(|| "Media Foundation frame timestamp overflowed".to_owned())?;

        let deadline = Instant::now() + EVENT_WAIT_TIMEOUT;
        for _ in 0..MAX_EVENTS_PER_FRAME {
            match self.next_event_until(deadline)? {
                MftEvent::NeedInput => {
                    self.need_input = true;
                    return Ok(None);
                }
                MftEvent::HaveOutput => return self.process_output(),
                MftEvent::Other => {}
            }
        }
        Err("Media Foundation H264 MFT emitted too many events without progress".to_owned())
    }

    fn wait_for_input_request(&mut self) -> Result<(), String> {
        if self.need_input {
            return Ok(());
        }
        let deadline = Instant::now() + EVENT_WAIT_TIMEOUT;
        for _ in 0..MAX_EVENTS_PER_FRAME {
            match self.next_event_until(deadline)? {
                MftEvent::NeedInput => {
                    self.need_input = true;
                    return Ok(());
                }
                MftEvent::HaveOutput => {
                    return Err("Media Foundation H264 MFT produced unclaimed output".to_owned());
                }
                MftEvent::Other => {}
            }
        }
        Err("Media Foundation H264 MFT did not request input".to_owned())
    }

    fn next_event_until(&self, deadline: Instant) -> Result<MftEvent, String> {
        let event = loop {
            // Polling with NO_WAIT keeps a broken driver from holding the capture
            // worker forever after the UI preparation deadline has expired.
            match unsafe { self.events.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
                Ok(event) => break event,
                Err(error) if error.code() == MF_E_NO_EVENTS_AVAILABLE => {
                    if Instant::now() >= deadline {
                        return Err("Media Foundation H264 MFT event wait timed out".to_owned());
                    }
                    thread::sleep(EVENT_POLL_INTERVAL);
                }
                Err(error) => {
                    return Err(format!("Media Foundation H264 event wait failed: {error}"));
                }
            }
        };
        // SAFETY: The returned event is initialized and owned by this function.
        let status = unsafe { event.GetStatus() }
            .map_err(|error| format!("Media Foundation H264 event status query failed: {error}"))?;
        status.ok().map_err(|error| {
            format!("Media Foundation H264 asynchronous operation failed: {error}")
        })?;
        // SAFETY: The returned event is initialized and owned by this function.
        let event_type = unsafe { event.GetType() }
            .map_err(|error| format!("Media Foundation H264 event type query failed: {error}"))?;
        Ok(if event_type == METransformNeedInput.0 as u32 {
            MftEvent::NeedInput
        } else if event_type == METransformHaveOutput.0 as u32 {
            MftEvent::HaveOutput
        } else {
            MftEvent::Other
        })
    }

    fn process_output(&self) -> Result<Option<EncodedH264Frame>, String> {
        if let Some(sample) = self.supplied_output_sample.as_ref() {
            reset_output_sample(sample)?;
        }
        let supplied_sample = self.supplied_output_sample.clone();
        let mut output = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: OUTPUT_STREAM_ID,
            pSample: ManuallyDrop::new(supplied_sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };
        let mut status = 0;
        // SAFETY: output is initialized according to GetOutputStreamInfo and
        // remains alive until ProcessOutput returns.
        let result = unsafe {
            self.transform
                .ProcessOutput(0, slice::from_mut(&mut output), &mut status)
        };
        // SAFETY: ProcessOutput has returned and no longer owns these fields.
        let sample = unsafe { ManuallyDrop::take(&mut output.pSample) };
        // SAFETY: Same as above. Dropping the optional collection releases it.
        drop(unsafe { ManuallyDrop::take(&mut output.pEvents) });
        result
            .map_err(|error| format!("Media Foundation H264 output retrieval failed: {error}"))?;

        let Some(sample) = sample else {
            return Ok(None);
        };
        let clean_point =
            unsafe { sample.GetUINT32(&MFSampleExtension_CleanPoint) }.unwrap_or(0) != 0;
        let bytes = read_sample(&sample)?;
        let mut annex_b = normalize_h264_access_unit(&bytes)?;
        let is_keyframe = clean_point || annex_b_contains_idr(&annex_b);
        if is_keyframe && (!contains_nal_type(&annex_b, 7) || !contains_nal_type(&annex_b, 8)) {
            let mut parameter_sets = self.parameter_sets()?.ok_or_else(|| {
                "Media Foundation IDR output omitted SPS/PPS and no sequence header is available"
                    .to_owned()
            })?;
            if !contains_nal_type(&parameter_sets, 7) || !contains_nal_type(&parameter_sets, 8) {
                return Err(
                    "Media Foundation sequence header does not contain both SPS and PPS".to_owned(),
                );
            }
            parameter_sets.extend_from_slice(&annex_b);
            annex_b = parameter_sets;
        }
        (!annex_b.is_empty())
            .then(|| EncodedH264Frame::new(annex_b, is_keyframe))
            .transpose()
    }

    fn start_stream(&mut self) -> Result<(), String> {
        // SAFETY: These messages follow completed type negotiation.
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .and_then(|_| {
                    self.transform
                        .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                })
        }
        .map_err(|error| format!("Media Foundation H264 stream start failed: {error}"))?;
        self.need_input = false;
        self.wait_for_input_request()
    }

    fn parameter_sets(&self) -> Result<Option<Vec<u8>>, String> {
        // SAFETY: The output type remains valid throughout the configured stream.
        let media_type = unsafe { self.transform.GetOutputCurrentType(OUTPUT_STREAM_ID) }
            .map_err(|error| format!("H264 MFT output type query failed: {error}"))?;
        let Ok(size) = (unsafe { media_type.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER) }) else {
            return Ok(None);
        };
        let mut bytes = vec![0; size as usize];
        // SAFETY: The destination has the exact size reported by GetBlobSize.
        unsafe { media_type.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut bytes, None) }
            .map_err(|error| format!("H264 MFT sequence header read failed: {error}"))?;
        Ok(Some(normalize_sequence_header(&bytes)?))
    }
}

impl Drop for MediaFoundationEncoder {
    fn drop(&mut self) {
        // SAFETY: Shutdown is best effort and occurs before the platform guards
        // stored after these fields are dropped.
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            let _ = self.activation.ShutdownObject();
        }
    }
}

struct MediaFoundationPlatform;

impl MediaFoundationPlatform {
    fn new() -> Result<Self, String> {
        // SAFETY: Every successful startup is paired with this guard's shutdown.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }
            .map_err(|error| format!("Media Foundation startup failed: {error}"))?;
        Ok(Self)
    }
}

impl Drop for MediaFoundationPlatform {
    fn drop(&mut self) {
        // SAFETY: This guard represents one successful MFStartup call.
        let _ = unsafe { MFShutdown() };
    }
}

struct ComApartment;

impl ComApartment {
    fn new() -> Result<Self, String> {
        // SAFETY: A null reserved pointer is required. The encoder is confined
        // to the thread on which this apartment is initialized.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|error| format!("COM initialization failed: {error}"))?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: This guard represents one successful CoInitializeEx call.
        unsafe { CoUninitialize() };
    }
}

enum MftEvent {
    NeedInput,
    HaveOutput,
    Other,
}

fn enumerate_hardware_encoders() -> Result<Vec<IMFActivate>, String> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let mut raw = ptr::null_mut();
    let mut count = 0;
    // SAFETY: MFTEnumEx initializes an allocated array and count on success.
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_ASYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut raw,
            &mut count,
        )
    }
    .map_err(|error| format!("Media Foundation hardware encoder enumeration failed: {error}"))?;

    if raw.is_null() || count == 0 {
        if !raw.is_null() {
            // SAFETY: The pointer was allocated by MFTEnumEx.
            unsafe { CoTaskMemFree(Some(raw.cast())) };
        }
        return Err("Media Foundation found no NV12 to H264 hardware encoders".to_owned());
    }

    // SAFETY: MFTEnumEx returned `count` initialized Option<IMFActivate> values.
    let entries = unsafe { slice::from_raw_parts_mut(raw, count as usize) };
    let activations = entries.iter_mut().filter_map(Option::take).collect();
    // SAFETY: Taking the entries transfers their COM references to the vector,
    // so only the array storage remains to free.
    unsafe { CoTaskMemFree(Some(raw.cast())) };
    Ok(activations)
}

fn configure_async_transform(transform: &IMFTransform) -> Result<(), String> {
    // SAFETY: The activated transform is initialized and its attributes remain
    // valid while the transform is alive.
    let attributes = unsafe { transform.GetAttributes() }
        .map_err(|error| format!("H264 MFT attribute query failed: {error}"))?;
    let is_async = unsafe { attributes.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0;
    if !is_async {
        return Err("hardware H264 MFT does not implement the asynchronous contract".to_owned());
    }
    // SAFETY: Microsoft requires this attribute before using an asynchronous MFT.
    unsafe { attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }
        .map_err(|error| format!("H264 MFT asynchronous unlock failed: {error}"))
}

fn configure_media_types(transform: &IMFTransform) -> Result<(), String> {
    let output_with_color = video_type(MFVideoFormat_H264, true)?;
    // SAFETY: Type objects are fully initialized and stream IDs are fixed at zero.
    if unsafe { transform.SetOutputType(OUTPUT_STREAM_ID, &output_with_color, 0) }.is_err() {
        logging::debug(
            "stream",
            "Media Foundation H264 MFT rejected BT.709 output metadata; retrying core type",
        );
        let output = video_type(MFVideoFormat_H264, false)?;
        // SAFETY: This retry omits only optional color metadata.
        unsafe { transform.SetOutputType(OUTPUT_STREAM_ID, &output, 0) }
            .map_err(|error| format!("H264 MFT output type negotiation failed: {error}"))?;
    }

    let input_with_color = video_type(MFVideoFormat_NV12, true)?;
    // Some older vendor MFTs reject otherwise valid colorimetry attributes. Keep
    // the core NV12 negotiation usable rather than rejecting hardware entirely.
    if unsafe { transform.SetInputType(INPUT_STREAM_ID, &input_with_color, 0) }.is_err() {
        logging::debug(
            "stream",
            "Media Foundation H264 MFT rejected BT.709 input metadata; retrying core type",
        );
        let input = video_type(MFVideoFormat_NV12, false)?;
        unsafe { transform.SetInputType(INPUT_STREAM_ID, &input, 0) }
            .map_err(|error| format!("H264 MFT input type negotiation failed: {error}"))?;
    }
    Ok(())
}

fn video_type(subtype: GUID, with_color: bool) -> Result<IMFMediaType, String> {
    // SAFETY: Media Foundation is initialized and returns an owned type object.
    let media_type = unsafe { MFCreateMediaType() }
        .map_err(|error| format!("Media Foundation video type creation failed: {error}"))?;
    // SAFETY: All values use typed Media Foundation constants and packed ratios.
    let configure = || -> windows::core::Result<()> {
        // SAFETY: All values use typed Media Foundation constants and packed ratios.
        unsafe {
            media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            media_type.SetGUID(&MF_MT_SUBTYPE, &subtype)?;
            media_type.SetUINT64(
                &MF_MT_FRAME_SIZE,
                pack_pair(STREAM_CAPTURE_WIDTH, STREAM_CAPTURE_HEIGHT),
            )?;
            media_type.SetUINT64(&MF_MT_FRAME_RATE, pack_pair(STREAM_CAPTURE_FPS, 1))?;
            media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_pair(1, 1))?;
            media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            if subtype == MFVideoFormat_H264 {
                media_type.SetUINT32(&MF_MT_AVG_BITRATE, STREAM_ENCODER_BITRATE)?;
                media_type.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32)?;
                media_type.SetUINT32(&MF_MT_MPEG2_LEVEL, eAVEncH264VLevel3_1.0 as u32)?;
            }
            if with_color {
                media_type.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)?;
                media_type.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)?;
                media_type.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?;
                media_type.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?;
            }
        }
        Ok(())
    };
    configure().map_err(|error| format!("Media Foundation video type setup failed: {error}"))?;
    Ok(media_type)
}

fn configure_codec(codec: &ICodecApi) -> Result<(), String> {
    // Static encoder properties are set before the output type. Some H264 MFTs
    // ignore these values after media type negotiation has completed.
    set_codec_u32(
        codec,
        &CODECAPI_AVEncCommonRateControlMode,
        eAVEncCommonRateControlMode_CBR.0 as u32,
    )?;
    set_codec_u32(
        codec,
        &CODECAPI_AVEncCommonMeanBitRate,
        STREAM_ENCODER_BITRATE,
    )?;
    set_codec_u32(
        codec,
        &CODECAPI_AVEncMPVGOPSize,
        STREAM_INTRA_FRAME_PERIOD_FRAMES,
    )?;
    set_optional_codec_bool(codec, &CODECAPI_AVEncCommonRealTime, true, "real-time mode");
    set_optional_codec_bool(
        codec,
        &CODECAPI_AVEncCommonLowLatency,
        true,
        "low decoding latency",
    );
    set_optional_codec_bool(
        codec,
        &CODECAPI_AVLowLatencyMode,
        true,
        "low-latency processing",
    );
    Ok(())
}

fn set_optional_codec_bool(codec: &ICodecApi, key: &GUID, value: bool, name: &str) {
    if let Err(error) = set_codec_bool(codec, key, value) {
        logging::debug(
            "stream",
            format!("Media Foundation H264 MFT does not support optional {name}: {error}"),
        );
    }
}

fn set_codec_u32(codec: &ICodecApi, key: &GUID, value: u32) -> Result<(), String> {
    let variant = variant_u32(value);
    // SAFETY: The VARIANT contains an inline VT_UI4 value.
    unsafe { codec.set_value(key, &variant) }
        .map_err(|error| format!("H264 codec property {key:?}={value} was rejected: {error}"))
}

fn set_codec_bool(codec: &ICodecApi, key: &GUID, value: bool) -> Result<(), String> {
    let variant = variant_bool(value);
    // SAFETY: The VARIANT contains an inline VT_BOOL value.
    unsafe { codec.set_value(key, &variant) }
        .map_err(|error| format!("H264 codec property {key:?}={value} was rejected: {error}"))
}

fn variant_u32(value: u32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_UI4,
                Anonymous: VARIANT_0_0_0 { ulVal: value },
                ..Default::default()
            }),
        },
    }
}

fn variant_bool(value: bool) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_BOOL,
                Anonymous: VARIANT_0_0_0 {
                    boolVal: VARIANT_BOOL(if value { -1 } else { 0 }),
                },
                ..Default::default()
            }),
        },
    }
}

fn nv12_frame_length() -> Result<usize, String> {
    let pixels = (STREAM_CAPTURE_WIDTH as usize)
        .checked_mul(STREAM_CAPTURE_HEIGHT as usize)
        .ok_or_else(|| "Media Foundation NV12 frame size overflowed".to_owned())?;
    pixels
        .checked_add(pixels / 2)
        .ok_or_else(|| "Media Foundation NV12 frame size overflowed".to_owned())
}

fn prepare_input_sample(
    sample: &IMFSample,
    input: I420Frame<'_>,
    frame_index: i64,
) -> Result<(), String> {
    let width = STREAM_CAPTURE_WIDTH as usize;
    let height = STREAM_CAPTURE_HEIGHT as usize;
    if input.width != width || input.height != height {
        return Err(format!(
            "Media Foundation input dimensions must be {width}x{height}, got {}x{}",
            input.width, input.height
        ));
    }
    let buffer = unsafe { sample.GetBufferByIndex(0) }
        .map_err(|error| format!("Media Foundation input buffer query failed: {error}"))?;
    write_i420_to_nv12_buffer(&buffer, input)?;
    let duration = HNS_PER_SECOND / i64::from(STREAM_CAPTURE_FPS);
    // SAFETY: Timestamps are non-negative 100 ns Media Foundation units.
    let timestamp = || -> windows::core::Result<()> {
        // SAFETY: Timestamps are non-negative 100 ns Media Foundation units.
        unsafe {
            sample.SetSampleTime(frame_index.saturating_mul(duration))?;
            sample.SetSampleDuration(duration)?;
        }
        Ok(())
    };
    timestamp()
        .map_err(|error| format!("Media Foundation input timestamp setup failed: {error}"))?;
    Ok(())
}

fn create_empty_sample(capacity: u32) -> Result<IMFSample, String> {
    // SAFETY: Media Foundation is initialized and owns returned COM objects.
    let sample = unsafe { MFCreateSample() }
        .map_err(|error| format!("Media Foundation sample creation failed: {error}"))?;
    let buffer = unsafe { MFCreateMemoryBuffer(capacity) }
        .map_err(|error| format!("Media Foundation buffer allocation failed: {error}"))?;
    unsafe { sample.AddBuffer(&buffer) }
        .map_err(|error| format!("Media Foundation sample buffer attachment failed: {error}"))?;
    Ok(sample)
}

fn write_i420_to_nv12_buffer(buffer: &IMFMediaBuffer, input: I420Frame<'_>) -> Result<(), String> {
    let width = input.width;
    let height = input.height;
    let y_len = width
        .checked_mul(height)
        .ok_or_else(|| "Media Foundation NV12 Y plane size overflowed".to_owned())?;
    let frame_len = y_len
        .checked_add(y_len / 2)
        .ok_or_else(|| "Media Foundation NV12 frame size overflowed".to_owned())?;
    let mut destination = ptr::null_mut();
    let mut capacity = 0;
    // SAFETY: Lock initializes the pointer and capacity until the matching Unlock.
    unsafe { buffer.Lock(&mut destination, Some(&mut capacity), None) }
        .map_err(|error| format!("Media Foundation input buffer lock failed: {error}"))?;
    let result = if frame_len <= capacity as usize {
        // SAFETY: Lock returned at least frame_len writable bytes until Unlock.
        let destination = unsafe { slice::from_raw_parts_mut(destination, frame_len) };
        let (y, uv) = destination.split_at_mut(y_len);
        copy_i420_to_nv12(input, y, uv, width, width)
    } else {
        Err("Media Foundation input buffer is smaller than one NV12 frame".to_owned())
    };
    // SAFETY: This matches the successful Lock above.
    let unlock = unsafe { buffer.Unlock() };
    result?;
    unlock.map_err(|error| format!("Media Foundation input buffer unlock failed: {error}"))?;
    unsafe { buffer.SetCurrentLength(frame_len as u32) }
        .map_err(|error| format!("Media Foundation input length update failed: {error}"))
}

fn reset_output_sample(sample: &IMFSample) -> Result<(), String> {
    // Reused output samples must not carry CleanPoint or other attributes from
    // the previous frame when a driver omits them on the next output.
    unsafe { sample.DeleteAllItems() }
        .map_err(|error| format!("Media Foundation output sample reset failed: {error}"))?;
    let buffer = unsafe { sample.GetBufferByIndex(0) }
        .map_err(|error| format!("Media Foundation output buffer query failed: {error}"))?;
    unsafe { buffer.SetCurrentLength(0) }
        .map_err(|error| format!("Media Foundation output buffer reset failed: {error}"))
}

fn read_sample(sample: &IMFSample) -> Result<Vec<u8>, String> {
    // SAFETY: The output sample owns all buffers until this function returns.
    let buffer = unsafe { sample.ConvertToContiguousBuffer() }
        .map_err(|error| format!("Media Foundation output buffer conversion failed: {error}"))?;
    let length = unsafe { buffer.GetCurrentLength() }
        .map_err(|error| format!("Media Foundation output length query failed: {error}"))?;
    if length == 0 {
        return Ok(Vec::new());
    }
    let mut source = ptr::null_mut();
    // SAFETY: Lock initializes source for the lifetime ending at Unlock.
    unsafe { buffer.Lock(&mut source, None, None) }
        .map_err(|error| format!("Media Foundation output buffer lock failed: {error}"))?;
    // SAFETY: GetCurrentLength bytes are readable while the buffer is locked.
    let bytes = unsafe { slice::from_raw_parts(source, length as usize) }.to_vec();
    // SAFETY: This matches the successful Lock above.
    unsafe { buffer.Unlock() }
        .map_err(|error| format!("Media Foundation output buffer unlock failed: {error}"))?;
    Ok(bytes)
}

fn normalize_sequence_header(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.starts_with(&[0, 0, 1]) || bytes.starts_with(&[0, 0, 0, 1]) {
        return Ok(bytes.to_vec());
    }
    if bytes.first() != Some(&1) || bytes.len() < 7 {
        return normalize_h264_access_unit(bytes);
    }

    // AVCDecoderConfigurationRecord stores SPS and PPS as 16-bit length-prefixed
    // arrays rather than as an access unit.
    let mut cursor = 6;
    let sps_count = usize::from(bytes[5] & 0x1f);
    let mut output = Vec::new();
    for _ in 0..sps_count {
        append_avcc_parameter_set(bytes, &mut cursor, &mut output)?;
    }
    let pps_count = *bytes
        .get(cursor)
        .ok_or_else(|| "Media Foundation AVC sequence header omits the PPS count".to_owned())?;
    cursor += 1;
    for _ in 0..pps_count {
        append_avcc_parameter_set(bytes, &mut cursor, &mut output)?;
    }
    if output.is_empty() {
        return Err("Media Foundation AVC sequence header contains no SPS/PPS".to_owned());
    }
    Ok(output)
}

fn append_avcc_parameter_set(
    bytes: &[u8],
    cursor: &mut usize,
    output: &mut Vec<u8>,
) -> Result<(), String> {
    let length_bytes = bytes
        .get(*cursor..*cursor + 2)
        .ok_or_else(|| "Media Foundation AVC sequence header is truncated".to_owned())?;
    let length = usize::from(u16::from_be_bytes([length_bytes[0], length_bytes[1]]));
    *cursor += 2;
    let end = cursor
        .checked_add(length)
        .ok_or_else(|| "Media Foundation AVC sequence header length overflowed".to_owned())?;
    let parameter_set = bytes
        .get(*cursor..end)
        .ok_or_else(|| "Media Foundation AVC parameter set is truncated".to_owned())?;
    output.extend_from_slice(&[0, 0, 0, 1]);
    output.extend_from_slice(parameter_set);
    *cursor = end;
    Ok(())
}

fn contains_nal_type(annex_b: &[u8], expected_type: u8) -> bool {
    let mut index = 0;
    while index + 3 <= annex_b.len() {
        let start_length = if annex_b[index..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if annex_b[index..].starts_with(&[0, 0, 1]) {
            3
        } else {
            index += 1;
            continue;
        };
        if annex_b
            .get(index + start_length)
            .is_some_and(|header| header & 0x1f == expected_type)
        {
            return true;
        }
        index += start_length;
    }
    false
}

const fn pack_pair(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a physical Media Foundation H264 encoder"]
    fn hardware_encoder_produces_parameterized_idr() {
        let mut encoder = MediaFoundationEncoder::new()
            .expect("Media Foundation hardware encoder should pass its startup probe");
        let width = STREAM_CAPTURE_WIDTH as usize;
        let height = STREAM_CAPTURE_HEIGHT as usize;
        let y = vec![16; width * height];
        let u = vec![128; width * height / 4];
        let v = vec![128; width * height / 4];
        let frame = I420Frame::new(&y, &u, &v, width, height, width, width / 2, width / 2);

        let mut encoded = None;
        for _ in 0..8 {
            if let Some(output) = encoder
                .encode(frame, true)
                .expect("Media Foundation should encode a production frame")
            {
                encoded = Some(output);
                break;
            }
        }
        let encoded = encoded.expect("Media Foundation should return an encoded frame");

        assert!(encoded.is_keyframe);
        assert!(contains_nal_type(&encoded.annex_b, 7));
        assert!(contains_nal_type(&encoded.annex_b, 8));
        assert!(contains_nal_type(&encoded.annex_b, 5));
    }
}
