mod avatar;
mod cache;
mod decode;
mod emoji;
mod preview;
mod protocol;
mod protocol_job;
mod targets;
mod work;

pub(super) use avatar::AvatarImageCache;
pub(super) use decode::{
    MediaImageDecodeCache, MediaImageDecodeDelivery, MediaImageDecodeKey, MediaImageDecodeResult,
    spawn_media_image_decode,
};
pub(super) use emoji::EmojiImageCache;
pub(super) use preview::ImagePreviewCache;
pub(in crate::tui) use preview::ImagePreviewKey;
#[cfg(test)]
use protocol_job::build_media_protocol;
pub(super) use protocol_job::{
    MediaProtocolBuildResult, MediaProtocolBuildTarget, spawn_media_protocol_build,
};
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
#[cfg(test)]
use protocol::clipped_media_image;
use protocol::{AVATAR_PREVIEW_HEIGHT, AVATAR_PREVIEW_WIDTH, avatar_preview_url, emoji_protocol};
pub(in crate::tui) use protocol::{
    MediaProtocolRenderSpec, clipped_media_protocol, fixed_media_protocol_render_spec,
    picker_font_size, query_image_picker,
};
pub(super) use protocol::{PROFILE_POPUP_AVATAR_HEIGHT, PROFILE_POPUP_AVATAR_WIDTH};

#[cfg(test)]
use avatar::{AvatarImageEntry, AvatarProtocolKey, MAX_AVATAR_IMAGE_CACHE_ENTRIES};
#[cfg(test)]
use decode::{
    MAX_DECODED_IMAGE_HEIGHT, MAX_DECODED_IMAGE_WIDTH, MAX_RETAINED_ANIMATION_FRAMES,
    decode_media_image_bytes,
};
#[cfg(test)]
use emoji::EmojiImageEntry;
#[cfg(test)]
use preview::{ImagePreviewEntry, MAX_IMAGE_PREVIEW_CACHE_ENTRIES};

#[cfg(test)]
mod tests;
