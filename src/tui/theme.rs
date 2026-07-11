#[cfg(test)]
use std::cell::RefCell;
use std::sync::OnceLock;

use ratatui::{
    style::{Color, Modifier, Style},
    symbols::border,
    widgets::BorderType,
};

use crate::config::{
    BorderShape, BorderShapeOptions, HighlightDefinitionOptions, HighlightLinkOptions, ThemeOptions,
};
pub(super) use crate::config::{BorderSurface, HighlightGroup};

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Theme {
    highlights: [ResolvedHighlight; HighlightGroup::COUNT],
    borders: [BorderType; BorderSurface::COUNT],
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ThemeAccessor;

#[derive(Clone, Copy, Debug, Default)]
struct HighlightDefinition {
    link: Option<HighlightGroup>,
    style: Style,
    clear_foreground: bool,
    clear_background: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ResolvedHighlight {
    style: Style,
    clear_background: bool,
}

#[derive(Clone, Copy, Debug)]
enum ColorOverride {
    Set(Color),
    Clear,
}

#[derive(Clone, Copy, Debug, Default)]
struct HighlightPatch {
    style: Style,
    foreground: Option<ColorOverride>,
    background: Option<ColorOverride>,
}

impl Default for Theme {
    fn default() -> Self {
        Self::from_options(&ThemeOptions::default(), &mut Vec::new())
    }
}

impl Theme {
    pub(super) fn from_options(options: &ThemeOptions, warnings: &mut Vec<String>) -> Self {
        let mut definitions = default_definitions();
        apply_highlight_overrides(&mut definitions, options, warnings);
        Self {
            highlights: resolve_definitions(&definitions, warnings),
            borders: resolve_border_types(options.border_shapes()),
        }
    }

    pub(super) fn style(&self, group: HighlightGroup) -> Style {
        self.highlights[group as usize].style
    }

    pub(super) fn background_is_cleared(&self, group: HighlightGroup) -> bool {
        self.highlights[group as usize].clear_background
    }

    #[cfg(test)]
    pub(super) fn apply(&self, group: HighlightGroup, base: Style) -> Style {
        base.patch(self.style(group))
    }

    #[cfg(test)]
    pub(super) fn foreground(&self, group: HighlightGroup) -> Color {
        self.style(group).fg.unwrap_or(Color::Reset)
    }

    pub(super) const fn border_type(&self, surface: BorderSurface) -> BorderType {
        self.borders[surface as usize]
    }

    #[cfg(test)]
    pub(super) fn border_set(&self, surface: BorderSurface) -> border::Set<'static> {
        self.border_type(surface).to_border_set()
    }

    #[cfg(test)]
    pub(super) fn with_style(mut self, group: HighlightGroup, style: Style) -> Self {
        self.highlights[group as usize] = ResolvedHighlight {
            style,
            ..ResolvedHighlight::default()
        };
        self
    }

    #[cfg(test)]
    pub(super) fn with_border_type(
        mut self,
        surface: BorderSurface,
        border_type: BorderType,
    ) -> Self {
        self.borders[surface as usize] = border_type;
        self
    }
}

impl ThemeAccessor {
    pub(super) fn style(self, group: HighlightGroup) -> Style {
        read_current_theme(|theme| theme.style(group))
    }

    pub(super) fn apply(self, group: HighlightGroup, base: Style) -> Style {
        base.patch(self.style(group))
    }

    #[cfg(test)]
    pub(super) fn foreground(self, group: HighlightGroup) -> Color {
        self.style(group).fg.unwrap_or(Color::Reset)
    }

    #[cfg(test)]
    pub(super) fn background(self, group: HighlightGroup) -> Color {
        self.style(group).bg.unwrap_or(Color::Reset)
    }

    pub(super) fn background_is_cleared(self, group: HighlightGroup) -> bool {
        read_current_theme(|theme| theme.background_is_cleared(group))
    }

    pub(super) fn border_type(self, surface: BorderSurface) -> BorderType {
        read_current_theme(|theme| theme.border_type(surface))
    }

    pub(super) fn border_set(self, surface: BorderSurface) -> border::Set<'static> {
        self.border_type(surface).to_border_set()
    }
}

