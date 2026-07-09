use std::sync::OnceLock;

use ratatui::style::Color;

use crate::config::ThemeOptions;

#[derive(Clone, Copy, Debug)]
pub(super) struct Theme {
    pub(super) accent: Color,
    pub(super) dim: Color,
    pub(super) border: Color,
    pub(super) background: Color,
    pub(super) text: Color,
    pub(super) active: Color,
    pub(super) panel_title: Color,
    /// Discord's "you were mentioned" orange, `#FFA500`.
    pub(super) mention: Color,
    /// Explicit RGB instead of `Modifier::DIM` so CJK wide characters dim
    /// uniformly with ASCII (most terminals ignore SGR dim on wide glyphs).
    pub(super) read_dim: Color,
    /// Explicit RGB instead of relying on `Modifier::BOLD` alone, which most
    /// monospace fonts can't apply to CJK glyphs.
    pub(super) unread_bright: Color,
    pub(super) scrollbar_thumb: Color,
    pub(super) selected_forum_post_border: Color,
    pub(super) selected_message_border: Color,
    pub(super) success: Color,
    pub(super) warning: Color,
    pub(super) error: Color,
    pub(super) info: Color,
    /// Discord's brand blurple, `#5865F2`.
    pub(super) blurple: Color,
    pub(super) selection_bg: Color,
    /// Discord's unread-divider red, distinct from `error`.
    pub(super) unread_badge: Color,
    pub(super) self_reaction: Color,
    /// Discord's gold "you were mentioned" text highlight background.
    pub(super) mention_self_bg: Color,
    /// Discord's blue "someone else was mentioned" text highlight background.
    pub(super) mention_other_bg: Color,
    pub(super) mention_other_fg: Color,
    pub(super) presence_idle: Color,
    pub(super) mention_role_fallback: Color,
    pub(super) dm_icon: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            accent: Color::Cyan,
            dim: Color::DarkGray,
            border: Color::DarkGray,
            background: Color::Rgb(24, 54, 65),
            text: Color::White,
            active: Color::Green,
            panel_title: Color::Reset,
            mention: Color::Rgb(255, 165, 0),
            read_dim: Color::Rgb(130, 130, 130),
            unread_bright: Color::Reset,
            scrollbar_thumb: Color::Rgb(170, 170, 170),
            selected_forum_post_border: Color::Green,
            selected_message_border: Color::Green,
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            info: Color::Blue,
            blurple: Color::Rgb(88, 101, 242),
            selection_bg: Color::Rgb(40, 45, 90),
            unread_badge: Color::Rgb(237, 66, 69),
            self_reaction: Color::Yellow,
            mention_self_bg: Color::Rgb(92, 76, 35),
            mention_other_bg: Color::Rgb(40, 50, 92),
            mention_other_fg: Color::Rgb(193, 206, 247),
            presence_idle: Color::Rgb(180, 140, 0),
            mention_role_fallback: Color::Magenta,
            dm_icon: Color::Magenta,
        }
    }
}

