use std::{collections::HashMap, ffi::c_void, mem, ptr};

use glow::HasContext;
use khronos_egl as egl;

use super::super::super::conversion::{CaptureColorInfo, CapturePixelFormat, normalize_rgba_color};

const EGL_PLATFORM_SURFACELESS_MESA: egl::Enum = 0x31dd;
const EGL_LINUX_DMA_BUF_EXT: egl::Enum = 0x3270;
const EGL_LINUX_DRM_FOURCC_EXT: egl::Attrib = 0x3271;
const EGL_DMA_BUF_PLANE_FD_EXT: [egl::Attrib; 4] = [0x3272, 0x3275, 0x3278, 0x3440];
const EGL_DMA_BUF_PLANE_OFFSET_EXT: [egl::Attrib; 4] = [0x3273, 0x3276, 0x3279, 0x3441];
const EGL_DMA_BUF_PLANE_PITCH_EXT: [egl::Attrib; 4] = [0x3274, 0x3277, 0x327a, 0x3442];
const EGL_DMA_BUF_PLANE_MODIFIER_LO_EXT: [egl::Attrib; 4] = [0x3443, 0x3445, 0x3447, 0x3449];
const EGL_DMA_BUF_PLANE_MODIFIER_HI_EXT: [egl::Attrib; 4] = [0x3444, 0x3446, 0x3448, 0x344a];
const DRM_FORMAT_MOD_INVALID: u64 = u64::MAX;

type QueryDmaBufFormats = unsafe extern "system" fn(
    egl::EGLDisplay,
    egl::Int,
    *mut egl::Int,
    *mut egl::Int,
) -> egl::Boolean;
type QueryDmaBufModifiers = unsafe extern "system" fn(
    egl::EGLDisplay,
    egl::Int,
    egl::Int,
    *mut u64,
    *mut egl::Boolean,
    *mut egl::Int,
) -> egl::Boolean;
type ImageTargetTexture2d = unsafe extern "system" fn(u32, *const c_void);

#[derive(Clone, Copy, Debug)]
pub(in super::super) struct DmaBufPlane {
    pub(in super::super) fd: i32,
    pub(in super::super) offset: u32,
    pub(in super::super) pitch: u32,
}

pub(in super::super) struct EglDmaBufImporter {
    egl: egl::DynamicInstance<egl::EGL1_5>,
    display: egl::Display,
    context: egl::Context,
    gl: glow::Context,
    image_target_texture_2d: ImageTargetTexture2d,
    program: glow::Program,
    vertex_array: glow::VertexArray,
    modifiers: HashMap<CapturePixelFormat, Vec<u64>>,
}

