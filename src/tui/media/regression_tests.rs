use std::io::Cursor;

use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use ratatui_image::picker::Picker;

use super::{
    AvatarImageCache, AvatarImageEntry, AvatarTarget, EmojiImageCache, EmojiImageEntry,
    EmojiImageTarget, MediaProtocolBuildTarget, MediaWorkError, build_media_protocol,
    decode_media_image_bytes,
};
use crate::{discord::AppCommand, tui::text::EmojiImageSize};

fn encoded_png() -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(4, 4, Rgba([0, 0, 0, 255])));
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, ImageFormat::Png)
        .expect("test PNG should encode");
    bytes.into_inner()
}

fn avatar_target(url: impl Into<String>, row: isize, visible_height: u16) -> AvatarTarget {
    AvatarTarget {
        row,
        visible_height,
        top_clip_rows: 0,
        url: url.into(),
    }
}

#[test]
fn avatar_request_limit_counts_distinct_urls_not_repeated_placements() {
    let repeated = avatar_target("https://cdn.example/avatar-a.png", 0, 2);
    let mut targets = (0..super::MAX_AVATAR_IMAGE_CACHE_ENTRIES)
        .map(|row| AvatarTarget {
            row: row as isize,
            ..repeated.clone()
        })
        .collect::<Vec<_>>();
    targets.push(avatar_target(
        "https://cdn.example/avatar-b.png",
        super::MAX_AVATAR_IMAGE_CACHE_ENTRIES as isize,
        2,
    ));
    let mut cache = AvatarImageCache::new(Some(Picker::halfblocks()));

    let requests = cache.next_requests(&targets);

    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| matches!(request, AppCommand::LoadAttachmentPreview { .. }))
    );
    assert_eq!(cache.cache.entries.len(), 2);

    let repeated_url = super::avatar_preview_url(
        &repeated.url,
        super::AVATAR_PREVIEW_WIDTH,
        super::AVATAR_PREVIEW_HEIGHT,
    );
    let distinct_url = super::avatar_preview_url(
        "https://cdn.example/avatar-b.png",
        super::AVATAR_PREVIEW_WIDTH,
        super::AVATAR_PREVIEW_HEIGHT,
    );
    cache.cache.entries.insert(
        repeated_url.clone(),
        AvatarImageEntry::Loading { last_used: 100 },
    );
    cache.cache.entries.insert(
        distinct_url.clone(),
        AvatarImageEntry::Loading { last_used: 0 },
    );
    for index in 0..super::MAX_AVATAR_IMAGE_CACHE_ENTRIES - 1 {
        cache.cache.entries.insert(
            format!("extra-avatar-{index}"),
            AvatarImageEntry::Loading {
                last_used: index as u64 + 1,
            },
        );
    }

    cache.prune_to_limit(&targets);

    assert_eq!(
        cache.cache.entries.len(),
        super::MAX_AVATAR_IMAGE_CACHE_ENTRIES
    );
    assert!(cache.cache.entries.contains_key(&repeated_url));
    assert!(
        cache.cache.entries.contains_key(&distinct_url),
        "the last admitted distinct avatar remains protected under pressure"
    );
}

#[test]
fn avatar_retry_deadline_includes_distinct_targets_and_profile_popup() {
    let repeated = avatar_target("https://cdn.example/avatar-a.png", 0, 2);
    let mut targets = (0..super::MAX_AVATAR_IMAGE_CACHE_ENTRIES)
        .map(|row| AvatarTarget {
            row: row as isize,
            ..repeated.clone()
        })
        .collect::<Vec<_>>();
    let distinct = avatar_target("https://cdn.example/avatar-b.png", 32, 2);
    targets.push(distinct.clone());
    let distinct_url = super::avatar_preview_url(
        &distinct.url,
        super::AVATAR_PREVIEW_WIDTH,
        super::AVATAR_PREVIEW_HEIGHT,
    );
    let popup_source_url = "https://cdn.discordapp.com/avatars/42/popup.png?size=1024";
    let popup_cache_url = super::avatar_preview_url(
        popup_source_url,
        super::PROFILE_POPUP_AVATAR_WIDTH,
        super::PROFILE_POPUP_AVATAR_HEIGHT,
    );
    assert_ne!(popup_cache_url, popup_source_url);
    let mut cache = AvatarImageCache::new(Some(Picker::halfblocks()));
    for url in [&distinct_url, &popup_cache_url] {
        cache
            .cache
            .entries
            .insert(url.clone(), AvatarImageEntry::Loading { last_used: 0 });
        cache
            .cache
            .store_failed_if_present(url.clone(), |last_used| AvatarImageEntry::Failed {
                last_used,
            });
    }

    assert!(cache.next_retry_deadline(&targets, None).is_some());
    assert!(
        cache
            .next_retry_deadline(&[], Some(popup_source_url))
            .is_some()
    );
}

