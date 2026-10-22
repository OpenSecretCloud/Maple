//! Markdown list continuation for the chat composer.
//!
//! Shift+Enter is the composer's newline. When that newline lands in an
//! unordered, ordered, or task item, the composer carries the marker onto
//! the next line. An empty item exits the list instead. These rules follow
//! the Research chat composer: indentation and marker style stay put, and
//! fenced code, horizontal rules, escaped markers, and malformed markers
//! are left alone. Paste and IME composition never call this module.

use std::ops::Range;

/// One composer edit. The caret is placed at the end of `replacement`.
pub(super) struct ListEdit {
    pub range: Range<usize>,
    pub replacement: String,
}

pub(super) fn edit(value: &str, selection_start: usize, selection_end: usize) -> Option<ListEdit> {
    if selection_start > selection_end
        || selection_end > value.len()
        || !value.is_char_boundary(selection_start)
        || !value.is_char_boundary(selection_end)
    {
        return None;
    }

    let line_start = value[..selection_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    if is_inside_fence(value, line_start) {
        return None;
    }

    let line_end = value[selection_start..]
        .find('\n')
        .map_or(value.len(), |index| selection_start + index);
    let line = &value[line_start..line_end];
    if is_horizontal_rule(line) {
        return None;
    }

    let marker = list_marker(&value[line_start..selection_start])?;
    let full_line = list_marker(line);
    if full_line.is_some_and(|full| full.item_text.trim().is_empty()) {
        let delete_from = line_start + marker.indent.len();
        let delete_to = line_end.max(selection_end);
        if delete_from > delete_to || !value.is_char_boundary(delete_from) {
            return None;
        }
        return Some(ListEdit {
            range: delete_from..delete_to,
            replacement: String::new(),
        });
    }

    Some(ListEdit {
        range: selection_start..selection_end,
        replacement: format!("\n{}", marker.prefix),
    })
}

struct Marker<'a> {
    indent: &'a str,
    prefix: String,
    item_text: &'a str,
}

fn list_marker(line: &str) -> Option<Marker<'_>> {
    let (indent, rest) = take_ascii_space(line);
    let marker = rest.chars().next()?;
    let after_marker = &rest[marker.len_utf8()..];

    if matches!(marker, '-' | '+' | '*') {
        let (spacing, item_text) = take_required_ascii_space(after_marker)?;
        if let Some(task) = task_item(item_text) {
            return Some(Marker {
                indent,
                prefix: format!("{indent}{marker}{spacing}[ ]{}", task.spacing),
                item_text: task.item_text,
            });
        }
        return Some(Marker {
            indent,
            prefix: format!("{indent}{marker}{spacing}"),
            item_text,
        });
    }

    let (digits, after_digits) = take_digits(rest);
    if digits.is_empty() {
        return None;
    }
    let delimiter = after_digits.chars().next()?;
    if !matches!(delimiter, '.' | ')') {
        return None;
    }
    let (spacing, item_text) = take_required_ascii_space(&after_digits[delimiter.len_utf8()..])?;
    Some(Marker {
        indent,
        prefix: format!("{indent}{}{delimiter}{spacing}", increment_decimal(digits)),
        item_text,
    })
}

struct Task<'a> {
    spacing: &'a str,
    item_text: &'a str,
}

/// A checkbox is a task item when a space or tab follows `]`, or when the
/// item is empty. `- [x]todo` stays an ordinary bullet.
fn task_item(item: &str) -> Option<Task<'_>> {
    let rest = item.strip_prefix('[')?;
    let check = rest.chars().next()?;
    if !matches!(check, ' ' | 'x' | 'X') {
        return None;
    }
    let after_check = rest.strip_prefix(check)?.strip_prefix(']')?;
    let (spacing, item_text) = take_ascii_space(after_check);
    if spacing.is_empty() && !item_text.is_empty() {
        return None;
    }
    Some(Task { spacing, item_text })
}