impl EglDmaBufImporter {
    pub(in super::super) fn new() -> Result<Self, String> {
        let egl = unsafe { egl::DynamicInstance::<egl::EGL1_5>::load_required() }
            .map_err(|error| format!("EGL library loading failed: {error}"))?;
        let display = unsafe {
            egl.get_platform_display(
                EGL_PLATFORM_SURFACELESS_MESA,
                ptr::null_mut(),
                &[egl::ATTRIB_NONE],
            )
        }
        .map_err(|error| format!("surfaceless EGL display creation failed: {error}"))?;
        egl.initialize(display)
            .map_err(|error| format!("EGL display initialization failed: {error}"))?;

        let extensions = egl
            .query_string(Some(display), egl::EXTENSIONS)
            .map_err(|error| format!("EGL extension query failed: {error}"))?
            .to_string_lossy();
        for required in [
            "EGL_EXT_image_dma_buf_import",
            "EGL_EXT_image_dma_buf_import_modifiers",
        ] {
            if !extensions
                .split_ascii_whitespace()
                .any(|name| name == required)
            {
                let _ = egl.terminate(display);
                return Err(format!("EGL display does not support {required}"));
            }
        }

        egl.bind_api(egl::OPENGL_ES_API)
            .map_err(|error| format!("OpenGL ES EGL binding failed: {error}"))?;
        let config = egl
            .choose_first_config(
                display,
                &[
                    egl::SURFACE_TYPE,
                    egl::PBUFFER_BIT,
                    egl::RENDERABLE_TYPE,
                    egl::OPENGL_ES3_BIT,
                    egl::RED_SIZE,
                    8,
                    egl::GREEN_SIZE,
                    8,
                    egl::BLUE_SIZE,
                    8,
                    egl::ALPHA_SIZE,
                    8,
                    egl::NONE,
                ],
            )
            .map_err(|error| format!("EGL configuration query failed: {error}"))?
            .ok_or_else(|| "EGL has no OpenGL ES 3 configuration".to_owned())?;
        let context = egl
            .create_context(
                display,
                config,
                None,
                &[egl::CONTEXT_CLIENT_VERSION, 3, egl::NONE],
            )
            .map_err(|error| format!("EGL context creation failed: {error}"))?;
        egl.make_current(display, None, None, Some(context))
            .map_err(|error| format!("EGL context activation failed: {error}"))?;

        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                egl.get_proc_address(name)
                    .map_or(ptr::null(), |address| address as *const () as *const c_void)
            })
        };
        let image_target_texture_2d =
            load_egl_proc::<ImageTargetTexture2d>(&egl, "glEGLImageTargetTexture2DOES")?;
        let query_formats = load_egl_proc::<QueryDmaBufFormats>(&egl, "eglQueryDmaBufFormatsEXT")?;
        let query_modifiers =
            load_egl_proc::<QueryDmaBufModifiers>(&egl, "eglQueryDmaBufModifiersEXT")?;
        let modifiers = query_supported_modifiers(display, query_formats, query_modifiers)?;
        let (program, vertex_array) = create_copy_pipeline(&gl)?;

        Ok(Self {
            egl,
            display,
            context,
            gl,
            image_target_texture_2d,
            program,
            vertex_array,
            modifiers,
        })
    }

    pub(in super::super) fn modifiers(&self, format: CapturePixelFormat) -> &[u64] {
        self.modifiers.get(&format).map_or(&[], Vec::as_slice)
    }

    pub(in super::super) fn supports(&self, format: CapturePixelFormat, modifier: u64) -> bool {
        self.modifiers(format).contains(&modifier)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn import(
        &mut self,
        width: u32,
        height: u32,
        format: CapturePixelFormat,
        modifier: u64,
        color: CaptureColorInfo,
        planes: &[DmaBufPlane],
        rgba: &mut [u8],
    ) -> Result<(), String> {
        if !self.supports(format, modifier) {
            return Err(format!(
                "EGL does not support capture format {format:?} with modifier {modifier:#x}"
            ));
        }
        if planes.is_empty() || planes.len() > EGL_DMA_BUF_PLANE_FD_EXT.len() {
            return Err(format!(
                "EGL DMA-BUF import received an invalid plane count: {}",
                planes.len()
            ));
        }
        let width_i32 =
            i32::try_from(width).map_err(|_| "EGL DMA-BUF width is too large".to_owned())?;
        let height_i32 =
            i32::try_from(height).map_err(|_| "EGL DMA-BUF height is too large".to_owned())?;
        let expected_length = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|length| usize::try_from(length).ok())
            .ok_or_else(|| "EGL RGBA output is too large".to_owned())?;
        if rgba.len() != expected_length {
            return Err(format!(
                "EGL RGBA output has the wrong length: required={expected_length} available={}",
                rgba.len()
            ));
        }

        self.egl
            .make_current(self.display, None, None, Some(self.context))
            .map_err(|error| format!("EGL context reactivation failed: {error}"))?;
        let mut attributes = vec![
            egl::WIDTH as egl::Attrib,
            width as egl::Attrib,
            egl::HEIGHT as egl::Attrib,
            height as egl::Attrib,
            EGL_LINUX_DRM_FOURCC_EXT,
            drm_format(format)? as egl::Attrib,
        ];
        for (index, plane) in planes.iter().enumerate() {
            attributes.extend_from_slice(&[
                EGL_DMA_BUF_PLANE_FD_EXT[index],
                plane.fd as egl::Attrib,
                EGL_DMA_BUF_PLANE_OFFSET_EXT[index],
                plane.offset as egl::Attrib,
                EGL_DMA_BUF_PLANE_PITCH_EXT[index],
                plane.pitch as egl::Attrib,
                EGL_DMA_BUF_PLANE_MODIFIER_LO_EXT[index],
                (modifier & u64::from(u32::MAX)) as egl::Attrib,
                EGL_DMA_BUF_PLANE_MODIFIER_HI_EXT[index],
                (modifier >> 32) as egl::Attrib,
            ]);
        }
        attributes.push(egl::ATTRIB_NONE);
        let no_context = unsafe { egl::Context::from_ptr(egl::NO_CONTEXT) };
        let no_buffer = unsafe { egl::ClientBuffer::from_ptr(ptr::null_mut()) };
        let image = self
            .egl
            .create_image(
                self.display,
                no_context,
                EGL_LINUX_DMA_BUF_EXT,
                no_buffer,
                &attributes,
            )
            .map_err(|error| format!("EGL DMA-BUF image import failed: {error}"))?;

        let result = self.render_image(image, width_i32, height_i32, rgba);
        let destroy_result = self
            .egl
            .destroy_image(self.display, image)
            .map_err(|error| format!("EGL DMA-BUF image cleanup failed: {error}"));
        match (result, destroy_result) {
            (Ok(()), Ok(())) => {
                normalize_rgba_color(rgba, color);
                Ok(())
            }
            (Err(error), _) | (Ok(()), Err(error)) => Err(error),
        }
    }

    fn render_image(
        &self,
        image: egl::Image,
        width: i32,
        height: i32,
        rgba: &mut [u8],
    ) -> Result<(), String> {
        let input = unsafe { self.gl.create_texture() }
            .map_err(|error| format!("EGL input texture creation failed: {error}"))?;
        let output = match unsafe { self.gl.create_texture() } {
            Ok(texture) => texture,
            Err(error) => {
                unsafe { self.gl.delete_texture(input) };
                return Err(format!("EGL output texture creation failed: {error}"));
            }
        };
        let framebuffer = match unsafe { self.gl.create_framebuffer() } {
            Ok(framebuffer) => framebuffer,
            Err(error) => {
                unsafe {
                    self.gl.delete_texture(output);
                    self.gl.delete_texture(input);
                }
                return Err(format!("EGL framebuffer creation failed: {error}"));
            }
        };

        let result = (|| {
            unsafe {
                self.gl.active_texture(glow::TEXTURE0);
                self.gl.bind_texture(glow::TEXTURE_2D, Some(input));
                (self.image_target_texture_2d)(glow::TEXTURE_2D, image.as_ptr());
                check_gl_error(&self.gl, "binding the EGL image to a texture")?;
                self.gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::NEAREST as i32,
                );
                self.gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::NEAREST as i32,
                );

                self.gl.bind_texture(glow::TEXTURE_2D, Some(output));
                self.gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    width,
                    height,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                self.gl
                    .bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
                self.gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(output),
                    0,
                );
                let status = self.gl.check_framebuffer_status(glow::FRAMEBUFFER);
                if status != glow::FRAMEBUFFER_COMPLETE {
                    return Err(format!("EGL framebuffer is incomplete: {status:#x}"));
                }

                self.gl.viewport(0, 0, width, height);
                self.gl.use_program(Some(self.program));
                self.gl.bind_vertex_array(Some(self.vertex_array));
                self.gl.active_texture(glow::TEXTURE0);
                self.gl.bind_texture(glow::TEXTURE_2D, Some(input));
                self.gl.draw_arrays(glow::TRIANGLES, 0, 3);
                self.gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
                self.gl.read_pixels(
                    0,
                    0,
                    width,
                    height,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(rgba)),
                );
                self.gl.finish();
                check_gl_error(&self.gl, "rendering the EGL image into RGBA")?;
            }
            Ok(())
        })();

        unsafe {
            self.gl.bind_vertex_array(None);
            self.gl.use_program(None);
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.bind_texture(glow::TEXTURE_2D, None);
            self.gl.delete_framebuffer(framebuffer);
            self.gl.delete_texture(output);
            self.gl.delete_texture(input);
        }
        result
    }
}

