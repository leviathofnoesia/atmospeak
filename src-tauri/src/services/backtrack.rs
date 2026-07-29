use regex::Regex;

/// Apply Wispr-style deterministic Backtrack: correction commands, triggered
/// replaces, stutter collapse, and conservative restatement cleanup.
pub fn apply_backtrack(value: &str) -> String {
    let mut text = apply_correction_commands(value);
    text = apply_triggered_backtrack(&text);
    text = collapse_stutter_repeats(&text);
    apply_restate_heuristic(&text)
}

fn apply_correction_commands(value: &str) -> String {
    let correction_pattern = Regex::new(
        r"(?i)\b(scratch that|delete that|never mind|nevermind|go back|forget that)\b",
    )
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

/// Keep prefix + replacement after markers like "actually", "I mean", "wait no".
fn apply_triggered_backtrack(value: &str) -> String {
    let trigger = Regex::new(r"(?i)\b(?:actually|i mean|wait[, ]+no|no[, ]+wait)\b")
        .expect("backtrack trigger regex");
    let mut text = value.to_string();
    let mut guard = 0;
    while guard < 8 {
        guard += 1;
        let Some(found) = trigger.find(&text) else {
            break;
        };
        let before = text[..found.start()].trim_end();
        let after = text[found.end()..]
            .trim_start()
            .trim_start_matches(|c: char| {
                c.is_whitespace() || [',', '.', '!', '?', ';', ':'].contains(&c)
            });
        if after.is_empty() {
            break;
        }
        if !looks_like_self_correction(before, after) {
            let skip_from = found.end();
            let head = text[..skip_from].to_string();
            let tail = apply_triggered_backtrack(&text[skip_from..]);
            return format!("{head}{tail}");
        }
        let prefix = strip_replaced_tail(before, after);
        text = if prefix.is_empty() {
            after.to_string()
        } else {
            format!("{prefix} {after}")
        };
    }
    text
}

fn looks_like_self_correction(before: &str, after: &str) -> bool {
    if before.is_empty() || after.is_empty() {
        return false;
    }
    let before_tokens = tokenize_words(before);
    let after_tokens = tokenize_words(after);
    if before_tokens.is_empty() || after_tokens.is_empty() {
        return false;
    }
    let last_before = before_tokens.last().map(String::as_str).unwrap_or("");
    let first_after = after_tokens.first().map(String::as_str).unwrap_or("");

    if last_before.chars().all(|c| c.is_ascii_digit())
        || first_after.chars().all(|c| c.is_ascii_digit())
    {
        return true;
    }
    if after_tokens.len() == 1 && !before_tokens.is_empty() {
        return true;
    }
    if after_tokens.len() <= 3 && before_tokens.len() >= 2 {
        return true;
    }
    false
}

fn strip_replaced_tail(before: &str, after: &str) -> String {
    let before_tokens = tokenize_words(before);
    let after_tokens = tokenize_words(after);
    if before_tokens.is_empty() {
        return before.trim_end().to_string();
    }
    let drop = if after_tokens.len() == 1 {
        1
    } else if after_tokens.len() <= 3 {
        after_tokens.len().min(before_tokens.len()).min(2)
    } else {
        1
    };
    let keep = before_tokens.len().saturating_sub(drop);
    reconstruct_prefix(before, keep)
}

fn reconstruct_prefix(original: &str, keep_word_count: usize) -> String {
    if keep_word_count == 0 {
        return String::new();
    }
    let mut kept = 0usize;
    let mut end = 0usize;
    let mut in_word = false;
    for (idx, ch) in original.char_indices() {
        let is_word = ch.is_alphanumeric() || ch == '\'';
        if is_word {
            if !in_word {
                in_word = true;
            }
            end = idx + ch.len_utf8();
        } else if in_word {
            kept += 1;
            in_word = false;
            if kept >= keep_word_count {
                break;
            }
            end = idx;
        } else {
            end = idx + ch.len_utf8();
        }
    }
    if in_word {
        kept += 1;
    }
    if kept < keep_word_count {
        return original.trim_end().to_string();
    }
    original[..end].trim_end().to_string()
}

fn collapse_stutter_repeats(value: &str) -> String {
    let mut words: Vec<String> = value.split_whitespace().map(str::to_string).collect();
    if words.len() < 2 {
        return value.to_string();
    }

    let mut changed = true;
    while changed {
        changed = false;
        let mut i = 0;
        while i < words.len() {
            let mut collapsed_here = false;
            let max_len = 4.min(words.len().saturating_sub(i) / 2);
            for phrase_len in (1..=max_len).rev() {
                if i + phrase_len * 2 > words.len() {
                    continue;
                }
                let left = &words[i..i + phrase_len];
                let right = &words[i + phrase_len..i + phrase_len * 2];
                if phrase_words_equal(left, right) {
                    words.drain(i + phrase_len..i + phrase_len * 2);
                    changed = true;
                    collapsed_here = true;
                    break;
                }
            }
            if !collapsed_here {
                i += 1;
            }
        }
    }

    words.join(" ")
}

fn phrase_words_equal(left: &[String], right: &[String]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(a, b)| normalize_token(a) == normalize_token(b))
}

