use std::{
    io::Cursor,
    time::{Duration, Instant},
};

use crate::discord::ids::{Id, marker::MessageMarker};
use crate::discord::test_builders::{
    GuildCreateFixture, MessageCreateFixture, empty_latest_message_history_loaded_event,
    guild_create_event, guild_message_create_fixture, message_create_event,
};
use image::{
    Delay, DynamicImage, Frame as ImageFrame, ImageBuffer, ImageFormat, Rgba,
    codecs::gif::{GifEncoder, Repeat},
};

use crate::{
    config::{DisplayOptions, ImagePreviewQualityPreset},
    discord::{
        ActivityEmoji, ActivityInfo, ActivityKind, AppCommand, AppEvent, AttachmentInfo,
        ChannelInfo, ChannelRecipientInfo, ComponentMediaInfo, ComponentMediaItemInfo,
        CustomEmojiInfo, EmbedInfo, ForumPostDataInfo, MESSAGE_FLAG_IS_COMPONENTS_V2,
        MessageComponentInfo, MessageInfo, MessageSnapshotInfo, MessageState, PresenceEventFields,
        PresenceStatus, ProfileAvatarUpload, ReactionEmoji, ReactionInfo, StickerFormat,
        StickerInfo,
    },
    tui::{
        message::time::test_message_id_for_unix_millis,
        state::{DashboardState, FocusPane},
        text::EmojiImageSize,
        ui::ImagePreviewLayout,
    },
};

use super::*;
use super::{decode::MAX_LOTTIE_JSON_BYTES, work::MediaWorkError};

fn layout(list_height: usize) -> ImagePreviewLayout {
    ImagePreviewLayout {
        list_height,
        list_width: 200,
        content_width: 200,
        preview_width: 16,
        max_preview_height: 3,
        viewer_preview_width: 76,
        viewer_max_preview_height: 13,
        font_size: None,
    }
}

fn push_media_message(state: &mut DashboardState, event: MessageCreateFixture) {
    state.push_event(message_create_event(event));
}

fn encoded_png(width: u32, height: u32) -> Vec<u8> {
    let image =
        DynamicImage::ImageRgba8(ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0])));
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, ImageFormat::Png)
        .expect("test image should encode");
    bytes.into_inner()
}

fn encoded_animated_gif() -> Vec<u8> {
    encoded_two_frame_gif(10)
}

fn encoded_two_frame_gif(delay_ms: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut bytes);
        encoder
            .set_repeat(Repeat::Infinite)
            .expect("test GIF repeat should encode");
        for color in [Rgba([255, 0, 0, 255]), Rgba([0, 0, 255, 255])] {
            encoder
                .encode_frame(ImageFrame::from_parts(
                    ImageBuffer::from_pixel(2, 2, color),
                    0,
                    0,
                    Delay::from_numer_denom_ms(delay_ms, 1),
                ))
                .expect("test GIF frame should encode");
        }
    }
    bytes
}

fn encoded_long_animated_gif() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut bytes);
        encoder
            .set_repeat(Repeat::Infinite)
            .expect("test GIF repeat should encode");
        for frame_index in 0..300u16 {
            let delay_ms = if frame_index < 240 { 20 } else { 300 };
            encoder
                .encode_frame(ImageFrame::from_parts(
                    ImageBuffer::from_pixel(
                        1,
                        1,
                        Rgba([frame_index as u8, (frame_index >> 8) as u8, 0, 255]),
                    ),
                    0,
                    0,
                    Delay::from_numer_denom_ms(delay_ms, 1),
                ))
                .expect("test GIF frame should encode");
        }
    }
    bytes
}

#[test]
fn image_preview_targets_stop_at_rendered_row_budget() {
    let mut state = state_with_image_messages(6, &[1, 3, 6]);
    state.set_message_view_height(6);

    let targets = visible_image_preview_targets(&state, layout(6));

    assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
}

#[test]
fn image_preview_targets_keep_background_previews_while_modal_is_open() {
    let mut state = state_with_image_messages(1, &[1]);
    state.set_message_view_height(6);
    state.open_channel_switcher();

    let targets = visible_image_preview_targets(&state, layout(6));

    assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
}

#[test]
fn image_preview_targets_include_multiple_attachments_from_one_message() {
    let mut state = state_with_image_messages(0, &[]);
    push_media_message(
        &mut state,
        MessageCreateFixture {
            message_id: Id::new(1),
            content: Some("album".to_owned()),
            attachments: vec![image_attachment(1), image_attachment(2)],
            ..guild_message_create_fixture()
        },
    );

    let targets = visible_image_preview_targets(&state, layout(12));

    assert_eq!(target_message_ids(&targets), vec![Id::new(1), Id::new(1)]);
    assert_eq!(
        targets
            .iter()
            .map(|target| target.url.as_str())
            .collect::<Vec<_>>(),
        vec![
            "https://cdn.discordapp.com/image-1.png",
            "https://cdn.discordapp.com/image-2.png",
        ]
    );
    assert_eq!(
        targets
            .iter()
            .map(|target| (
                target.preview_x_offset_columns,
                target.preview_y_offset_rows,
                target.preview_width,
                target.preview_height,
            ))
            .collect::<Vec<_>>(),
        vec![(0, 0, 8, 3), (8, 0, 8, 3)]
    );
}

#[test]
fn image_preview_quality_rewrites_attachment_preview_urls() {
    let cases = [
        (
            ImagePreviewQualityPreset::Efficient,
            None,
            None,
            concat!(
                "https://media.discordapp.net/attachments/691/150/photo.png",
                "?ex=abc&is=def&hm=123&format=png&quality=lossless&width=4000&height=3000"
            ),
            concat!(
                "https://media.discordapp.net/attachments/691/150/photo.png",
                "?ex=abc&is=def&hm=123&format=webp&quality=low&width=192&height=144"
            ),
        ),
        (
            ImagePreviewQualityPreset::Efficient,
            Some(1000),
            Some(2000),
            concat!(
                "https://media.discordapp.net/attachments/691/150/photo.png",
                "?ex=abc&is=def&hm=123&format=png&width=1000&height=2000"
            ),
            concat!(
                "https://media.discordapp.net/attachments/691/150/photo.png",
                "?ex=abc&is=def&hm=123&format=webp&quality=low&width=300&height=600"
            ),
        ),
        (
            ImagePreviewQualityPreset::High,
            None,
            None,
            concat!(
                "https://media.discordapp.net/attachments/691/150/photo.png",
                "?ex=abc&is=def&hm=123&format=png&width=4000&height=3000"
            ),
            concat!(
                "https://media.discordapp.net/attachments/691/150/photo.png",
                "?ex=abc&is=def&hm=123&format=webp&quality=lossless&width=640&height=480"
            ),
        ),
        (
            ImagePreviewQualityPreset::Original,
            None,
            None,
            concat!(
                "https://media.discordapp.net/attachments/691/150/photo.png",
                "?ex=abc&is=def&hm=123&format=png&width=4000&height=3000"
            ),
            "https://cdn.discordapp.com/image-1.png",
        ),
    ];

    for (quality, width, height, proxy_url, expected_url) in cases {
        let mut state = state_with_image_messages_and_display_options(
            0,
            &[],
            DisplayOptions {
                image_preview_quality: quality,
                ..DisplayOptions::default()
            },
        );
        let mut attachment = image_attachment(1);
        if width.is_some() || height.is_some() {
            attachment.width = width;
            attachment.height = height;
        }
        attachment.proxy_url = proxy_url.to_owned();
        push_attachment_message(&mut state, attachment);

        let target = visible_image_preview_targets(&state, layout(12))
            .into_iter()
            .next()
            .expect("image attachment should produce preview target");

        assert_eq!(target.url, expected_url);
    }
}

#[test]
fn animated_attachment_previews_keep_animation_through_the_media_proxy() {
    for (name, filename, flags) in [
        ("animated WebP flag", "animation.webp", 1 << 5),
        ("GIF filename fallback", "animation.gif", 0),
    ] {
        let mut state = state_with_image_messages(0, &[]);
        let mut attachment = image_attachment(1);
        attachment.filename = filename.to_owned();
        attachment.flags = flags;
        attachment.proxy_url = concat!(
            "https://media.discordapp.net/attachments/691/150/animation.webp",
            "?ex=abc&is=def&hm=123&format=png&animated=false&width=4000&height=3000"
        )
        .to_owned();
        push_attachment_message(&mut state, attachment);

        let target = visible_image_preview_targets(&state, layout(12))
            .into_iter()
            .next()
            .expect("animated attachment should produce a preview target");

        assert_eq!(
            target.url,
            concat!(
                "https://media.discordapp.net/attachments/691/150/animation.webp",
                "?ex=abc&is=def&hm=123&format=webp&animated=true&width=320&height=240"
            ),
            "{name}"
        );
    }
}

#[test]
fn original_image_preview_quality_applies_to_attachment_viewer_preview() {
    let mut state = state_with_image_messages_and_display_options(
        0,
        &[],
        DisplayOptions {
            image_preview_quality: ImagePreviewQualityPreset::Original,
            ..DisplayOptions::default()
        },
    );
    let mut attachment = image_attachment(1);
    attachment.proxy_url = concat!(
        "https://media.discordapp.net/attachments/691/150/photo.png",
        "?ex=abc&is=def&hm=123&format=png&width=4000&height=3000"
    )
    .to_owned();
    push_media_message(
        &mut state,
        MessageCreateFixture {
            message_id: Id::new(1),
            content: Some("photo".to_owned()),
            attachments: vec![attachment],
            ..guild_message_create_fixture()
        },
    );
    state.focus_pane(FocusPane::Messages);
    assert!(state.open_attachment_viewer_for_selected_message());

    let target = visible_image_preview_targets(&state, layout(12))
        .into_iter()
        .next()
        .expect("attachment viewer should produce preview target");

    assert!(target.viewer);
    assert_eq!(target.url, "https://cdn.discordapp.com/image-1.png");
}

#[test]
fn image_preview_quality_does_not_change_avatar_or_custom_emoji_requests() {
    let mut state = state_with_image_messages_and_display_options(
        0,
        &[],
        DisplayOptions {
            image_preview_quality: ImagePreviewQualityPreset::Original,
            ..DisplayOptions::default()
        },
    );
    push_media_message(
        &mut state,
        MessageCreateFixture {
            message_id: Id::new(1),
            author_avatar_url: Some("https://cdn.discordapp.com/avatars/1/hash.png".to_owned()),
            content: Some("hello <:party:50>".to_owned()),
            ..guild_message_create_fixture()
        },
    );

    assert_eq!(
        state.image_preview_quality(),
        ImagePreviewQualityPreset::Original
    );
    assert_eq!(
        visible_avatar_targets(&state, layout(2))[0].url,
        "https://cdn.discordapp.com/avatars/1/hash.png"
    );
    assert_eq!(
        avatar_preview_url("https://cdn.discordapp.com/avatars/1/hash.png", 2, 2),
        "https://cdn.discordapp.com/avatars/1/hash.png?size=64"
    );
    assert_eq!(
        visible_emoji_image_targets(&state),
        vec![EmojiImageTarget {
            url: "https://cdn.discordapp.com/emojis/50.png".to_owned(),
            image_size: EmojiImageSize::Compact,
        }]
    );
}

