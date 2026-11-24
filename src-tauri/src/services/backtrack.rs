use std::sync::LazyLock;

use regex::Regex;

static COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:scratch that|delete that|forget that|never mind|nevermind)\b").unwrap()
});
static TRIGGER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:actually|i mean|wait[, ]+no|no[, ]+wait)\b").unwrap());
static WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\p{L}\p{N}_]+(?:['’:.@/-][\p{L}\p{N}_]+)*").unwrap());

/// Corrections affect only the current sentence, never completed paragraphs.
/// Ambiguous uses of "actually" remain speech unless a replacement is explicit.
pub fn apply_backtrack(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            let mut text = String::new();
            let mut cursor = 0;
            for found in COMMAND.find_iter(line) {
                if quoted_at(line, found.start()) {
                    continue;
                }
                text.push_str(&line[cursor..found.start()]);
                text.truncate(sentence_start(&text));
                if !text.is_empty() && !text.ends_with(char::is_whitespace) {
                    text.push(' ');
                }
                cursor = found.end();
                while let Some(ch) = line[cursor..].chars().next() {
                    if !ch.is_whitespace() && !",.!?;:".contains(ch) {
                        break;
                    }
                    cursor += ch.len_utf8();
                }
            }
            text.push_str(&line[cursor..]);
            let start = crate::services::list_formatting::content_start(line);
            if start > line.len() - line.trim_start().len() && text.trim() == line[..start].trim() {
                String::new()
            } else {
                collapse_stutters(&apply_triggered_backtrack(&text))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn quoted_at(value: &str, offset: usize) -> bool {
    value[..offset]
        .chars()
        .filter(|ch| matches!(ch, '"' | '“' | '”'))
        .count()
        % 2
        == 1
}

fn sentence_start(value: &str) -> usize {
    let trimmed = value.trim_end_matches(|ch: char| ch.is_whitespace() || ",.!?;:".contains(ch));
    trimmed
        .char_indices()
        .rev()
        .find_map(|(idx, ch)| {
            (matches!(ch, '.' | '!' | '?' | '\n')
                && value[idx + ch.len_utf8()..].starts_with(char::is_whitespace))
            .then_some(idx + ch.len_utf8())
        })
        .unwrap_or_else(|| crate::services::list_formatting::content_start(value))
}

fn apply_triggered_backtrack(value: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    for marker in TRIGGER.find_iter(value) {
        output.push_str(&value[cursor..marker.start()]);
        let after = value[marker.end()..]
            .trim_start_matches(|ch: char| ch.is_whitespace() || ",.!?;:".contains(ch));
        let scope = sentence_start(&output);
        let before = &output[scope..];
        let left: Vec<_> = WORD.find_iter(before).collect();
        let right: Vec<_> = WORD.find_iter(after).take(4).collect();
        let replacement = if quoted_at(value, marker.start())
            || left.is_empty()
            || right.is_empty()
            || (marker.as_str().eq_ignore_ascii_case("i mean")
                && matches!(
                    right[0].as_str().to_ascii_lowercase().as_str(),
                    "is" | "by" | "when"
                )) {
            None
        } else {
            // A repeated grammatical anchor identifies the span being replaced.
            let anchor = left.iter().enumerate().rev().find_map(|(idx, word)| {
                let same = word.as_str().eq_ignore_ascii_case(right[0].as_str());
                let pair = idx + 1 < left.len()
                    && right.len() > 1
                    && left[idx + 1]
                        .as_str()
                        .eq_ignore_ascii_case(right[1].as_str());
                let preposition = matches!(
                    right[0].as_str().to_ascii_lowercase().as_str(),
                    "at" | "on" | "in" | "to" | "for" | "as" | "with" | "by"
                );
                (same && (pair || preposition)).then_some(word.start())
            });
            let last = left.last().unwrap();
            let numeric_start = left
                .iter()
                .rev()
                .take(2)
                .find(|word| word.as_str().starts_with(|ch: char| ch.is_ascii_digit()))
                .filter(|word| {
                    word.start() == last.start()
                        || matches!(
                            last.as_str().to_ascii_lowercase().trim_end_matches('.'),
                            "am" | "pm" | "dollars" | "minutes" | "hours" | "days" | "percent"
                        )
                })
                .map(|word| word.start());
            let numeric = right[0]
                .as_str()
                .starts_with(|ch: char| ch.is_ascii_digit());
            let explicit = !marker.as_str().eq_ignore_ascii_case("actually")
                || before.trim_end().ends_with(',');
            anchor
                .or_else(|| numeric.then_some(numeric_start).flatten())
                .or_else(|| explicit.then_some(last.start()))
        };
        if let Some(start) = replacement {
            output.truncate(scope + start);
            if !output.is_empty() && !output.ends_with(char::is_whitespace) {
                output.push(' ');
            }
            cursor = value.len() - after.len();
        } else {
            output.push_str(marker.as_str());
            cursor = marker.end();
        }
    }
    output.push_str(&value[cursor..]);
    output.trim().to_string()
}

fn normalize_token(token: &str) -> String {
    if token.chars().any(char::is_alphanumeric) {
        token.to_lowercase()
    } else {
        String::new()
    }
}

fn has_boundary(tokens: &[&str]) -> bool {
    tokens
        .iter()
        .any(|token| token.contains(['.', '!', '?', ',', ';', ':']))
}

fn collapse_stutters(value: &str) -> String {
    let mut words: Vec<_> = value.split_whitespace().collect();
    let mut idx = 0;
    while idx < words.len() {
        let mut changed = false;
        for len in (1..=4.min((words.len() - idx) / 2)).rev() {
            let left = &words[idx..idx + len];
            let right = &words[idx + len..idx + len * 2];
            let intentional = len == 1
                && matches!(
                    normalize_token(left[0]).as_str(),
                    "had" | "that" | "very" | "really" | "so" | "no" | "bye"
                );
            if !intentional
                && !has_boundary(left)
                && !has_boundary(right)
                && left.iter().zip(right).all(|(a, b)| {
                    !normalize_token(a).is_empty() && normalize_token(a) == normalize_token(b)
                })
            {
                words.drain(idx..idx + len);
                changed = true;
                break;
            }
        }
        if !changed {
            idx += 1;
        }
    }
    words.join(" ")
}

/// Only adjacent restatements can supersede a phrase; intervening content survives.
pub fn apply_short_phrase_restate(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            let mut words: Vec<_> = line.split_whitespace().collect();
            let mut idx = 0;
            while idx < words.len() {
                let mut changed = false;
                for len in (3..=4).rev() {
                    if idx + len * 2 > words.len() {
                        continue;
                    }
                    let left = &words[idx..idx + len];
                    let right = &words[idx + len..idx + len * 2];
                    if !has_boundary(left)
                        && !has_boundary(right)
                        && left[..len - 1].iter().zip(&right[..len - 1]).all(|(a, b)| {
                            !normalize_token(a).is_empty()
                                && normalize_token(a) == normalize_token(b)
                        })
                        && normalize_token(left[len - 1]) != normalize_token(right[len - 1])
                    {
                        words.drain(idx..idx + len);
                        changed = true;
                        break;
                    }
                }
                if !changed {
                    idx += 1;
                }
            }
            words.join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_stutter_unit() {
        assert_eq!(collapse_stutters("is not is not"), "is not");
        assert_eq!(
            collapse_stutters("right there. Right there"),
            "right there. Right there"
        );
    }

    #[test]
    fn actually_replaces_prior_token() {
        assert_eq!(
            apply_backtrack("Let's do coffee at 2 actually 3"),
            "Let's do coffee at 3"
        );
    }

    #[test]
    fn preserves_non_correction_actually() {
        assert_eq!(
            apply_backtrack("I actually enjoyed the movie"),
            "I actually enjoyed the movie"
        );
    }

    #[test]
    fn unmarked_roles_remain_literal() {
        assert_eq!(
            apply_backtrack("Hire me as a doctor as a nurse please"),
            "Hire me as a doctor as a nurse please"
        );
    }

    #[test]
    fn restate_short_phrase_keeps_second_ending() {
        assert_eq!(
            apply_short_phrase_restate("I want coffee I want tea"),
            "I want tea"
        );
        assert_eq!(
            apply_short_phrase_restate("send it tomorrow send it Friday"),
            "send it Friday"
        );
    }
    #[test]
    fn corrections_preserve_completed_sentences_and_literal_speech() {
        for (input, expected) in [
            (
                "Keep this. Send the old note scratch that send the new note",
                "Keep this. send the new note",
            ),
            ("I don't actually agree", "I don't actually agree"),
            ("What I mean is this", "What I mean is this"),
            ("Use foo/bar foobar", "Use foo/bar foobar"),
            ("This is actually very good", "This is actually very good"),
            ("Say \"scratch that\" aloud", "Say \"scratch that\" aloud"),
            ("We should go back home", "We should go back home"),
            ("Meet at 2, actually at 3 tomorrow", "Meet at 3 tomorrow"),
            ("Send to Alice, I mean Bob tomorrow", "Send to Bob tomorrow"),
            ("At 2 actually 3 actually 4", "At 4"),
            ("Meet at 2 pm, actually 3 pm", "Meet at 3 pm"),
            ("I had had enough", "I had had enough"),
            ("It's very very good", "It's very very good"),
        ] {
            assert_eq!(apply_backtrack(input), expected, "{input}");
        }
        assert_eq!(
            apply_short_phrase_restate("I want coffee and a cake I want tea"),
            "I want coffee and a cake I want tea"
        );
        assert_eq!(
            apply_backtrack("first line\n\nsecond line"),
            "first line\n\nsecond line"
        );
    }
}
