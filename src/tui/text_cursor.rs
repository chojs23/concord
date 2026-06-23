use unicode_segmentation::UnicodeSegmentation;

pub(in crate::tui) fn clamp_cursor_index(value: &str, index: usize) -> usize {
    let mut index = index.min(value.len());
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

pub(in crate::tui) fn previous_char_boundary(value: &str, index: usize) -> usize {
    let index = clamp_cursor_index(value, index);
    value[..index]
        .grapheme_indices(true)
        .next_back()
        .map(|(start, _)| start)
        .unwrap_or(0)
}

pub(in crate::tui) fn next_char_boundary(value: &str, index: usize) -> usize {
    let index = clamp_cursor_index(value, index);
    value[index..]
        .grapheme_indices(true)
        .nth(1)
        .map(|(offset, _)| index + offset)
        .unwrap_or(value.len())
}

pub(in crate::tui) fn previous_word_boundary(input: &str, index: usize) -> usize {
    let index = clamp_cursor_index(input, index);
    let mut prefix = input[..index].char_indices().rev().peekable();
    while matches!(prefix.peek(), Some((_, c)) if c.is_whitespace()) {
        prefix.next();
    }
    let mut word_start = None;
    while let Some(&(byte_idx, c)) = prefix.peek() {
        if c.is_whitespace() {
            break;
        }
        word_start = Some(byte_idx);
        prefix.next();
    }
    word_start.unwrap_or(0)
}

pub(in crate::tui) fn next_word_boundary(input: &str, index: usize) -> usize {
    let index = clamp_cursor_index(input, index);
    let mut suffix = input[index..].char_indices().peekable();
    while matches!(suffix.peek(), Some((_, c)) if !c.is_whitespace()) {
        suffix.next();
    }
    while matches!(suffix.peek(), Some((_, c)) if c.is_whitespace()) {
        suffix.next();
    }
    match suffix.peek() {
        Some(&(rel, _)) => index + rel,
        None => input.len(),
    }
}

/// Byte offset for moving the cursor one line up (`direction == -1`) or down
/// (`direction == 1`), keeping roughly the same column. Returns `None` when
/// there is no line in that direction (so single-line inputs never move).
pub(in crate::tui) fn vertical_cursor_target(
    input: &str,
    cursor: usize,
    direction: isize,
) -> Option<usize> {
    let cursor = clamp_cursor_index(input, cursor);
    let line_start = line_start_before(input, cursor);
    let line_end = line_end_after(input, cursor);
    let column = input[line_start..cursor].chars().count();

    match direction {
        -1 => {
            if line_start == 0 {
                return None;
            }
            let target_end = line_start - 1;
            let target_start = line_start_before(input, target_end);
            Some(byte_index_for_line_column(
                input,
                target_start,
                target_end,
                column,
            ))
        }
        1 => {
            let next_start = line_end.checked_add(1)?;
            if next_start > input.len() {
                return None;
            }
            let target_end = line_end_after(input, next_start);
            Some(byte_index_for_line_column(
                input, next_start, target_end, column,
            ))
        }
        _ => None,
    }
}

fn line_start_before(input: &str, index: usize) -> usize {
    input[..index]
        .rfind('\n')
        .map(|offset| offset + '\n'.len_utf8())
        .unwrap_or(0)
}

fn line_end_after(input: &str, index: usize) -> usize {
    input[index..]
        .find('\n')
        .map(|offset| index + offset)
        .unwrap_or(input.len())
}

fn byte_index_for_line_column(input: &str, start: usize, end: usize, column: usize) -> usize {
    input[start..end]
        .char_indices()
        .nth(column)
        .map(|(offset, _)| start + offset)
        .unwrap_or(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_boundaries_step_over_graphemes() {
        let value = "a🇰🇷e\u{301}z";
        let flag_end = "a🇰🇷".len();
        let accent_end = "a🇰🇷e\u{301}".len();

        assert_eq!(next_char_boundary(value, 0), "a".len());
        assert_eq!(next_char_boundary(value, "a".len()), flag_end);
        assert_eq!(previous_char_boundary(value, flag_end), "a".len());
        assert_eq!(previous_char_boundary(value, accent_end), flag_end);
    }

    #[derive(Clone, Copy)]
    enum Dir {
        Left,
        Right,
    }

    fn step_word(dir: Dir, before: &str) -> String {
        let idx = before
            .find('|')
            .expect("fixture must mark the cursor with `|`");
        let mut input = String::with_capacity(before.len() - 1);
        input.push_str(&before[..idx]);
        input.push_str(&before[idx + 1..]);
        let next = match dir {
            Dir::Left => previous_word_boundary(&input, idx),
            Dir::Right => next_word_boundary(&input, idx),
        };
        let mut out = input.clone();
        out.insert(next, '|');
        out
    }

    #[test]
    fn word_boundaries_land_on_word_starts() {
        let cases: &[(Dir, &str, &str)] = &[
            (Dir::Left, "hello world|", "hello |world"),
            (Dir::Left, "hello   |world", "|hello   world"),
            (Dir::Left, "hello |  world", "|hello   world"),
            (Dir::Right, "|hello world", "hello |world"),
            (Dir::Right, "hello|   world", "hello   |world"),
            (Dir::Right, "hello   world|", "hello   world|"),
        ];
        for (dir, before, expected) in cases {
            assert_eq!(step_word(*dir, before), *expected);
        }
    }
}
