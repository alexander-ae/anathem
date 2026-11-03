use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

struct Rule {
    tag: &'static str,
    pat: &'static str,
}

/// The default taxonomy: a small set of general-purpose engineering tags —
/// no proper nouns and no vocabulary tied to a particular stack or team.
///
/// Each pattern covers both languages, which is not decoration: the rules
/// were first written against a corpus of Spanish memories, and for someone
/// saving in English several of them stayed silent. A tool that claims to
/// classify on its own and tags nothing does not look neutral, it looks
/// broken. `code-review` also knows about merge requests, because this
/// project lives on GitLab, where nobody says pull request.
///
/// Any installation can replace it entirely with a `tags.json` file next to
/// the database; see `load_rules` and the README.
const DEFAULT_RULES: &[Rule] = &[
    Rule { tag: "decision",    pat: r"(?i)\bse decidi[oó]\b|\bse descart[oó]\b|\bse opt[oó] por\b|\bpor qu[eé] se eligi[oó]\b|\bwe (decided|chose|settled on|ruled out)\b|\bdecided (to|against)\b|\btrade-?offs?\b|\brationale\b" },
    Rule { tag: "bug",         pat: r"(?i)\bbugs?\b|\berror(es)?\b|\bfalla(s)?\b|\bregresi[oó]n(es)?\b|\bincidente(s)?\b|\bregressions?\b|\bincidents?\b|\boutages?\b|\bcrash(es|ed)?\b" },
    Rule { tag: "convention",  pat: r"(?i)\bconvenci[oó]n(es)?\b|\best[aá]ndar(es)?\b|\blint\b|\bstyle ?guide\b|\bconventions?\b|\bstandards?\b|\bguidelines?\b" },
    Rule { tag: "performance", pat: r"(?i)\bN\+1\b|\b\d+\s*MB\b|\bqueries?\b|\bconsultas?\b|\blatencia\b|\blatency\b|\bbenchmark\b|\bbloat\b|\bthroughput\b|\bslow(er|ness)?\b" },
    Rule { tag: "testing",     pat: r"(?i)\btests?\b|\btesting\b|\bspecs?\b|\bsuites?\b|\bcobertura\b|\bcoverage\b|\bflaky\b" },
    Rule { tag: "docs",        pat: r"(?i)\bdocs?\b|\bdocumentaci[oó]n\b|\bdocumentation\b|\breadme\b|\bchangelog\b" },
    Rule { tag: "code-review", pat: r"(?i)\bPR\s*#\d+|\bMR\s*!\d+|\bpull requests?\b|\bmerge requests?\b|\breview(?:er|ed|s|ó|o)?\b|\brevis(?:ar|é|ó|ando|or|ión|iones)\b" },
];

/// One `{tag, pattern}` rule exactly as it comes from an overriding
/// `tags.json` file.
#[derive(Deserialize)]
struct OverrideRule {
    tag: String,
    pattern: String,
}

pub type Rules = Vec<(String, Regex)>;

fn default_rules() -> Rules {
    DEFAULT_RULES
        .iter()
        .map(|r| (r.tag.to_string(), Regex::new(r.pat).expect("invalid default tag regex")))
        .collect()
}

/// Compiles the rules from an overriding file. It fails when the file
/// cannot be read, is not valid JSON, is empty, or when some `pattern` does
/// not compile as a regex — in every case the message names the file (and,
/// for an invalid regex, the offending rule too), because whoever wrote a
/// configuration file and sees default behaviour has no way to notice the
/// mistake: there is never a silent fallback to the default rules.
pub fn load_rules_from_file(path: &Path) -> Result<Rules, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("tags: could not read {}: {e}", path.display()))?;
    let entries: Vec<OverrideRule> = serde_json::from_str(&raw)
        .map_err(|e| format!("tags: {} is not valid JSON: {e}", path.display()))?;
    if entries.is_empty() {
        return Err(format!("tags: {} defines no rules", path.display()));
    }
    entries
        .into_iter()
        .map(|r| {
            let re = Regex::new(&r.pattern).map_err(|e| {
                format!(
                    "tags: {}: rule '{}' has an invalid pattern: {e}",
                    path.display(),
                    r.tag
                )
            })?;
            Ok((r.tag, re))
        })
        .collect()
}

