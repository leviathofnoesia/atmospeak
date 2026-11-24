use std::sync::LazyLock;

use regex::Regex;

static BULLET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:(?:next|new) bullet(?: point)?|bullet point)\b[.,:;]?").unwrap()
});
static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:number|item) (one|two|three|four|five|six|seven|eight|nine|ten|[1-9][0-9]?)\b[.,:;]?").unwrap()
});
static LIST_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:start (?:a )?)?(?:bulleted|bullet|numbered) list[.,:;]?\s*$").unwrap()
});
static LIST_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bend (?:of )?list\b[.,:;]?").unwrap());

/// Spoken list markers are commands only when they form a list. Ordinary
/// ordinals and numbers stay prose; numbering must begin at one and be sequential.
pub fn format_lists(value: &str) -> String {
    let end = LIST_END.find(value);
    let (body, tail) = end.map_or((value, ""), |end| {
        (&value[..end.start()], &value[end.end()..])
    });
    let formatted = format_bullets(body).or_else(|| format_numbers(body));
    let Some(mut output) = formatted else {
        return value.to_string();
    };
    if !tail.trim().is_empty() {
        output.push_str("\n\n");
        output.push_str(tail.trim());
    }
    output
}

fn prefix(value: &str) -> String {
    LIST_START
        .replace(value.trim_end(), "")
        .trim_end_matches(|ch: char| ch.is_whitespace() || ",;".contains(ch))
        .to_string()
}

fn item(value: &str) -> &str {
    value.trim_matches(|ch: char| ch.is_whitespace() || ",;:".contains(ch))
}

fn format_bullets(value: &str) -> Option<String> {
    let markers: Vec<_> = BULLET.find_iter(value).collect();
    let first = markers.first()?;
    let leading = &value[..first.start()];
    let has_intro = LIST_START.is_match(leading);
    if !has_intro && !leading.trim().is_empty() && !leading.trim_end().ends_with(':') {
        return None;
    }
    // A lone "bullet" in prose is not a list command.
    if markers.len() < 2 && !has_intro {
        return None;
    }
    if markers.iter().any(|marker| {
        let end = value[marker.end()..]
            .find('\n')
            .map_or(value.len(), |idx| marker.end() + idx);
        item(&value[marker.end()..end]).is_empty()
    }) {
        return None;
    }
    let mut output = prefix(&value[..first.start()]);
    for (idx, marker) in markers.iter().enumerate() {
        let end = markers
            .get(idx + 1)
            .map_or(value.len(), |next| next.start());
        let content = item(&value[marker.end()..end]);
        if content.is_empty() {
            return None;
        }
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str("- ");
        output.push_str(content);
    }
    Some(output)
}

fn format_numbers(value: &str) -> Option<String> {
    let markers: Vec<_> = NUMBER.captures_iter(value).collect();
    if markers.len() < 2 {
        return None;
    }
    let leading = &value[..markers[0].get(0)?.start()];
    if !LIST_START.is_match(leading)
        && !leading.trim().is_empty()
        && !leading.trim_end().ends_with(':')
    {
        return None;
    }
    let mut output = prefix(leading);
    for (idx, marker) in markers.iter().enumerate() {
        let number = match marker[1].to_ascii_lowercase().as_str() {
            "one" => 1,
            "two" => 2,
            "three" => 3,
            "four" => 4,
            "five" => 5,
            "six" => 6,
            "seven" => 7,
            "eight" => 8,
            "nine" => 9,
            "ten" => 10,
            value => value.parse().ok()?,
        };
        if number != idx + 1 {
            return None;
        }
        let end = markers
            .get(idx + 1)
            .map_or(value.len(), |next| next.get(0).unwrap().start());
        let content = item(&value[marker.get(0)?.end()..end]);
        if content.is_empty() {
            return None;
        }
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&format!("{number}. {content}"));
    }
    Some(output)
}

pub fn content_start(line: &str) -> usize {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        return indent + 2;
    }
    if let Some((number, _)) = trimmed.split_once(". ") {
        if !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit()) {
            return indent + number.len() + 2;
        }
    }
    indent
}