fn resolve_border_types(options: &BorderShapeOptions) -> [BorderType; BorderSurface::COUNT] {
    let configured_default = options.default.map(border_type);
    let default = configured_default.unwrap_or(BorderType::Plain);
    std::array::from_fn(|index| {
        let surface = BorderSurface::ALL[index];
        options.get(surface).map(border_type).unwrap_or_else(|| {
            if surface.rounded_by_default() {
                configured_default.unwrap_or(BorderType::Rounded)
            } else {
                default
            }
        })
    })
}

const fn border_type(shape: BorderShape) -> BorderType {
    match shape {
        BorderShape::Plain => BorderType::Plain,
        BorderShape::Rounded => BorderType::Rounded,
        BorderShape::Double => BorderType::Double,
        BorderShape::Thick => BorderType::Thick,
        BorderShape::LightDoubleDashed => BorderType::LightDoubleDashed,
        BorderShape::HeavyDoubleDashed => BorderType::HeavyDoubleDashed,
        BorderShape::LightTripleDashed => BorderType::LightTripleDashed,
        BorderShape::HeavyTripleDashed => BorderType::HeavyTripleDashed,
        BorderShape::LightQuadrupleDashed => BorderType::LightQuadrupleDashed,
        BorderShape::HeavyQuadrupleDashed => BorderType::HeavyQuadrupleDashed,
        BorderShape::QuadrantInside => BorderType::QuadrantInside,
        BorderShape::QuadrantOutside => BorderType::QuadrantOutside,
    }
}

