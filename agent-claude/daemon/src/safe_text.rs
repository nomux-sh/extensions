//! Texts that the adapter takes from a hook event and puts in a report: one
//! line, a length limit, and no word that looks like a secret.

/// The largest number of bytes of a message, a title or a tool name in a
/// report of the host.
pub const MAX_LINE: usize = 512;

/// What a word that looks like a secret becomes.
const HIDDEN: &str = "[hidden]";

/// The parts of a variable or option name that say that its value is a
/// secret.
const SECRET_NAMES: [&str; 8] = [
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "API_KEY",
    "APIKEY",
    "AUTH",
    "CREDENTIAL",
];

/// The starts of the keys and tokens of common services.
const SECRET_PREFIXES: [&str; 12] = [
    "sk-",
    "sk_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "AKIA",
    "eyJ",
];

/// The shortest word without a separator that the adapter takes for a key.
const MIN_KEY_LENGTH: usize = 32;

/// `text` as one line of `max` bytes at most: each control character
/// becomes a space, a run of spaces becomes one space, and a longer text is
/// cut at the end of a character with `…`.
#[must_use]
pub fn one_line(text: &str, max: usize) -> String {
    let mut line = String::with_capacity(text.len().min(max));
    for word in text.split(|c: char| c.is_whitespace() || c.is_control()) {
        if word.is_empty() {
            continue;
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
        if line.len() > max {
            break;
        }
    }
    truncated(line, max)
}

/// `text` as [`one_line`], with each word that looks like a secret replaced
/// by `[hidden]`, and the word after an option or a header that takes a
/// secret too.
#[must_use]
pub fn one_line_without_secrets(text: &str, max: usize) -> String {
    let mut words = Vec::new();
    let mut hide_next = false;
    for word in text.split(|c: char| c.is_whitespace() || c.is_control()) {
        if word.is_empty() {
            continue;
        }
        let hidden = hide_next || looks_like_secret(word);
        hide_next = takes_a_secret(word);
        words.push(if hidden { HIDDEN } else { word });
        if words.iter().map(|word| word.len() + 1).sum::<usize>() > max {
            break;
        }
    }
    truncated(words.join(" "), max)
}

/// `true` if `word` looks like a key, a token or a password, or assigns one
/// to a variable or an option.
#[must_use]
pub fn looks_like_secret(word: &str) -> bool {
    let word = word.trim_matches(|c| matches!(c, '"' | '\'' | '`' | '(' | ')' | ','));
    if let Some((name, value)) = word.split_once('=') {
        return !value.is_empty() && (names_a_secret(name) || looks_like_secret(value));
    }
    let word = word.trim_start_matches(['-', '@']);
    SECRET_PREFIXES
        .iter()
        .any(|prefix| word.starts_with(prefix))
        || looks_like_key(word)
}

/// `true` for a word such as `Bearer` or `--token`: the next word is a
/// secret.
fn takes_a_secret(word: &str) -> bool {
    let word = word.trim_matches(|c| matches!(c, '"' | '\'' | ':'));
    word.eq_ignore_ascii_case("bearer")
        || word.eq_ignore_ascii_case("basic")
        || (word.starts_with('-') && !word.contains('=') && names_a_secret(word))
}

fn names_a_secret(name: &str) -> bool {
    let name = name.to_ascii_uppercase().replace('-', "_");
    SECRET_NAMES.iter().any(|part| name.contains(part))
}

/// `true` for a long word of letters and digits, as a key or a hash has.
fn looks_like_key(word: &str) -> bool {
    word.len() >= MIN_KEY_LENGTH
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '='))
        && word.chars().any(|c| c.is_ascii_digit())
        && word.chars().any(|c| c.is_ascii_alphabetic())
}

fn truncated(mut line: String, max: usize) -> String {
    if line.len() <= max {
        return line;
    }
    let mark = if max >= '…'.len_utf8() { "…" } else { "" };
    let end = line
        .char_indices()
        .map(|(index, c)| index + c.len_utf8())
        .take_while(|end| *end + mark.len() <= max)
        .last()
        .unwrap_or(0);
    line.truncate(end);
    line.push_str(mark);
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_becomes_one_line_of_the_limit_at_the_end_of_a_character() {
        assert_eq!(one_line("a\nb\t\u{7}c  d", 100), "a b c d");
        let long = "é".repeat(400);
        let line = one_line(&long, MAX_LINE);
        assert!(line.len() <= MAX_LINE);
        assert!(line.ends_with('…'));
        assert_eq!(one_line("", 10), "");
        assert_eq!(one_line("abc", 1), "a");
        assert_eq!(one_line("abcdef", 4), "a…");
    }

    #[test]
    fn a_word_that_looks_like_a_secret_is_hidden() {
        let line = one_line_without_secrets(
            "curl -H 'Authorization: Bearer abc.def' --token xyz API_KEY=v1 https://x.test sk-ant-123",
            MAX_LINE,
        );
        assert_eq!(
            line,
            "curl -H 'Authorization: Bearer [hidden] --token [hidden] [hidden] https://x.test [hidden]"
        );
        assert!(looks_like_secret("ghp_0123456789abcdef"));
        assert!(looks_like_secret(&format!("{}1", "a".repeat(40))));
        assert!(!looks_like_secret("cargo"));
        assert!(!looks_like_secret("--model=claude-haiku-4-5"));
        assert!(!looks_like_secret("/home/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p1"));
    }
}