fn take_ascii_space(text: &str) -> (&str, &str) {
    let end = text
        .bytes()
        .position(|byte| byte != b' ' && byte != b'\t')
        .unwrap_or(text.len());
    text.split_at(end)
}

fn take_required_ascii_space(text: &str) -> Option<(&str, &str)> {
    let (spacing, rest) = take_ascii_space(text);
    (!spacing.is_empty()).then_some((spacing, rest))
}

fn take_digits(text: &str) -> (&str, &str) {
    let end = text
        .bytes()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(text.len());
    text.split_at(end)
}

/// Decimal increment without a fixed-width integer, matching an unbounded
/// integer parse: leading zeros are not preserved (`09` becomes `10`).
fn increment_decimal(digits: &str) -> String {
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let mut bytes = digits.as_bytes().to_vec();
    let mut carry = true;
    for byte in bytes.iter_mut().rev() {
        if !carry {
            break;
        }
        if *byte == b'9' {
            *byte = b'0';
        } else {
            *byte += 1;
            carry = false;
        }
    }
    if carry {
        bytes.insert(0, b'1');
    }
    String::from_utf8(bytes).expect("decimal digits stay ASCII")
}

fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    is_repeated_marker(trimmed, '-') || is_repeated_marker(trimmed, '*')
}

fn is_repeated_marker(trimmed: &str, marker: char) -> bool {
    let mut count = 0;
    let mut rest = trimmed;
    while !rest.is_empty() {
        if !rest.starts_with(marker) {
            return false;
        }
        count += 1;
        rest = &rest[marker.len_utf8()..];
        let spacing = rest
            .char_indices()
            .find(|(_, character)| !character.is_whitespace())
            .map_or(rest.len(), |(index, _)| index);
        rest = &rest[spacing..];
    }
    count >= 3
}

fn is_inside_fence(value: &str, line_start: usize) -> bool {
    let mut fence: Option<(char, usize)> = None;
    for line in value[..line_start].split('\n') {
        if let Some((marker, length)) = fence {
            if closing_fence(line, marker).is_some_and(|close| close >= length) {
                fence = None;
            }
            continue;
        }
        fence = opening_fence(line);
    }
    fence.is_some()
}

fn opening_fence(line: &str) -> Option<(char, usize)> {
    let rest = strip_fence_indent(line)?;
    let marker = rest.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let length = rest
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (length >= 3).then_some((marker, length))
}

fn closing_fence(line: &str, marker: char) -> Option<usize> {
    let rest = strip_fence_indent(line)?;
    if !rest.starts_with(marker) {
        return None;
    }
    let length = rest
        .chars()
        .take_while(|character| *character == marker)
        .count();
    let after = &rest[length..];
    after
        .bytes()
        .all(|byte| byte == b' ' || byte == b'\t')
        .then_some(length)
}

