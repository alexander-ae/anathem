//! Secrets do not go in the memory; their address does.
//!
//! A note that says "the deploy token is in `~/.config/deploy/env`" is useful
//! for years. A note that says "the deploy token is `glpat-…`" is a copy of
//! the credential in a second place, and that second place is read into the
//! context of every session that searches, so it reaches the model provider.
//! The database being `0600` says nothing about that trip.
//!
//! The detector is deliberately deterministic: patterns, not a model. It runs
//! on the way in — CLI, MCP, import — so what is already stored is reported
//! by `mem doctor --secrets` and never rewritten behind anyone's back.
use regex::Regex;
use std::sync::OnceLock;

/// Pattern, and what to call it when it matches.
type Rule = (&'static str, fn() -> &'static Regex);

fn re(pattern: &'static str) -> Regex {
    Regex::new(pattern).expect("a malformed secret pattern is a bug, not input")
}

static RULES: OnceLock<Vec<Rule>> = OnceLock::new();

fn rules() -> &'static [Rule] {
    RULES.get_or_init(|| {
        vec![
            ("a PEM private key", || {
                static R: OnceLock<Regex> = OnceLock::new();
                R.get_or_init(|| re(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----"))
            }),
            ("an AWS access key", || {
                static R: OnceLock<Regex> = OnceLock::new();
                R.get_or_init(|| re(r"\bAKIA[0-9A-Z]{16}\b"))
            }),
            ("a GitHub token", || {
                static R: OnceLock<Regex> = OnceLock::new();
                R.get_or_init(|| re(r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})\b"))
            }),
            ("a GitLab token", || {
                static R: OnceLock<Regex> = OnceLock::new();
                R.get_or_init(|| re(r"\bglpat-[A-Za-z0-9_-]{20,}\b"))
            }),
            ("a Slack token", || {
                static R: OnceLock<Regex> = OnceLock::new();
                R.get_or_init(|| re(r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b"))
            }),
            ("an API key", || {
                static R: OnceLock<Regex> = OnceLock::new();
                R.get_or_init(|| re(r"\bsk-[A-Za-z0-9_-]{20,}\b"))
            }),
            ("a JSON Web Token", || {
                static R: OnceLock<Regex> = OnceLock::new();
                R.get_or_init(|| re(r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b"))
            }),
            ("a connection string with a password", || {
                static R: OnceLock<Regex> = OnceLock::new();
                R.get_or_init(|| re(r"[A-Za-z][A-Za-z0-9+.-]*://[^/\s:@]+:[^/\s@]{6,}@"))
            }),
            // Last, and the loosest: `password: …`, `token=…`, `api_key: "…"`.
            // A path after the sign does not match, because `/` and `.` are
            // not in the value class — which is the whole point: where it
            // lives keeps getting saved.
            ("a credential written out", || {
                static R: OnceLock<Regex> = OnceLock::new();
                R.get_or_init(|| {
                    re(r#"(?i)\b(?:password|passwd|secret|token|api[_-]?key)\b\s*[:=]\s*["']?([A-Za-z0-9+/=_-]{16,})"#)
                })
            }),
        ]
    })
}

/// What kind of secret this text carries, if any.
pub fn find(text: &str) -> Option<&'static str> {
    rules()
        .iter()
        .find(|(_, build)| build().is_match(text))
        .map(|(what, _)| *what)
}

/// The same text with every match replaced by `[redacted: …]`.
///
/// Used where refusing is not an option — the search log, which must keep a
/// row for `mem stats` to count — and never on a stored body.
pub fn redact(text: &str) -> String {
    let mut out = text.to_string();
    for (what, build) in rules() {
        out = build().replace_all(&out, format!("[redacted: {what}]").as_str()).into_owned();
    }
    out
}

/// The sentence a person reads when a save is refused.
pub fn refusal(what: &str) -> String {
    format!(
        "Not saved: that is {what}. A credential in a memory is a second copy of it, and \
         it travels: every search that matches it puts it in a session's context, and that \
         goes to the model provider. Save where it lives instead — \"the deploy token is in \
         ~/.config/deploy/env\" — and keep the value in the file. \
         `--allow-secret` saves it anyway if you know why you want that."
    )
}

/// The same refusal for an agent, which has no `--allow-secret` to reach for.
pub fn refusal_from_agent(what: &str) -> String {
    format!(
        "Not saved: that is {what}. A credential in a memory is a second copy of it and it \
         reaches the model provider with every search that matches. Save where it lives \
         instead — \"the deploy token is in ~/.config/deploy/env\" — and never the value."
    )
}