#[test]
fn image_preview_targets_choose_embed_media_url() {
    for (name, embed, content, expected_url) in [
        (
            "youtube thumbnail is downgraded to a preview size",
            youtube_embed(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg",
        ),
        (
            "youtube thumbnail that is already small is kept",
            EmbedInfo {
                thumbnail_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/default.jpg".to_owned()),
                thumbnail_width: Some(120),
                thumbnail_height: Some(90),
                ..youtube_embed()
            },
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/default.jpg",
        ),
        (
            "media proxy is resized",
            EmbedInfo {
                thumbnail_url: Some("https://example.com/photo.png".to_owned()),
                thumbnail_proxy_url: Some(
                    concat!(
                        "https://media.discordapp.net/external/cache-key/https/example.com/photo.png",
                        "?ex=abc&is=def&hm=123&format=png&width=4000&height=3000"
                    )
                    .to_owned(),
                ),
                ..youtube_embed()
            },
            "https://example.com/post",
            concat!(
                "https://media.discordapp.net/external/cache-key/https/example.com/photo.png",
                "?ex=abc&is=def&hm=123&format=webp&width=240&height=180"
            ),
        ),
        (
            "images-ext proxy is resized",
            EmbedInfo {
                thumbnail_url: Some("https://example.com/photo.png".to_owned()),
                thumbnail_proxy_url: Some(
                    concat!(
                        "https://images-ext-1.discordapp.net/external/cache-key/https/example.com/photo.png",
                        "?width=4000&height=3000"
                    )
                    .to_owned(),
                ),
                ..youtube_embed()
            },
            "https://example.com/post",
            concat!(
                "https://images-ext-1.discordapp.net/external/cache-key/https/example.com/photo.png",
                "?format=webp&width=240&height=180"
            ),
        ),
        (
            "proxy outside the resizable routes falls back to the source url",
            EmbedInfo {
                thumbnail_url: Some("https://example.com/photo.png".to_owned()),
                thumbnail_proxy_url: Some(
                    "https://media.discordapp.net/avatars/1/hash.png".to_owned(),
                ),
                ..youtube_embed()
            },
            "https://example.com/post",
            "https://example.com/photo.png",
        ),
    ] {
        let mut state = state_with_image_messages(1, &[]);
        push_media_message(
            &mut state,
            MessageCreateFixture {
                message_id: Id::new(2),
                content: Some(content.to_owned()),
                embeds: vec![embed],
                ..guild_message_create_fixture()
            },
        );

        let targets = visible_image_preview_targets(&state, layout(8));

        assert_eq!(target_message_ids(&targets), vec![Id::new(2)], "{name}");
        assert_eq!(targets[0].url, expected_url, "{name}");
        assert_eq!(targets[0].filename, "embed-thumbnail", "{name}");
        assert!(targets[0].show_play_marker, "{name}");
    }

    let mut state = state_with_image_messages(1, &[]);
    push_media_message(
        &mut state,
        MessageCreateFixture {
            message_id: Id::new(2),
            content: Some("https://giphy.com/gifs/hvY8Ahy9r340SU8xLY".to_owned()),
            embeds: vec![EmbedInfo {
                url: Some("https://giphy.com/gifs/hvY8Ahy9r340SU8xLY".to_owned()),
                thumbnail_url: Some(
                    "https://media2.giphy.com/media/hvY8Ahy9r340SU8xLY/giphy_s.gif".to_owned(),
                ),
                thumbnail_width: Some(500),
                thumbnail_height: Some(599),
                gifv_image_url: Some(
                    "https://media2.giphy.com/media/hvY8Ahy9r340SU8xLY/giphy.webp".to_owned(),
                ),
                video_url: Some(
                    "https://media2.giphy.com/media/hvY8Ahy9r340SU8xLY/giphy.mp4".to_owned(),
                ),
                ..EmbedInfo::test()
            }],
            ..guild_message_create_fixture()
        },
    );

    let target = visible_image_preview_targets(&state, layout(8))
        .into_iter()
        .next()
        .expect("gifv embed should produce an inline preview");

    assert_eq!(
        target.url,
        "https://media2.giphy.com/media/hvY8Ahy9r340SU8xLY/giphy.webp"
    );
    assert_eq!(target.filename, "embed-gifv");
}

#[test]
fn rich_embed_exposes_image_and_thumbnail_as_distinct_previews() {
    let message = MessageState {
        embeds: vec![EmbedInfo {
            kind: Some("rich".to_owned()),
            image_url: Some("https://example.com/main.png".to_owned()),
            image_width: Some(1200),
            image_height: Some(800),
            thumbnail_url: Some("https://example.com/thumb.png".to_owned()),
            thumbnail_width: Some(200),
            thumbnail_height: Some(200),
            ..EmbedInfo::test()
        }],
        ..MessageState::default()
    };

    let previews = message.inline_previews();

    assert_eq!(previews.len(), 2);
    assert_eq!(previews[0].filename, "embed-image");
    assert_eq!(previews[0].url, "https://example.com/main.png");
    assert_eq!(previews[1].filename, "embed-thumbnail");
    assert_eq!(previews[1].url, "https://example.com/thumb.png");
}

#[test]
fn video_only_embed_uses_documented_video_proxy_as_preview_fallback() {
    let message = MessageState {
        embeds: vec![EmbedInfo {
            kind: Some("video".to_owned()),
            video_url: Some("https://cdn.example.com/video.mp4".to_owned()),
            video_proxy_url: Some("https://media.discordapp.net/external/video.mp4".to_owned()),
            video_width: Some(1920),
            video_height: Some(1080),
            ..EmbedInfo::test()
        }],
        ..MessageState::default()
    };

    let previews = message.inline_previews();

    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].filename, "embed-video");
    assert_eq!(
        previews[0].url,
        "https://media.discordapp.net/external/video.mp4"
    );
    assert_eq!(previews[0].width, Some(1920));
    assert_eq!(previews[0].height, Some(1080));
    assert!(previews[0].proxy_preview_only);
    assert!(previews[0].show_play_marker);
}

#[test]
fn components_v2_media_accepts_received_animation_flag_variants() {
    for flags in [1, 1 << 5] {
        let message = MessageState {
            flags: MESSAGE_FLAG_IS_COMPONENTS_V2,
            components: vec![MessageComponentInfo::MediaGallery {
                items: vec![ComponentMediaItemInfo {
                    media: ComponentMediaInfo {
                        url: "https://example.com/animated.webp".to_owned(),
                        content_type: Some("image/webp".to_owned()),
                        flags,
                        ..ComponentMediaInfo::default()
                    },
                    description: None,
                    spoiler: false,
                }],
            }],
            ..MessageState::default()
        };

        let previews = message.inline_previews();

        assert_eq!(previews.len(), 1);
        assert!(previews[0].animated, "flags={flags}");
    }
}

#[test]
fn components_v2_hides_attachments_that_are_not_exposed_by_components() {
    let message = MessageState {
        flags: MESSAGE_FLAG_IS_COMPONENTS_V2,
        attachments: vec![image_attachment(1)],
        components: vec![MessageComponentInfo::TextDisplay {
            content: "Only this component is visible".to_owned(),
        }],
        ..MessageState::default()
    };

    assert!(message.inline_previews().is_empty());
    assert!(message.attachments_in_display_order().next().is_none());
}

#[test]
fn spoiler_and_sensitive_media_remain_visible_in_inline_previews() {
    let mut attachment = image_attachment(1);
    attachment.filename = "SPOILER_image-1.png".to_owned();
    attachment.flags = 1 << 3;
    let attachment_message = MessageState {
        attachments: vec![attachment],
        ..MessageState::default()
    };

    let embed_message = MessageState {
        embeds: vec![EmbedInfo {
            flags: 1 << 4,
            image_url: Some("https://example.com/sensitive.png".to_owned()),
            ..EmbedInfo::test()
        }],
        ..MessageState::default()
    };

    let component_message = MessageState {
        flags: MESSAGE_FLAG_IS_COMPONENTS_V2,
        components: vec![MessageComponentInfo::Container {
            components: vec![MessageComponentInfo::MediaGallery {
                items: vec![ComponentMediaItemInfo {
                    media: ComponentMediaInfo {
                        url: "https://example.com/sensitive.png".to_owned(),
                        content_type: Some("image/png".to_owned()),
                        flags: 1 << 6,
                        ..ComponentMediaInfo::default()
                    },
                    description: None,
                    spoiler: true,
                }],
            }],
            accent_color: None,
            spoiler: true,
        }],
        ..MessageState::default()
    };

    assert!(
        attachment_message.attachments[0]
            .inline_preview_url()
            .is_some()
    );
    for (label, message) in [
        ("attachment", attachment_message),
        ("embed", embed_message),
        ("component", component_message),
    ] {
        assert_eq!(message.inline_previews().len(), 1, "{label}");
    }
}

#[test]
fn image_preview_targets_preserve_non_giphy_gifv_thumbnail_animation() {
    let mut state = state_with_image_messages_and_display_options(
        1,
        &[],
        DisplayOptions {
            image_preview_quality: ImagePreviewQualityPreset::High,
            ..DisplayOptions::default()
        },
    );
    push_media_message(
        &mut state,
        MessageCreateFixture {
            message_id: Id::new(2),
            content: Some("https://klipy.com/gifs/sleep-l0T".to_owned()),
            embeds: vec![EmbedInfo {
                url: Some("https://klipy.com/gifs/sleep-l0T".to_owned()),
                thumbnail_url: Some("https://static.klipy.com/media/thumbnail.webp".to_owned()),
                thumbnail_proxy_url: Some(
                    "https://images-ext-1.discordapp.net/external/cache/https/static.klipy.com/media/thumbnail.webp"
                        .to_owned(),
                ),
                thumbnail_width: Some(498),
                thumbnail_height: Some(279),
                gifv_image_url: Some(
                    "https://static.klipy.com/media/thumbnail.webp".to_owned(),
                ),
                gifv_image_proxy_url: Some(
                    "https://images-ext-1.discordapp.net/external/cache/https/static.klipy.com/media/thumbnail.webp"
                        .to_owned(),
                ),
                video_url: Some("https://static.klipy.com/media/video.mp4".to_owned()),
                ..EmbedInfo::test()
            }],
            ..guild_message_create_fixture()
        },
    );

    let target = visible_image_preview_targets(&state, layout(8))
        .into_iter()
        .next()
        .expect("gifv thumbnail should produce an inline preview");

    assert_eq!(
        target.url,
        concat!(
            "https://images-ext-1.discordapp.net/external/cache/https/static.klipy.com/media/thumbnail.webp",
            "?format=webp&animated=true&quality=lossless&width=498&height=279"
        )
    );
    assert_eq!(target.filename, "embed-gifv");
    assert!(!target.show_play_marker);
}

#[test]
fn image_preview_targets_do_not_mark_plain_image_embed_thumbnail_as_playable() {
    let mut state = state_with_image_messages(1, &[]);
    push_media_message(
        &mut state,
        MessageCreateFixture {
            message_id: Id::new(2),
            content: Some("https://example.com/post".to_owned()),
            embeds: vec![EmbedInfo {
                thumbnail_url: Some("https://example.com/photo.png".to_owned()),
                thumbnail_width: Some(640),
                thumbnail_height: Some(480),
                ..EmbedInfo::test()
            }],
            ..guild_message_create_fixture()
        },
    );

    let targets = visible_image_preview_targets(&state, layout(8));

    assert_eq!(target_message_ids(&targets), vec![Id::new(2)]);
    assert_eq!(targets[0].filename, "embed-thumbnail");
    assert!(!targets[0].show_play_marker);
}

#[test]
fn image_preview_targets_layout_album_grids() {
    let portrait_album = {
        let mut first = image_attachment(1);
        first.width = Some(1080);
        first.height = Some(1920);
        let mut second = image_attachment(2);
        second.width = Some(1080);
        second.height = Some(1920);
        vec![first, second]
    };
    let cases = [
        (
            (1..=3).map(image_attachment).collect::<Vec<_>>(),
            vec![(0, 0, 0, 8, 3), (1, 8, 0, 8, 2), (2, 8, 2, 4, 1)],
        ),
        (
            (1..=4).map(image_attachment).collect::<Vec<_>>(),
            vec![
                (0, 0, 0, 8, 2),
                (1, 8, 0, 8, 2),
                (2, 0, 2, 4, 1),
                (3, 4, 2, 4, 1),
            ],
        ),
        (
            (1..=5).map(image_attachment).collect::<Vec<_>>(),
            vec![
                (0, 0, 0, 8, 2),
                (1, 8, 0, 8, 2),
                (2, 0, 2, 4, 1),
                (3, 4, 2, 4, 1),
            ],
        ),
        (portrait_album, vec![(0, 0, 0, 5, 3), (1, 5, 0, 5, 3)]),
    ];

    for (attachments, expected_geometry) in cases {
        let mut state = state_with_image_messages(0, &[]);
        push_media_message(
            &mut state,
            MessageCreateFixture {
                message_id: Id::new(1),
                content: Some("album".to_owned()),
                attachments,
                ..guild_message_create_fixture()
            },
        );

        let targets = visible_image_preview_targets(&state, layout(12));

        assert_eq!(
            targets
                .iter()
                .map(|target| (
                    target.preview_index,
                    target.preview_x_offset_columns,
                    target.preview_y_offset_rows,
                    target.preview_width,
                    target.preview_height,
                ))
                .collect::<Vec<_>>(),
            expected_geometry
        );
    }
}