fn apply_restate_heuristic(value: &str) -> String {
    let as_a = Regex::new(r"(?i)\bas a ([a-z]+)\b.*?\bas a ([a-z]+)\b").expect("restate regex");
    if let Some(caps) = as_a.captures(&value.to_ascii_lowercase()) {
        let first = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let second = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        if !first.is_empty() && !second.is_empty() && first != second {
            if let Some(full) = caps.get(0) {
                let before = &value[..full.start()];
                let second_pat =
                    Regex::new(&format!(r"(?i)\bas a {}\b", regex::escape(second))).unwrap();
                if let Some(second_m) = second_pat.find_iter(value).last() {
                    let remainder = &value[second_m.end()..];
                    return format!("{before}as a {second}{remainder}");
                }
            }
        }
    }
    value.to_string()
}

/// Collapse restated 3–4 token phrases that share a prefix but change the ending
/// (e.g. "I want coffee I want tea" → "I want tea").
///
/// Call after spoken-punctuation normalization so phrases like `at sign X at sign Y`
/// are already `@ X @ Y` and are not mistaken for restates.
pub fn apply_short_phrase_restate(value: &str) -> String {
    let tokens: Vec<&str> = value.split_whitespace().collect();
    if tokens.len() < 6 {
        return value.to_string();
    }

    for len in (3..=4).rev() {
        if tokens.len() < len * 2 {
            continue;
        }
        for end in (len * 2..=tokens.len()).rev() {
            let second = &tokens[end - len..end];
            for start in 0..=end - len * 2 {
                let first = &tokens[start..start + len];
                if short_phrase_restate_pair(first, second) {
                    let mut out = Vec::with_capacity(tokens.len() - len);
                    out.extend_from_slice(&tokens[..start]);
                    out.extend_from_slice(second);
                    out.extend_from_slice(&tokens[end..]);
                    return out.join(" ");
                }
            }
        }
    }
    value.to_string()
}

fn short_phrase_restate_pair(first: &[&str], second: &[&str]) -> bool {
    if first.len() != second.len() || first.len() < 3 {
        return false;
    }
    let last = first.len() - 1;
    for i in 0..last {
        let left = normalize_token(first[i]);
        let right = normalize_token(second[i]);
        // Punctuation-only tokens normalize to "" and must not count as a shared prefix.
        if left.is_empty() || right.is_empty() || left != right {
            return false;
        }
    }
    let left_last = normalize_token(first[last]);
    let right_last = normalize_token(second[last]);
    !left_last.is_empty()
        && !right_last.is_empty()
        && left_last != right_last
}

fn tokenize_words(value: &str) -> Vec<String> {
    value
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn normalize_token(token: &str) -> String {
    token
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_stutter_unit() {
        assert_eq!(collapse_stutter_repeats("is not is not"), "is not");
        assert_eq!(
            collapse_stutter_repeats("right there. Right there"),
            "right there."
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
    fn restate_as_a_keeps_second_role() {
        assert_eq!(
            apply_backtrack("Hire me as a doctor as a nurse please"),
            "Hire me as a nurse please"
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
}
