use std::cmp::Reverse;
use std::collections::BinaryHeap;

use regex::Regex;

use crate::models::{DictionaryEntry, Snippet};
use crate::services::{backtrack, list_formatting};

mod formatting;
use formatting::{
    ensure_terminal_punctuation, normalize_spacing, normalize_spoken_punctuation,
    sentence_case_sentences,
};

/// Final-only local edits. Raw ASR stays in History; substitutions are last so
/// their literal bodies and internal formatting never pass through speech rules.
pub fn clean_text(raw: &str, dictionary: &[DictionaryEntry], snippets: &[Snippet]) -> String {
    let mut text = formatting::strip_timestamps(raw);
    text = normalize_spoken_punctuation(&text);
    text = list_formatting::format_lists(&text);
    text = backtrack::apply_backtrack(&text);
    text = backtrack::apply_short_phrase_restate(&text);
    text = formatting::remove_fillers(&text);
    text = normalize_spacing(&text);
    text = sentence_case_sentences(&text);
    text = ensure_terminal_punctuation(&text);
    text = apply_replacements(
        &text,
        dictionary
            .iter()
            .filter(|entry| entry.enabled)
            .map(|entry| (entry.phrase.trim(), entry.replacement.as_str())),
        false,
    );
    apply_replacements(
        &text,
        snippets
            .iter()
            .filter(|entry| entry.enabled)
            .map(|entry| (entry.trigger.trim(), entry.body.as_str())),
        true,
    )
}

fn next_valid_match(pattern: &Regex, value: &str, cursor: usize) -> Option<(usize, usize)> {
    pattern.find_iter(&value[cursor..]).find_map(|found| {
        let start = cursor + found.start();
        let end = cursor + found.end();
        (!value[..start]
            .chars()
            .next_back()
            .is_some_and(word_character)
            && !value[end..].chars().next().is_some_and(word_character))
        .then_some((start, end))
    })
}

fn push_next_candidate(
    candidates: &mut BinaryHeap<(Reverse<usize>, usize, Reverse<usize>)>,
    patterns: &[(Regex, &str)],
    value: &str,
    cursor: usize,
    pattern_index: usize,
) {
    if cursor >= value.len() {
        return;
    }
    if let Some((start, end)) = next_valid_match(&patterns[pattern_index].0, value, cursor) {
        candidates.push((Reverse(start), end - start, Reverse(pattern_index)));
    }
}

fn apply_replacements<'a>(
    value: &str,
    entries: impl Iterator<Item = (&'a str, &'a str)>,
    verbatim_end: bool,
) -> String {
    let patterns: Vec<_> = entries
        .filter(|(from, _)| !from.is_empty())
        .map(|(from, to)| {
            (
                Regex::new(&format!("(?i){}", regex::escape(from))).unwrap(),
                to,
            )
        })
        .collect();
    let mut output = String::with_capacity(value.len());
    let mut candidates = BinaryHeap::new();
    for pattern_index in 0..patterns.len() {
        push_next_candidate(&mut candidates, &patterns, value, 0, pattern_index);
    }
    let mut cursor = 0;
    while let Some(candidate) = candidates.pop() {
        let (Reverse(start), length, Reverse(pattern_index)) = candidate;
        if start < cursor {
            push_next_candidate(&mut candidates, &patterns, value, cursor, pattern_index);
            continue;
        }
        let end = start + length;
        output.push_str(&value[cursor..start]);
        // No regex replacement expansion: "$1" and "$HOME" are literal text.
        output.push_str(patterns[pattern_index].1);
        cursor = end;
        if (patterns[pattern_index].1.ends_with(['.', '!', '?'])
            || (verbatim_end && cursor + 1 == value.len()))
            && value[cursor..].starts_with('.')
        {
            cursor += 1;
        }
        push_next_candidate(&mut candidates, &patterns, value, cursor, pattern_index);
    }
    output.push_str(&value[cursor..]);
    output
}