#[test]
fn attachment_viewer_target_fits_source_image_inside_viewer_layout() {
    let mut state = state_with_image_messages(1, &[1]);
    state.focus_pane(FocusPane::Messages);
    state.direct_open_selected_message_attachment_viewer();

    let target = visible_image_preview_targets(&state, layout(12))
        .into_iter()
        .next()
        .expect("viewer should create one image target");

    assert!(target.viewer);
    assert_eq!(target.preview_width, 52);
    assert_eq!(target.preview_height, 13);
    assert_eq!(target.visible_preview_height, 13);
}

#[test]
fn attachment_viewer_target_shows_video_thumbnail_preview() {
    let mut state = state_with_image_messages(1, &[]);
    push_media_message(
        &mut state,
        MessageCreateFixture {
            message_id: Id::new(2),
            content: Some("clip".to_owned()),
            attachments: vec![video_attachment(2)],
            ..guild_message_create_fixture()
        },
    );
    state.focus_pane(FocusPane::Messages);
    state.move_down();
    state.direct_open_selected_message_attachment_viewer();

    let target = visible_image_preview_targets(&state, layout(12))
        .into_iter()
        .next()
        .expect("viewer should create one video thumbnail target");

    assert!(target.viewer);
    assert!(target.show_play_marker);
    assert_eq!(
        target.url,
        "https://media.discordapp.net/attachments/691/150/clip-2.mp4?format=webp&width=563&height=1000"
    );
}

#[test]
fn image_preview_targets_account_for_first_message_line_offset() {
    let mut state = state_with_image_messages(1, &[1]);
    state.focus_pane(FocusPane::Messages);
    state.clamp_message_viewport_for_image_previews(200, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(200, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    let targets = visible_image_preview_targets(&state, layout(2));

    assert_eq!(
        target_message_ids(&targets),
        Vec::<Id<MessageMarker>>::new()
    );
}

#[test]
fn avatar_targets_include_visible_author_avatar() {
    let state = state_with_avatar_messages(1);

    let targets = visible_avatar_targets(&state, layout(2));

    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].row, 1);
    assert_eq!(targets[0].visible_height, 1);
    assert_eq!(targets[0].top_clip_rows, 0);
    assert_eq!(targets[0].url, "https://cdn.discordapp.com/avatar-1.png");
}

#[test]
fn avatar_preview_url_sizes_user_avatars_but_not_default_ones() {
    for (url, width, height, expected) in [
        (
            "https://cdn.discordapp.com/avatars/1/hash.png",
            2,
            2,
            "https://cdn.discordapp.com/avatars/1/hash.png?size=64",
        ),
        (
            "https://cdn.discordapp.com/avatars/1/hash.png?size=1024&foo=bar",
            8,
            4,
            "https://cdn.discordapp.com/avatars/1/hash.png?foo=bar&size=128",
        ),
        (
            "https://cdn.discordapp.com/guilds/1/users/2/avatars/hash.webp?animated=true",
            2,
            2,
            "https://cdn.discordapp.com/guilds/1/users/2/avatars/hash.webp?animated=true&size=64",
        ),
        (
            "https://cdn.discordapp.com/embed/avatars/0.png",
            8,
            4,
            "https://cdn.discordapp.com/embed/avatars/0.png",
        ),
    ] {
        assert_eq!(avatar_preview_url(url, width, height), expected, "{url}");
    }
}

#[test]
fn avatar_targets_clip_first_message_avatar_after_line_scroll() {
    let mut state = state_with_avatar_messages(1);
    state.focus_pane(FocusPane::Messages);
    state.clamp_message_viewport_for_image_previews(200, 16, 3);
    state.scroll_message_viewport_down();
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    let targets = visible_avatar_targets(&state, layout(1));

    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].row, 0);
    assert_eq!(targets[0].visible_height, 1);
    assert_eq!(targets[0].top_clip_rows, 0);
}

#[test]
fn avatar_image_cache_evicts_least_recently_used_when_over_capacity() {
    let mut cache = AvatarImageCache::new(None);
    for id in 0..MAX_AVATAR_IMAGE_CACHE_ENTRIES {
        let url = avatar_preview_url(
            &format!("https://cdn.discordapp.com/avatars/{id}.png"),
            AVATAR_PREVIEW_WIDTH,
            AVATAR_PREVIEW_HEIGHT,
        );
        cache.cache.entries.insert(
            url,
            AvatarImageEntry::Failed {
                last_used: id as u64,
            },
        );
    }
    cache.cache.tick = MAX_AVATAR_IMAGE_CACHE_ENTRIES as u64;
    cache.cache.entries.insert(
        "https://cdn.discordapp.com/avatars/oldest.png".to_owned(),
        AvatarImageEntry::Failed { last_used: 0 },
    );

    let visible_url = "https://cdn.discordapp.com/avatars/0.png".to_owned();
    let visible_cache_url =
        avatar_preview_url(&visible_url, AVATAR_PREVIEW_WIDTH, AVATAR_PREVIEW_HEIGHT);
    let targets = vec![AvatarTarget {
        row: 0,
        visible_height: 1,
        top_clip_rows: 0,
        url: visible_url.clone(),
    }];
    cache.prune_to_limit(&targets);

    assert_eq!(cache.cache.entries.len(), MAX_AVATAR_IMAGE_CACHE_ENTRIES);
    assert!(cache.cache.entries.contains_key(&visible_cache_url));
    assert!(
        !cache
            .cache
            .entries
            .contains_key("https://cdn.discordapp.com/avatars/oldest.png")
    );
}

#[test]
fn avatar_protocol_key_tracks_render_clipping() {
    let full = AvatarTarget {
        row: 0,
        visible_height: AVATAR_PREVIEW_HEIGHT,
        top_clip_rows: 0,
        url: "https://cdn.discordapp.com/avatars/1.png".to_owned(),
    };
    let clipped = AvatarTarget {
        visible_height: 1,
        top_clip_rows: 1,
        ..full.clone()
    };

    assert_ne!(
        AvatarProtocolKey::message_avatar(&full, false),
        AvatarProtocolKey::message_avatar(&clipped, false)
    );
    assert_ne!(
        AvatarProtocolKey::message_avatar(&full, false),
        AvatarProtocolKey::profile_popup(PROFILE_POPUP_AVATAR_HEIGHT, 0, false)
    );
    assert_ne!(
        AvatarProtocolKey::message_avatar(&full, false),
        AvatarProtocolKey::message_avatar(&full, true)
    );
    assert_ne!(
        AvatarProtocolKey::profile_popup(PROFILE_POPUP_AVATAR_HEIGHT, 0, false),
        AvatarProtocolKey::profile_popup(PROFILE_POPUP_AVATAR_HEIGHT - 1, 1, false)
    );
}

#[test]
fn avatar_popup_request_prunes_cache_to_limit() {
    let mut cache = AvatarImageCache::new(None);
    for id in 0..MAX_AVATAR_IMAGE_CACHE_ENTRIES {
        cache.cache.entries.insert(
            format!("https://cdn.discordapp.com/avatars/{id}.png"),
            AvatarImageEntry::Failed {
                last_used: id as u64,
            },
        );
    }

    let request = cache.next_request_for_url("https://cdn.discordapp.com/avatars/new.png");

    assert_eq!(
        request,
        Some(AppCommand::LoadAttachmentPreview {
            url: "https://cdn.discordapp.com/avatars/new.png?size=128".to_owned(),
        })
    );
    assert_eq!(cache.cache.entries.len(), MAX_AVATAR_IMAGE_CACHE_ENTRIES);
    assert!(
        cache
            .cache
            .entries
            .contains_key("https://cdn.discordapp.com/avatars/new.png?size=128")
    );
}

#[test]
fn avatar_popup_upload_request_uses_local_preview_command() {
    let mut cache = AvatarImageCache::new(None);
    let upload = ProfileAvatarUpload::from_bytes("avatar.png".to_owned(), vec![1, 2, 3]);

    let request = cache.next_request_for_profile_upload("pending-avatar", || Some(upload.clone()));

    assert_eq!(
        request,
        Some(AppCommand::LoadProfileAvatarPreview {
            key: "pending-avatar".to_owned(),
            upload,
        })
    );
    assert!(cache.cache.entries.contains_key("pending-avatar"));
}

#[test]
fn avatar_cache_pruning_preserves_active_popup_avatar() {
    let popup_url = "https://cdn.discordapp.com/avatars/popup.png?size=128";
    let mut cache = AvatarImageCache::new(None);
    cache.active_popup_avatar_url = Some(popup_url.to_owned());
    for id in 0..MAX_AVATAR_IMAGE_CACHE_ENTRIES {
        let url = avatar_preview_url(
            &format!("https://cdn.discordapp.com/avatars/{id}.png"),
            AVATAR_PREVIEW_WIDTH,
            AVATAR_PREVIEW_HEIGHT,
        );
        cache.cache.entries.insert(
            url,
            AvatarImageEntry::Failed {
                last_used: id as u64,
            },
        );
    }
    cache.cache.entries.insert(
        popup_url.to_owned(),
        AvatarImageEntry::Failed { last_used: 0 },
    );

    let targets = (0..MAX_AVATAR_IMAGE_CACHE_ENTRIES)
        .map(|id| AvatarTarget {
            row: 0,
            visible_height: 1,
            top_clip_rows: 0,
            url: format!("https://cdn.discordapp.com/avatars/{id}.png"),
        })
        .collect::<Vec<_>>();

    cache.prune_to_limit(&targets);

    assert_eq!(
        cache.cache.entries.len(),
        MAX_AVATAR_IMAGE_CACHE_ENTRIES + 1
    );
    assert!(cache.cache.entries.contains_key(popup_url));
}

#[test]
fn image_preview_targets_include_top_clipped_preview_rows() {
    let mut state = state_with_image_messages(1, &[1]);
    state.focus_pane(FocusPane::Messages);
    state.clamp_message_viewport_for_image_previews(200, 16, 3);
    for _ in 0..4 {
        state.scroll_message_viewport_down();
        state.clamp_message_viewport_for_image_previews(200, 16, 3);
    }

    let targets = visible_image_preview_targets(&state, layout(2));

    assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    assert_eq!(targets[0].visible_preview_height, 2);
    assert_eq!(targets[0].top_clip_rows, 0);
}

#[test]
fn image_preview_targets_clip_album_bottom_row_after_line_scroll() {
    let mut state = state_with_image_messages(0, &[]);
    push_album_message(&mut state, 1, 4);
    state.focus_pane(FocusPane::Messages);
    state.clamp_message_viewport_for_image_previews(200, 16, 3);
    for _ in 0..16 {
        state.scroll_message_viewport_down();
        let targets = visible_image_preview_targets(&state, layout(2));
        if targets
            .first()
            .is_some_and(|target| target.preview_index == 2)
        {
            break;
        }
    }

    let targets = visible_image_preview_targets(&state, layout(2));

    assert_eq!(
        targets
            .iter()
            .map(|target| (
                target.preview_index,
                target.preview_y_offset_rows,
                target.visible_preview_height,
                target.top_clip_rows,
            ))
            .collect::<Vec<_>>(),
        vec![(2, 2, 1, 0), (3, 2, 1, 0)]
    );
}

#[test]
fn image_preview_targets_account_for_date_separator_rows() {
    let mut state = state_with_cross_day_image_message();
    state.set_message_view_height(4);

    let targets = visible_image_preview_targets(&state, layout(4));

    assert!(targets.is_empty());
}

