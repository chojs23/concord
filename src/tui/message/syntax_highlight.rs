use std::{cell::RefCell, collections::HashMap, sync::LazyLock};

use ratatui::style::{Color, Modifier, Style};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, ThemeSet},
    parsing::SyntaxSet,
};

use crate::logging;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_nonewlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

fn compute_key(lines: &[String], language: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    lines.hash(&mut hasher);
    language.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Default)]
pub(in crate::tui) struct SyntaxHighlightCache {
    state: RefCell<SyntaxHighlightCacheState>,
}

#[derive(Debug, Default)]
struct SyntaxHighlightCacheState {
    entries: HashMap<u64, SyntaxHighlightEntry>,
    tick: u64,
}

#[derive(Debug)]
struct SyntaxHighlightEntry {
    lines: Vec<Vec<(Style, String)>>,
    last_used: u64,
}

const MAX_CACHE_ENTRIES: usize = 32;

impl SyntaxHighlightCache {
    pub(super) fn highlight(&self, lines: &[String], language: &str) -> Vec<Vec<(Style, String)>> {
        let key = compute_key(lines, language);
        let mut state = self.state.borrow_mut();
        state.tick = state.tick.wrapping_add(1);
        let next_tick = state.tick;

        if let Some(entry) = state.entries.get_mut(&key) {
            entry.last_used = next_tick;
            return entry.lines.clone();
        }

        let highlighted_lines = do_highlight(lines, language);

        if state.entries.len() >= MAX_CACHE_ENTRIES
            && let Some(oldest) = state
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| *k)
        {
            state.entries.remove(&oldest);
        }

        state.entries.insert(
            key,
            SyntaxHighlightEntry {
                lines: highlighted_lines.clone(),
                last_used: next_tick,
            },
        );

        highlighted_lines
    }
}

fn syntect_style_to_ratatui(s: syntect::highlighting::Style) -> Style {
    let mut style = Style::default().fg(Color::Rgb(s.foreground.r, s.foreground.g, s.foreground.b));
    if s.font_style.contains(FontStyle::BOLD) {
        style = style.add_modifier(Modifier::BOLD);
    }
    if s.font_style.contains(FontStyle::ITALIC) {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if s.font_style.contains(FontStyle::UNDERLINE) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

fn syntax_lookup_token(language: &str) -> &str {
    let token = language.split_whitespace().next().unwrap_or(language);
    if is_typescript_alias(token) {
        "js"
    } else {
        token
    }
}

fn is_typescript_alias(token: &str) -> bool {
    ["ts", "tsx", "typescript", "mts", "cts"]
        .iter()
        .any(|alias| token.eq_ignore_ascii_case(alias))
}

fn do_highlight(lines: &[String], language: &str) -> Vec<Vec<(Style, String)>> {
    let language = syntax_lookup_token(language);
    let syntax = SYNTAX_SET
        .find_syntax_by_token(language)
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    let theme = THEME_SET
        .themes
        .get("base16-ocean.dark")
        .expect("should be included default theme");

    let mut h = HighlightLines::new(syntax, theme);
    let mut result = Vec::new();

    for line in lines {
        let line = line.trim_end_matches('\n');
        let regions = match h.highlight_line(line, &SYNTAX_SET) {
            Ok(regions) => regions,
            Err(error) => {
                logging::error(
                    "syntax_highlight",
                    format!("There was an error while calling highlight_line: {error}"),
                );
                result.push(vec![(Style::default(), line.to_owned())]);
                continue;
            }
        };
        result.push(
            regions
                .into_iter()
                .filter(|(_, text)| !text.is_empty())
                .map(|(style, text)| (syntect_style_to_ratatui(style), text.to_owned()))
                .collect(),
        );
    }

    result
}

#[test]
fn syntax_highlight_cache_stores_cached_elements() {
    let cache = SyntaxHighlightCache::default();
    let code = ["let works = true;".to_string()];
    let language = "rust";
    let key = compute_key(&code, language);
    cache.highlight(&code, language);
    assert_eq!(cache.state.borrow().tick, 1);
    assert_eq!(cache.state.borrow().entries.len(), 1);
    cache.highlight(&code, language);
    assert_eq!(cache.state.borrow().tick, 2);
    assert_eq!(cache.state.borrow().entries.len(), 1);
    assert!(cache.state.borrow().entries.contains_key(&key));
    cache.highlight(&code, "js");
    assert_eq!(cache.state.borrow().tick, 3);
    assert_eq!(cache.state.borrow().entries.len(), 2);
}

#[test]
fn syntax_lookup_token_maps_typescript_aliases_to_javascript() {
    for language in [
        "ts",
        "tsx",
        "typescript",
        "TypeScript",
        "mts",
        "cts",
        "typescript ignore",
    ] {
        assert_eq!(syntax_lookup_token(language), "js");
    }

    assert_eq!(syntax_lookup_token("rust"), "rust");
    assert_eq!(syntax_lookup_token("javascript"), "javascript");
}

#[test]
fn syntax_highlight_uses_javascript_for_typescript_aliases() {
    let code = ["const value: string = 'hello';".to_string()];
    assert_eq!(do_highlight(&code, "typescript"), do_highlight(&code, "js"));
    assert_eq!(do_highlight(&code, "tsx"), do_highlight(&code, "js"));
}

#[test]
fn syntax_highlight_cache_no_over_limit() {
    let cache = SyntaxHighlightCache::default();
    for i in 0..(MAX_CACHE_ENTRIES + 5) {
        cache.highlight(&[i.to_string()], "rust");
    }
    assert_eq!(cache.state.borrow().entries.len(), MAX_CACHE_ENTRIES);
    let key_too_old = compute_key(&[4.to_string()], "rust");
    let key_not_old = compute_key(&[5.to_string()], "rust");
    assert!(!cache.state.borrow().entries.contains_key(&key_too_old));
    assert!(cache.state.borrow().entries.contains_key(&key_not_old));
}