impl Drop for EglDmaBufImporter {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_vertex_array(self.vertex_array);
            self.gl.delete_program(self.program);
        }
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_context(self.display, self.context);
        let _ = self.egl.terminate(self.display);
    }
}

fn check_gl_error(gl: &glow::Context, context: &str) -> Result<(), String> {
    let error = unsafe { gl.get_error() };
    if error == glow::NO_ERROR {
        Ok(())
    } else {
        Err(format!("OpenGL ES failed while {context}: {error:#x}"))
    }
}

fn load_egl_proc<T: Copy>(
    egl: &egl::DynamicInstance<egl::EGL1_5>,
    name: &str,
) -> Result<T, String> {
    let address = egl
        .get_proc_address(name)
        .ok_or_else(|| format!("EGL function {name} is unavailable"))?;
    if mem::size_of::<T>() != mem::size_of_val(&address) {
        return Err(format!(
            "EGL function {name} has an unexpected pointer size"
        ));
    }
    Ok(unsafe { mem::transmute_copy(&address) })
}

fn query_supported_modifiers(
    display: egl::Display,
    query_formats: QueryDmaBufFormats,
    query_modifiers: QueryDmaBufModifiers,
) -> Result<HashMap<CapturePixelFormat, Vec<u64>>, String> {
    let mut format_count = 0;
    if unsafe { query_formats(display.as_ptr(), 0, ptr::null_mut(), &mut format_count) }
        != egl::TRUE
    {
        return Err("EGL DMA-BUF format count query failed".to_owned());
    }
    let mut drm_formats = vec![0; format_count.max(0) as usize];
    if format_count > 0
        && unsafe {
            query_formats(
                display.as_ptr(),
                format_count,
                drm_formats.as_mut_ptr(),
                &mut format_count,
            )
        } != egl::TRUE
    {
        return Err("EGL DMA-BUF format query failed".to_owned());
    }

    let mut supported = HashMap::new();
    for capture_format in egl_capture_formats() {
        let drm_format = drm_format(capture_format)?;
        if !drm_formats.contains(&(drm_format as egl::Int)) {
            continue;
        }
        let mut modifier_count = 0;
        if unsafe {
            query_modifiers(
                display.as_ptr(),
                drm_format as egl::Int,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut modifier_count,
            )
        } != egl::TRUE
        {
            continue;
        }
        let mut modifiers = vec![0; modifier_count.max(0) as usize];
        let mut external_only = vec![egl::FALSE; modifiers.len()];
        if modifier_count > 0
            && unsafe {
                query_modifiers(
                    display.as_ptr(),
                    drm_format as egl::Int,
                    modifier_count,
                    modifiers.as_mut_ptr(),
                    external_only.as_mut_ptr(),
                    &mut modifier_count,
                )
            } != egl::TRUE
        {
            continue;
        }
        modifiers.truncate(modifier_count.max(0) as usize);
        external_only.truncate(modifiers.len());
        let modifiers = modifiers
            .into_iter()
            .zip(external_only)
            .filter_map(|(modifier, external_only)| {
                // Linear stays in: importing it through EGL is what keeps the
                // CPU from reading GPU memory. Dropping it from the advertised
                // formats is the caller's job.
                (external_only == egl::FALSE && modifier != DRM_FORMAT_MOD_INVALID)
                    .then_some(modifier)
            })
            .collect::<Vec<_>>();
        if !modifiers.is_empty() {
            supported.insert(capture_format, modifiers);
        }
    }
    Ok(supported)
}