#[test]
fn video_attachment_uses_proxy_webp_thumbnail_as_image_preview() {
    let mut state = state_with_image_messages(1, &[]);
    push_media_message(
        &mut state,
        MessageCreateFixture {
            message_id: Id::new(2),
            content: Some("clip".to_owned()),
            attachments: vec![video_attachment(2)],
            ..guild_message_create_fixture()
        },
    );

    let targets = visible_image_preview_targets(&state, layout(6));

    assert_eq!(target_message_ids(&targets), vec![Id::new(2)]);
    assert_eq!(
        targets[0].url,
        "https://media.discordapp.net/attachments/691/150/clip-2.mp4?format=webp&width=540&height=960"
    );
    assert_eq!(targets[0].filename, "clip-2.mp4");
    assert!(targets[0].show_play_marker);
    assert_eq!(targets[0].preview_width, 5);
    assert_eq!(targets[0].preview_height, 3);
}

#[test]
fn original_quality_video_attachment_still_uses_proxy_webp_thumbnail() {
    let mut state = state_with_image_messages_and_display_options(
        0,
        &[],
        DisplayOptions {
            image_preview_quality: ImagePreviewQualityPreset::Original,
            ..DisplayOptions::default()
        },
    );
    let mut attachment = video_attachment(2);
    attachment.proxy_url = concat!(
        "https://media.discordapp.net/attachments/691/150/clip.mp4",
        "?ex=abc&is=def&hm=123&format=png&width=4000&height=3000"
    )
    .to_owned();
    push_attachment_message(&mut state, attachment);

    let target = visible_image_preview_targets(&state, layout(12))
        .into_iter()
        .next()
        .expect("video attachment should produce preview target");

    assert_eq!(
        target.url,
        concat!(
            "https://media.discordapp.net/attachments/691/150/clip.mp4",
            "?ex=abc&is=def&hm=123&format=webp&width=563&height=1000"
        )
    );
}

#[test]
fn image_preview_targets_downscale_youtube_embed_image_url() {
    let mut embed = youtube_embed();
    embed.thumbnail_url = None;
    embed.thumbnail_width = None;
    embed.thumbnail_height = None;
    embed.image_url =
        Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg?token=abc".to_owned());
    embed.image_width = Some(1280);
    embed.image_height = Some(720);
    let mut state = state_with_image_messages(1, &[]);
    push_media_message(
        &mut state,
        MessageCreateFixture {
            message_id: Id::new(2),
            content: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
            embeds: vec![embed],
            ..guild_message_create_fixture()
        },
    );

    let targets = visible_image_preview_targets(&state, layout(8));

    assert_eq!(target_message_ids(&targets), vec![Id::new(2)]);
    assert_eq!(
        targets[0].url,
        "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg?token=abc"
    );
    assert_eq!(targets[0].filename, "embed-image");
    assert!(targets[0].show_play_marker);
}

#[test]
fn image_preview_targets_include_forwarded_image_attachments() {
    let mut state = state_with_image_messages(1, &[]);
    push_media_message(
        &mut state,
        MessageCreateFixture {
            message_id: Id::new(2),
            content: Some(String::new()),
            forwarded_snapshots: vec![forwarded_snapshot(2)],
            ..guild_message_create_fixture()
        },
    );

    let targets = visible_image_preview_targets(&state, layout(6));

    assert_eq!(target_message_ids(&targets), vec![Id::new(2)]);
    assert_eq!(targets[0].url, "https://cdn.discordapp.com/image-2.png");
}

#[test]
fn image_preview_targets_include_guild_stickers() {
    let mut state = state_with_image_messages(0, &[]);
    push_media_message(
        &mut state,
        MessageCreateFixture {
            message_id: Id::new(1),
            content: Some(String::new()),
            stickers: vec![StickerInfo::new(Id::new(11), "Laugh", StickerFormat::Png)],
            ..guild_message_create_fixture()
        },
    );

    let targets = visible_image_preview_targets(&state, layout(12));

    assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    assert_eq!(
        targets[0].url,
        "https://media.discordapp.net/stickers/11.png?size=160&passthrough=false"
    );
    assert_eq!(targets[0].filename, "Laugh");
}

#[test]
fn image_preview_targets_include_lottie_stickers() {
    let mut state = state_with_image_messages(0, &[]);
    push_media_message(
        &mut state,
        MessageCreateFixture {
            message_id: Id::new(1),
            content: Some(String::new()),
            stickers: vec![StickerInfo::new(
                Id::new(12),
                "Wumpus",
                StickerFormat::Lottie,
            )],
            ..guild_message_create_fixture()
        },
    );

    let targets = visible_image_preview_targets(&state, layout(12));

    assert_eq!(target_message_ids(&targets), vec![Id::new(1)]);
    assert_eq!(
        targets[0].url,
        "https://cdn.discordapp.com/stickers/12.json"
    );
    assert_eq!(targets[0].filename, "Wumpus");
}

#[test]
fn image_preview_targets_include_image_messages_in_scrolloff_context() {
    let mut state = state_with_image_messages(8, &[5, 6, 7]);
    state.focus_pane(FocusPane::Messages);
    state.set_message_view_height(14);
    while state.selected_message() > 3 {
        state.move_up();
    }
    state.clamp_message_viewport_for_image_previews(200, 16, 3);

    let targets = visible_image_preview_targets(&state, layout(14));

    assert_eq!(target_message_ids(&targets), vec![Id::new(5), Id::new(6)]);
}

#[test]
fn image_preview_request_is_created_for_draw_target() {
    let mut cache = ImagePreviewCache::new(None);
    let target = image_preview_target(1);

    assert!(cache.cache.entries.is_empty());
    assert_eq!(cache.render_state(std::slice::from_ref(&target)).len(), 1);
    assert!(cache.cache.entries.is_empty());

    let requests = cache.next_requests(std::slice::from_ref(&target));

    assert_eq!(
        requests,
        vec![AppCommand::LoadAttachmentPreview {
            url: target.url.clone()
        }]
    );
    assert_eq!(cache.cache.entries.len(), 1);
}

#[test]
fn image_preview_render_state_preserves_target_order() {
    let mut cache = ImagePreviewCache::new(None);
    let first = image_preview_target(1);
    let second = ImagePreviewTarget {
        message_id: Id::new(1),
        preview_index: 1,
        preview_x_offset_columns: 8,
        ..image_preview_target(2)
    };
    cache.cache.entries.insert(
        second.key(),
        ImagePreviewEntry::Loading {
            filename: second.filename.clone(),
            protocol_spec: second.protocol_render_spec(),
            last_used: 1,
        },
    );
    cache.cache.entries.insert(
        first.key(),
        ImagePreviewEntry::Loading {
            filename: first.filename.clone(),
            protocol_spec: first.protocol_render_spec(),
            last_used: 2,
        },
    );

    let previews = cache.render_state(&[first, second]);

    assert_eq!(
        previews
            .into_iter()
            .map(|preview| match preview.state {
                super::super::ui::ImagePreviewState::Loading { filename } => filename,
                _ => "unexpected state".to_owned(),
            })
            .collect::<Vec<_>>(),
        vec!["image-1.png", "image-2.png"]
    );
}

#[test]
fn image_preview_protocol_spec_ignores_screen_placement() {
    let target = image_preview_target(1);
    let original_spec = target.protocol_render_spec();
    let moved = ImagePreviewTarget {
        message_index: 3,
        preview_x_offset_columns: target.preview_x_offset_columns + 2,
        accent_color: Some(0x12_34_56),
        ..target.clone()
    };
    assert_eq!(original_spec, moved.protocol_render_spec());

    let resized = ImagePreviewTarget {
        preview_width: target.preview_width + 1,
        ..target
    };
    assert_ne!(original_spec, resized.protocol_render_spec());
}

#[test]
fn image_preview_cache_keeps_duplicate_urls_as_separate_preview_instances() {
    let mut cache = ImagePreviewCache::new(None);
    let first = image_preview_target(1);
    let second = ImagePreviewTarget {
        preview_index: 1,
        preview_x_offset_columns: 8,
        ..image_preview_target(1)
    };

    let requests = cache.next_requests(&[first, second]);

    assert_eq!(requests.len(), 1);
    assert_eq!(cache.cache.entries.len(), 2);
    let previews = cache.render_state(&[
        image_preview_target(1),
        ImagePreviewTarget {
            preview_index: 1,
            preview_x_offset_columns: 8,
            ..image_preview_target(1)
        },
    ]);

    assert_eq!(previews.len(), 2);
    assert_eq!(previews[0].preview_x_offset_columns, 0);
    assert_eq!(previews[1].preview_x_offset_columns, 8);
}

#[test]
fn image_preview_cache_deduplicates_url_already_loading_from_previous_frame() {
    let mut cache = ImagePreviewCache::new(None);
    let first = image_preview_target(1);
    cache.next_requests(std::slice::from_ref(&first));
    let second = ImagePreviewTarget {
        preview_index: 1,
        preview_x_offset_columns: 8,
        ..image_preview_target(1)
    };

    let requests = cache.next_requests(std::slice::from_ref(&second));

    assert!(requests.is_empty());
    assert_eq!(cache.cache.entries.len(), 2);
}

#[test]
fn image_preview_cache_keeps_viewer_and_inline_entries_separate() {
    let mut cache = ImagePreviewCache::new(None);
    let inline = image_preview_target(1);
    let viewer = ImagePreviewTarget {
        viewer: true,
        preview_width: 76,
        preview_height: 13,
        visible_preview_height: 13,
        ..image_preview_target(1)
    };

    let inline_requests = cache.next_requests(std::slice::from_ref(&inline));
    let viewer_requests = cache.next_requests(std::slice::from_ref(&viewer));

    assert_eq!(inline_requests.len(), 1);
    assert!(viewer_requests.is_empty());
    assert_eq!(cache.cache.entries.len(), 2);
    assert!(cache.cache.entries.contains_key(&inline.key()));
    assert!(cache.cache.entries.contains_key(&viewer.key()));
}

#[test]
fn image_preview_cache_evicts_least_recently_used_entries() {
    let mut cache = ImagePreviewCache::new(None);
    let existing_targets = (1..=MAX_IMAGE_PREVIEW_CACHE_ENTRIES as u64)
        .map(image_preview_target)
        .collect::<Vec<_>>();
    cache.next_requests(&existing_targets);
    cache.render_state(std::slice::from_ref(&existing_targets[0]));

    let new_target = image_preview_target(999);
    cache.next_requests(std::slice::from_ref(&new_target));

    assert_eq!(cache.cache.entries.len(), MAX_IMAGE_PREVIEW_CACHE_ENTRIES);
    assert!(cache.cache.entries.contains_key(&existing_targets[0].key()));
    assert!(!cache.cache.entries.contains_key(&existing_targets[1].key()));
    assert!(cache.cache.entries.contains_key(&new_target.key()));

    let mut decoded_cache =
        ImagePreviewCache::new(Some(ratatui_image::picker::Picker::halfblocks()));
    let first = image_preview_target(1);
    let second = image_preview_target(2);
    for (last_used, target) in [first.clone(), second.clone()].into_iter().enumerate() {
        decoded_cache.cache.entries.insert(
            target.key(),
            ImagePreviewEntry::Ready {
                filename: target.filename.clone(),
                generation: 1,
                image: decode_media_image_bytes(&encoded_png(2, 2))
                    .expect("small preview should decode"),
                protocol_spec: target.protocol_render_spec(),
                protocols: Box::new(super::cache::RenderProtocolCache::new()),
                last_used: last_used as u64,
            },
        );
    }
    decoded_cache
        .cache
        .prune_to_limits(MAX_IMAGE_PREVIEW_CACHE_ENTRIES, 16, |_| false);
    assert_eq!(decoded_cache.cache.retained_decoded_bytes(), 16);
    assert!(!decoded_cache.cache.entries.contains_key(&first.key()));
    assert!(decoded_cache.cache.entries.contains_key(&second.key()));
}

