mod cache;
mod preview;
mod protocol;
mod targets;

pub(super) use cache::{AvatarImageCache, EmojiImageCache};
pub(super) use preview::{ImagePreviewCache, ImagePreviewDecodeResult, spawn_image_preview_decode};
#[cfg(test)]
use targets::image_preview_height_for_dimensions;
pub(super) use targets::{
    AvatarTarget, EmojiImageTarget, ImagePreviewTarget, image_preview_album_layout,
    visible_avatar_targets, visible_emoji_image_targets, visible_image_preview_targets,
};

use protocol::{
    AVATAR_PREVIEW_HEIGHT, AVATAR_PREVIEW_WIDTH, ImagePreviewRenderInfo, avatar_preview_url,
    clipped_preview_image, clipped_preview_protocol, emoji_protocol, query_image_picker,
};
pub(super) use protocol::{PROFILE_POPUP_AVATAR_HEIGHT, PROFILE_POPUP_AVATAR_WIDTH};

#[cfg(test)]
use cache::{
    AvatarImageEntry, AvatarProtocolKey, EmojiImageEntry, MAX_AVATAR_IMAGE_CACHE_ENTRIES,
    MAX_EMOJI_IMAGE_CACHE_ENTRIES,
};
#[cfg(test)]
use preview::{ImagePreviewEntry, MAX_IMAGE_PREVIEW_CACHE_ENTRIES, decode_original_preview_image};

#[cfg(test)]
mod tests;