/// A fence indent is at most three ASCII spaces. A fourth space, or a tab,
/// means this line is not a fence marker.
fn strip_fence_indent(line: &str) -> Option<&str> {
    let spaces = line.bytes().take_while(|byte| *byte == b' ').count();
    if spaces > 3 {
        return None;
    }
    Some(&line[spaces..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(value: &str, start: usize, end: usize) -> Option<(String, usize)> {
        let edit = edit(value, start, end)?;
        let mut next = String::new();
        next.push_str(&value[..edit.range.start]);
        next.push_str(&edit.replacement);
        next.push_str(&value[edit.range.end..]);
        Some((next, edit.range.start + edit.replacement.len()))
    }

    fn edit_at_end(value: &str) -> Option<(String, usize)> {
        apply(value, value.len(), value.len())
    }

    fn assert_continued(value: &str, continuation: &str) {
        let (next, caret) = edit_at_end(value).expect("list continues");
        let expected = format!("{value}\n{continuation}");
        assert_eq!(next, expected);
        assert_eq!(caret, expected.len());
    }

    #[test]
    fn continues_unordered_markers() {
        for marker in ['-', '*', '+'] {
            assert_continued(&format!("{marker} first item"), &format!("{marker} "));
        }
    }

    #[test]
    fn continues_a_task_list_with_an_unchecked_box() {
        assert_continued("- [ ] todo", "- [ ] ");
        assert_continued("* [x] done", "* [ ] ");
        assert_continued("\t+  [X]\tfinished", "\t+  [ ]\t");
        // A missing space after the box is an ordinary bullet, not a task.
        assert_continued("- [x]todo", "- ");
    }

    #[test]
    fn increments_an_ordered_list_and_preserves_its_style() {
        assert_continued("  9) ninth item", "  10) ");
        assert_continued("\t99. ninety-ninth", "\t100. ");
        assert_continued("09. padded", "10. ");
        let digits = "9".repeat(20);
        let incremented = format!("1{}", "0".repeat(20));
        assert_continued(&format!("{digits}. item"), &format!("{incremented}. "));
    }

    #[test]
    fn continues_the_current_list_in_a_mixed_draft() {
        let value = "- first\n\n1. second\n\n* third";
        assert_continued(value, "* ");
    }

    #[test]
    fn continues_at_the_caret_and_replaces_the_selection() {
        let value = "Intro\n1. first item selected";
        let start = value.find(" selected").unwrap();
        let (next, caret) = apply(value, start, value.len()).unwrap();
        assert_eq!(next, "Intro\n1. first item\n2. ");
        assert_eq!(caret, start + 4);
    }

    #[test]
    fn replaces_a_selection_that_spans_later_lines() {
        let value = "1. first selected\nsecond\nthird";
        let start = value.find(" selected").unwrap();
        let end = value.find("third").unwrap();
        let (next, caret) = apply(value, start, end).unwrap();
        assert_eq!(next, "1. first\n2. third");
        assert_eq!(caret, start + 4);
    }

    #[test]
    fn moves_text_after_the_caret_onto_the_generated_item() {
        let (next, caret) = apply("- item", 2, 2).unwrap();
        assert_eq!(next, "- \n- item");
        assert_eq!(caret, 5);
    }

    #[test]
    fn ends_an_empty_list_item() {
        for value in ["First\n  - ", "First\n\t3)  ", "First\n  * [x]  "] {
            let line_start = value.rfind('\n').unwrap() + 1;
            let indent = value[line_start..]
                .chars()
                .take_while(|character| *character == ' ' || *character == '\t')
                .count();
            let caret = line_start + indent;
            let (next, cursor) = edit_at_end(value).unwrap();
            assert_eq!(next, &value[..caret]);
            assert_eq!(cursor, caret);
        }
    }

    #[test]
    fn removes_an_empty_marker_and_the_selected_following_text() {
        let (next, caret) = apply("- \nselected", 2, 11).unwrap();
        assert_eq!(next, "");
        assert_eq!(caret, 0);
    }

    #[test]
    fn ignores_non_lists() {
        for value in [
            "ordinary text",
            "prefix - item",
            "\\- escaped",
            "-not a list",
            "- - -",
            "* * * *",
        ] {
            assert_eq!(edit_at_end(value), None, "{value}");
        }
    }

    #[test]
    fn does_not_continue_inside_a_fence() {
        for value in [
            "```\n- code item",
            "~~~ts\n1. code item",
            "before\n````js\n* code item",
            "before\n~~~\n+ code item",
        ] {
            assert_eq!(edit_at_end(value), None, "{value}");
        }
    }

    #[test]
    fn continues_again_after_the_matching_fence_closes() {
        assert_continued("```\n- code\n```\n- real", "- ");
        assert_eq!(edit_at_end("~~~~\n1. code\n~~~\n2. still code"), None);
    }

    #[test]
    fn rejects_a_selection_that_is_not_on_a_char_boundary() {
        assert!(edit("é", 1, 1).is_none());
    }
}