#[test]
fn image_preview_cache_limits_visible_requests() {
    let mut cache = ImagePreviewCache::new(None);
    let targets = (1..=MAX_IMAGE_PREVIEW_CACHE_ENTRIES as u64 + 2)
        .map(image_preview_target)
        .collect::<Vec<_>>();

    let requests = cache.next_requests(&targets);

    assert_eq!(cache.cache.entries.len(), MAX_IMAGE_PREVIEW_CACHE_ENTRIES);
    assert_eq!(requests.len(), MAX_IMAGE_PREVIEW_CACHE_ENTRIES);
    assert!(cache.cache.entries.contains_key(&targets[0].key()));
    assert!(
        !cache
            .cache
            .entries
            .contains_key(&targets[MAX_IMAGE_PREVIEW_CACHE_ENTRIES].key())
    );
}

#[test]
fn image_preview_store_loaded_preserves_existing_non_loading_entries() {
    let mut cache = ImagePreviewCache::new(None);
    let existing = image_preview_target(1).key();
    let loading = ImagePreviewTarget {
        message_id: Id::new(2),
        ..image_preview_target(1)
    }
    .key();
    cache.cache.entries.insert(
        existing.clone(),
        ImagePreviewEntry::Failed {
            filename: "existing.png".to_owned(),
            message: "existing failure".to_owned(),
            last_used: 1,
        },
    );
    cache.cache.entries.insert(
        loading.clone(),
        ImagePreviewEntry::Loading {
            filename: "loading.png".to_owned(),
            protocol_spec: image_preview_target(1).protocol_render_spec(),
            last_used: 2,
        },
    );

    cache.store_loaded(&existing.url);

    assert!(matches!(
        cache.cache.entries.get(&existing),
        Some(ImagePreviewEntry::Failed { message, .. }) if message == "existing failure"
    ));
    assert!(matches!(
        cache.cache.entries.get(&loading),
        Some(ImagePreviewEntry::Failed { message, .. })
            if message == "inline preview unavailable in this terminal"
    ));
}

#[test]
fn media_decode_cache_shares_one_url_decode_across_preview_requests() {
    let first = image_preview_target(1);
    let second = ImagePreviewTarget {
        preview_index: 1,
        preview_x_offset_columns: 8,
        ..image_preview_target(1)
    };
    let keys = [first.key(), second.key()];
    let requests = keys
        .iter()
        .enumerate()
        .map(|(index, key)| super::decode::MediaImageDecodeRequest {
            key: MediaImageDecodeKey::Preview(key.clone()),
            generation: index as u64 + 1,
        })
        .collect();
    let mut decoded_images = MediaImageDecodeCache::new();
    let encoded = encoded_animated_gif();
    let outcome = decoded_images.request(&first.url, &encoded, requests);

    assert!(outcome.deliveries.is_empty());
    assert_eq!(
        outcome.job.as_ref().map(|job| job.bytes.as_ref()),
        Some(encoded.as_slice())
    );
    let deliveries = decoded_images.complete(MediaImageDecodeResult {
        url: first.url.clone(),
        result: decode_media_image_bytes(&encoded).map_err(MediaWorkError::Failed),
    });
    assert_eq!(deliveries.len(), 2);
    assert_eq!(
        deliveries
            .iter()
            .map(|delivery| (&delivery.key, delivery.generation))
            .collect::<Vec<_>>(),
        vec![
            (&MediaImageDecodeKey::Preview(keys[0].clone()), 1),
            (&MediaImageDecodeKey::Preview(keys[1].clone()), 2),
        ]
    );
    let first_image = deliveries[0]
        .result
        .as_ref()
        .expect("shared decode should succeed");
    let second_image = deliveries[1]
        .result
        .as_ref()
        .expect("shared decode should succeed");
    assert!(first_image.shares_frames_with(second_image));

    let third = ImagePreviewTarget {
        message_id: Id::new(2),
        ..image_preview_target(1)
    };
    let third_key = third.key();
    let cached = decoded_images.request(
        &third.url,
        &encoded,
        vec![super::decode::MediaImageDecodeRequest {
            key: MediaImageDecodeKey::Preview(third_key.clone()),
            generation: 3,
        }],
    );
    assert!(cached.job.is_none());
    assert_eq!(cached.deliveries.len(), 1);
    let delivery = &cached.deliveries[0];
    assert_eq!(
        (&delivery.key, delivery.generation),
        (&MediaImageDecodeKey::Preview(third_key), 3)
    );
    let third_image = delivery
        .result
        .as_ref()
        .expect("cached decode should succeed");
    assert!(first_image.shares_frames_with(third_image));
}

#[test]
fn image_preview_store_decoded_records_decode_failure() {
    let mut cache = ImagePreviewCache::new(None);
    let target = image_preview_target(1);
    let key = target.key();
    let protocol_spec = target.protocol_render_spec();
    cache.cache.entries.insert(
        key.clone(),
        ImagePreviewEntry::Decoding {
            filename: "loading.png".to_owned(),
            generation: 1,
            protocol_spec,
            last_used: 1,
        },
    );

    cache.store_decoded(
        key.clone(),
        1,
        Err(MediaWorkError::Failed(
            "decode failed: invalid image".to_owned(),
        )),
    );

    assert!(matches!(
        cache.cache.entries.get(&key),
        Some(ImagePreviewEntry::Failed { filename, message, .. })
            if filename == "loading.png" && message == "decode failed: invalid image"
    ));
}

#[test]
fn media_decode_queue_pressure_retries_all_consumers() {
    let preview_target = image_preview_target(1);
    let preview_key = preview_target.key();
    let mut previews = ImagePreviewCache::new(Some(ratatui_image::picker::Picker::halfblocks()));
    previews.cache.entries.insert(
        preview_key.clone(),
        ImagePreviewEntry::Decoding {
            filename: preview_target.filename.clone(),
            generation: 1,
            protocol_spec: preview_target.protocol_render_spec(),
            last_used: 1,
        },
    );
    previews.store_decoded(preview_key.clone(), 1, Err(MediaWorkError::Busy));
    assert!(!previews.cache.entries.contains_key(&preview_key));
    assert_eq!(
        previews.next_requests(std::slice::from_ref(&preview_target)),
        vec![AppCommand::LoadAttachmentPreview {
            url: preview_target.url.clone(),
        }]
    );

    let avatar_target = AvatarTarget {
        row: 0,
        visible_height: 1,
        top_clip_rows: 0,
        url: "https://cdn.discordapp.com/avatars/1/hash.png".to_owned(),
    };
    let avatar_cache_url = avatar_preview_url(
        &avatar_target.url,
        AVATAR_PREVIEW_WIDTH,
        AVATAR_PREVIEW_HEIGHT,
    );
    let mut avatars = AvatarImageCache::new(Some(ratatui_image::picker::Picker::halfblocks()));
    avatars.cache.entries.insert(
        avatar_cache_url.clone(),
        AvatarImageEntry::Decoding {
            generation: 1,
            last_used: 1,
        },
    );
    avatars.store_decoded(avatar_cache_url.clone(), 1, Err(MediaWorkError::Busy));
    assert!(!avatars.cache.entries.contains_key(&avatar_cache_url));
    assert_eq!(
        avatars.next_requests(std::slice::from_ref(&avatar_target)),
        vec![AppCommand::LoadAttachmentPreview {
            url: avatar_cache_url,
        }]
    );

    let emoji_target = EmojiImageTarget {
        url: "https://cdn.discordapp.com/emojis/1.gif".to_owned(),
        image_size: EmojiImageSize::Compact,
    };
    let mut emojis = EmojiImageCache::new(Some(ratatui_image::picker::Picker::halfblocks()));
    emojis.cache.entries.insert(
        emoji_target.url.clone(),
        EmojiImageEntry::Decoding {
            generation: 1,
            last_used: 1,
        },
    );
    emojis.store_decoded(emoji_target.url.clone(), 1, Err(MediaWorkError::Busy));
    assert!(!emojis.cache.entries.contains_key(&emoji_target.url));
    assert_eq!(
        emojis.next_requests(std::slice::from_ref(&emoji_target)),
        vec![AppCommand::LoadAttachmentPreview {
            url: emoji_target.url,
        }]
    );
}

#[test]
fn protocol_queue_pressure_does_not_consume_failure_attempts() {
    let mut protocols = super::cache::RenderProtocolCache::<usize>::new();

    for _ in 0..3 {
        assert!(protocols.request_build(&0));
        assert_eq!(protocols.store_result(0, Err(MediaWorkError::Busy)), Ok(()));
        assert!(!protocols.is_terminally_failed(&0));
    }

    assert!(protocols.request_build(&0));
    assert_eq!(
        protocols.store_result(
            0,
            Err(MediaWorkError::Failed(
                "temporary protocol failure".to_owned(),
            )),
        ),
        Ok(())
    );
    assert!(!protocols.is_terminally_failed(&0));

    assert!(protocols.request_build(&0));
    assert_eq!(
        protocols.store_result(
            0,
            Err(MediaWorkError::Failed(
                "terminal protocol failure".to_owned(),
            )),
        ),
        Err("terminal protocol failure".to_owned())
    );
    assert!(protocols.is_terminally_failed(&0));
}

#[test]
fn image_preview_store_decoded_ignores_replaced_decoding_generation() {
    let mut cache = ImagePreviewCache::new(None);
    let target = image_preview_target(1);
    let key = target.key();
    let protocol_spec = target.protocol_render_spec();
    cache.cache.entries.insert(
        key.clone(),
        ImagePreviewEntry::Decoding {
            filename: "newer.png".to_owned(),
            generation: 2,
            protocol_spec,
            last_used: 2,
        },
    );

    cache.store_decoded(
        key.clone(),
        1,
        Err(MediaWorkError::Failed(
            "decode failed: old generation".to_owned(),
        )),
    );

    assert!(matches!(
        cache.cache.entries.get(&key),
        Some(ImagePreviewEntry::Decoding { filename, generation, .. })
            if filename == "newer.png" && *generation == 2
    ));
}

#[test]
fn decode_image_bytes_reports_invalid_bytes() {
    let error =
        decode_image_bytes(b"not an image").expect_err("invalid bytes should fail to decode");

    assert!(error.starts_with("decode failed:"));
}

#[test]
fn media_decoder_preserves_and_plays_gif_and_webp_animation_frames() {
    let gif_10_ms = encoded_two_frame_gif(10);
    let gif_20_ms = encoded_two_frame_gif(20);
    let gif_30_ms = encoded_two_frame_gif(30);
    let gif_40_ms = encoded_two_frame_gif(40);
    let cases: [(&str, &[u8]); 6] = [
        ("10 ms GIF", &gif_10_ms),
        ("20 ms GIF", &gif_20_ms),
        ("30 ms GIF", &gif_30_ms),
        ("40 ms GIF", &gif_40_ms),
        ("WebP", include_bytes!("testdata/two-frame.webp")),
        ("APNG", include_bytes!("testdata/two-frame.apng")),
    ];

    for (label, bytes) in cases {
        let mut image = decode_media_image_bytes(bytes)
            .unwrap_or_else(|error| panic!("{label} animation should decode: {error}"));
        assert_eq!(image.frame_count(), 2, "{label}");
        assert!(image.is_animated(), "{label}");
        assert_eq!(image.retained_bytes(), 32, "{label}");

        let first_pixel = image.current_frame().to_rgba8().get_pixel(0, 0).0;
        let started_at = Instant::now();
        image.start_animation(started_at);
        let first_deadline = image
            .next_frame_deadline()
            .expect("visible animation should schedule its next frame");
        assert!(first_deadline >= started_at + Duration::from_millis(50));

        assert!(image.advance_frame(first_deadline), "{label}");
        assert_eq!(image.current_frame_index(), 1, "{label}");
        assert_ne!(
            first_pixel,
            image.current_frame().to_rgba8().get_pixel(0, 0).0,
            "{label} frame should visibly change"
        );

        let second_deadline = image
            .next_frame_deadline()
            .expect("animation should schedule the following frame");
        assert!(image.advance_frame(second_deadline), "{label}");
        assert_eq!(image.current_frame_index(), 0, "{label} should loop");

        image.pause_animation();
        assert_eq!(image.next_frame_deadline(), None, "{label}");
    }

    let encoded = encoded_animated_gif();
    let error = match decode_media_image_bytes(&encoded[..encoded.len() - 2]) {
        Ok(_) => panic!("a corrupt later GIF frame should fail the full decode"),
        Err(error) => error,
    };
    assert!(error.starts_with("decode failed at animation frame 2:"));
}