fn create_copy_pipeline(gl: &glow::Context) -> Result<(glow::Program, glow::VertexArray), String> {
    let program = unsafe { gl.create_program() }
        .map_err(|error| format!("EGL copy program creation failed: {error}"))?;
    let vertex = compile_shader(
        gl,
        glow::VERTEX_SHADER,
        r#"#version 300 es
        out vec2 texture_coordinate;
        const vec2 positions[3] = vec2[3](
            vec2(-1.0, -1.0),
            vec2(3.0, -1.0),
            vec2(-1.0, 3.0)
        );
        void main() {
            vec2 position = positions[gl_VertexID];
            texture_coordinate = position * 0.5 + 0.5;
            gl_Position = vec4(position, 0.0, 1.0);
        }"#,
    )?;
    let fragment = match compile_shader(
        gl,
        glow::FRAGMENT_SHADER,
        r#"#version 300 es
        precision highp float;
        in vec2 texture_coordinate;
        uniform sampler2D captured_frame;
        out vec4 output_color;
        void main() {
            output_color = texture(captured_frame, texture_coordinate);
        }"#,
    ) {
        Ok(shader) => shader,
        Err(error) => {
            unsafe {
                gl.delete_shader(vertex);
                gl.delete_program(program);
            }
            return Err(error);
        }
    };
    unsafe {
        gl.attach_shader(program, vertex);
        gl.attach_shader(program, fragment);
        gl.link_program(program);
        gl.detach_shader(program, fragment);
        gl.detach_shader(program, vertex);
        gl.delete_shader(fragment);
        gl.delete_shader(vertex);
    }
    if !unsafe { gl.get_program_link_status(program) } {
        let log = unsafe { gl.get_program_info_log(program) };
        unsafe { gl.delete_program(program) };
        return Err(format!("EGL copy program linking failed: {log}"));
    }
    let vertex_array = match unsafe { gl.create_vertex_array() } {
        Ok(vertex_array) => vertex_array,
        Err(error) => {
            unsafe { gl.delete_program(program) };
            return Err(format!("EGL copy vertex array creation failed: {error}"));
        }
    };
    unsafe {
        gl.use_program(Some(program));
        if let Some(location) = gl.get_uniform_location(program, "captured_frame") {
            gl.uniform_1_i32(Some(&location), 0);
        }
        gl.use_program(None);
    }
    Ok((program, vertex_array))
}