fn default_definitions() -> [HighlightDefinition; HighlightGroup::COUNT] {
    use HighlightGroup as H;

    let mut definitions = [None; HighlightGroup::COUNT];
    let mut define = |group, link, style| {
        let slot = &mut definitions[group as usize];
        assert!(slot.is_none(), "highlight group has one default definition");
        *slot = Some(HighlightDefinition {
            link,
            style,
            clear_foreground: false,
            clear_background: false,
        });
    };

    define(
        H::Normal,
        None,
        Style::default().fg(Color::Reset).bg(Color::Reset),
    );
    define(
        H::Strong,
        None,
        Style::default().add_modifier(Modifier::BOLD),
    );
    define(
        H::Emphasis,
        None,
        Style::default().add_modifier(Modifier::ITALIC),
    );
    define(H::Muted, None, Style::default().add_modifier(Modifier::DIM));
    define(H::Title, Some(H::Strong), Style::default());
    define(H::Heading, Some(H::Strong), Style::default());
    define(H::Decoration, Some(H::Muted), Style::default());
    define(H::Hint, Some(H::Muted), Style::default());
    define(H::Description, Some(H::Muted), Style::default());
    define(H::Shortcut, Some(H::Muted), Style::default());
    define(H::Activity, Some(H::Muted), Style::default());
    define(H::ChannelTypeMarker, Some(H::Muted), Style::default());
    define(H::FieldLabel, Some(H::Muted), Style::default());
    define(H::SearchContext, Some(H::Muted), Style::default());
    define(H::Timestamp, Some(H::Muted), Style::default());
    define(H::Placeholder, Some(H::Muted), Style::default());
    define(H::Disabled, Some(H::Muted), Style::default());
    define(H::Loading, Some(H::Muted), Style::default());
    define(
        H::Edited,
        Some(H::Muted),
        Style::default().add_modifier(Modifier::ITALIC),
    );
    define(
        H::Unavailable,
        Some(H::Muted),
        Style::default().add_modifier(Modifier::CROSSED_OUT),
    );
    define(
        H::LoginTitle,
        Some(H::Title),
        Style::default().fg(Color::Cyan),
    );
    define(H::LoginHint, Some(H::Muted), Style::default());
    define(H::PaneTitle, Some(H::Title), Style::default());
    define(H::ModalTitle, Some(H::Title), Style::default());
    define(H::ComposerTitle, Some(H::Title), Style::default());
    define(
        H::HeaderTitle,
        Some(H::Title),
        Style::default().fg(Color::Cyan),
    );
    define(H::HeaderLabel, Some(H::Muted), Style::default());
    define(H::MessageAuthor, Some(H::Strong), Style::default());
    define(H::MessageTimestamp, Some(H::Timestamp), Style::default());
    define(H::CategoryHeading, Some(H::Heading), Style::default());
    define(H::MemberGroupHeading, Some(H::Heading), Style::default());
    define(H::MessageSecondary, Some(H::Muted), Style::default());
    define(H::ForumSecondary, Some(H::Muted), Style::default());
    define(H::EmbedAuthor, Some(H::Emphasis), Style::default());
    define(
        H::EmbedTitle,
        Some(H::Strong),
        Style::default().fg(Color::Blue),
    );
    define(
        H::EmbedFieldName,
        Some(H::Strong),
        Style::default().add_modifier(Modifier::UNDERLINED),
    );
    define(
        H::EmbedFooter,
        Some(H::Muted),
        Style::default().add_modifier(Modifier::ITALIC),
    );
    define(
        H::CodeBlockBorder,
        Some(H::Border),
        Style::default().add_modifier(Modifier::DIM),
    );
    define(
        H::ScrollbarTrack,
        Some(H::ScrollbarThumb),
        Style::default().add_modifier(Modifier::DIM),
    );
    define(H::UnavailableEmoji, Some(H::Unavailable), Style::default());
    define(
        H::HeaderError,
        Some(H::Error),
        Style::default().add_modifier(Modifier::BOLD),
    );
    define(
        H::HeaderWarning,
        Some(H::Warning),
        Style::default().add_modifier(Modifier::BOLD),
    );
    define(H::Border, None, Style::default().fg(Color::DarkGray));
    define(H::FocusBorder, None, Style::default().fg(Color::Cyan));
    define(
        H::Selection,
        None,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
            .remove_modifier(Modifier::DIM),
    );
    define(
        H::SelectionBorder,
        None,
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    );

    define(H::PaneBorder, Some(H::Border), Style::default());
    define(
        H::FocusedPaneBorder,
        Some(H::FocusBorder),
        Style::default().add_modifier(Modifier::BOLD),
    );
    define(H::LoginBorder, Some(H::FocusBorder), Style::default());
    define(H::ComposerBorder, Some(H::Border), Style::default());
    define(
        H::ActiveComposerBorder,
        Some(H::FocusBorder),
        Style::default().add_modifier(Modifier::BOLD),
    );
    define(
        H::ModalBorder,
        Some(H::FocusBorder),
        Style::default().add_modifier(Modifier::BOLD),
    );
    define(
        H::ComposerPickerBorder,
        Some(H::FocusBorder),
        Style::default(),
    );
    define(H::SelectedRow, Some(H::Selection), Style::default());
    define(H::SelectionMarker, Some(H::Selection), Style::default());
    define(
        H::ActiveField,
        None,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    define(H::ActiveTab, Some(H::Selection), Style::default());
    define(
        H::MessageSelectedBorder,
        Some(H::SelectionBorder),
        Style::default(),
    );
    define(H::ForumBorder, Some(H::FocusBorder), Style::default());
    define(
        H::ForumSelectedBorder,
        Some(H::SelectionBorder),
        Style::default(),
    );

    define(
        H::ScrollbarThumb,
        None,
        Style::default().fg(Color::Rgb(170, 170, 170)),
    );
    define(
        H::UnreadNotice,
        None,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    define(H::Editing, None, Style::default().fg(Color::Yellow));
    define(H::Reaction, None, Style::default().fg(Color::Cyan));
    define(H::SelfReaction, None, Style::default().fg(Color::Yellow));
    define(H::PresenceOnline, None, Style::default().fg(Color::Green));
    define(
        H::PresenceIdle,
        None,
        Style::default().fg(Color::Rgb(180, 140, 0)),
    );
    define(H::PresenceDnd, None, Style::default().fg(Color::Red));
    define(
        H::PresenceOffline,
        Some(H::Normal),
        Style::default().add_modifier(Modifier::DIM),
    );
    define(H::VoiceDisabled, None, Style::default().fg(Color::Yellow));
    define(
        H::VoiceConnection,
        None,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    define(H::FolderFallback, None, Style::default().fg(Color::Cyan));
    define(
        H::NavigationActive,
        None,
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    );
    define(
        H::NavigationMentioned,
        None,
        Style::default().fg(Color::Rgb(255, 165, 0)),
    );
    define(
        H::NavigationNotified,
        None,
        Style::default().fg(Color::Reset),
    );
    define(H::NavigationUnread, None, Style::default().fg(Color::Reset));
    define(
        H::MentionBadge,
        None,
        Style::default().fg(Color::Rgb(255, 165, 0)),
    );
    define(
        H::NotificationBadge,
        None,
        Style::default().fg(Color::Reset),
    );
    define(
        H::JoinedVoiceChannel,
        None,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    define(
        H::VoiceSpeaking,
        None,
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    );
    define(H::ReplyPingEnabled, None, Style::default().fg(Color::Cyan));
    define(H::Tag, None, Style::default().fg(Color::Cyan));
    define(
        H::RelationshipFriend,
        None,
        Style::default().fg(Color::Green),
    );
    define(
        H::RelationshipIncoming,
        None,
        Style::default().fg(Color::Yellow),
    );
    define(
        H::RelationshipOutgoing,
        None,
        Style::default().fg(Color::Yellow),
    );
    define(
        H::RelationshipBlocked,
        None,
        Style::default().fg(Color::Red),
    );
    define(
        H::RelationshipNone,
        None,
        Style::default()
            .fg(Color::Reset)
            .add_modifier(Modifier::DIM),
    );
    define(H::GaugeFill, None, Style::default().fg(Color::Cyan));
    define(H::MessageBody, None, Style::default().fg(Color::Reset));
    define(
        H::MarkdownHeading1,
        Some(H::Heading),
        Style::default().fg(Color::Cyan),
    );
    define(
        H::MarkdownHeading2,
        Some(H::Heading),
        Style::default().add_modifier(Modifier::UNDERLINED),
    );
    define(H::MarkdownHeading3, Some(H::Heading), Style::default());
    define(H::MarkdownQuote, None, Style::default().fg(Color::DarkGray));
    define(
        H::MarkdownMarker,
        None,
        Style::default().fg(Color::DarkGray),
    );
    define(H::MessageAttachment, None, Style::default().fg(Color::Cyan));
    define(
        H::ImageOverflow,
        Some(H::MessageAttachment),
        Style::default().add_modifier(Modifier::BOLD),
    );
    define(
        H::InlineCode,
        None,
        Style::default().fg(Color::Rgb(255, 165, 0)),
    );
    define(
        H::MessageLink,
        None,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::UNDERLINED),
    );
    define(
        H::MentionSelf,
        None,
        Style::default()
            .fg(Color::Yellow)
            .bg(Color::Rgb(92, 76, 35)),
    );
    define(
        H::MentionOther,
        None,
        Style::default()
            .fg(Color::Rgb(193, 206, 247))
            .bg(Color::Rgb(40, 50, 92)),
    );
    define(H::MentionRole, None, Style::default());
    define(
        H::MentionPickerRole,
        None,
        Style::default().fg(Color::Magenta),
    );
    define(H::EmbedGutter, None, Style::default().fg(Color::Red));
    define(
        H::EmbedLink,
        None,
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::UNDERLINED),
    );
    define(
        H::CommandName,
        Some(H::MessageSecondary),
        Style::default().fg(Color::Rgb(88, 101, 242)),
    );
    define(
        H::SystemThreadName,
        None,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    define(
        H::PollAnswerSelected,
        None,
        Style::default()
            .fg(Color::Reset)
            .add_modifier(Modifier::BOLD),
    );
    define(
        H::PollWinner,
        None,
        Style::default()
            .fg(Color::Reset)
            .add_modifier(Modifier::BOLD),
    );
    define(
        H::UnreadBanner,
        None,
        Style::default()
            .fg(Color::Reset)
            .bg(Color::Rgb(88, 101, 242)),
    );
    define(
        H::UnreadDivider,
        None,
        Style::default().fg(Color::Rgb(237, 66, 69)),
    );
    define(
        H::ForumPinnedBadge,
        None,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    define(
        H::BotBadge,
        Some(H::Normal),
        Style::default()
            .bg(Color::Rgb(88, 101, 242))
            .add_modifier(Modifier::BOLD),
    );
    define(H::Error, None, Style::default().fg(Color::Red));
    define(H::Warning, None, Style::default().fg(Color::Yellow));
    define(H::Success, None, Style::default().fg(Color::Green));
    define(H::Info, None, Style::default().fg(Color::Cyan));

    definitions[H::Selection as usize]
        .as_mut()
        .expect("selection highlight is defined")
        .clear_background = true;

    assert!(
        definitions.iter().all(Option::is_some),
        "every highlight group has a default definition"
    );
    definitions.map(|definition| definition.expect("highlight definition is complete"))
}

fn apply_highlight_overrides(
    definitions: &mut [HighlightDefinition; HighlightGroup::COUNT],
    options: &ThemeOptions,
    warnings: &mut Vec<String>,
) {
    for (&group, override_options) in options.highlights() {
        let definition = &mut definitions[group as usize];
        if let Some(link) = override_options.link {
            definition.link = match link {
                HighlightLinkOptions::Inherit(group) => Some(group),
                HighlightLinkOptions::Detached => None,
            };
        }
        let patch = highlight_patch(group, override_options, warnings);
        definition.style = definition.style.patch(patch.style);
        apply_color_override(
            &mut definition.style.fg,
            &mut definition.clear_foreground,
            patch.foreground,
        );
        apply_color_override(
            &mut definition.style.bg,
            &mut definition.clear_background,
            patch.background,
        );
    }
}

fn apply_color_override(
    channel: &mut Option<Color>,
    clear: &mut bool,
    override_value: Option<ColorOverride>,
) {
    match override_value {
        Some(ColorOverride::Set(color)) => {
            *channel = Some(color);
            *clear = false;
        }
        Some(ColorOverride::Clear) => {
            *channel = None;
            *clear = true;
        }
        None => {}
    }
}

fn highlight_patch(
    group: HighlightGroup,
    options: &HighlightDefinitionOptions,
    warnings: &mut Vec<String>,
) -> HighlightPatch {
    let mut patch = HighlightPatch::default();
    if let Some(raw) = options.foreground.as_deref() {
        patch.foreground = parse_color_override(group, "foreground", raw, warnings);
    }
    if let Some(raw) = options.background.as_deref() {
        patch.background = parse_color_override(group, "background", raw, warnings);
    }
    for (enabled, modifier) in [
        (options.bold, Modifier::BOLD),
        (options.italic, Modifier::ITALIC),
        (options.dim, Modifier::DIM),
        (options.underline, Modifier::UNDERLINED),
        (options.strikethrough, Modifier::CROSSED_OUT),
    ] {
        if let Some(enabled) = enabled {
            patch.style = if enabled {
                patch.style.add_modifier(modifier)
            } else {
                patch.style.remove_modifier(modifier)
            };
        }
    }
    patch
}

fn parse_color_override(
    group: HighlightGroup,
    field: &str,
    raw: &str,
    warnings: &mut Vec<String>,
) -> Option<ColorOverride> {
    if raw == "none" {
        return Some(ColorOverride::Clear);
    }
    match parse_color(raw) {
        Some(color) => Some(ColorOverride::Set(color)),
        None => {
            warnings.push(invalid_highlight_color(group, field, raw));
            None
        }
    }
}

fn invalid_highlight_color(group: HighlightGroup, field: &str, raw: &str) -> String {
    format!(
        "[highlight.{}] {field} = \"{raw}\" is not a supported color and was ignored",
        group.name()
    )
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ResolveState {
    Unvisited,
    Visiting,
    Resolved,
}

fn resolve_definitions(
    definitions: &[HighlightDefinition; HighlightGroup::COUNT],
    warnings: &mut Vec<String>,
) -> [ResolvedHighlight; HighlightGroup::COUNT] {
    let mut highlights = [ResolvedHighlight::default(); HighlightGroup::COUNT];
    let mut states = [ResolveState::Unvisited; HighlightGroup::COUNT];
    for &group in HighlightGroup::ALL {
        let _ = resolve_group(group, definitions, &mut highlights, &mut states, warnings);
    }
    highlights
}

fn resolve_group(
    group: HighlightGroup,
    definitions: &[HighlightDefinition; HighlightGroup::COUNT],
    highlights: &mut [ResolvedHighlight; HighlightGroup::COUNT],
    states: &mut [ResolveState; HighlightGroup::COUNT],
    warnings: &mut Vec<String>,
) -> Result<ResolvedHighlight, ()> {
    let index = group as usize;
    match states[index] {
        ResolveState::Resolved => return Ok(highlights[index]),
        ResolveState::Visiting => {
            warnings.push(format!(
                "[highlight.{}] link cycle detected; cyclic inheritance was ignored",
                group.name()
            ));
            return Err(());
        }
        ResolveState::Unvisited => {}
    }

    states[index] = ResolveState::Visiting;
    let definition = definitions[index];
    let inherited = match definition.link {
        Some(link) => match resolve_group(link, definitions, highlights, states, warnings) {
            Ok(highlight) => highlight,
            Err(()) => {
                highlights[index] = resolve_definition(ResolvedHighlight::default(), definition);
                states[index] = ResolveState::Resolved;
                return Err(());
            }
        },
        None => ResolvedHighlight::default(),
    };
    let resolved = resolve_definition(inherited, definition);
    highlights[index] = resolved;
    states[index] = ResolveState::Resolved;
    Ok(resolved)
}

fn resolve_definition(
    inherited: ResolvedHighlight,
    definition: HighlightDefinition,
) -> ResolvedHighlight {
    let mut resolved = ResolvedHighlight {
        style: inherited.style.patch(definition.style),
        ..inherited
    };
    if definition.style.bg.is_some() {
        resolved.clear_background = false;
    }
    if definition.clear_foreground {
        resolved.style.fg = None;
    }
    if definition.clear_background {
        resolved.style.bg = None;
        resolved.clear_background = true;
    }
    resolved
}

fn parse_color(value: &str) -> Option<Color> {
    match value {
        "terminal_default" => Some(Color::Reset),
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "gray" => Some(Color::Gray),
        "dark_gray" => Some(Color::DarkGray),
        "light_red" => Some(Color::LightRed),
        "light_green" => Some(Color::LightGreen),
        "light_yellow" => Some(Color::LightYellow),
        "light_blue" => Some(Color::LightBlue),
        "light_magenta" => Some(Color::LightMagenta),
        "light_cyan" => Some(Color::LightCyan),
        "white" => Some(Color::White),
        raw => parse_hex_color(raw),
    }
}

fn parse_hex_color(value: &str) -> Option<Color> {
    let hex = match value.strip_prefix('#') {
        Some(hex) => hex,
        None => value,
    };
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let channel = |range| u8::from_str_radix(&hex[range], 16).ok();
    Some(Color::Rgb(channel(0..2)?, channel(2..4)?, channel(4..6)?))
}

static THEME: OnceLock<Theme> = OnceLock::new();
static DEFAULT_THEME: OnceLock<Theme> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static TEST_THEME: RefCell<Option<Theme>> = const { RefCell::new(None) };
}

pub(super) fn init(theme: Theme) {
    let _ = THEME.set(theme);
}

pub(super) const fn current() -> ThemeAccessor {
    ThemeAccessor
}

fn read_current_theme<T>(read: impl Fn(&Theme) -> T) -> T {
    #[cfg(test)]
    if let Some(value) = TEST_THEME.with(|slot| {
        let slot = slot.borrow();
        slot.as_ref().map(&read)
    }) {
        return value;
    }

    let theme = THEME
        .get()
        .unwrap_or_else(|| DEFAULT_THEME.get_or_init(Theme::default));
    read(theme)
}

#[cfg(test)]
pub(super) fn with_test_theme<T>(theme: Theme, test: impl FnOnce() -> T) -> T {
    struct RestoreTheme(Option<Theme>);

    impl Drop for RestoreTheme {
        fn drop(&mut self) {
            let previous = self.0.take();
            TEST_THEME.with(|slot| *slot.borrow_mut() = previous);
        }
    }

    let previous = TEST_THEME.with(|slot| slot.borrow_mut().replace(theme));
    let restore = RestoreTheme(previous);
    let result = test();
    drop(restore);
    result
}

#[cfg(test)]
mod tests {
    use super::{
        BorderShape, BorderSurface, BorderType, Color, HighlightGroup, HighlightLinkOptions,
        Modifier, Style, Theme, ThemeOptions, parse_color,
    };

    #[test]
    fn readme_default_theme_matches_builtin_theme() {
        let readme = include_str!("../../README.md");
        let (_, after_start) = readme
            .split_once("<!-- default-theme-config:start -->")
            .expect("README should contain the default theme start marker");
        let (block, _) = after_start
            .split_once("<!-- default-theme-config:end -->")
            .expect("README should contain the default theme end marker");
        let content = block
            .trim()
            .strip_prefix("```toml\n")
            .and_then(|content| content.strip_suffix("\n```"))
            .expect("default theme should be a TOML code block");
        let documented_groups = content
            .lines()
            .filter_map(|line| {
                line.strip_prefix("[highlight.")
                    .and_then(|name| name.strip_suffix(']'))
            })
            .collect::<Vec<_>>();
        assert_eq!(documented_groups.len(), HighlightGroup::COUNT);
        for group in HighlightGroup::ALL {
            assert!(
                documented_groups.contains(&group.name()),
                "README should document {}",
                group.name()
            );
        }

        let (options, parse_warnings) = crate::config::parse_theme_options_for_test(content)
            .expect("README default theme should parse");
        assert!(parse_warnings.is_empty());

        let mut resolution_warnings = Vec::new();
        let documented = Theme::from_options(&options, &mut resolution_warnings);
        assert!(resolution_warnings.is_empty());
        assert_eq!(documented, Theme::default());
    }

    #[test]
    fn from_options_falls_back_only_for_the_invalid_field() {
        let cases = [
            ("terminal_default", Color::Reset),
            ("black", Color::Black),
            ("red", Color::Red),
            ("green", Color::Green),
            ("yellow", Color::Yellow),
            ("blue", Color::Blue),
            ("magenta", Color::Magenta),
            ("cyan", Color::Cyan),
            ("gray", Color::Gray),
            ("dark_gray", Color::DarkGray),
            ("light_red", Color::LightRed),
            ("light_green", Color::LightGreen),
            ("light_yellow", Color::LightYellow),
            ("light_blue", Color::LightBlue),
            ("light_magenta", Color::LightMagenta),
            ("light_cyan", Color::LightCyan),
            ("white", Color::White),
            ("#112233", Color::Rgb(0x11, 0x22, 0x33)),
            ("AABBCC", Color::Rgb(0xAA, 0xBB, 0xCC)),
        ];

        for (raw, expected) in cases {
            assert_eq!(parse_color(raw), Some(expected), "color {raw}");
        }
        for raw in [
            "reset",
            "darkgray",
            "bright_red",
            "#1234",
            "#gggggg",
            "aéabc",
            "",
        ] {
            assert_eq!(parse_color(raw), None, "color {raw}");
        }

        let defaults = Theme::default();
        assert_eq!(defaults.style(HighlightGroup::Selection).bg, None);
        assert!(defaults.background_is_cleared(HighlightGroup::Selection));
        assert!(defaults.background_is_cleared(HighlightGroup::SelectedRow));
        assert_eq!(defaults.border_type(BorderSurface::Pane), BorderType::Plain);
        assert_eq!(
            defaults.border_type(BorderSurface::Composer),
            BorderType::Rounded
        );
        assert_eq!(
            defaults.border_type(BorderSurface::Message),
            BorderType::Rounded
        );
        assert_eq!(
            defaults.border_type(BorderSurface::Forum),
            BorderType::Rounded
        );
        assert_eq!(
            defaults.style(HighlightGroup::UnreadBanner),
            Style::default()
                .fg(Color::Reset)
                .bg(Color::Rgb(88, 101, 242))
        );
        assert_eq!(
            defaults.style(HighlightGroup::ForumBorder).fg,
            Some(Color::Cyan)
        );
        assert_eq!(
            defaults.style(HighlightGroup::EmbedLink).fg,
            Some(Color::Blue)
        );

        let mut options = ThemeOptions::default();
        options.highlight_mut(HighlightGroup::Border).foreground = Some("not-a-color".to_owned());
        options
            .highlight_mut(HighlightGroup::FocusBorder)
            .foreground = Some("#778899".to_owned());
        options.highlight_mut(HighlightGroup::Selection).background = Some("none".to_owned());
        options
            .highlight_mut(HighlightGroup::MentionRole)
            .background = Some("none".to_owned());
        options
            .highlight_mut(HighlightGroup::MessageLink)
            .foreground = Some("#112233".to_owned());
        options
            .highlight_mut(HighlightGroup::MessageAttachment)
            .foreground = Some("#445566".to_owned());
        options.highlight_mut(HighlightGroup::Normal).foreground = Some("magenta".to_owned());
        options
            .highlight_mut(HighlightGroup::ComposerPickerBorder)
            .italic = Some(true);
        options
            .highlight_mut(HighlightGroup::ComposerPickerBorder)
            .foreground = Some("none".to_owned());
        options.highlight_mut(HighlightGroup::Selection).bold = Some(false);
        options.highlight_mut(HighlightGroup::Strong).bold = Some(false);
        options
            .highlight_mut(HighlightGroup::Unavailable)
            .strikethrough = Some(false);
        options.highlight_mut(HighlightGroup::PaneBorder).link = Some(
            HighlightLinkOptions::Inherit(HighlightGroup::ComposerBorder),
        );
        options.highlight_mut(HighlightGroup::PaneBorder).foreground = Some("red".to_owned());
        options.highlight_mut(HighlightGroup::ComposerBorder).link =
            Some(HighlightLinkOptions::Inherit(HighlightGroup::PaneBorder));
        options
            .highlight_mut(HighlightGroup::ComposerBorder)
            .background = Some("blue".to_owned());
        options.border_shapes_mut().default = Some(BorderShape::Double);
        options
            .border_shapes_mut()
            .set(BorderSurface::Modal, BorderShape::Thick);
        options
            .border_shapes_mut()
            .set(BorderSurface::Forum, BorderShape::HeavyDoubleDashed);
        let mut warnings = Vec::new();

        let theme = Theme::from_options(&options, &mut warnings);

        assert_eq!(theme.foreground(HighlightGroup::Border), Color::DarkGray);
        assert_eq!(
            theme.foreground(HighlightGroup::FocusBorder),
            Color::Rgb(0x77, 0x88, 0x99)
        );
        assert_eq!(theme.style(HighlightGroup::Selection).bg, None);
        assert_eq!(theme.style(HighlightGroup::SelectedRow).bg, None);
        assert!(theme.background_is_cleared(HighlightGroup::Selection));
        assert!(theme.background_is_cleared(HighlightGroup::SelectedRow));
        assert!(theme.background_is_cleared(HighlightGroup::MentionRole));
        assert_eq!(
            theme.foreground(HighlightGroup::MessageLink),
            Color::Rgb(0x11, 0x22, 0x33)
        );
        assert_eq!(
            theme.foreground(HighlightGroup::MessageAttachment),
            Color::Rgb(0x44, 0x55, 0x66)
        );
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].contains("[highlight.Border]"));
        assert!(warnings[1].contains("link cycle"));

        let linked = theme.style(HighlightGroup::ImageOverflow);
        assert_eq!(linked.fg, Some(Color::Rgb(0x44, 0x55, 0x66)));
        assert_eq!(
            theme.style(HighlightGroup::BotBadge).fg,
            Some(Color::Magenta)
        );
        assert_eq!(
            theme.style(HighlightGroup::PresenceOffline).fg,
            Some(Color::Magenta)
        );
        assert_eq!(
            theme.style(HighlightGroup::MentionPickerRole).fg,
            Some(Color::Magenta)
        );
        assert_eq!(
            theme.style(HighlightGroup::EmbedGutter).fg,
            Some(Color::Red)
        );
        let picker = theme.style(HighlightGroup::ComposerPickerBorder);
        assert_eq!(picker.fg, None);
        assert!(picker.add_modifier.contains(Modifier::ITALIC));
        assert!(
            !theme
                .style(HighlightGroup::Selection)
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            !theme
                .style(HighlightGroup::PaneTitle)
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            !theme
                .style(HighlightGroup::UnavailableEmoji)
                .add_modifier
                .contains(Modifier::CROSSED_OUT)
        );
        assert_eq!(
            theme.apply(HighlightGroup::Border, Style::default()).fg,
            Some(Color::DarkGray)
        );
        assert_eq!(theme.style(HighlightGroup::PaneBorder).fg, Some(Color::Red));
        assert_eq!(theme.style(HighlightGroup::PaneBorder).bg, None);
        assert_eq!(theme.style(HighlightGroup::ComposerBorder).fg, None);
        assert_eq!(
            theme.style(HighlightGroup::ComposerBorder).bg,
            Some(Color::Blue)
        );
        assert_eq!(theme.border_type(BorderSurface::Pane), BorderType::Double);
        assert_eq!(theme.border_type(BorderSurface::Modal), BorderType::Thick);
        assert_eq!(theme.border_type(BorderSurface::Picker), BorderType::Double);
        assert_eq!(theme.border_type(BorderSurface::Login), BorderType::Double);
        assert_eq!(
            theme.border_type(BorderSurface::Composer),
            BorderType::Double
        );
        assert_eq!(
            theme.border_type(BorderSurface::Message),
            BorderType::Double
        );
        assert_eq!(
            theme.border_type(BorderSurface::Forum),
            BorderType::HeavyDoubleDashed
        );
        assert_eq!(theme.border_set(BorderSurface::Message).top_left, "╔");
        assert_eq!(theme.border_set(BorderSurface::Forum).horizontal_top, "╍");
    }
}
