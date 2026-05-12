use crate::discord::{ActivityInfo, ActivityKind};
use crate::tui::format::sanitize_for_display_width;

use super::types::EmojiImage;

/// Glyph rendered at the start of an activity primary line.
#[derive(Clone, Debug)]
pub(super) enum ActivityLeading {
    /// Nothing precedes the body (used by `Competing` / `Unknown` and by
    /// `Custom` when there is no emoji to show).
    None,
    /// A single-char accent rendered in green by callers. The shared rule of
    /// thumb: `▶` Playing, `◉` Streaming, `♪` Listening, `▷` Watching.
    Icon(char),
    /// A custom emoji image that should be overlaid on top of a 2-cell
    /// placeholder. The string is the CDN URL used by both the cache and the
    /// later overlay pass.
    Image(String),
}

#[derive(Clone, Debug)]
pub(super) struct ActivityRender {
    pub(super) leading: ActivityLeading,
    pub(super) body: String,
}

impl ActivityRender {
    pub(super) fn is_empty(&self) -> bool {
        matches!(self.leading, ActivityLeading::None) && self.body.trim().is_empty()
    }

    #[cfg(test)]
    pub(super) fn to_display_string(&self) -> String {
        match &self.leading {
            ActivityLeading::None => self.body.clone(),
            ActivityLeading::Icon(c) if self.body.is_empty() => c.to_string(),
            ActivityLeading::Icon(c) => format!("{c} {}", self.body),
            ActivityLeading::Image(_) if self.body.is_empty() => "  ".to_owned(),
            ActivityLeading::Image(_) => format!("   {}", self.body),
        }
    }
}

pub(super) fn build_activity_render(
    activity: &ActivityInfo,
    emoji_images: &[EmojiImage<'_>],
    compact: bool,
) -> ActivityRender {
    match activity.kind {
        ActivityKind::Custom => build_custom(activity, emoji_images),
        ActivityKind::Playing => ActivityRender {
            leading: ActivityLeading::Icon('▶'),
            body: sanitize_for_display_width(&activity.name),
        },
        ActivityKind::Streaming => ActivityRender {
            leading: ActivityLeading::Icon('◉'),
            body: sanitize_for_display_width(&activity.name),
        },
        ActivityKind::Listening => {
            let name = sanitize_for_display_width(&activity.name);
            let body = if compact {
                match (activity.details.as_deref(), activity.state.as_deref()) {
                    (Some(track), Some(artist)) => format!("{name} — {track} by {artist}"),
                    (Some(track), None) => format!("{name} — {track}"),
                    _ => name,
                }
            } else {
                name
            };
            ActivityRender {
                leading: ActivityLeading::Icon('♪'),
                body,
            }
        }
        ActivityKind::Watching => ActivityRender {
            leading: ActivityLeading::Icon('▷'),
            body: sanitize_for_display_width(&activity.name),
        },
        ActivityKind::Competing => ActivityRender {
            leading: ActivityLeading::None,
            body: format!(
                "Competing in {}",
                sanitize_for_display_width(&activity.name)
            ),
        },
        ActivityKind::Unknown => ActivityRender {
            leading: ActivityLeading::None,
            body: sanitize_for_display_width(&activity.name),
        },
    }
}

fn build_custom(activity: &ActivityInfo, emoji_images: &[EmojiImage<'_>]) -> ActivityRender {
    let image_url = activity
        .emoji
        .as_ref()
        .and_then(|emoji| emoji.image_url())
        .filter(|url| emoji_images.iter().any(|img| img.url == *url));

    let body_text = activity.state.clone().unwrap_or_default();

    if let Some(url) = image_url {
        return ActivityRender {
            leading: ActivityLeading::Image(url),
            body: body_text,
        };
    }

    let emoji_text = activity
        .emoji
        .as_ref()
        .map(|emoji| {
            if emoji.id.is_some() {
                format!(":{}:", emoji.name)
            } else {
                emoji.name.clone()
            }
        })
        .unwrap_or_default();

    let body = match (emoji_text.is_empty(), body_text.is_empty()) {
        (true, true) => String::new(),
        (false, true) => emoji_text,
        (true, false) => body_text,
        (false, false) => format!("{emoji_text} {body_text}"),
    };

    ActivityRender {
        leading: ActivityLeading::None,
        body,
    }
}