fn compile_shader(gl: &glow::Context, kind: u32, source: &str) -> Result<glow::Shader, String> {
    let shader = unsafe { gl.create_shader(kind) }
        .map_err(|error| format!("EGL copy shader creation failed: {error}"))?;
    unsafe {
        gl.shader_source(shader, source);
        gl.compile_shader(shader);
    }
    if unsafe { gl.get_shader_compile_status(shader) } {
        Ok(shader)
    } else {
        let log = unsafe { gl.get_shader_info_log(shader) };
        unsafe { gl.delete_shader(shader) };
        Err(format!("EGL copy shader compilation failed: {log}"))
    }
}

fn egl_capture_formats() -> [CapturePixelFormat; 8] {
    [
        CapturePixelFormat::Rgba,
        CapturePixelFormat::Rgbx,
        CapturePixelFormat::Bgra,
        CapturePixelFormat::Bgrx,
        CapturePixelFormat::Xrgb210Le,
        CapturePixelFormat::Xbgr210Le,
        CapturePixelFormat::Rgbx102Le,
        CapturePixelFormat::Bgrx102Le,
    ]
}

fn drm_format(format: CapturePixelFormat) -> Result<u32, String> {
    let code = match format {
        CapturePixelFormat::Rgba => fourcc(*b"AB24"),
        CapturePixelFormat::Rgbx => fourcc(*b"XB24"),
        CapturePixelFormat::Bgra => fourcc(*b"AR24"),
        CapturePixelFormat::Bgrx => fourcc(*b"XR24"),
        CapturePixelFormat::Xrgb210Le => fourcc(*b"XR30"),
        CapturePixelFormat::Xbgr210Le => fourcc(*b"XB30"),
        CapturePixelFormat::Rgbx102Le => fourcc(*b"RX30"),
        CapturePixelFormat::Bgrx102Le => fourcc(*b"BX30"),
        CapturePixelFormat::Nv12 | CapturePixelFormat::P010Le => {
            return Err(format!(
                "EGL non-linear import is not enabled for capture format {format:?}"
            ));
        }
    };
    Ok(code)
}

const fn fourcc(bytes: [u8; 4]) -> u32 {
    bytes[0] as u32 | (bytes[1] as u32) << 8 | (bytes[2] as u32) << 16 | (bytes[3] as u32) << 24
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipewire_formats_map_to_the_expected_drm_fourcc_values() {
        let cases = [
            (CapturePixelFormat::Rgba, *b"AB24"),
            (CapturePixelFormat::Rgbx, *b"XB24"),
            (CapturePixelFormat::Bgra, *b"AR24"),
            (CapturePixelFormat::Bgrx, *b"XR24"),
            (CapturePixelFormat::Xrgb210Le, *b"XR30"),
            (CapturePixelFormat::Xbgr210Le, *b"XB30"),
            (CapturePixelFormat::Rgbx102Le, *b"RX30"),
            (CapturePixelFormat::Bgrx102Le, *b"BX30"),
        ];
        for (format, code) in cases {
            assert_eq!(drm_format(format), Ok(fourcc(code)), "format={format:?}");
        }
    }
}