impl Theme {
    /// Resolves user overrides from `theme.toml` against the built-in
    /// defaults. A field left unset keeps the default silently; a field set
    /// to something that isn't a valid `#rrggbb` string also keeps the
    /// default but is reported in `warnings` instead of failing the theme.
    pub(super) fn from_options(options: &ThemeOptions, warnings: &mut Vec<String>) -> Self {
        let default = Self::default();
        Self {
            accent: resolve("accent", &options.accent, default.accent, warnings),
            dim: resolve("dim", &options.dim, default.dim, warnings),
            border: resolve("border", &options.border, default.border, warnings),
            background: resolve(
                "background",
                &options.background,
                default.background,
                warnings,
            ),
            text: resolve("text", &options.text, default.text, warnings),
            active: resolve("active", &options.active, default.active, warnings),
            panel_title: resolve(
                "panel_title",
                &options.panel_title,
                default.panel_title,
                warnings,
            ),
            mention: resolve("mention", &options.mention, default.mention, warnings),
            read_dim: resolve("read_dim", &options.read_dim, default.read_dim, warnings),
            unread_bright: resolve(
                "unread_bright",
                &options.unread_bright,
                default.unread_bright,
                warnings,
            ),
            scrollbar_thumb: resolve(
                "scrollbar_thumb",
                &options.scrollbar_thumb,
                default.scrollbar_thumb,
                warnings,
            ),
            selected_forum_post_border: resolve(
                "selected_forum_post_border",
                &options.selected_forum_post_border,
                default.selected_forum_post_border,
                warnings,
            ),
            selected_message_border: resolve(
                "selected_message_border",
                &options.selected_message_border,
                default.selected_message_border,
                warnings,
            ),
            success: resolve("success", &options.success, default.success, warnings),
            warning: resolve("warning", &options.warning, default.warning, warnings),
            error: resolve("error", &options.error, default.error, warnings),
            info: resolve("info", &options.info, default.info, warnings),
            blurple: resolve("blurple", &options.blurple, default.blurple, warnings),
            selection_bg: resolve(
                "selection_bg",
                &options.selection_bg,
                default.selection_bg,
                warnings,
            ),
            unread_badge: resolve(
                "unread_badge",
                &options.unread_badge,
                default.unread_badge,
                warnings,
            ),
            self_reaction: resolve(
                "self_reaction",
                &options.self_reaction,
                default.self_reaction,
                warnings,
            ),
            mention_self_bg: resolve(
                "mention_self_bg",
                &options.mention_self_bg,
                default.mention_self_bg,
                warnings,
            ),
            mention_other_bg: resolve(
                "mention_other_bg",
                &options.mention_other_bg,
                default.mention_other_bg,
                warnings,
            ),
            mention_other_fg: resolve(
                "mention_other_fg",
                &options.mention_other_fg,
                default.mention_other_fg,
                warnings,
            ),
            presence_idle: resolve(
                "presence_idle",
                &options.presence_idle,
                default.presence_idle,
                warnings,
            ),
            mention_role_fallback: resolve(
                "mention_role_fallback",
                &options.mention_role_fallback,
                default.mention_role_fallback,
                warnings,
            ),
            dm_icon: resolve("dm_icon", &options.dm_icon, default.dm_icon, warnings),
        }
    }
}

/// Parses `"#rrggbb"` into a `Color::Rgb`. A value that's present but invalid
/// keeps `default` and pushes a warning; an absent value keeps `default`
/// silently since that just means the field wasn't customized.
fn resolve(
    field: &str,
    value: &Option<String>,
    default: Color,
    warnings: &mut Vec<String>,
) -> Color {
    match value.as_deref() {
        None => default,
        Some(raw) => match hex_to_color(raw) {
            Some(color) => color,
            None => {
                warnings.push(format!(
                    "[theme] {field} = \"{raw}\" is not a valid #rrggbb color, using default"
                ));
                default
            }
        },
    }
}

fn hex_to_color(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 {
        return None;
    }
    let channel = |range| u8::from_str_radix(&hex[range], 16).ok();
    Some(Color::Rgb(channel(0..2)?, channel(2..4)?, channel(4..6)?))
}

static THEME: OnceLock<Theme> = OnceLock::new();

pub(super) fn init(theme: Theme) {
    let _ = THEME.set(theme);
}

pub(super) fn current() -> Theme {
    *THEME.get_or_init(Theme::default)
}

#[cfg(test)]
mod tests {
    use super::{Color, Theme, ThemeOptions, hex_to_color};

    #[test]
    fn hex_to_color_accepts_with_or_without_hash() {
        assert_eq!(hex_to_color("#112233"), Some(Color::Rgb(0x11, 0x22, 0x33)));
        assert_eq!(hex_to_color("112233"), Some(Color::Rgb(0x11, 0x22, 0x33)));
    }

    #[test]
    fn hex_to_color_rejects_wrong_length_or_non_hex() {
        assert_eq!(hex_to_color("#1234"), None);
        assert_eq!(hex_to_color("#gggggg"), None);
        assert_eq!(hex_to_color(""), None);
    }

    #[test]
    fn from_options_applies_overrides_and_leaves_the_rest_default() {
        let options = ThemeOptions {
            accent: Some("#112233".to_owned()),
            ..ThemeOptions::default()
        };
        let mut warnings = Vec::new();

        let theme = Theme::from_options(&options, &mut warnings);

        assert_eq!(theme.accent, Color::Rgb(0x11, 0x22, 0x33));
        assert_eq!(theme.dim, Theme::default().dim, "unset field keeps default");
        assert!(warnings.is_empty());
    }

    #[test]
    fn from_options_falls_back_and_warns_on_invalid_hex() {
        let options = ThemeOptions {
            error: Some("not-a-color".to_owned()),
            ..ThemeOptions::default()
        };
        let mut warnings = Vec::new();

        let theme = Theme::from_options(&options, &mut warnings);

        assert_eq!(theme.error, Theme::default().error);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("error"));
    }
}
