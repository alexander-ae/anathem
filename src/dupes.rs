//! Pairs of memories that look like the same fact written twice.
//!
//! This reports and never acts. Merging two memories destroys the one that
//! loses, and deciding which of two overlapping notes is the keeper needs
//! judgement about what they are *for*, which nothing here has. A work list
//! a person can read is wrong harmlessly; an automatic merge is not.
//!
//! What it is **not** is contradiction detection. Two notes can share every
//! word and disagree on the one that matters — `ships on Tuesday` against
//! `does not ship on Tuesday` — and token overlap cannot see the
//! difference. Telling those apart is comprehension, and the only one in
//! reach here is a bag-of-words average already measured to miss paraphrase.
//!
//! The scale this was written for: over a real 682-memory corpus,
//! near-duplicate pairs came to **0.21 %** of all pairs. That number is why
//! nothing automatic was built, and why this is a diagnostic rather than a
//! feature. Anyone finding a much larger share should say so — it would
//! mean the corpora differ in a way worth knowing about.

use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// Tokens shorter than this carry no signal and are skipped. It is a crude
/// stand-in for a stopword list, and deliberately so: anathem indexes with
/// `unicode61` and no language-specific stopwords, so introducing one here
/// would make the diagnostic disagree with the index it is diagnosing.
const MIN_TOKEN: usize = 4;

/// A token in more memories than this is not discriminating, and pairing
/// every memory that shares one would cost the square of the corpus. This is
/// what keeps the work proportional to the rare vocabulary instead.
const MAX_DF: usize = 20;

/// Below this share of tokens in common, two memories are merely about the
/// same subject, which is normal and not worth reporting.
const JACCARD: f64 = 0.5;

/// At most this many pairs are listed. The rest are counted, because a
/// diagnostic that prints two hundred lines is one nobody reads.
pub const MAX_LISTED: usize = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub a: i64,
    pub b: i64,
    pub project: String,
    /// Share of tokens the two have in common, 0 to 1.
    pub overlap: f64,
}

/// The tokens of `text`, lowercased, split on anything not alphanumeric.
///
/// A set and not a bag: repeating a word twenty times in one note should not
/// make it look more like another note that does the same.
fn tokens(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= MIN_TOKEN)
        .map(|t| t.to_lowercase())
        .collect()
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let shared = a.intersection(b).count();
    if shared == 0 {
        return 0.0;
    }
    let total = a.len() + b.len() - shared;
    shared as f64 / total as f64
}

/// Near-duplicate pairs among the active memories, best overlap first.
///
/// Confined to pairs inside one project: two projects describing the same
/// thing in the same words is not duplication, it is two projects, and
/// merging across them is exactly what the scoping in `search_memory`
/// exists to prevent.
pub fn find(conn: &Connection) -> Result<Vec<Pair>> {
    let mut st = conn.prepare(
        "select id, project, title || ' ' || body from memories where status = 'active'",
    )?;
    let rows = st.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })?;

    let mut bodies: HashMap<i64, (String, HashSet<String>)> = HashMap::new();
    // token -> the memories carrying it, so only memories sharing a rare
    // word are ever compared.
    let mut index: HashMap<String, Vec<i64>> = HashMap::new();
    for row in rows {
        let (id, project, text) = row?;
        let set = tokens(&text);
        for t in &set {
            index.entry(t.clone()).or_default().push(id);
        }
        bodies.insert(id, (project, set));
    }

    let mut seen: HashSet<(i64, i64)> = HashSet::new();
    let mut out: Vec<Pair> = Vec::new();
    for ids in index.values() {
        if ids.len() < 2 || ids.len() > MAX_DF {
            continue;
        }
        for (i, &a) in ids.iter().enumerate() {
            for &b in &ids[i + 1..] {
                let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                if !seen.insert((lo, hi)) {
                    continue;
                }
                let (pa, sa) = &bodies[&lo];
                let (pb, sb) = &bodies[&hi];
                if pa != pb {
                    continue;
                }
                let overlap = jaccard(sa, sb);
                if overlap >= JACCARD {
                    out.push(Pair { a: lo, b: hi, project: pa.clone(), overlap });
                }
            }
        }
    }
    out.sort_by(|x, y| {
        y.overlap.partial_cmp(&x.overlap).unwrap_or(std::cmp::Ordering::Equal)
            .then(x.a.cmp(&y.a))
            .then(x.b.cmp(&y.b))
    });
    Ok(out)
}
