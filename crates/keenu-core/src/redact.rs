// SPDX-License-Identifier: GPL-3.0-or-later

//! Takes secrets out of text before it reaches a log or a diagnostics report.

/// Field names whose value is a secret, wherever they appear.
const SECRET_NAMES: [&str; 11] = [
    "access_token",
    "refresh_token",
    "id_token",
    "client_secret",
    "api_key",
    "apikey",
    "authorization",
    "password",
    "passwd",
    "secret",
    "code_verifier",
];
const HIDDEN: &str = "[redacted]";

/// `text` with the values of secret fields, bearer tokens, URL passwords and well-known key formats
/// replaced. It errs towards hiding too much: this is for logs, not for reading back.
pub fn redact(text: &str) -> String {
    let mut out = bearer(text);
    for name in SECRET_NAMES {
        out = field(&out, name);
    }
    url_password(&known_keys(&out))
}

/// `Bearer abc` and `Basic abc`.
fn bearer(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let lower = rest.to_ascii_lowercase();
        let found = ["bearer ", "basic "]
            .iter()
            .filter_map(|word| lower.find(word).map(|at| at + word.len()))
            .min();
        let Some(start) = found else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..start]);
        out.push_str(HIDDEN);
        let tail = &rest[start..];
        let end = tail.find(|c: char| !is_token(c)).unwrap_or(tail.len());
        rest = &tail[end..];
    }
}

/// `name=value`, `name: value` and `"name": "value"`, in any case.
fn field(text: &str, name: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let after = from + found + name.len();
        from = after;
        let tail = &text[after..];
        let skipped = tail
            .find(|c: char| !matches!(c, '"' | '\'' | ' ' | '\t'))
            .unwrap_or(tail.len());
        let Some(sep) = tail[skipped..]
            .chars()
            .next()
            .filter(|c| matches!(c, ':' | '='))
        else {
            continue;
        };
        let value = &tail[skipped + sep.len_utf8()..];
        let lead = value
            .find(|c: char| !matches!(c, ' ' | '\t'))
            .unwrap_or(value.len());
        let value = &value[lead..];
        let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''));
        let (open, len) = match quote {
            Some(quote) => {
                let inner = &value[1..];
                (1, inner.find(quote).unwrap_or(inner.len()))
            }
            None => (
                0,
                value
                    .find(|c: char| c.is_whitespace() || matches!(c, '&' | ',' | ';' | '}' | ')'))
                    .unwrap_or(value.len()),
            ),
        };
        let start = after + skipped + sep.len_utf8() + lead + open;
        out.push_str(&text[copied..start]);
        out.push_str(HIDDEN);
        copied = start + len;
        from = copied;
    }
    out.push_str(&text[copied..]);
    out
}

/// `sk-...`, `ghp_...` and the like, with no field name to go by.
fn known_keys(text: &str) -> String {
    const PREFIXES: [&str; 6] = ["sk-", "ghp_", "gho_", "github_pat_", "xoxb-", "ya29."];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = PREFIXES.iter().filter_map(|prefix| rest.find(prefix)).min() {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail.find(|c: char| !is_token(c)).unwrap_or(tail.len());
        // Too short to be a key: "task-" in a word, say, keeps its text.
        if end >= 16 {
            out.push_str(HIDDEN);
        } else {
            out.push_str(&tail[..end]);
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// `https://user:password@host`.
fn url_password(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(scheme) = rest.find("://") {
        let after = scheme + 3;
        out.push_str(&rest[..after]);
        let tail = &rest[after..];
        let authority = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '/' | '?' | '#' | '"' | '\''))
            .unwrap_or(tail.len());
        match tail[..authority].rfind('@') {
            Some(at) => {
                let user = tail[..at].split(':').next().unwrap_or_default();
                out.push_str(user);
                if tail[..at].contains(':') {
                    out.push(':');
                    out.push_str(HIDDEN);
                }
                rest = &tail[at..];
            }
            None => rest = tail,
        }
    }
    out.push_str(rest);
    out
}

fn is_token(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '+' | '/' | '=')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn hides(text: &str, secret: &str) {
        let out = redact(text);
        assert!(!out.contains(secret), "{out}");
        assert!(out.contains(HIDDEN), "{out}");
    }

    #[test]
    fn hides_secrets_in_every_shape_they_come_in() {
        hides("Authorization: Bearer abc.def-ghi", "abc.def-ghi");
        hides("authorization: Basic dXNlcjpwYXNz", "dXNlcjpwYXNz");
        hides(r#"{"access_token": "tok123", "x": 1}"#, "tok123");
        hides(r#"{"refresh_token":"tok456"}"#, "tok456");
        hides("POST /token?client_secret=shh&grant=1", "shh");
        hides("password=hunter2 next", "hunter2");
        hides("API_KEY: abc123", "abc123");
        hides("using sk-abcdefghijklmnopqrstuvwxyz", "abcdefghijklmnop");
        hides("https://me:hunter2@dav.example.org/cal", "hunter2");
        hides("code_verifier=verifier123", "verifier123");
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        for text in [
            "Could not reach https://dav.example.org/cal: timed out",
            "the task-list has 4 items",
            "basically fine",
            "a secret garden",
        ] {
            assert_eq!(redact(text), text);
        }
    }

    #[test]
    fn what_is_around_a_secret_stays() {
        assert_eq!(
            redact("before password=x after"),
            format!("before password={HIDDEN} after")
        );
    }
}
