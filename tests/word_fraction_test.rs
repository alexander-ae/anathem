//! How much of the query a hit actually contains, and the stopword that used
//! to hide the answer.
//!
//! The warning for "nothing here matched" used to read `bm25.is_some()`,
//! which asks whether anything matched at all rather than whether anything
//! useful did. One `de` in the query gave every memory in a Spanish corpus a
//! score, so the warning went silent for exactly the questions phrased the
//! way a person asks them — and stayed loud for the keyword queries that
//! least needed it.
//!
//! None of this needs a pack. The hits here come back through bm25 on the
//! stopword alone, which is the whole point: the corpus answers a question
//! it knows nothing about, with no vector layer involved.

use anathem::{config::Config, db, mcp, model::NewMemory, search};
use serde_json::json;

/// Enough memories to put the corpus over `MIN_CHUNKS_FOR_DF`, so document
/// frequency means something. Every one of them carries `de`, as a Spanish
/// corpus does.
const RELLENO: usize = 40;

fn insert(conn: &rusqlite::Connection, ext: &str, body: &str) {
    anathem::ingest::insert(conn, &NewMemory {
        project: "p".into(),
        title: body.chars().take(30).collect(),
        body: body.into(),
        created: "2026-11-07".into(),
        event_start: None,
        event_end: None,
        source: "migrated".into(),
        ext_id: Some(ext.into()),
        tags: vec![],
        refs: vec![],
    })
    .unwrap();
}

/// A corpus big enough for frequency to carry information, plus whatever the
/// caller wants in it.
fn corpus(extra: &[(&str, &str)]) -> (tempfile::TempDir, rusqlite::Connection) {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    for i in 0..RELLENO {
        insert(&conn, &format!("f{i}"), &format!("nota {i} de la semana sobre el equipo"));
    }
    for (ext, body) in extra {
        insert(&conn, ext, body);
    }
    (d, conn)
}

fn answer(conn: &rusqlite::Connection, query: &str) -> String {
    let ctx = search::SearchContext::bm25_only();
    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "search_memory",
                   "arguments": {"query": query, "all_projects": true}}
    });
    let res = mcp::handle(conn, &ctx, &Config::default(), &req).unwrap().unwrap();
    res["result"]["content"][0]["text"].as_str().unwrap().to_string()
}

fn hits(out: &str) -> Vec<&str> {
    out.lines().filter(|l| l.starts_with("[id:")).collect()
}

/// The defect itself: one stopword in the query used to turn the warning off
/// for every hit in the answer.
#[test]
fn one_stopword_does_not_silence_the_warning() {
    let (_d, conn) = corpus(&[]);
    let out = answer(&conn, "de quuxly frobnicate");
    let h = hits(&out);
    assert!(!h.is_empty(), "the stopword should still bring the corpus back:\n{out}");
    for l in &h {
        assert!(
            l.contains("[no word match]"),
            "a hit that matched only the stopword came back unlabelled:\n{l}"
        );
    }
}

/// Without the stopword the warning always worked. It has to keep working.
#[test]
fn the_same_query_without_the_stopword_is_still_labelled() {
    let (_d, conn) = corpus(&[]);
    let out = answer(&conn, "quuxly frobnicate");
    for l in hits(&out) {
        assert!(l.contains("[no word match]"), "a hit nothing matched went unlabelled:\n{l}");
    }
}

/// A query made of nothing but words the whole corpus shares says nothing,
/// and the answer should say so rather than present its ranking as findings.
#[test]
fn a_query_of_only_common_words_is_all_labelled() {
    let (_d, conn) = corpus(&[]);
    let out = answer(&conn, "de la el");
    let h = hits(&out);
    assert!(!h.is_empty(), "expected the corpus to answer anyway:\n{out}");
    for l in &h {
        assert!(l.contains("[no word match]"), "a query that says nothing got a clean hit:\n{l}");
    }
}

/// The part the boolean could never say: half the question landed.
#[test]
fn a_partial_match_reports_the_fraction() {
    let (_d, conn) = corpus(&[("x", "zeta frobnicate y nada mas en esta nota")]);
    let out = answer(&conn, "quuxly frobnicate");
    let line = hits(&out)
        .into_iter()
        .find(|l| l.contains("zeta"))
        .unwrap_or_else(|| panic!("the lexical hit is missing:\n{out}"));
    assert!(
        line.contains("[1/2 words]"),
        "a hit that matched one word of two did not say so:\n{line}"
    );
}

/// And when the whole question landed there is nothing to warn about.
#[test]
fn a_full_match_is_not_labelled() {
    let (_d, conn) = corpus(&[("x", "zeta quuxly y frobnicate en esta nota")]);
    let out = answer(&conn, "quuxly frobnicate");
    let line = hits(&out)
        .into_iter()
        .find(|l| l.contains("zeta"))
        .unwrap_or_else(|| panic!("the lexical hit is missing:\n{out}"));
    assert!(!line.contains("words]"), "a hit that matched everything was labelled:\n{line}");
    assert!(!line.contains("[no word match]"), "a full match was called a miss:\n{line}");
}

/// The stopword is dropped for being common, not for being short: `api` and
/// `ci` are the words a stopword list would get wrong, and frequency does
/// not.
#[test]
fn a_short_rare_word_still_counts() {
    let (_d, conn) = corpus(&[("x", "zeta el deploy del api de pagos quedo listo")]);
    let out = answer(&conn, "api frobnicate");
    let line = hits(&out)
        .into_iter()
        .find(|l| l.contains("zeta"))
        .unwrap_or_else(|| panic!("the lexical hit is missing:\n{out}"));
    assert!(line.contains("[1/2 words]"), "a short rare word was thrown away with the stopwords:\n{line}");
}

/// Below the floor, frequency says nothing. In a corpus of two notes a word
/// in both of them is not a stopword, it is a small corpus, and discarding
/// it would leave the search with no evidence at all.
#[test]
fn a_small_corpus_keeps_every_word() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    insert(&conn, "a", "the deployment pipeline runs nightly");
    insert(&conn, "b", "the deployment pipeline runs on merges");
    let out = answer(&conn, "deployment");
    let h = hits(&out);
    assert!(!h.is_empty(), "expected both memories back:\n{out}");
    for l in &h {
        assert!(
            !l.contains("[no word match]"),
            "the only word there was got discarded as common:\n{l}"
        );
    }
}

/// Labelling and not filtering, as before: the hit is still in the answer.
#[test]
fn the_label_does_not_remove_the_hit() {
    let (_d, conn) = corpus(&[]);
    let out = answer(&conn, "de quuxly frobnicate");
    assert!(hits(&out).len() > 1, "labelled hits were dropped from the answer:\n{out}");
}