#[test]
fn media_decoder_rasterizes_and_plays_lottie_animation_frames() {
    let mut image = decode_media_image_bytes(include_bytes!("testdata/moving-square-lottie.json"))
        .expect("Lottie animation should decode");

    assert_eq!(image.frame_count(), 20);
    assert!(image.is_animated());
    assert_eq!(image.retained_bytes(), 16 * 16 * 4 * 20);

    let first_frame = image.current_frame().to_rgba8();
    let last_frame = image.frame_shared(image.frame_count() - 1).to_rgba8();
    assert_ne!(first_frame, last_frame);

    let started_at = Instant::now();
    image.start_animation(started_at);
    let first_deadline = image
        .next_frame_deadline()
        .expect("visible Lottie animation should schedule its next frame");
    assert!(image.advance_frame(first_deadline));
    assert_eq!(image.current_frame_index(), 1);
}

#[test]
fn media_decoder_rejects_invalid_lottie_documents() {
    let mut oversized = vec![b' '; MAX_LOTTIE_JSON_BYTES + 1];
    oversized[0] = b'{';
    let cases = [
        ("malformed", br#"{"v":"5.7""#.as_slice()),
        ("oversized", oversized.as_slice()),
    ];

    for (label, bytes) in cases {
        let error = decode_media_image_bytes(bytes)
            .err()
            .unwrap_or_else(|| panic!("{label} Lottie document should fail"));
        assert!(error.starts_with("decode failed:"), "{label}: {error}");
    }
}

#[test]
fn media_decoder_keeps_static_png_still() {
    let image = decode_media_image_bytes(&encoded_png(2, 2)).expect("static PNG should decode");
    assert!(!image.is_animated());
    assert_eq!(image.frame_count(), 1);
}

#[test]
fn media_decoder_samples_long_animations_across_the_full_timeline() {
    let mut image = decode_media_image_bytes(&encoded_long_animated_gif())
        .expect("long GIF animation should decode");
    assert_eq!(image.frame_count(), MAX_RETAINED_ANIMATION_FRAMES);

    let mut sampled_source_frames = Vec::new();
    let started_at = Instant::now();
    let mut frame_started_at = started_at;
    let mut sampled_duration = Duration::ZERO;
    image.start_animation(started_at);

    for _ in 0..image.frame_count() {
        let pixel = image.current_frame().to_rgba8().get_pixel(0, 0).0;
        sampled_source_frames.push(u16::from(pixel[0]) | (u16::from(pixel[1]) << 8));
        let deadline = image
            .next_frame_deadline()
            .expect("sampled animation should schedule every retained frame");
        let frame_duration = deadline.duration_since(frame_started_at);
        assert!(frame_duration >= Duration::from_millis(50));
        sampled_duration += frame_duration;
        assert!(image.advance_frame(deadline));
        frame_started_at = deadline;
    }

    assert_eq!(sampled_duration, Duration::from_millis(22_800));
    assert_eq!(sampled_source_frames.first(), Some(&0));
    assert_eq!(sampled_source_frames.last(), Some(&299));
    assert!(
        sampled_source_frames
            .iter()
            .filter(|frame| **frame >= 240)
            .count()
            >= 16,
        "longer source delays should retain more representatives"
    );
}

#[test]
fn emoji_animation_clock_runs_only_while_the_image_is_visible() {
    let url = "https://cdn.discordapp.com/emojis/42.webp?animated=true".to_owned();
    let target = EmojiImageTarget {
        url: url.clone(),
        image_size: EmojiImageSize::Compact,
    };
    let mut cache = EmojiImageCache::new(Some(ratatui_image::picker::Picker::halfblocks()));
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
        decode_media_image_bytes(&encoded_animated_gif()).map_err(MediaWorkError::Failed),
    );
    let _ = cache.render_state(std::slice::from_ref(&target));
    let jobs = cache.take_protocol_jobs();
    assert_eq!(jobs.len(), 1);
    let mut failed = build_media_protocol(
        jobs.into_iter()
            .next()
            .expect("first frame protocol job should exist"),
    );
    failed.result = Err(MediaWorkError::Failed(
        "temporary protocol worker failure".to_owned(),
    ));
    cache.store_protocol(failed);
    let _ = cache.render_state(std::slice::from_ref(&target));
    let retry_jobs = cache.take_protocol_jobs();
    assert_eq!(retry_jobs.len(), 1);
    for job in retry_jobs {
        cache.store_protocol(build_media_protocol(job));
    }
    assert_eq!(cache.render_state(std::slice::from_ref(&target)).len(), 1);

    let started_at = Instant::now();
    cache.sync_animation_visibility(std::slice::from_ref(&target), started_at);
    let deadline = cache
        .next_animation_deadline()
        .expect("visible emoji should schedule an animation frame");
    assert!(cache.advance_animations(deadline));
    {
        let rendered = cache.render_state(std::slice::from_ref(&target));
        assert_eq!(rendered.len(), 1);
    }
    let jobs = cache.take_protocol_jobs();
    assert_eq!(jobs.len(), 1);
    for job in jobs {
        cache.store_protocol(build_media_protocol(job));
    }
    assert_eq!(cache.render_state(std::slice::from_ref(&target)).len(), 1);
    assert!(matches!(
        cache.cache.entries.get(&url),
        Some(EmojiImageEntry::Ready {
            image,
            protocols,
            ..
        }) if image.current_frame_index() == 1 && protocols.compact.len() == 2
    ));

    let loop_deadline = cache
        .next_animation_deadline()
        .expect("animated emoji should schedule its loop frame");
    assert!(cache.advance_animations(loop_deadline));
    assert_eq!(cache.render_state(std::slice::from_ref(&target)).len(), 1);
    assert!(matches!(
        cache.cache.entries.get(&url),
        Some(EmojiImageEntry::Ready {
            image,
            protocols,
            ..
        }) if image.current_frame_index() == 0 && protocols.compact.len() == 2
    ));

    cache.sync_animation_visibility(&[], loop_deadline);
    assert_eq!(cache.next_animation_deadline(), None);
    assert!(!cache.advance_animations(loop_deadline + Duration::from_secs(1)));
}

#[test]
fn attachment_preview_waits_for_a_ready_or_failed_next_animation_frame() {
    let target = image_preview_target(42);
    let key = target.key();
    let protocol_spec = target.protocol_render_spec();
    let mut cache = ImagePreviewCache::new(Some(ratatui_image::picker::Picker::halfblocks()));
    cache.cache.entries.insert(
        key.clone(),
        ImagePreviewEntry::Decoding {
            filename: target.filename.clone(),
            generation: 1,
            protocol_spec,
            last_used: 1,
        },
    );
    cache.store_decoded(
        key.clone(),
        1,
        decode_media_image_bytes(&encoded_animated_gif()).map_err(MediaWorkError::Failed),
    );
    let _ = cache.render_state(std::slice::from_ref(&target));
    let jobs = cache.take_protocol_jobs();
    assert_eq!(jobs.len(), 1);

    let started_at = Instant::now();
    cache.sync_animation_visibility(std::slice::from_ref(&target), started_at);
    assert_eq!(cache.next_animation_deadline(), None);

    for job in jobs {
        cache.store_protocol(build_media_protocol(job));
    }
    assert_eq!(cache.render_state(std::slice::from_ref(&target)).len(), 1);
    let jobs = cache.take_protocol_jobs();
    assert_eq!(jobs.len(), 1);
    cache.sync_animation_visibility(std::slice::from_ref(&target), started_at);
    assert_eq!(cache.next_animation_deadline(), None);
    let mut failed = build_media_protocol(
        jobs.into_iter()
            .next()
            .expect("next frame protocol job should exist"),
    );
    failed.result = Err(MediaWorkError::Failed(
        "unsupported frame protocol".to_owned(),
    ));
    cache.store_protocol(failed);
    let _ = cache.render_state(std::slice::from_ref(&target));
    let mut failed = build_media_protocol(
        cache
            .take_protocol_jobs()
            .into_iter()
            .next()
            .expect("failed frame protocol should retry once"),
    );
    failed.result = Err(MediaWorkError::Failed(
        "unsupported frame protocol".to_owned(),
    ));
    cache.store_protocol(failed);
    assert_eq!(cache.render_state(std::slice::from_ref(&target)).len(), 1);
    assert!(cache.take_protocol_jobs().is_empty());

    cache.sync_animation_visibility(std::slice::from_ref(&target), started_at);
    let deadline = cache
        .next_animation_deadline()
        .expect("visible attachment should schedule an animation frame");
    assert!(cache.advance_animations(deadline));
    {
        let rendered = cache.render_state(std::slice::from_ref(&target));
        assert_eq!(rendered.len(), 1);
    }
    assert!(cache.take_protocol_jobs().is_empty());
    assert!(matches!(
        cache.cache.entries.get(&key),
        Some(ImagePreviewEntry::Ready {
            image,
            protocols,
            ..
        }) if image.current_frame_index() == 1 && protocols.len() == 1
    ));

    let loop_deadline = cache
        .next_animation_deadline()
        .expect("animated attachment should schedule its loop frame");
    assert!(cache.advance_animations(loop_deadline));
    assert_eq!(cache.render_state(std::slice::from_ref(&target)).len(), 1);
    assert!(matches!(
        cache.cache.entries.get(&key),
        Some(ImagePreviewEntry::Ready {
            image,
            protocols,
            ..
        }) if image.current_frame_index() == 0 && protocols.len() == 1
    ));

    cache.sync_animation_visibility(&[], loop_deadline);
    assert_eq!(cache.next_animation_deadline(), None);
}

#[test]
fn media_decode_rejects_oversized_image_dimensions() {
    for (width, height) in [
        (MAX_DECODED_IMAGE_WIDTH + 1, 1),
        (1, MAX_DECODED_IMAGE_HEIGHT + 1),
    ] {
        let bytes = encoded_png(width, height);
        let error = decode_image_bytes(&bytes).expect_err("oversized image should be rejected");

        assert!(
            error.starts_with("decode failed:"),
            "oversized {width}x{height} image should fail with decode context, got {error:?}"
        );
    }
}

#[test]
fn clipped_media_image_stays_within_preview_pixel_bounds() {
    let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(400, 400, Rgba([0, 0, 0, 255])));
    let render_spec = MediaProtocolRenderSpec {
        width: 16,
        height: 3,
        visible_height: 3,
        top_clip_rows: 0,
        show_play_marker: false,
        mask_circular: false,
    };

    let resized = clipped_media_image(&image, (10, 20), render_spec)
        .expect("preview dimensions should produce resized image");

    assert!(resized.width() <= 160);
    assert!(resized.height() <= 60);
    assert!(resized.width() < image.width());
    assert!(resized.height() < image.height());
}

#[test]
fn clipped_video_preview_draws_play_marker_into_image_pixels() {
    let image =
        DynamicImage::ImageRgba8(ImageBuffer::from_pixel(200, 400, Rgba([20, 30, 40, 255])));
    let render_spec = MediaProtocolRenderSpec {
        width: 16,
        height: 3,
        visible_height: 3,
        top_clip_rows: 0,
        show_play_marker: true,
        mask_circular: false,
    };

    let marked = clipped_media_image(&image, (10, 20), render_spec)
        .expect("preview dimensions should produce resized image")
        .to_rgba8();
    let center = marked.get_pixel(marked.width() / 2, marked.height() / 2);

    assert!(
        center.0[0] > 150 && center.0[1] > 150 && center.0[2] > 150,
        "center pixel should contain the bright play triangle, got {center:?}"
    );
    assert_eq!(
        center.0[3], 255,
        "play marker should be drawn over fitted image content, got {center:?}"
    );
    let left_edge = marked.get_pixel(0, marked.height() / 2);
    assert_eq!(
        left_edge.0[3], 0,
        "portrait thumbnail should be centered inside transparent canvas, got {left_edge:?}"
    );
}