#[test]
fn avatar_refresh_retries_failed_layout_without_using_another_layout() {
    for (name, has_full_layout_protocol) in [
        ("without an existing layout", false),
        ("with an existing full layout", true),
    ] {
        let url = format!("https://cdn.example/{name}.png");
        let full = avatar_target(&url, 0, 2);
        let clipped = AvatarTarget {
            visible_height: 1,
            top_clip_rows: 1,
            ..full.clone()
        };
        let cache_url = super::avatar_preview_url(
            &url,
            super::AVATAR_PREVIEW_WIDTH,
            super::AVATAR_PREVIEW_HEIGHT,
        );
        let mut cache = AvatarImageCache::new(Some(Picker::halfblocks()));
        cache.cache.entries.insert(
            cache_url.clone(),
            AvatarImageEntry::Decoding {
                generation: 1,
                last_used: 1,
            },
        );
        cache.store_decoded(
            cache_url.clone(),
            1,
            decode_media_image_bytes(&encoded_png()).map_err(MediaWorkError::Failed),
        );

        if has_full_layout_protocol {
            cache.prepare(std::slice::from_ref(&full), None, None, false);
            for job in cache.take_protocol_jobs() {
                cache.store_protocol(build_media_protocol(job));
            }
            assert_eq!(
                cache
                    .render_state_with_popup(std::slice::from_ref(&full), None, None, false)
                    .0
                    .len(),
                1,
                "{name}"
            );
        }

        for attempt in 0..2 {
            cache.prepare(std::slice::from_ref(&clipped), None, None, false);
            let mut failed = build_media_protocol(
                cache
                    .take_protocol_jobs()
                    .into_iter()
                    .next()
                    .expect("clipped avatar protocol should be attempted"),
            );
            failed.result = Err(MediaWorkError::Failed(
                "temporary clipped avatar protocol failure".to_owned(),
            ));
            cache.store_protocol(failed);
            assert!(
                matches!(
                    cache.cache.entries.get(&cache_url),
                    Some(AvatarImageEntry::Ready { .. })
                ),
                "{name}, attempt {attempt}: render failure must retain the decoded source"
            );
        }
        assert!(
            cache
                .render_state_with_popup(std::slice::from_ref(&clipped), None, None, false)
                .0
                .is_empty(),
            "{name}: another layout must not be reused"
        );

        cache.forget_failures();
        assert!(
            cache
                .next_requests(std::slice::from_ref(&clipped))
                .is_empty(),
            "{name}: refresh should rebuild from decoded data without downloading"
        );
        cache.prepare(std::slice::from_ref(&clipped), None, None, false);
        let jobs = cache.take_protocol_jobs();
        assert_eq!(jobs.len(), 1, "{name}");
        for job in jobs {
            cache.store_protocol(build_media_protocol(job));
        }
        assert_eq!(
            cache
                .render_state_with_popup(std::slice::from_ref(&clipped), None, None, false)
                .0
                .len(),
            1,
            "{name}"
        );
    }
}

#[test]
fn emoji_refresh_retries_each_failed_size_without_downloading() {
    for failed_size in [EmojiImageSize::Compact, EmojiImageSize::Standalone] {
        let url = format!("https://cdn.example/emoji-{failed_size:?}.png");
        let target = EmojiImageTarget {
            url: url.clone(),
            image_size: failed_size,
        };
        let mut cache = EmojiImageCache::new(Some(Picker::halfblocks()));
        cache.cache.entries.insert(
            url.clone(),
            EmojiImageEntry::Decoding {
                generation: 1,
                last_used: 1,
            },
        );
        cache.store_decoded(
            url.clone(),
            1,
            decode_media_image_bytes(&encoded_png()).map_err(MediaWorkError::Failed),
        );

        for attempt in 0..2 {
            cache.prepare(std::slice::from_ref(&target));
            let jobs = cache.take_protocol_jobs();
            assert!(!jobs.is_empty(), "{failed_size:?}, attempt {attempt}");
            for job in jobs {
                let mut result = build_media_protocol(job);
                if matches!(
                    result.target,
                    MediaProtocolBuildTarget::Emoji { image_size, .. }
                        if image_size == failed_size
                ) {
                    result.result = Err(MediaWorkError::Failed(format!(
                        "temporary {failed_size:?} emoji protocol failure"
                    )));
                }
                cache.store_protocol(result);
            }
            assert!(
                matches!(
                    cache.cache.entries.get(&url),
                    Some(EmojiImageEntry::Ready { .. })
                ),
                "{failed_size:?}, attempt {attempt}: render failure must retain decoded data"
            );
        }

        cache.forget_failures();
        assert!(
            cache
                .next_requests(std::slice::from_ref(&target))
                .is_empty(),
            "{failed_size:?}: refresh should not redownload decoded data"
        );
        cache.prepare(std::slice::from_ref(&target));
        let jobs = cache.take_protocol_jobs();
        assert_eq!(jobs.len(), 1, "{failed_size:?}");
        for job in jobs {
            cache.store_protocol(build_media_protocol(job));
        }
        assert_eq!(
            cache.render_state(std::slice::from_ref(&target)).len(),
            1,
            "{failed_size:?}"
        );
    }
}

#[test]
fn emoji_retry_deadline_is_disabled_without_a_picker() {
    let target = EmojiImageTarget {
        url: "https://cdn.example/emoji.png".to_owned(),
        image_size: EmojiImageSize::Compact,
    };
    let mut cache = EmojiImageCache::new(None);
    cache.cache.entries.insert(
        target.url.clone(),
        EmojiImageEntry::Loading { last_used: 0 },
    );
    cache
        .cache
        .store_failed_if_present(target.url.clone(), |last_used| EmojiImageEntry::Failed {
            last_used,
        });

    assert_eq!(
        cache.next_retry_deadline(std::slice::from_ref(&target)),
        None
    );
}
