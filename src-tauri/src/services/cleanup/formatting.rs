use std::sync::LazyLock;

use crate::services::list_formatting;
use regex::Regex;

static TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\[[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{3}\s+-->\s+[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{3}\]",
    )
    .unwrap()
});
static FILLER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:um+|uh+|erm)\b[, ]*").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]{2,}").unwrap());
static BEFORE_PUNCT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+([,.;:!?])").unwrap());
static OPEN_PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([ \t]+").unwrap());
static CLOSE_PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+\)").unwrap());
static NEWLINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());
static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b([a-z0-9._%+-]+)[ \t]+at sign[ \t]+([a-z0-9-]+(?:[ \t]+(?:period|dot)[ \t]+[a-z0-9-]+)+)\b").unwrap()
});
static DOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)[ \t]+(?:period|dot)[ \t]+").unwrap());
static SPOKEN: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
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
    ]
    .into_iter()
    .map(|(from, to)| {
        (
            Regex::new(&format!(r"(?i)\b{}\b", regex::escape(from))).unwrap(),
            to,
        )
    })
    .collect()
});

pub(super) fn normalize_spoken_punctuation(value: &str) -> String {
    let addresses = EMAIL.replace_all(value, |caps: &regex::Captures<'_>| {
        format!("{}@{}", &caps[1], DOT.replace_all(&caps[2], "."))
    });
    SPOKEN
        .iter()
        .fold(addresses.to_string(), |text, (pattern, replacement)| {
            // ASR may punctuate a spoken command itself ("new line.").
            let mut output = String::new();
            let mut cursor = 0;
            for found in pattern.find_iter(&text) {
                let before = text[..found.start()].chars().next_back();
                let after = text[found.end()..].chars().next();
                if before.is_some_and(|ch| !ch.is_whitespace() && !"([{".contains(ch))
                    || after.is_some_and(|ch| !ch.is_whitespace() && !".,!?;:)]}".contains(ch))
                {
                    continue;
                }
                output.push_str(&text[cursor..found.start()]);
                output.push_str(replacement);
                cursor = found.end();
                if text[cursor..].starts_with(['.', ',']) {
                    cursor += 1;
                }
            }
            output.push_str(&text[cursor..]);
            output
        })
}

pub(super) fn normalize_spacing(value: &str) -> String {
    let lines = value
        .trim()
        .lines()
        .map(|line| {
            let text = SPACES.replace_all(line.trim(), " ");
            let text = BEFORE_PUNCT.replace_all(&text, "$1");
            let text = OPEN_PAREN.replace_all(&text, "(");
            CLOSE_PAREN.replace_all(&text, ")").to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    NEWLINES.replace_all(&lines, "\n\n").to_string()
}

pub(super) fn sentence_case_sentences(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            let start = list_formatting::content_start(line);
            let mut output = line[..start].to_string();
            let first = line[start..].split_whitespace().next().unwrap_or("");
            let mut uppercase = !first.contains('@') && !first.contains("://");
            let mut chars = line[start..].chars().peekable();
            while let Some(ch) = chars.next() {
                if uppercase && ch.is_alphabetic() {
                    output.extend(ch.to_uppercase());
                    uppercase = false;
                } else {
                    output.push(ch);
                    if matches!(ch, '.' | '!' | '?')
                        && chars.peek().is_none_or(|next| next.is_whitespace())
                    {
                        uppercase = true;
                    } else if !ch.is_whitespace() && !matches!(ch, '"' | '\'' | '(' | '[' | '{') {
                        uppercase = false;
                    }
                }
            }
            output
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn ensure_terminal_punctuation(value: &str) -> String {
    let last_line = value.lines().last().unwrap_or("");
    let last_word = last_line.split_whitespace().last().unwrap_or("");
    let list_item =
        list_formatting::content_start(last_line) > last_line.len() - last_line.trim_start().len();
    if value.is_empty()
        || value.ends_with(['.', '!', '?', ':', '"', '”', '`'])
        || list_item
        || last_word.contains("://")
        || last_word.contains('@')
    {
        value.to_string()
    } else {
        format!("{value}.")
    }
}

pub(super) fn strip_timestamps(value: &str) -> String {
    TIMESTAMP.replace_all(value, "").to_string()
}
pub(super) fn remove_fillers(value: &str) -> String {
    FILLER.replace_all(value, "").to_string()
}
