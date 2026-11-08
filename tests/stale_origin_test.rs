//! Picking what to review, and finding the evidence for it.
//!
//! Neither of these decides anything. That is the point: judging whether a
//! note is wrong needs a model, and a binary that called one would need an
//! API key of its own. What is tested here is only that the mechanical half
//! is honest — that the queue says what its signal means, and that a memory
//! with no transcript is reported as having none rather than as an error.

use anathem::{db, model::NewMemory, stale};
use std::path::PathBuf;

fn insert(conn: &rusqlite::Connection, project: &str, body: &str) -> i64 {
    anathem::ingest::insert(conn, &NewMemory {
        project: project.into(),
        title: body.chars().take(30).collect(),
        body: body.into(),
        created: "2026-11-08".into(),
        event_start: None,
        event_end: None,
        source: "migrated".into(),
        ext_id: None,
        tags: vec![],
        refs: vec![],
    })
    .unwrap()
}

fn abierta(conn: &rusqlite::Connection, id: i64, cuando: &str) {
    conn.execute("update memories set last_accessed = ?2 where id = ?1",
        rusqlite::params![id, cuando]).unwrap();
}

const LARGA: &str = "the deployment pipeline runs nightly on the build host and \
                     reports into the release channel when it finishes";

#[test]
fn never_opened_comes_before_opened_long_ago() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let vieja = insert(&conn, "p", &format!("{LARGA} one"));
    let nunca = insert(&conn, "p", &format!("{LARGA} two"));
    let reciente = insert(&conn, "p", &format!("{LARGA} three"));
    abierta(&conn, vieja, "2026-01-01T00:00:00+00:00");
    abierta(&conn, reciente, "2026-11-01T00:00:00+00:00");

    let v = stale::never_read(&conn, None, 10).unwrap();
    let ids: Vec<i64> = v.iter().map(|s| s.id).collect();
    assert_eq!(ids[0], nunca, "the one nobody opened has to come first: {ids:?}");
    assert_eq!(ids[1], vieja, "then the oldest opening: {ids:?}");
    assert_eq!(ids[2], reciente);
    assert!(v[0].last_accessed.is_none());
}

#[test]
fn it_can_be_narrowed_to_one_project() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    insert(&conn, "uno", &format!("{LARGA} a"));
    insert(&conn, "dos", &format!("{LARGA} b"));
    let v = stale::never_read(&conn, Some("uno"), 10).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].project, "uno");
}

#[test]
fn a_forgotten_memory_is_not_a_candidate() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let id = insert(&conn, "p", &format!("{LARGA} x"));
    anathem::maint::forget(&conn, id).unwrap();
    assert!(stale::never_read(&conn, None, 10).unwrap().is_empty(),
        "something out of search has no business in a review queue");
}

// -- origin --------------------------------------------------------------

/// The needle has to come from prose, not from a heading: a short line
/// matches half the corpus and would point at the wrong session.
#[test]
fn the_needle_skips_short_lines() {
    let body = format!("# a heading\n- short\n{LARGA}");
    let n = stale::needle(&body).unwrap();
    assert!(n.starts_with("the deployment pipeline"), "took the wrong line: {n}");
    assert!(!n.contains("heading"));
}

#[test]
fn a_body_with_no_long_line_has_no_needle() {
    assert!(stale::needle("short\n- also short\n# tiny").is_none());
}

fn transcripcion(dir: &std::path::Path, nombre: &str, lineas: &[&str]) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(nombre);
    std::fs::write(&p, lineas.join("\n")).unwrap();
    p
}

#[test]
fn it_finds_the_file_and_the_line() {
    let d = tempfile::tempdir().unwrap();
    let sub = d.path().join("proyecto-x");
    let f = transcripcion(&sub, "sesion.jsonl", &[
        r#"{"type":"user","text":"hola"}"#,
        r#"{"type":"assistant","text":"algo"}"#,
        &format!(r#"{{"type":"tool_use","name":"save_memory","input":{{"text":"{LARGA}"}}}}"#),
        r#"{"type":"user","text":"gracias"}"#,
    ]);
    let n = stale::needle(LARGA).unwrap();
    let hits = stale::find_origin(&[d.path().to_path_buf()], &n).unwrap();
    assert_eq!(hits.len(), 1, "exactly one transcript holds it: {hits:?}");
    assert_eq!(hits[0].file, f);
    assert_eq!(hits[0].line, 3, "the line is 1-based so it can be pasted after a colon");
}

/// A note typed by a person in their own terminal is in no log. That is the
/// honest answer, not a failure.
#[test]
fn nothing_found_is_an_empty_list_and_not_an_error() {
    let d = tempfile::tempdir().unwrap();
    transcripcion(&d.path().join("x"), "s.jsonl", &[r#"{"text":"something else"}"#]);
    let hits = stale::find_origin(&[d.path().to_path_buf()], "a needle nobody wrote").unwrap();
    assert!(hits.is_empty());
}

#[test]
fn a_root_that_does_not_exist_is_skipped() {
    let hits = stale::find_origin(&[PathBuf::from("/no/such/place/at/all")], "x").unwrap();
    assert!(hits.is_empty(), "a missing directory must not be fatal");
}

#[test]
fn only_jsonl_is_searched() {
    let d = tempfile::tempdir().unwrap();
    let sub = d.path().join("x");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("notas.md"), LARGA).unwrap();
    let n = stale::needle(LARGA).unwrap();
    assert!(stale::find_origin(&[d.path().to_path_buf()], &n).unwrap().is_empty(),
        "a markdown file next to the logs is not a transcript");
}

/// Transcripts hold whatever was typed or pasted, so a line that is not
/// valid UTF-8 is possible and must not stop the search before the match.
#[test]
fn invalid_utf8_does_not_stop_the_search() {
    let d = tempfile::tempdir().unwrap();
    let sub = d.path().join("x");
    std::fs::create_dir_all(&sub).unwrap();
    let mut bytes: Vec<u8> = vec![0xff, 0xfe, b'\n'];
    bytes.extend_from_slice(LARGA.as_bytes());
    std::fs::write(sub.join("s.jsonl"), bytes).unwrap();
    let n = stale::needle(LARGA).unwrap();
    let hits = stale::find_origin(&[d.path().to_path_buf()], &n).unwrap();
    assert_eq!(hits.len(), 1, "the bad line should be skipped, not the file");
    assert_eq!(hits[0].line, 2);
}

#[test]
fn several_transcripts_come_back_sorted() {
    let d = tempfile::tempdir().unwrap();
    for nombre in ["b.jsonl", "a.jsonl"] {
        transcripcion(&d.path().join("x"), nombre, &["filler", LARGA]);
    }
    let n = stale::needle(LARGA).unwrap();
    let hits = stale::find_origin(&[d.path().to_path_buf()], &n).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits[0].file < hits[1].file, "a stable order makes the output diffable");
}

#[test]
fn the_default_root_is_where_claude_code_writes() {
    let roots = stale::default_roots();
    if let Some(r) = roots.first() {
        assert!(r.ends_with("projects"), "unexpected default: {}", r.display());
        assert!(r.to_string_lossy().contains(".claude"));
    }
}
