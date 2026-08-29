//! Spell checker for the composer. A Hunspell-compatible en_US dictionary
//! is embedded in the binary and parsed once on a background thread, so
//! startup and the UI thread never wait for it.

use std::ops::Range;
use std::sync::OnceLock;

use spellbook::Dictionary;
use unicode_segmentation::UnicodeSegmentation;

static DICTIONARY: OnceLock<Dictionary> = OnceLock::new();

/// Start the dictionary parse on a background thread. Safe to call more
/// than once; only the first call does work.
pub fn preload() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        std::thread::Builder::new()
            .name("spell-dict".into())
            .spawn(|| {
                let aff = include_str!("../../assets/dict/en_US.aff");
                let dic = include_str!("../../assets/dict/en_US.dic");
                let started = std::time::Instant::now();
                match Dictionary::new(aff, dic) {
                    Ok(dictionary) => {
                        let _ = DICTIONARY.set(dictionary);
                        log::debug!("Spell dictionary loaded in {:?}", started.elapsed());
                    }
                    Err(error) => log::warn!("Cannot load the spell dictionary: {error}"),
                }
            })
            .ok();
    });
}

fn dictionary() -> Option<&'static Dictionary> {
    DICTIONARY.get()
}

#[cfg(test)]
fn is_ready() -> bool {
    dictionary().is_some()
}

/// Words the checker skips: anything with a digit, an ALL-CAPS token
/// (acronyms, env vars), and a word next to path or URL punctuation.
fn should_check(text: &str, range: &Range<usize>, word: &str) -> bool {
    if word.chars().any(|ch| ch.is_ascii_digit()) {
        return false;
    }
    if word.chars().filter(|ch| ch.is_alphabetic()).count() < 2 {
        return false;
    }
    if word.chars().all(|ch| !ch.is_lowercase()) {
        return false;
    }
    let before = text[..range.start].chars().next_back();
    let after = text[range.end..].chars().next();
    let glue = |ch: Option<char>| {
        matches!(
            ch,
            Some('/' | '\\' | '.' | ':' | '@' | '#' | '_' | '-' | '`' | '=' | '~')
        )
    };
    !(glue(before) || glue(after))
}

/// Byte ranges of the words in `text` that the dictionary rejects. Empty
/// until the dictionary is loaded. Text inside backtick code spans is
/// skipped.
pub fn misspelled_ranges(text: &str) -> Vec<Range<usize>> {
    let Some(dictionary) = dictionary() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut in_code = false;
    for (start, word) in text.split_word_bound_indices() {
        if word.starts_with('`') {
            in_code = !in_code;
            continue;
        }
        if in_code || !word.chars().any(|ch| ch.is_alphabetic()) {
            continue;
        }
        let range = start..start + word.len();
        // Segmentation keeps a trailing apostrophe-s attached ("it's"), but
        // curly quotes make the checker fall back to the raw token.
        let word = word.replace('\u{2019}', "'");
        if !should_check(text, &range, &word) {
            continue;
        }
        if !dictionary.check(&word) && !dictionary.check(word.trim_matches('\'')) {
            out.push(range);
        }
    }
    out
}

/// Up to `limit` replacement candidates for a misspelled word.
pub fn suggestions(word: &str, limit: usize) -> Vec<String> {
    let Some(dictionary) = dictionary() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    dictionary.suggest(&word.replace('\u{2019}', "'"), &mut out);
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load() {
        preload();
        while !is_ready() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn flags_misspelled_words_only() {
        load();
        let text = "Please chekc the README at ./src/main.rs and `foo_bar` v2";
        let ranges = misspelled_ranges(text);
        let words: Vec<&str> = ranges.iter().map(|r| &text[r.clone()]).collect();
        assert_eq!(words, vec!["chekc"]);
    }

    #[test]
    fn accepts_contractions_and_capitalized_words() {
        load();
        assert!(misspelled_ranges("It's fine. Don't worry, Ben.").is_empty());
        assert!(misspelled_ranges("It\u{2019}s fine").is_empty());
    }

    #[test]
    fn suggests_replacements() {
        load();
        let suggestions = suggestions("chekc", 3);
        assert!(suggestions.iter().any(|s| s == "check"), "{suggestions:?}");
    }
}
