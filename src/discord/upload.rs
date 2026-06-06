use crate::discord::{MAX_PROFILE_AVATAR_BYTES, ProfileAvatarUpload};
use image::ImageFormat;

pub(crate) struct ProfileAvatarImage {
    pub(crate) content_type: String,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) async fn read_profile_avatar_image(
    upload: &ProfileAvatarUpload,
) -> std::result::Result<ProfileAvatarImage, String> {
    if let Some(bytes) = upload.bytes() {
        if bytes.len() as u64 > MAX_PROFILE_AVATAR_BYTES {
            return Err(format!(
                "profile avatar is too large: {} bytes",
                bytes.len()
            ));
        }
        let content_type = profile_avatar_content_type(bytes)?;
        return Ok(ProfileAvatarImage {
            content_type,
            bytes: bytes.to_vec(),
        });
    }

    let Some(path) = upload.path() else {
        return Err("profile avatar has no image data".to_owned());
    };
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| format!("stat profile image failed: {error}"))?;
    if !metadata.is_file() {
        return Err("profile avatar must be a regular file".to_owned());
    }
    let size = metadata.len();
    if size > MAX_PROFILE_AVATAR_BYTES {
        return Err(format!("profile avatar is too large: {size} bytes"));
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| format!("read profile image failed: {error}"))?;
    if bytes.len() as u64 > MAX_PROFILE_AVATAR_BYTES {
        return Err(format!(
            "profile avatar is too large: {} bytes",
            bytes.len()
        ));
    }
    let content_type = profile_avatar_content_type(&bytes)?;

    Ok(ProfileAvatarImage {
        content_type,
        bytes,
    })
}

fn profile_avatar_content_type(bytes: &[u8]) -> std::result::Result<String, String> {
    let format = image::guess_format(bytes).map_err(|_| {
        "profile avatar must contain valid PNG, JPEG, GIF, or WebP image data".to_owned()
    })?;
    image::load_from_memory_with_format(bytes, format).map_err(|_| {
        "profile avatar must contain valid PNG, JPEG, GIF, or WebP image data".to_owned()
    })?;
    match format {
        ImageFormat::Png => Ok("image/png".to_owned()),
        ImageFormat::Jpeg => Ok("image/jpeg".to_owned()),
        ImageFormat::Gif => Ok("image/gif".to_owned()),
        ImageFormat::WebP => Ok("image/webp".to_owned()),
        _ => Err("profile avatar must be a PNG, JPEG, GIF, or WebP image".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::profile_avatar_content_type;
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

    #[test]
    fn profile_avatar_content_type_uses_image_bytes() {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0])));
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, ImageFormat::Png)
            .expect("test png should encode");

        assert_eq!(
            profile_avatar_content_type(png.get_ref()),
            Ok("image/png".to_owned())
        );
    }

    #[test]
    fn profile_avatar_content_type_rejects_non_image_bytes() {
        let error = profile_avatar_content_type(b"not actually a png")
            .expect_err("non-image bytes should fail");

        assert_eq!(
            error,
            "profile avatar must contain valid PNG, JPEG, GIF, or WebP image data"
        );
    }
}
