//! Anchors: what a memory was checked against, and whether it still is.
//!
//! A verified memory is a claim; an anchored one is a claim with an address.
//! The file it was written against is recorded with its SHA-256, and `mem
//! verify --check` recomputes it later. The point is not to grade the memory —
//! it is to let the repository, which changes without asking, contradict it.
//!
//! **Relative paths are resolved against the directory the command runs in**,
//! at save time and at check time. An agent writes and re-checks standing in
//! the repository, so the same relative path means the same file; resolving
//! against the database's directory would make every anchor point at nothing.
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const OK: &str = "ok";
/// The file is there and is not the one the memory was checked against.
pub const MOVED: &str = "moved";
/// The file is not there.
pub const MISSING: &str = "missing";

/// `src/search.rs:fn run_explained` -> (`src/search.rs`, `fn run_explained`).
///
/// The symbol is metadata for now: it is printed and exported, and the hash
/// decides the state. A symbol that has moved within an unchanged file is a
/// note that the line number drifted, not that the memory is wrong.
pub fn parse(spec: &str) -> (String, Option<String>) {
    match spec.split_once(':') {
        // A Windows drive letter is not a symbol separator, and neither is a
        // colon inside a path that keeps going.
        Some((path, symbol))
            if !symbol.is_empty()
                && !symbol.starts_with(['\\', '/'])
                && !(path.len() == 1 && path.chars().all(|c| c.is_ascii_alphabetic())) =>
        {
            (path.to_string(), Some(symbol.to_string()))
        }
        _ => (spec.to_string(), None),
    }
}

fn resolve(root: &Path, spec: &str) -> PathBuf {
    let (path, _) = parse(spec);
    let p = PathBuf::from(&path);
    if p.is_absolute() {
        p
    } else {
        root.join(p)
    }
}

/// The bytes that decide the state: the symbol's body when one is named and
/// can be found, the whole file otherwise.
///
/// The whole file is too coarse to mean anything. Over eight public
/// repositories and 219,138 (commit, file) pairs (`tools/anchor_decay.py`),
/// **95.4 % of file-level anchors were already stale at HEAD**: an alarm that
/// fires nineteen times out of twenty is not a signal. The body of the
/// function a memory talks about holds far longer — unchanged 100 % within
/// seven days, 94 % within a month, 90 % within three, 79 % within a year —
/// and then it decays too, which is why an anchor is a check and not a
/// guarantee.
///
/// Returns `(hash, the symbol was found)`; the second value is what tells a
/// renamed function apart from a changed one.
fn fingerprint(file: &Path, symbol: Option<&str>) -> Result<(String, bool)> {
    let bytes = std::fs::read(file)
        .with_context(|| format!("could not read {}", file.display()))?;
    if let Some(sym) = symbol {
        if let Ok(text) = std::str::from_utf8(&bytes) {
            if let Some(block) = symbol_body(text, sym) {
                return Ok((hex(block.as_bytes()), true));
            }
        }
        // The file is there and the symbol is not: the caller reads that as
        // missing, so a renamed function is not reported as "changed".
        return Ok((hex(&bytes), false));
    }
    Ok((hex(&bytes), true))
}

fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// From the body's first `{` to its matching `}`. `None` when the name is not
/// there, or when what follows it is a call and not a definition.
fn symbol_body<'a>(text: &'a str, symbol: &str) -> Option<&'a str> {
    let name = symbol
        .rsplit(|c: char| c.is_whitespace() || c == ':')
        .next()
        .unwrap_or(symbol);
    if name.is_empty() {
        return None;
    }
    let mut from = 0;
    while let Some(rel) = text[from..].find(name) {
        let at = from + rel;
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        let boundary = !matches!(before, Some(c) if c.is_alphanumeric() || c == '_')
            && !matches!(after, Some(c) if c.is_alphanumeric() || c == '_');
        if boundary {
            if let Some(rel_open) = text[at..].find('{') {
                let start = at + rel_open;
                let head = &text[at..start];
                // A definition puts its arguments between the name and the
                // body; a call ends with `;` and its block belongs to
                // whatever encloses it.
                if head.chars().count() < 300 && !head.contains(';') {
                    if let Some(end) = matching_brace(text, start) {
                        return Some(&text[start..=end]);
                    }
                }
            }
        }
        from = at + name.len();
        if from >= text.len() {
            break;
        }
    }
    None
}

fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in text[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Records the anchor, and refuses one it cannot read.
///
/// An anchor to a file that is not there is a claim nobody can check; saving
/// it would produce a memory that looks watched and is not.
pub fn set(conn: &Connection, memory_id: i64, spec: &str, root: &Path) -> Result<String> {
    let file = resolve(root, spec);
    if !file.is_file() {
        anyhow::bail!(
            "the anchor points at {}, which is not a file: an anchor has to be checkable",
            file.display()
        );
    }
    let (path, symbol) = parse(spec);
    let (sha, _) = fingerprint(&file, symbol.as_deref())?;
    conn.execute(
        "insert into memory_anchors (memory_id, path, symbol, sha256, checked_at, state)
         values (?1, ?2, ?3, ?4, datetime('now'), 'ok')
         on conflict(memory_id) do update set
           path = excluded.path,
           symbol = excluded.symbol,
           sha256 = excluded.sha256,
           checked_at = excluded.checked_at,
           state = 'ok'",
        params![memory_id, path, symbol, sha],
    )?;
    Ok(sha)
}

/// Re-reads one anchor and records what it finds. `Ok(None)` when the memory
/// has no anchor: nothing to check is not a failure.
pub fn check(conn: &Connection, memory_id: i64, root: &Path) -> Result<Option<String>> {
    let Some((path, symbol, sha)) = anchor_of(conn, memory_id)? else {
        return Ok(None);
    };
    let file = resolve(root, &path);
    let state = if !file.is_file() {
        MISSING
    } else {
        let (now, found) = fingerprint(&file, symbol.as_deref())?;
        if !found {
            MISSING
        } else if now == sha {
            OK
        } else {
            MOVED
        }
    };
    conn.execute(
        "update memory_anchors set state = ?2, checked_at = datetime('now') where memory_id = ?1",
        params![memory_id, state],
    )?;
    Ok(Some(state.to_string()))
}

/// Checks every anchor in the database: (ok, moved, missing).
pub fn check_all(conn: &Connection, root: &Path) -> Result<(usize, usize, usize)> {
    let ids: Vec<i64> = conn
        .prepare("select memory_id from memory_anchors order by memory_id")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let (mut ok, mut moved, mut missing) = (0, 0, 0);
    for id in ids {
        match check(conn, id, root)?.as_deref() {
            Some(OK) => ok += 1,
            Some(MOVED) => moved += 1,
            Some(MISSING) => missing += 1,
            _ => {}
        }
    }
    Ok((ok, moved, missing))
}

/// `(path, symbol, sha256)` for one memory, when it has an anchor.
pub fn anchor_of(conn: &Connection, memory_id: i64) -> Result<Option<(String, Option<String>, String)>> {
    Ok(conn
        .query_row(
            "select path, symbol, sha256 from memory_anchors where memory_id = ?1",
            [memory_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
}

/// Everything stored about one anchor: (path, symbol, sha256, state, checked_at).
pub type Row = (String, Option<String>, String, String, String);

pub fn get(conn: &Connection, memory_id: i64) -> Result<Option<Row>> {
    Ok(conn
        .query_row(
            "select path, symbol, sha256, state, checked_at
               from memory_anchors where memory_id = ?1",
            [memory_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?)
}

/// (anchored, stale) for the whole database, for `mem doctor`.
pub fn counts(conn: &Connection) -> Result<(i64, i64)> {
    Ok(conn.query_row(
        "select count(*), coalesce(sum(state != 'ok'), 0) from memory_anchors",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}
