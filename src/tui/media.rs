mod avatar;
mod cache;
mod decode;
mod emoji;
mod preview;
mod protocol;
mod targets;

pub(super) use avatar::AvatarImageCache;
pub(super) use decode::{MediaImageDecodeKey, MediaImageDecodeResult, spawn_media_image_decode};
pub(super) use emoji::EmojiImageCache;
pub(super) use preview::ImagePreviewCache;
#[cfg(test)]
use targets::image_preview_height_for_dimensions;
pub(super) use targets::{
    AvatarTarget, EmojiImageTarget, ImagePreviewTarget, image_preview_album_layout,
    visible_avatar_targets_from_plan, visible_emoji_image_targets,
    visible_image_preview_targets_from_plan,
};
#[cfg(test)]
pub(super) use targets::{visible_avatar_targets, visible_image_preview_targets};

pub(in crate::tui) use decode::decode_image_bytes;
use protocol::{
    AVATAR_PREVIEW_HEIGHT, AVATAR_PREVIEW_WIDTH, avatar_preview_url, clipped_preview_image,
    emoji_protocol, picker_font_size,
};
pub(in crate::tui) use protocol::{
    ImagePreviewRenderInfo, clipped_preview_protocol, fixed_image_preview_render_info,
    query_image_picker,
};
pub(super) use protocol::{PROFILE_POPUP_AVATAR_HEIGHT, PROFILE_POPUP_AVATAR_WIDTH};

#[cfg(test)]
use avatar::{AvatarImageEntry, AvatarProtocolKey, MAX_AVATAR_IMAGE_CACHE_ENTRIES};
#[cfg(test)]
use decode::{MAX_DECODED_IMAGE_HEIGHT, MAX_DECODED_IMAGE_WIDTH};
#[cfg(test)]
use emoji::{EmojiImageEntry, MAX_EMOJI_IMAGE_CACHE_ENTRIES};
#[cfg(test)]
use preview::{ImagePreviewEntry, MAX_IMAGE_PREVIEW_CACHE_ENTRIES, decode_original_preview_image};

#[cfg(test)]
mod tests;
