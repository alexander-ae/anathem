//! Making `registro` find `registros`, without breaking what already works.
//!
//! The risk is not the stemmer, it is the two sides disagreeing. An index
//! written with stems and a query that is not — or the reverse — matches
//! nothing, silently, and no exception is raised to notice it by. That is
//! why the setting lives in the database rather than in `config.json`, and
//! why these tests spend most of their effort on the boundary rather than on
//! Spanish grammar.

use anathem::{db, model::NewMemory, search, stem};

fn insert(conn: &rusqlite::Connection, body: &str) -> i64 {
    anathem::ingest::insert(conn, &NewMemory {
        project: "p".into(),
        title: body.chars().take(30).collect(),
        body: body.into(),
        created: "2026-11-10".into(),
        event_start: None,
        event_end: None,
        source: "migrated".into(),
        ext_id: None,
        tags: vec![],
        refs: vec![],
    })
    .unwrap()
}

fn count(conn: &rusqlite::Connection, q: &str) -> usize {
    let query = search::Query { text: q.into(), project: None, ..Default::default() };
    search::run(conn, &query, &search::SearchContext::bm25_only()).unwrap().len()
}

fn db() -> (tempfile::TempDir, rusqlite::Connection) {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    (d, conn)
}

#[test]
fn off_by_default_so_nothing_already_written_changes() {
    let (_d, conn) = db();
    assert!(!stem::enabled(&conn), "a fresh database must not stem until asked");
}

/// The case the whole issue is about. Not "ranks low" — absent.
#[test]
fn a_singular_does_not_find_a_plural_without_stemming() {
    let (_d, conn) = db();
    insert(&conn, "los registros de auditoria quedaron incompletos");
    assert_eq!(count(&conn, "registros"), 1);
    assert_eq!(count(&conn, "registro"), 0, "this is the defect, and it has to be reproducible");
}

#[test]
fn with_stemming_it_does() {
    let (_d, conn) = db();
    insert(&conn, "los registros de auditoria quedaron incompletos");
    stem::set(&conn, true).unwrap();
    anathem::edit::reindex(&conn).unwrap();
    assert_eq!(count(&conn, "registro"), 1, "the singular has to reach the plural now");
    assert_eq!(count(&conn, "registros"), 1, "and the exact word must still work");
}

#[test]
fn english_too_and_not_only_spanish() {
    let (_d, conn) = db();
    insert(&conn, "the deployment pipeline runs nightly on the build host");
    stem::set(&conn, true).unwrap();
    anathem::edit::reindex(&conn).unwrap();
    assert_eq!(count(&conn, "deployments"), 1, "both languages or neither");
}

/// Turning it off has to put the index back, or a user who tries it once is
/// stuck with it.
#[test]
fn it_can_be_turned_off_again() {
    let (_d, conn) = db();
    insert(&conn, "los registros de auditoria quedaron incompletos");
    stem::set(&conn, true).unwrap();
    anathem::edit::reindex(&conn).unwrap();
    assert_eq!(count(&conn, "registro"), 1);
    stem::set(&conn, false).unwrap();
    anathem::edit::reindex(&conn).unwrap();
    assert_eq!(count(&conn, "registro"), 0, "off has to mean off, in the index too");
    assert_eq!(count(&conn, "registros"), 1, "and the exact word survives either way");
}

/// A memory saved *after* the setting was changed has to be written the same
/// way as the ones the reindex rewrote, or half the corpus answers and half
/// does not.
#[test]
fn memories_saved_afterwards_are_written_the_same_way() {
    let (_d, conn) = db();
    stem::set(&conn, true).unwrap();
    insert(&conn, "los registros de auditoria quedaron incompletos");
    assert_eq!(count(&conn, "registro"), 1, "a save after the switch must stem too");
}

// -- what must not be touched -------------------------------------------

/// What BM25 answers best is what a stemmer has most to break: lexical
/// queries are at 88% recall@5 and identifiers are how they get there.
#[test]
fn identifiers_are_left_alone() {
    for w in ["RateLimitable", "OP-204", "v0.8.0", "n+1", "502c79e"] {
        let a = stem::augment(w);
        assert_eq!(a, w, "«{w}» must reach the index exactly as written, got «{a}»");
    }
}

#[test]
fn short_words_are_left_alone() {
    // Three letters or fewer: a flag, an abbreviation, a number. A stemmer
    // does nothing useful to them and can still mangle them.
    assert_eq!(stem::augment("api ci de la"), "api ci de la");
}

#[test]
fn the_original_words_are_kept_not_replaced() {
    let a = stem::augment("registros auditoria");
    assert!(a.starts_with("registros auditoria"), "the exact form has to survive: {a}");
    assert!(a.len() > "registros auditoria".len(), "and stems have to be added: {a}");
}

/// The index and the query go through one function, so a change to either
/// side cannot be made without the other.
#[test]
fn the_indexed_form_is_the_identity_when_off() {
    let t = "los registros de auditoria";
    assert_eq!(stem::indexed_form(t, false), anathem::chunk::segment_cjk(t));
}

#[test]
fn the_setting_survives_reopening_the_database() {
    let d = tempfile::tempdir().unwrap();
    let ruta = d.path().join("t.db");
    {
        let conn = db::open(&ruta).unwrap();
        stem::set(&conn, true).unwrap();
    }
    let conn = db::open(&ruta).unwrap();
    assert!(stem::enabled(&conn), "it describes the rows, so it has to live with them");
}