fn word_character(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

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
            "email ops at sign example period com new line open paren urgent close paren",
            &[],
            &[],
        );

        assert_eq!(result, "Email ops@example.com\n(Urgent).");
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

    #[test]
    fn explicit_forget_command_and_literal_go_back() {
        assert_eq!(
            clean_text("draft one go back draft two", &[], &[]),
            "Draft one go back draft two."
        );
        assert_eq!(
            clean_text("old plan forget that new plan", &[], &[]),
            "New plan."
        );
    }

    #[test]
    fn collapses_stutter_phrase_repeats() {
        assert_eq!(
            clean_text("the accuracy is not is not as resilient", &[], &[]),
            "The accuracy is not as resilient."
        );
        assert_eq!(
            clean_text("right there. Right there I've been using", &[], &[]),
            "Right there. Right there I've been using."
        );
        assert_eq!(
            clean_text("if I go too fast fast or it's too short", &[], &[]),
            "If I go too fast or it's too short."
        );
        assert_eq!(
            clean_text("it seems seems to lag", &[], &[]),
            "It seems to lag."
        );
    }

    #[test]
    fn actually_replaces_prior_token() {
        assert_eq!(
            clean_text("Let's do coffee at 2 actually 3", &[], &[]),
            "Let's do coffee at 3."
        );
    }

    #[test]
    fn preserves_non_correction_actually() {
        let result = clean_text("I actually enjoyed the movie", &[], &[]);
        assert_eq!(result, "I actually enjoyed the movie.");
    }

    #[test]
    fn restate_as_a_keeps_later_ending() {
        assert_eq!(
            clean_text("I wanted to buy a record as a gift as a present", &[], &[],),
            "I wanted to buy a record as a present."
        );
    }
    #[test]
    fn formats_explicit_lists_and_keeps_items_independent() {
        for (raw, expected) in [
            (
                "bullet point. apples. next bullet. pears.",
                "- Apples.\n- Pears.",
            ),
            (
                "bullet point apples next bullet pears scratch that",
                "- Apples",
            ),
            ("email ops at sign example dot com", "Email ops@example.com"),
            (
                "open https://example.com/period",
                "Open https://example.com/period",
            ),
            ("ops@example.com", "ops@example.com"),
            (
                "shopping list colon bullet point apples next bullet pears end list thank you",
                "Shopping list:\n- Apples\n- Pears\n\nThank you.",
            ),
            (
                "number one check the build number two ship it number three monitor",
                "1. Check the build\n2. Ship it\n3. Monitor",
            ),
            (
                "bullet point apples next bullet oranges scratch that pears",
                "- Apples\n- Pears",
            ),
            (
                "Keep this. Meet Tuesday scratch that meet Wednesday",
                "Keep this. Meet Wednesday.",
            ),
            (
                "I like this and I had had enough",
                "I like this and I had had enough.",
            ),
            (
                "Version 1.2.3 costs $4.50 at 2:30. Email a.b@example.com",
                "Version 1.2.3 costs $4.50 at 2:30. Email a.b@example.com",
            ),
            (
                "read https://example.com/a?x=1.2",
                "Read https://example.com/a?x=1.2",
            ),
            ("first line\n\nsecond line", "First line\n\nSecond line."),
            ("Budget [2026] stays", "Budget [2026] stays."),
            (
                "number one first number three third",
                "Number one first number three third.",
            ),
            (
                "I want coffee and cake I want tea",
                "I want coffee and cake I want tea.",
            ),
            (
                "um I do not like that uh please keep it",
                "I do not like that please keep it.",
            ),
            (
                "bullet point not today next bullet no thanks",
                "- Not today\n- No thanks",
            ),
        ] {
            assert_eq!(clean_text(raw, &[], &[]), expected, "{raw}");
        }
    }

    #[test]
    fn dictionary_and_snippets_are_literal_and_longest_match_wins() {
        let dictionary = [
            DictionaryEntry {
                id: "1".into(),
                phrase: "c++".into(),
                replacement: "C++".into(),
                enabled: true,
                created_at: Utc::now(),
            },
            DictionaryEntry {
                id: "2".into(),
                phrase: "dollar macro".into(),
                replacement: "$HOME/$1".into(),
                enabled: true,
                created_at: Utc::now(),
            },
        ];
        let snippets = [
            Snippet {
                id: "1".into(),
                trigger: "sign off".into(),
                body: "thanks,\nBilly ($1)".into(),
                enabled: true,
                created_at: Utc::now(),
            },
            Snippet {
                id: "2".into(),
                trigger: "sign".into(),
                body: "WRONG".into(),
                enabled: true,
                created_at: Utc::now(),
            },
        ];
        assert_eq!(
            clean_text(
                "use c++ with dollar macro then sign off",
                &dictionary,
                &snippets
            ),
            "Use C++ with $HOME/$1 then thanks,\nBilly ($1)"
        );
        assert_eq!(clean_text("c++17", &dictionary, &[]), "C++17.");
        assert_eq!(
            clean_text("sign off", &[], &snippets),
            "thanks,\nBilly ($1)"
        );
        let code = [Snippet {
            body: "return $1;\n}".into(),
            ..snippets[0].clone()
        }];
        assert_eq!(clean_text("sign off.", &[], &code), "return $1;\n}");
        let tied = [
            Snippet {
                trigger: "same phrase".into(),
                body: "first".into(),
                ..snippets[0].clone()
            },
            Snippet {
                trigger: "same phrase".into(),
                body: "second".into(),
                ..snippets[0].clone()
            },
        ];
        assert_eq!(clean_text("same phrase", &[], &tied), "first");

        let mut long_entries = vec![("common phrase".to_string(), "COMMON".to_string())];
        long_entries.extend((0..255).map(|index| {
            (
                format!("rare dictionary term {index}"),
                format!("replacement {index}"),
            )
        }));
        let long_value = std::iter::repeat_n("common phrase", 2_048)
            .collect::<Vec<_>>()
            .join(" ");
        let started = Instant::now();
        let long_result = apply_replacements(
            &long_value,
            long_entries
                .iter()
                .map(|(from, to)| (from.as_str(), to.as_str())),
            false,
        );
        eprintln!(
            "cleanup-long-replacement elapsed_ms={} patterns={} matches={}",
            started.elapsed().as_secs_f64() * 1_000.0,
            long_entries.len(),
            long_result.matches("COMMON").count(),
        );
        assert_eq!(long_result.matches("COMMON").count(), 2_048);
    }
}
