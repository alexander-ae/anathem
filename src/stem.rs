//! Making `registro` find `registros`.
//!
//! The index is `unicode61` with no stemming, so a singular against a plural
//! is not a result ranked low — it is no result at all. Measured over the
//! author's 50 queries, adding morphological variants moved recall@5 from
//! 26/50 to 33/50, gaining eight cases and losing one. `docs/search.md`
//! carries the table.
//!
//! **The setting lives in the database and not in `config.json`.** It
//! describes how the rows were written, not how the user likes things, and
//! the two sides have to agree: a query stemmed against an index that is not
//! matches nothing, silently, which is the worst failure this code could
//! have. Keeping it in `meta` means the answer travels with the rows it
//! describes, and `mem reindex --stem` is the only way to change it, because
//! changing it *is* rewriting every chunk.
//!
//! **Both languages or neither.** Stemming English and leaving Spanish alone
//! would make the corpus behave differently depending on which half a note
//! was written in, and this corpus mixes them inside single notes. Each word
//! gets both stems, and the index holds whichever differ from the original.
//! That costs index size and buys not having to guess a language per word.

use anyhow::Result;
use rusqlite::Connection;

/// The `meta` key recording whether the chunks were written with stems.
const KEY: &str = "stemming";

/// A token shorter than this is an abbreviation, a flag or a number, and a
/// stemmer does nothing useful to it — but can still mangle it.
const MIN: usize = 4;

/// Whether this database's index was built with stemming.
///
/// Defaults to off, so every database written before this existed reads
/// correctly without being touched.
pub fn enabled(conn: &Connection) -> bool {
    conn.query_row("select value from meta where key = ?1", [KEY], |r| {
        r.get::<_, String>(0)
    })
    .map(|v| v == "on")
    .unwrap_or(false)
}

/// Records the setting. Only `reindex` should call this, and only together
/// with rewriting every chunk.
pub fn set(conn: &Connection, on: bool) -> Result<()> {
    conn.execute(
        "insert into meta (key, value) values (?1, ?2)
         on conflict(key) do update set value = excluded.value",
        rusqlite::params![KEY, if on { "on" } else { "off" }],
    )?;
    Ok(())
}

/// A word that must reach the index exactly as written.
///
/// These are what BM25 answers best — 88% recall@5 on the lexical half of
/// the eval set — so they are what a stemmer has most to break. Anything
/// carrying a digit or an uppercase letter inside it is a name, an
/// identifier or a version, not prose: `n+1`, `OP-204`, `RateLimitable`,
/// `v0.8.0`, a SHA.
fn is_identifier(w: &str) -> bool {
    let mut chars = w.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return true,
    };
    w.chars().any(|c| c.is_ascii_digit())
        || chars.any(|c| c.is_uppercase())
        || (first.is_uppercase() && w.chars().filter(|c| c.is_uppercase()).count() > 1)
}

/// The text as the index should hold it: the original, plus the stems of its
/// words where those differ.
///
/// The original is kept so an exact query still matches exactly and ranks
/// where it did. The stems are added, never substituted — this widens what
/// the index answers and takes nothing away.
pub fn augment(text: &str) -> String {
    use rust_stemmers::{Algorithm, Stemmer};
    let en = Stemmer::create(Algorithm::English);
    let es = Stemmer::create(Algorithm::Spanish);

    let mut extra: Vec<String> = Vec::new();
    let mut vistos: std::collections::HashSet<String> = std::collections::HashSet::new();
    for w in text.split(|c: char| !c.is_alphanumeric() && c != '+' && c != '-') {
        if w.chars().count() < MIN || is_identifier(w) {
            continue;
        }
        let lower = w.to_lowercase();
        for s in [en.stem(&lower).to_string(), es.stem(&lower).to_string()] {
            if s != lower && s.chars().count() >= MIN && vistos.insert(s.clone()) {
                extra.push(s);
            }
        }
    }
    if extra.is_empty() {
        return text.to_string();
    }
    format!("{text} {}", extra.join(" "))
}

/// What goes into `chunks_fts`, and what a query has to become to match it.
///
/// One function for both sides on purpose: `segment_cjk` already had to run
/// over the query as well as the body, and a second transformation that only
/// ran on one side would make the index unsearchable in exactly the ways
/// nobody tests for.
pub fn indexed_form(text: &str, stemming: bool) -> String {
    let base = crate::chunk::segment_cjk(text);
    if stemming {
        augment(&base)
    } else {
        base
    }
}
