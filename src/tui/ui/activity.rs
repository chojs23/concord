use ratatui::text::{Line, Span};

use crate::discord::{ActivityInfo, ActivityKind};
use crate::tui::text::{sanitize_for_display_width, truncate_display_width_from};

use super::{theme, types::EmojiImage};

const ACTIVITY_IMAGE_WIDTH: usize = 2;

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
        compact_activity_line(self.clone(), 0, usize::MAX, 0)
            .line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }
}

pub(super) struct ActivityImagePlacement {
    pub(super) column: usize,
    pub(super) url: String,
}

pub(super) struct CompactActivityLine {
    pub(super) line: Line<'static>,
    pub(super) image: Option<ActivityImagePlacement>,
}

/// Builds the compact activity row shared by list panes.
///
/// Each pane owns the columns before the activity and passes that value as
/// `leading_width`. This function owns the layout after that boundary so icon
/// spacing, the two-cell image placeholder, truncation, and image placement
/// cannot drift between panes.
pub(super) fn compact_activity_line(
    render: ActivityRender,
    leading_width: usize,
    available_width: usize,
    horizontal_scroll: usize,
) -> CompactActivityLine {
    let has_body = !render.body.is_empty();
    let content_prefix_width = match &render.leading {
        ActivityLeading::None => 0,
        ActivityLeading::Icon(_) => 1 + usize::from(has_body),
        ActivityLeading::Image(_) => ACTIVITY_IMAGE_WIDTH,
    };
    let body = truncate_display_width_from(
        &render.body,
        horizontal_scroll,
        available_width
            .saturating_sub(leading_width)
            .saturating_sub(content_prefix_width),
    );
    let leading = Span::raw(" ".repeat(leading_width));
    let body = Span::styled(
        body,
        theme::current().style(theme::HighlightGroup::Activity),
    );

    match render.leading {
        ActivityLeading::Image(url) => CompactActivityLine {
            line: Line::from(vec![
                leading,
                Span::raw(" ".repeat(ACTIVITY_IMAGE_WIDTH)),
                body,
            ]),
            image: Some(ActivityImagePlacement {
                column: leading_width,
                url,
            }),
        },
        ActivityLeading::Icon(icon) => {
            let mut spans = vec![
                leading,
                Span::styled(
                    icon.to_string(),
                    theme::current().style(theme::HighlightGroup::PresenceOnline),
                ),
            ];
            if has_body {
                spans.push(Span::raw(" "));
            }
            spans.push(body);
            CompactActivityLine {
                line: Line::from(spans),
                image: None,
            }
        }
        ActivityLeading::None => CompactActivityLine {
            line: Line::from(vec![leading, body]),
            image: None,
        },
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
                let details = activity.details.as_deref().map(sanitize_for_display_width);
                let state = activity.state.as_deref().map(sanitize_for_display_width);
                match (details.as_deref(), state.as_deref()) {
                    (Some(track), Some(artist)) => format!("{name} - {track} by {artist}"),
                    (Some(track), None) => format!("{name} - {track}"),
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

    let body_text = activity
        .state
        .as_deref()
        .map(sanitize_for_display_width)
        .unwrap_or_default();

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
            let text = if emoji.id.is_some() {
                format!(":{}:", emoji.name)
            } else {
                emoji.name.clone()
            };
            sanitize_for_display_width(&text)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_activity_line_aligns_custom_image_with_icon() {
        let leading_width = 4;
        let available_width = 20;
        let image = compact_activity_line(
            ActivityRender {
                leading: ActivityLeading::Image("https://example.com/emoji.png".to_owned()),
                body: "status".to_owned(),
            },
            leading_width,
            available_width,
            0,
        );
        let icon = compact_activity_line(
            ActivityRender {
                leading: ActivityLeading::Icon('▶'),
                body: "status".to_owned(),
            },
            leading_width,
            available_width,
            0,
        );

        let image_placement = image.image.expect("custom image placement");
        assert_eq!(image_placement.column, leading_width);
        assert_eq!(
            image_placement.url,
            "https://example.com/emoji.png".to_owned()
        );
        assert_eq!(image.line.spans[0].content, icon.line.spans[0].content);
        assert_eq!(image.line.spans[1].content.as_ref(), "  ");
        assert_eq!(image.line.spans[2].content.as_ref(), "status");
        assert_eq!(icon.line.spans[1].content.as_ref(), "▶");
        assert_eq!(icon.line.spans[2].content.as_ref(), " ");
        assert_eq!(icon.line.spans[3].content.as_ref(), "status");
    }
}