#[test]
fn emoji_image_targets_include_visible_custom_reactions() {
    let mut state = state_with_image_messages(1, &[]);
    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(1),
        emojis: vec![CustomEmojiInfo::test(Id::new(50), "party")],
    });
    state.focus_pane(FocusPane::Messages);
    state.open_emoji_reaction_picker();

    let targets = visible_emoji_image_targets(&state);

    assert_eq!(
        targets,
        vec![EmojiImageTarget {
            url: "https://cdn.discordapp.com/emojis/50.png".to_owned(),
            image_size: EmojiImageSize::Compact,
        }]
    );
}

#[test]
fn emoji_image_targets_deduplicate_and_promote_emoji_only_message_slots() {
    let mut state = state_with_image_messages(0, &[]);
    push_media_message(
        &mut state,
        MessageCreateFixture {
            message_id: Id::new(1),
            content: Some(" 😀 <:solo:50> ❤️ ".to_owned()),
            reactions: vec![ReactionInfo::test(ReactionEmoji::Custom {
                id: Id::new(50),
                name: Some("solo".to_owned()),
                animated: false,
            })],
            ..guild_message_create_fixture()
        },
    );

    assert_eq!(
        visible_emoji_image_targets(&state),
        vec![
            EmojiImageTarget {
                url: "https://cdn.discordapp.com/emojis/50.png".to_owned(),
                image_size: EmojiImageSize::Standalone,
            },
            EmojiImageTarget {
                url: "https://cdn.jsdelivr.net/gh/jdecked/twemoji@17.0.3/assets/72x72/1f600.png"
                    .to_owned(),
                image_size: EmojiImageSize::Standalone,
            },
            EmojiImageTarget {
                url: "https://cdn.jsdelivr.net/gh/jdecked/twemoji@17.0.3/assets/72x72/2764.png"
                    .to_owned(),
                image_size: EmojiImageSize::Standalone,
            },
        ]
    );
}

#[test]
fn standalone_emoji_cache_builds_compact_and_large_protocols() {
    let url = "https://cdn.discordapp.com/emojis/50.png".to_owned();
    let target = EmojiImageTarget {
        url: url.clone(),
        image_size: EmojiImageSize::Standalone,
    };
    let mut cache = EmojiImageCache::new(Some(ratatui_image::picker::Picker::halfblocks()));
    cache.cache.entries.insert(
        url.clone(),
        EmojiImageEntry::Decoding {
            generation: 1,
            last_used: 1,
        },
    );
    cache.store_decoded(
        url,
        1,
        decode_media_image_bytes(&encoded_png(32, 32)).map_err(MediaWorkError::Failed),
    );

    assert!(cache.render_state(std::slice::from_ref(&target)).is_empty());
    let results = cache
        .take_protocol_jobs()
        .into_iter()
        .map(build_media_protocol)
        .collect::<Vec<_>>();
    let mut sizes = results
        .iter()
        .filter_map(|result| match result.target {
            MediaProtocolBuildTarget::Emoji { image_size, .. } => Some(image_size),
            _ => None,
        })
        .collect::<Vec<_>>();
    sizes.sort_by_key(|size| size.height());
    assert_eq!(
        sizes,
        vec![EmojiImageSize::Compact, EmojiImageSize::Standalone]
    );
    for result in results {
        cache.store_protocol(result);
    }

    let rendered = cache.render_state(std::slice::from_ref(&target));
    assert_eq!(rendered.len(), 1);
    assert!(rendered[0].standalone_protocol.is_some());
}

#[test]
fn emoji_image_targets_include_visible_composer_custom_emoji_picker_candidates() {
    for (emoji, query) in [
        (CustomEmojiInfo::test(Id::new(50), "party"), ":pa"),
        (
            CustomEmojiInfo {
                available: false,
                ..CustomEmojiInfo::test(Id::new(51), "gone")
            },
            ":go",
        ),
    ] {
        let expected_url = format!("https://cdn.discordapp.com/emojis/{}.png", emoji.id.get());
        let mut state = state_with_image_messages(1, &[]);
        state.push_event(AppEvent::GuildEmojisUpdate {
            guild_id: Id::new(1),
            emojis: vec![emoji],
        });
        state.start_composer();
        for ch in query.chars() {
            state.push_composer_char(ch);
        }

        let targets = visible_emoji_image_targets(&state);

        assert_eq!(
            targets,
            vec![EmojiImageTarget {
                url: expected_url,
                image_size: EmojiImageSize::Compact,
            }]
        );
    }
}

#[test]
fn emoji_image_targets_include_confirmed_composer_custom_emoji() {
    let mut state = state_with_image_messages(1, &[]);
    state.push_event(AppEvent::GuildEmojisUpdate {
        guild_id: Id::new(1),
        emojis: vec![CustomEmojiInfo::test(Id::new(60), "wave")],
    });
    state.start_composer();
    for ch in ":wa".chars() {
        state.push_composer_char(ch);
    }
    assert!(state.confirm_composer_emoji());

    let targets = visible_emoji_image_targets(&state);

    assert_eq!(
        targets,
        vec![EmojiImageTarget {
            url: "https://cdn.discordapp.com/emojis/60.png".to_owned(),
            image_size: EmojiImageSize::Compact,
        }]
    );
}

#[test]
fn emoji_image_targets_include_visible_non_selected_dm_activity() {
    let alice = Id::new(10);
    let bob = Id::new(20);
    let mut state = DashboardState::new();
    for (channel_id, user_id, name, last_message_id) in [
        (Id::new(100), alice, "alice", Id::new(200)),
        (Id::new(101), bob, "bob", Id::new(100)),
    ] {
        state.push_event(AppEvent::ChannelUpsert(ChannelInfo {
            last_message_id: Some(last_message_id),
            recipients: Some(vec![ChannelRecipientInfo {
                status: Some(PresenceStatus::Online),
                ..ChannelRecipientInfo::test(user_id, name)
            }]),
            ..ChannelInfo::test(channel_id, "dm")
        }));
    }
    state.push_event(AppEvent::PresenceUpdate {
        guild_id: None,
        presence: PresenceEventFields {
            user_id: bob,
            status: PresenceStatus::Online,
            activities: vec![ActivityInfo {
                emoji: Some(ActivityEmoji {
                    name: "coffee".to_owned(),
                    id: Some(Id::new(70)),
                    animated: false,
                }),
                state: Some("Taking a break".to_owned()),
                ..ActivityInfo::test(ActivityKind::Custom, "Custom Status")
            }],
        },
    });
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.set_channel_view_height(10);

    let targets = visible_emoji_image_targets(&state);

    assert_eq!(
        targets,
        vec![EmojiImageTarget {
            url: "https://cdn.discordapp.com/emojis/70.png".to_owned(),
            image_size: EmojiImageSize::Compact,
        }]
    );
}

#[test]
fn emoji_image_targets_include_visible_forum_preview_custom_reactions() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(30);
    let thread = ChannelInfo {
        guild_id: Some(guild_id),
        parent_id: Some(forum_id),
        last_message_id: Some(Id::new(300)),
        name: "welcome".to_owned(),
        message_count: Some(1),
        total_message_sent: Some(1),
        thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
        flags: Some(0),
        ..ChannelInfo::test(thread_id, "GuildPublicThread")
    };
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                name: "forum".to_owned(),
                ..ChannelInfo::test(forum_id, "GuildForum")
            },
            thread,
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::ForumPostDataLoaded {
        channel_id: forum_id,
        requested_thread_ids: vec![thread_id],
        posts: vec![ForumPostDataInfo {
            thread_id,
            owner: None,
            first_message: Some(MessageInfo {
                guild_id: Some(guild_id),
                channel_id: thread_id,
                message_id: Id::new(thread_id.get()),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                reactions: vec![ReactionInfo::test(ReactionEmoji::Custom {
                    id: Id::new(50),
                    name: Some("party".to_owned()),
                    animated: false,
                })],
                content: Some("first post".to_owned()),
                ..MessageInfo::default()
            }),
            extra_fields: std::collections::BTreeMap::new(),
        }],
    });

    let targets = visible_emoji_image_targets(&state);

    assert_eq!(
        targets,
        vec![EmojiImageTarget {
            url: "https://cdn.discordapp.com/emojis/50.png".to_owned(),
            image_size: EmojiImageSize::Compact,
        }]
    );
}

#[test]
fn image_preview_targets_place_thread_attachments_in_the_card_right_column() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(30);
    let mut state = DashboardState::new();
    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                name: "forum".to_owned(),
                ..ChannelInfo::test(forum_id, "GuildForum")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(forum_id),
                last_message_id: Some(Id::new(300)),
                name: "welcome".to_owned(),
                message_count: Some(1),
                total_message_sent: Some(1),
                thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
                flags: Some(0),
                ..ChannelInfo::test(thread_id, "GuildPublicThread")
            },
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(AppEvent::ForumPostDataLoaded {
        channel_id: forum_id,
        requested_thread_ids: vec![thread_id],
        posts: vec![ForumPostDataInfo {
            thread_id,
            owner: None,
            first_message: Some(MessageInfo {
                guild_id: Some(guild_id),
                channel_id: thread_id,
                message_id: Id::new(thread_id.get()),
                author_id: Id::new(99),
                author: "neo".to_owned(),
                content: Some("first post".to_owned()),
                attachments: vec![image_attachment(7)],
                ..MessageInfo::default()
            }),
            extra_fields: std::collections::BTreeMap::new(),
        }],
    });
    let mut preview_layout = layout(30);
    preview_layout.list_width = 100;

    let targets = visible_image_preview_targets(&state, preview_layout);

    assert_eq!(targets.len(), 1);
    let target = &targets[0];
    assert!(target.thread_card);
    assert_eq!(target.message_id, Id::new(thread_id.get()));
    assert_eq!(target.preview_y_offset_rows, 2);
    assert!(target.preview_x_offset_columns >= 80);
    assert!(target.preview_width <= 20);
    assert!(target.preview_height <= 4);
    assert_eq!(target.filename, "image-7.png");

    let text_channel_id = Id::new(40);
    let text_thread_id = Id::new(50);
    let mut state = DashboardState::new();
    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                name: "general".to_owned(),
                ..ChannelInfo::test(text_channel_id, "GuildText")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(text_channel_id),
                last_message_id: Some(Id::new(500)),
                name: "design discussion".to_owned(),
                message_count: Some(1),
                total_message_sent: Some(1),
                thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
                ..ChannelInfo::test(text_thread_id, "GuildPublicThread")
            },
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.enter_channel_thread_list_view(text_channel_id);
    state.push_event(message_create_event(MessageInfo {
        guild_id: Some(guild_id),
        channel_id: text_thread_id,
        message_id: Id::new(500),
        author_id: Id::new(99),
        author: "neo".to_owned(),
        content: Some("thread reply".to_owned()),
        attachments: vec![image_attachment(8)],
        ..MessageInfo::default()
    }));

    let targets = visible_image_preview_targets(&state, preview_layout);

    assert_eq!(targets.len(), 1);
    let target = &targets[0];
    assert!(target.thread_card);
    assert_eq!(target.message_id, Id::new(500));
    assert_eq!(target.preview_y_offset_rows, 2);
    assert!(target.preview_x_offset_columns >= 80);
    assert!(target.preview_width <= 20);
    assert!(target.preview_height <= 4);
    assert_eq!(target.filename, "image-8.png");

    let parent_channel_id = Id::new(60);
    let embedded_thread_id = Id::new(70);
    let mut state = DashboardState::new();
    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                name: "general".to_owned(),
                ..ChannelInfo::test(parent_channel_id, "GuildText")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(parent_channel_id),
                last_message_id: Some(Id::new(700)),
                name: "image thread".to_owned(),
                message_count: Some(1),
                total_message_sent: Some(1),
                thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
                ..ChannelInfo::test(embedded_thread_id, "GuildPublicThread")
            },
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(guild_id),
        channel_id: parent_channel_id,
        message_id: Id::new(600),
        content: Some("image thread".to_owned()),
        message_kind: crate::discord::MessageKind::new(18),
        ..guild_message_create_fixture()
    }));
    state.push_event(message_create_event(MessageCreateFixture {
        guild_id: Some(guild_id),
        channel_id: embedded_thread_id,
        message_id: Id::new(700),
        content: Some(String::new()),
        attachments: vec![image_attachment(9)],
        ..guild_message_create_fixture()
    }));
    let mut embedded_layout = layout(30);
    embedded_layout.list_width = 100;
    embedded_layout.content_width = 88;

    let targets = visible_image_preview_targets(&state, embedded_layout);

    assert_eq!(targets.len(), 1);
    let target = &targets[0];
    assert!(target.thread_card);
    assert_eq!(target.message_id, Id::new(700));
    assert!(target.preview_y_offset_rows >= 2);
    let card_left = crate::tui::ui::avatar_gutter_width(state.show_avatars());
    let card_right = card_left.saturating_add(
        u16::try_from(crate::tui::ui::thread_card::thread_card_width_in_message(
            embedded_layout.content_width,
        ))
        .expect("embedded card width fits u16"),
    );
    assert!(target.preview_x_offset_columns >= card_right.saturating_sub(20));
    assert!(
        target
            .preview_x_offset_columns
            .saturating_add(target.preview_width)
            <= card_right
    );
    assert!(target.preview_height <= 4);
    assert_eq!(target.filename, "image-9.png");
}

