use regex::{Captures, Regex};

use crate::models::{DictionaryEntry, Snippet};

pub fn clean_text(raw: &str, dictionary: &[DictionaryEntry], snippets: &[Snippet]) -> String {
    let mut text = raw.replace('\n', " ");
    text = strip_whisper_timestamps(&text);
    text = normalize_spoken_punctuation(&text);
    text = remove_fillers(&text);
    text = apply_dictionary(&text, dictionary);
    text = apply_snippets(&text, snippets);
    text = normalize_spacing(&text);
    text = sentence_case(&text);
    ensure_terminal_punctuation(&text)
}

fn strip_whisper_timestamps(value: &str) -> String {
    let timestamp_pattern = Regex::new(r"\[[0-9:.\s>\-]+\]").expect("timestamp regex");
    timestamp_pattern.replace_all(value, "").to_string()
}

fn normalize_spoken_punctuation(value: &str) -> String {
    let replacements = [
        ("comma", ","),
        ("period", "."),
        ("full stop", "."),
        ("question mark", "?"),
        ("exclamation mark", "!"),
        ("colon", ":"),
        ("semicolon", ";"),
        ("new line", "\n"),
        ("new paragraph", "\n\n"),
    ];

    replacements
        .iter()
        .fold(value.to_string(), |acc, (from, to)| {
            let regex = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(from)))
                .expect("spoken punctuation regex");
            regex.replace_all(&acc, *to).to_string()
        })
}

fn remove_fillers(value: &str) -> String {
    let filler_pattern = Regex::new(r"(?i)\b(um+|uh+|erm|ah|like)\b[, ]*").expect("filler regex");
    filler_pattern.replace_all(value, "").to_string()
}

fn apply_dictionary(value: &str, dictionary: &[DictionaryEntry]) -> String {
    dictionary
        .iter()
        .filter(|entry| entry.enabled && !entry.phrase.trim().is_empty())
        .fold(value.to_string(), |acc, entry| {
            let regex = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(entry.phrase.trim())))
                .expect("dictionary regex");
            regex
                .replace_all(&acc, entry.replacement.as_str())
                .to_string()
        })
}

fn apply_snippets(value: &str, snippets: &[Snippet]) -> String {
    snippets
        .iter()
        .filter(|snippet| snippet.enabled && !snippet.trigger.trim().is_empty())
        .fold(value.to_string(), |acc, snippet| {
            let regex = Regex::new(&format!(
                r"(?i)\b{}\b",
                regex::escape(snippet.trigger.trim())
            ))
            .expect("snippet regex");
            regex.replace_all(&acc, snippet.body.as_str()).to_string()
        })
}

fn normalize_spacing(value: &str) -> String {
    let multi_space = Regex::new(r"[ \t]{2,}").expect("space regex");
    let before_punctuation = Regex::new(r"\s+([,.;:!?])").expect("punctuation regex");
    let after_punctuation = Regex::new(r"([,.;:!?])([^\s])").expect("after punctuation regex");

    let text = multi_space.replace_all(value.trim(), " ");
    let text = before_punctuation.replace_all(&text, "$1");
    after_punctuation.replace_all(&text, "$1 $2").to_string()
}

fn sentence_case(value: &str) -> String {
    let first_letter = Regex::new(r"[A-Za-z]").expect("letter regex");
    first_letter
        .replace(value, |captures: &Captures<'_>| captures[0].to_uppercase())
        .to_string()
}

fn ensure_terminal_punctuation(value: &str) -> String {
    if value.is_empty() || value.ends_with(['.', '!', '?', ':']) {
        value.to_string()
    } else {
        format!("{value}.")
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn cleans_spoken_dictation() {
        let dictionary = vec![DictionaryEntry {
            id: "1".to_string(),
            phrase: "bridge mind".to_string(),
            replacement: "BridgeMind".to_string(),
            enabled: true,
            created_at: Utc::now(),
        }];
        let snippets = vec![Snippet {
            id: "1".to_string(),
            trigger: "ship intro".to_string(),
            body: "Thanks for the review.".to_string(),
            enabled: true,
            created_at: Utc::now(),
        }];

        let result = clean_text(
            "[00:00:00.000 --> 00:00:02.000] um bridge mind comma ship intro",
            &dictionary,
            &snippets,
        );

        assert_eq!(result, "BridgeMind, Thanks for the review.");
    }
}