/// Loads the active rules for directory `dir` (the database's directory):
/// if `tags.json` exists in `dir`, its rules *completely replace* the
/// defaults; if it does not exist (or `dir` is `None`, as in the unit
/// tests), the defaults are used.
pub fn load_rules(dir: Option<&Path>) -> Result<Rules, String> {
    match dir.map(|d| d.join("tags.json")) {
        Some(path) if path.exists() => load_rules_from_file(&path),
        _ => Ok(default_rules()),
    }
}

/// The directory where `classify`/`extract_refs` will look for
/// `tags.json`. It is set once, with `init`, before the first use of
/// `classify`; later calls to `init` have no effect. If it is never called
/// (as in the tests) the directory stays `None` and the default rules are
/// used.
static DB_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Sets the active database's directory for this run of the binary. It has
/// to be called, if it is called at all, before the first call to
/// `classify`.
pub fn init(db_path: &Path) {
    let _ = DB_DIR.set(db_path.parent().map(|p| p.to_path_buf()));
}

fn active() -> Result<&'static Rules, String> {
    static ACTIVE: OnceLock<Result<Rules, String>> = OnceLock::new();
    ACTIVE
        .get_or_init(|| {
            let dir = DB_DIR.get().cloned().flatten();
            load_rules(dir.as_deref())
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Classifies `body` against the active rules (the defaults, or those from
/// `tags.json` when one is installed — see `init`/`load_rules`).
pub fn classify(body: &str) -> Result<Vec<String>, String> {
    let rules = active()?;
    Ok(rules
        .iter()
        .filter(|(_, re)| re.is_match(body))
        .map(|(t, _)| t.clone())
        .collect())
}

fn re_ticket() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // The generic ticket-id convention (Jira, Linear, GitHub Projects,
    // ...): an uppercase project prefix followed by a number. It assumes no
    // particular prefix.
    R.get_or_init(|| Regex::new(r"\b[A-Z]{2,10}-\d+\b").unwrap())
}
fn re_pr() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\bPR\s*#(\d+)\b").unwrap())
}
fn re_sha() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // Match 8-40 hex characters; validate at least one [a-f] in extract_refs.
    // This avoids lookarounds which the regex crate doesn't support.
    R.get_or_init(|| Regex::new(r"\b[0-9a-f]{8,40}\b").unwrap())
}

pub fn extract_refs(body: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for m in re_ticket().find_iter(body) {
        out.push(("ticket".into(), m.as_str().to_string()));
    }
    for c in re_pr().captures_iter(body) {
        out.push(("pr".into(), c[1].to_string()));
    }
    for m in re_sha().find_iter(body) {
        let s = m.as_str();
        // Validate that it contains at least one hex letter (a-f) to avoid matching pure decimal numbers.
        if s.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F')) {
            out.push(("sha".into(), s.to_string()));
        }
    }
    out.sort();
    out.dedup();
    out
}

fn re_save() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\[SAVE:[^:\]]+:(\d{4}-\d{2}-\d{2})\]\s*").unwrap())
}

fn re_save_end() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // The terminator can be preceded by whitespace (including the newline
    // that normally separates it from the body); we consume it together with
    // the marker so no blank line is left dangling.
    R.get_or_init(|| Regex::new(r"\s*\[/SAVE\]\s*$").unwrap())
}

/// Strips the opening `[SAVE:project:date]` marker and the closing
/// `[/SAVE]`. The opening one was removed from the start; the closing one
/// was not — it survived in 87% of migrated bodies and leaked into MCP
/// search snippets, exported files and the FTS index.
pub fn strip_save_prefix(body: &str) -> (String, Option<String>) {
    let (rest, date) = match re_save().captures(body) {
        Some(c) => {
            let date = c.get(1).map(|m| m.as_str().to_string());
            let rest = re_save().replace(body, "").trim_start().to_string();
            (rest, date)
        }
        None => (body.to_string(), None),
    };
    let rest = re_save_end().replace(&rest, "").trim_end().to_string();
    (rest, date)
}
