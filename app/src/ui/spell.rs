//! Spell checker for the composer. A Hunspell-compatible en_US dictionary
//! is embedded in the binary and parsed once on a background thread, so
//! startup and the UI thread never wait for it.

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{OnceLock, RwLock};

use spellbook::Dictionary;
use unicode_segmentation::UnicodeSegmentation;

static DICTIONARY: OnceLock<Dictionary> = OnceLock::new();

/// Bumped when the dictionary or the user word list becomes available, so
/// an input checked before the load can tell its ranges are stale.
static GENERATION: AtomicU32 = AtomicU32::new(0);

/// Current dictionary generation; compare with the value saved at the
/// last check to see if a re-check is needed.
pub fn generation() -> u32 {
    GENERATION.load(Ordering::Acquire)
}

/// Words the user added with "Add to dictionary". Persisted one per line
/// in `dictionary.txt` next to the settings file.
static USER_WORDS: RwLock<Option<HashSet<String>>> = RwLock::new(None);

fn user_words_file() -> PathBuf {
    crate::backend::app_config_root().join("dictionary.txt")
}

fn load_user_words() -> HashSet<String> {
    std::fs::read_to_string(user_words_file())
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Remember `word` for this user and write the list in the background.
pub fn add_word(word: &str) {
    let word = word.trim().trim_matches('\'').to_owned();
    if word.is_empty() {
        return;
    }
    let mut guard = USER_WORDS.write().unwrap_or_else(|e| e.into_inner());
    let words = guard.get_or_insert_with(load_user_words);
    if !words.insert(word) {
        return;
    }
    let mut lines: Vec<&String> = words.iter().collect();
    lines.sort();
    let text = lines.iter().map(|w| format!("{w}\n")).collect::<String>();
    drop(guard);
    std::thread::spawn(move || {
        let path = user_words_file();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(&path, text) {
            log::warn!(
                "Cannot save the user dictionary to {}: {error}",
                path.display()
            );
        }
    });
}

fn is_user_word(word: &str) -> bool {
    let guard = USER_WORDS.read().unwrap_or_else(|e| e.into_inner());
    let Some(words) = guard.as_ref() else {
        return false;
    };
    words.contains(word) || words.contains(&word.to_lowercase())
}

/// Start the dictionary parse on a background thread. Safe to call more
/// than once; only the first call does work.
pub fn preload() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        std::thread::Builder::new()
            .name("spell-dict".into())
            .spawn(|| {
                // The user list is small, but it is a file read, so it
                // stays off the UI thread with the dictionary parse.
                let user_words = load_user_words();
                {
                    let mut guard = USER_WORDS.write().unwrap_or_else(|e| e.into_inner());
                    if guard.is_none() {
                        *guard = Some(user_words);
                    }
                }
                let aff = include_str!("../../assets/dict/en_US.aff");
                let dic = include_str!("../../assets/dict/en_US.dic");
                let started = std::time::Instant::now();
                match Dictionary::new(aff, dic) {
                    Ok(mut dictionary) => {
                        for word in include_str!("../../assets/dict/extra.txt")
                            .lines()
                            .map(str::trim)
                            .filter(|line| !line.is_empty() && !line.starts_with('#'))
                        {
                            if let Err(error) = dictionary.add(word) {
                                log::warn!("Cannot add {word:?} to the spell dictionary: {error}");
                            }
                        }
                        let _ = DICTIONARY.set(dictionary);
                        GENERATION.fetch_add(1, Ordering::AcqRel);
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
        let bare = word.trim_matches('\'');
        if !dictionary.check(&word)
            && !dictionary.check(bare)
            && !is_user_word(&word)
            && !is_user_word(bare)
        {
            out.push(range);
        }
    }
    out
}

/// Words longer than this get no suggestions: the ngram pass scales with
/// word length (60-70 ms at 35+ chars) and such tokens are rarely typos.
const MAX_SUGGEST_LEN: usize = 20;

/// Up to `limit` replacement candidates for a misspelled word.
///
/// Performance: this runs synchronously on the UI thread and costs
/// 5-20 ms for a typical typo, because spellbook's ngram pass scans
/// every stem in the dictionary. That is accepted on purpose: it runs
/// once per right-click, when the user has already paused, and the
/// alternative (open the menu empty and fill it in later) reads as a
/// bug. If it ever shows in a profile, precompute suggestions in the
/// background for each newly flagged word after `misspelled_ranges`
/// instead of making this call async.
pub fn suggestions(word: &str, limit: usize) -> Vec<String> {
    let Some(dictionary) = dictionary() else {
        return Vec::new();
    };
    if word.chars().count() > MAX_SUGGEST_LEN {
        return Vec::new();
    }
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
    fn accepts_bundled_extra_words() {
        load();
        assert!(misspelled_ranges("Maple talks to OpenSecret over gpui").is_empty());
    }

    #[test]
    fn suggests_replacements() {
        load();
        let suggestions = suggestions("chekc", 3);
        assert!(suggestions.iter().any(|s| s == "check"), "{suggestions:?}");
    }
}
