use std::fmt::{self, Write};

const CUSTOM_EMOJI_CDN_BASE: &str = "https://cdn.discordapp.com/emojis";
// Twemoji graphics are licensed under CC-BY 4.0. README.md contains the
// required attribution alongside the user-facing third-party asset notice.
const TWEMOJI_CDN_BASE: &str = "https://cdn.jsdelivr.net/gh/jdecked/twemoji@17.0.3/assets/72x72";

/// Builds the CDN URL Discord documents for a custom emoji.
///
/// Animated emoji may originate as WebP or AVIF and Discord does not convert
/// those uploads to GIF. Requesting animated WebP works for every supported
/// source format and matches the format used by Discord's own client.
pub(crate) fn custom_emoji_image_url(id: impl fmt::Display, animated: bool) -> String {
    if animated {
        format!("{CUSTOM_EMOJI_CDN_BASE}/{id}.webp?animated=true")
    } else {
        format!("{CUSTOM_EMOJI_CDN_BASE}/{id}.png")
    }
}

/// Maps an RGI Unicode emoji to the matching pinned Twemoji PNG.
///
/// Twemoji omits variation selectors from non-ZWJ filenames, while selectors
/// inside ZWJ sequences remain part of the asset name. `emojis::get` also
/// canonicalizes accepted text-presentation variants before the path is built.
pub(crate) fn unicode_emoji_image_url(value: &str) -> Option<String> {
    let emoji = emojis::get(value)?;
    let canonical = emoji.as_str();
    let keep_variation_selectors = canonical.contains('\u{200d}');
    let mut codepoints = String::new();
    for codepoint in canonical
        .chars()
        .filter(|codepoint| keep_variation_selectors || *codepoint != '\u{fe0f}')
    {
        if !codepoints.is_empty() {
            codepoints.push('-');
        }
        write!(&mut codepoints, "{:x}", u32::from(codepoint))
            .expect("writing to a String cannot fail");
    }

    (!codepoints.is_empty()).then(|| format!("{TWEMOJI_CDN_BASE}/{codepoints}.png"))
}

#[cfg(test)]
mod tests {
    use super::unicode_emoji_image_url;

    #[test]
    fn unicode_emoji_urls_follow_twemoji_sequence_names() {
        for (emoji, filename) in [
            ("😀", "1f600.png"),
            ("❤️", "2764.png"),
            ("👍🏽", "1f44d-1f3fd.png"),
            ("👨‍❤️‍👨", "1f468-200d-2764-fe0f-200d-1f468.png"),
            ("🇰🇷", "1f1f0-1f1f7.png"),
            ("#️⃣", "23-20e3.png"),
        ] {
            let expected = format!(
                "https://cdn.jsdelivr.net/gh/jdecked/twemoji@17.0.3/assets/72x72/{filename}"
            );
            assert_eq!(
                unicode_emoji_image_url(emoji).as_deref(),
                Some(expected.as_str()),
                "{emoji}"
            );
        }
        assert!(unicode_emoji_image_url("not emoji").is_none());
    }
}
