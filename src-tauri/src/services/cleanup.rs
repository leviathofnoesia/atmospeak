use regex::Regex;

use crate::models::{DictionaryEntry, Snippet};

pub fn clean_text(raw: &str, dictionary: &[DictionaryEntry], snippets: &[Snippet]) -> String {
    let mut text = raw.replace('\n', " ");
    text = strip_whisper_timestamps(&text);
    text = apply_correction_commands(&text);
    text = normalize_spoken_punctuation(&text);
    text = remove_fillers(&text);
    text = apply_dictionary(&text, dictionary);
    text = apply_snippets(&text, snippets);
    text = normalize_spacing(&text);
    text = sentence_case_sentences(&text);
    ensure_terminal_punctuation(&text)
}

fn strip_whisper_timestamps(value: &str) -> String {
    let timestamp_pattern = Regex::new(r"\[[0-9:.\s>\-]+\]").expect("timestamp regex");
    timestamp_pattern.replace_all(value, "").to_string()
}

fn normalize_spoken_punctuation(value: &str) -> String {
    let replacements = [
        ("new paragraph", "\n\n"),
        ("new line", "\n"),
        ("comma", ","),
        ("period", "."),
        ("full stop", "."),
        ("question mark", "?"),
        ("exclamation mark", "!"),
        ("exclamation point", "!"),
        ("colon", ":"),
        ("semicolon", ";"),
        ("open parenthesis", "("),
        ("close parenthesis", ")"),
        ("open paren", "("),
        ("close paren", ")"),
        ("left paren", "("),
        ("right paren", ")"),
        ("dash", "-"),
        ("hyphen", "-"),
        ("slash", "/"),
        ("at sign", "@"),
        ("ampersand", "&"),
    ];

    replacements
        .iter()
        .fold(value.to_string(), |acc, (from, to)| {
            let regex = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(from)))
                .expect("spoken punctuation regex");
            regex.replace_all(&acc, *to).to_string()
        })
}

fn apply_correction_commands(value: &str) -> String {
    let correction_pattern = Regex::new(r"(?i)\b(scratch that|delete that|never mind|nevermind)\b")
        .expect("correction command regex");
    correction_pattern
        .split(value)
        .last()
        .unwrap_or(value)
        .trim_start_matches(|character: char| {
            character.is_whitespace() || [',', '.', '!', '?', ';', ':'].contains(&character)
        })
        .to_string()
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
    let around_open_paren = Regex::new(r"\(\s+").expect("open paren regex");
    let around_close_paren = Regex::new(r"\s+\)").expect("close paren regex");
    let too_many_newlines = Regex::new(r"\n{3,}").expect("newline regex");

    let normalized_lines = value
        .trim()
        .lines()
        .map(|line| {
            let text = multi_space.replace_all(line.trim(), " ");
            let text = before_punctuation.replace_all(&text, "$1");
            let text = after_punctuation.replace_all(&text, "$1 $2");
            let text = around_open_paren.replace_all(&text, "(");
            around_close_paren.replace_all(&text, ")").to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    too_many_newlines
        .replace_all(&normalized_lines, "\n\n")
        .to_string()
}

fn sentence_case_sentences(value: &str) -> String {
    let mut next_letter_should_be_uppercase = true;
    let mut output = String::with_capacity(value.len());

    for character in value.chars() {
        if next_letter_should_be_uppercase && character.is_ascii_alphabetic() {
            output.push(character.to_ascii_uppercase());
            next_letter_should_be_uppercase = false;
            continue;
        }

        output.push(character);
        if matches!(character, '.' | '!' | '?' | '\n') {
            next_letter_should_be_uppercase = true;
        } else if !character.is_whitespace() && !matches!(character, '"' | '\'' | '(' | '[' | '{') {
            next_letter_should_be_uppercase = false;
        }
    }

    output
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

    #[test]
    fn preserves_paragraphs_and_cases_each_sentence() {
        let result = clean_text(
            "first sentence period second sentence question mark new paragraph final line",
            &[],
            &[],
        );

        assert_eq!(result, "First sentence. Second sentence?\n\nFinal line.");
    }

    #[test]
    fn handles_spoken_symbols_and_parentheses() {
        let result = clean_text(
            "email me at sign ops at sign example period com new line open paren urgent close paren",
            &[],
            &[],
        );

        assert_eq!(result, "Email me @ ops @ example. Com\n(Urgent).");
    }

    #[test]
    fn scratch_that_keeps_only_following_text() {
        let result = clean_text(
            "send the old note scratch that send the revised note",
            &[],
            &[],
        );

        assert_eq!(result, "Send the revised note.");
    }
}