#[test]
fn emoji_image_targets_include_visible_forum_post_custom_tag_emoji() {
    let guild_id = Id::new(1);
    let forum_id = Id::new(20);
    let thread_id = Id::new(30);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        emojis: vec![CustomEmojiInfo {
            animated: true,
            ..CustomEmojiInfo::test(Id::new(77), "bug")
        }],
        channels: vec![
            ChannelInfo {
                guild_id: Some(guild_id),
                name: "forum".to_owned(),
                // A custom-emoji tag carries `emoji_id` while a Unicode tag
                // renders directly from `emoji_name`.
                available_tags: vec![
                    crate::discord::ForumTagInfo {
                        id: Id::new(101),
                        name: "bug".to_owned(),
                        moderated: false,
                        emoji_id: Some(Id::new(77)),
                        emoji_name: None,
                    },
                    crate::discord::ForumTagInfo {
                        id: Id::new(102),
                        name: "fire".to_owned(),
                        moderated: false,
                        emoji_id: None,
                        emoji_name: Some("🔥".to_owned()),
                    },
                ],
                ..ChannelInfo::test(forum_id, "GuildForum")
            },
            ChannelInfo {
                guild_id: Some(guild_id),
                parent_id: Some(forum_id),
                last_message_id: Some(Id::new(300)),
                name: "welcome".to_owned(),
                message_count: Some(1),
                total_message_sent: Some(1),
                thread_metadata: Some(crate::discord::ThreadMetadataInfo::test(false, false)),
                flags: Some(0),
                applied_tags: vec![Id::new(101), Id::new(102)],
                ..ChannelInfo::test(thread_id, "GuildPublicThread")
            },
        ],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    let targets = visible_emoji_image_targets(&state);

    assert_eq!(
        targets,
        vec![EmojiImageTarget {
            url: "https://cdn.discordapp.com/emojis/77.webp?animated=true".to_owned(),
            image_size: EmojiImageSize::Compact,
        }]
    );
}

#[test]
fn emoji_image_cache_skips_requests_without_image_protocol() {
    let mut cache = EmojiImageCache::new(None);
    let target = EmojiImageTarget {
        url: "https://cdn.discordapp.com/emojis/50.png".to_owned(),
        image_size: EmojiImageSize::Compact,
    };

    let requests = cache.next_requests(std::slice::from_ref(&target));

    assert!(requests.is_empty());
    assert!(cache.cache.entries.is_empty());
}

#[test]
fn image_preview_height_respects_dimensions_and_fallbacks() {
    let cases = [
        (60, 10, Some(2400), Some(600), 5),
        (60, 10, Some(800), Some(800), 10),
        (72, 10, Some(481), Some(160), 6),
        (72, 10, Some(100), Some(100), 4),
        (72, 10, Some(32), Some(32), 3),
        (72, 10, Some(100), Some(40), 3),
        (72, 10, Some(128), Some(128), 5),
        (60, 10, None, None, 10),
        (60, 10, Some(0), Some(100), 10),
    ];

    for (width, max_height, image_width, image_height, expected) in cases {
        assert_eq!(
            image_preview_height_for_dimensions(width, max_height, image_width, image_height),
            expected
        );
    }
    assert!(
        image_preview_height_for_dimensions(60, 10, Some(2400), Some(600))
            < image_preview_height_for_dimensions(60, 10, Some(800), Some(800))
    );
}

fn state_with_image_messages(count: u64, image_message_ids: &[u64]) -> DashboardState {
    state_with_image_messages_and_display_options(
        count,
        image_message_ids,
        DisplayOptions::default(),
    )
}

fn state_with_image_messages_and_display_options(
    count: u64,
    image_message_ids: &[u64],
    display_options: DisplayOptions,
) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new_with_display_options(display_options);

    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    for id in 1..=count {
        push_media_message(
            &mut state,
            MessageCreateFixture {
                channel_id,
                message_id: Id::new(id),
                content: Some(format!("msg {id}")),
                attachments: image_message_ids
                    .contains(&id)
                    .then(|| image_attachment(id))
                    .into_iter()
                    .collect(),
                ..guild_message_create_fixture()
            },
        );
    }

    state.push_event(empty_latest_message_history_loaded_event(channel_id));

    state
}

fn state_with_avatar_messages(count: u64) -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    for id in 1..=count {
        push_media_message(
            &mut state,
            MessageCreateFixture {
                channel_id,
                message_id: Id::new(id),
                author_avatar_url: Some(format!("https://cdn.discordapp.com/avatar-{id}.png")),
                content: Some(format!("msg {id}")),
                ..guild_message_create_fixture()
            },
        );
    }

    state
}

fn state_with_cross_day_image_message() -> DashboardState {
    let guild_id = Id::new(1);
    let channel_id = Id::new(2);
    let mut state = DashboardState::new();

    state.push_event(guild_create_event(GuildCreateFixture {
        channels: vec![ChannelInfo {
            guild_id: Some(guild_id),
            name: "general".to_owned(),
            ..ChannelInfo::test(channel_id, "GuildText")
        }],
        ..GuildCreateFixture::new(guild_id)
    }));
    state.confirm_selected_guild();
    state.confirm_selected_channel();

    let day_one = test_message_id_for_unix_millis(1_743_465_600_000);
    let day_two = test_message_id_for_unix_millis(1_743_465_600_000 + 24 * 60 * 60 * 1000);
    for (message_id, attachments) in [(day_one, Vec::new()), (day_two, vec![image_attachment(2)])] {
        push_media_message(
            &mut state,
            MessageCreateFixture {
                channel_id,
                message_id,
                content: Some("msg".to_owned()),
                attachments,
                ..guild_message_create_fixture()
            },
        );
    }

    state
}

fn target_message_ids(targets: &[ImagePreviewTarget]) -> Vec<Id<MessageMarker>> {
    targets.iter().map(|target| target.message_id).collect()
}

fn push_album_message(state: &mut DashboardState, message_id: u64, attachment_count: u64) {
    push_media_message(
        state,
        MessageCreateFixture {
            message_id: Id::new(message_id),
            content: Some("album".to_owned()),
            attachments: (1..=attachment_count).map(image_attachment).collect(),
            ..guild_message_create_fixture()
        },
    );
}

fn push_attachment_message(state: &mut DashboardState, attachment: AttachmentInfo) {
    push_media_message(
        state,
        MessageCreateFixture {
            message_id: Id::new(1),
            content: Some("photo".to_owned()),
            attachments: vec![attachment],
            ..guild_message_create_fixture()
        },
    );
}

fn image_preview_target(id: u64) -> ImagePreviewTarget {
    ImagePreviewTarget {
        viewer: false,
        thread_card: false,
        message_index: 0,
        preview_index: 0,
        preview_x_offset_columns: 0,
        preview_y_offset_rows: 0,
        preview_width: 16,
        preview_height: 3,
        visible_preview_height: 3,
        top_clip_rows: 0,
        accent_color: None,
        show_play_marker: false,
        message_id: Id::new(id),
        url: format!("https://cdn.discordapp.com/image-{id}.png"),
        filename: format!("image-{id}.png"),
    }
}

fn image_attachment(id: u64) -> AttachmentInfo {
    AttachmentInfo {
        url: format!("https://cdn.discordapp.com/image-{id}.png"),
        proxy_url: format!("https://media.discordapp.net/image-{id}.png"),
        content_type: Some("image/png".to_owned()),
        size: 2048,
        width: Some(640),
        height: Some(480),
        ..AttachmentInfo::test(Id::new(id), format!("image-{id}.png"))
    }
}

fn video_attachment(id: u64) -> AttachmentInfo {
    AttachmentInfo {
        url: format!("https://cdn.discordapp.com/clip-{id}.mp4"),
        proxy_url: format!("https://media.discordapp.net/attachments/691/150/clip-{id}.mp4"),
        content_type: Some("video/mp4".to_owned()),
        size: 78_364_758,
        width: Some(1080),
        height: Some(1920),
        ..AttachmentInfo::test(Id::new(id), format!("clip-{id}.mp4"))
    }
}

fn youtube_embed() -> EmbedInfo {
    EmbedInfo {
        color: Some(0xff0000),
        provider_name: Some("YouTube".to_owned()),
        title: Some("Example Video".to_owned()),
        description: Some("A video description".to_owned()),
        url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_owned()),
        thumbnail_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".to_owned()),
        thumbnail_width: Some(480),
        thumbnail_height: Some(360),
        video_url: Some("https://www.youtube.com/embed/dQw4w9WgXcQ".to_owned()),
        ..EmbedInfo::test()
    }
}

fn forwarded_snapshot(id: u64) -> MessageSnapshotInfo {
    MessageSnapshotInfo {
        content: Some(format!("forwarded {id}")),
        attachments: vec![image_attachment(id)],
        ..MessageSnapshotInfo::test()
    }
}

#[test]
fn image_preview_targets_track_the_visible_message_window() {
    // (label, message count, image ids, view height, layout height, expected targets)
    let cases = [
        (
            "a preview clipped by the viewport still counts",
            2,
            &[1, 2][..],
            6,
            6,
            1,
        ),
        (
            "no visible preview row still targets the message",
            2,
            &[1, 2][..],
            5,
            5,
            1,
        ),
        (
            "scrolling moves the target to the visible message",
            8,
            &[1, 6][..],
            6,
            7,
            6,
        ),
    ];

    for (label, count, image_ids, view_height, layout_height, expected) in cases {
        let mut state = state_with_image_messages(count, image_ids);
        state.set_message_view_height(view_height);

        let targets = visible_image_preview_targets(&state, layout(layout_height));

        assert_eq!(
            target_message_ids(&targets),
            vec![Id::new(expected)],
            "{label}"
        );
    }
}

#[test]
fn image_preview_targets_resize_every_media_proxy_url_shape() {
    let cases = [
        (
            "https://media.discordapp.net/attachments/691/150/photo.png?ex=abc&is=def&hm=123&format=png&width=4000&height=3000",
            "https://media.discordapp.net/attachments/691/150/photo.png?ex=abc&is=def&hm=123&format=webp&width=320&height=240",
        ),
        (
            "https://media.discordapp.net/ephemeral-attachments/691/150/photo.png?ex=abc&is=def&hm=123&width=4000&height=3000",
            "https://media.discordapp.net/ephemeral-attachments/691/150/photo.png?ex=abc&is=def&hm=123&format=webp&width=320&height=240",
        ),
    ];

    for (proxy_url, expected) in cases {
        let mut state = state_with_image_messages(0, &[]);
        let mut attachment = image_attachment(1);
        attachment.proxy_url = proxy_url.to_owned();
        push_media_message(
            &mut state,
            MessageCreateFixture {
                message_id: Id::new(1),
                content: Some("photo".to_owned()),
                attachments: vec![attachment],
                ..guild_message_create_fixture()
            },
        );

        let target = visible_image_preview_targets(&state, layout(12))
            .into_iter()
            .next()
            .expect("image attachment should produce preview target");

        assert_eq!(target.url, expected, "{proxy_url}");
    }
}
