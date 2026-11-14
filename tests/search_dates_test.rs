//! `mem search --since` / `--until`, and the same two on `search_memory`.
//!
//! Relevance alone answers a question about the past wrongly. Asked what was
//! decided about the payments provider *before* the migration, the newest
//! note on the subject is the most relevant candidate and the one that does
//! not answer the question. `mem recent` and `mem log` could already bound an
//! answer by time; search could not.
//!
//! What is checked here is mostly the boring half: that nothing changes
//! without a bound, that a date names the whole day it says, and that the
//! memories predating `created_ts` — which have a date and no time — are
//! inside the window and not quietly outside it.

use anathem::{config::Config, db, mcp, model::NewMemory, search};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

/// A memory with the date it says and no `created_ts`, which is the shape
/// everything saved before that column has.
fn dated(conn: &Connection, project: &str, ext: &str, created: &str, body: &str) {
    anathem::ingest::insert(
        conn,
        &NewMemory {
            project: project.into(),
            title: body.chars().take(40).collect(),
            body: body.into(),
            created: created.into(),
            event_start: None,
            event_end: None,
            source: "migrated".into(),
            ext_id: Some(ext.into()),
            tags: vec![],
            refs: vec![],
        },
    )
    .unwrap();
}

/// Gives a memory the full timestamp a memory saved today would carry, so
/// the two shapes can be told apart in the same corpus.
fn with_timestamp(conn: &Connection, ext: &str, ts: &str) {
    let n = conn
        .execute("update memories set created_ts = ?1 where ext_id = ?2", rusqlite::params![ts, ext])
        .unwrap();
    assert_eq!(n, 1, "there is no memory {ext}");
}

/// Three notes about one subject, written on three different days. They
/// share every search term, so only the date can tell them apart: anything
/// that separates them separated them by date.
fn three_eras() -> (tempfile::TempDir, Connection, String) {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let p = "dates".to_string();
    dated(&conn, &p, "old", "2026-03-01", "the payment provider is stripe, decided in march");
    dated(&conn, &p, "mid", "2026-07-15", "the payment provider moved to adyen in july");
    dated(&conn, &p, "new", "2026-10-20", "the payment provider is now mercadopago");
    // The newest one also carries a time of day, the way anything saved
    // today does. The two older ones do not, and must not drop out for it.
    with_timestamp(&conn, "new", "2026-10-20T16:40:00-05:00");
    (d, conn, p)
}

fn call(conn: &Connection, args: Value) -> String {
    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "search_memory", "arguments": args}
    });
    let res = mcp::handle(conn, &search::SearchContext::bm25_only(), &Config::default(), &req)
        .unwrap()
        .unwrap();
    res["result"]["content"][0]["text"].as_str().unwrap().to_string()
}

fn eras_in(answer: &str) -> Vec<&'static str> {
    [("march", "old"), ("july", "mid"), ("mercadopago", "new")]
        .iter()
        .filter(|(word, _)| answer.contains(word))
        .map(|(_, name)| *name)
        .collect()
}

#[test]
fn without_a_bound_nothing_changes() {
    let (_d, conn, p) = three_eras();
    let plain = call(&conn, json!({"query": "payment provider", "project": p}));
    assert_eq!(eras_in(&plain), vec!["old", "mid", "new"], "{plain}");

    // `all` is the word `mem recent` already uses for "no bound", and it has
    // to mean the same here: byte for byte the answer with no bound at all.
    let unbounded = call(&conn, json!({"query": "payment provider", "project": p, "since": "all"}));
    assert_eq!(unbounded, plain, "«all» has to be identical to not filtering");
}

#[test]
fn an_until_date_includes_the_whole_day_it_names() {
    let (_d, conn, p) = three_eras();
    // The 15th of July is the day `mid` was written. Naming it has to
    // include it: a bound read as "before midnight of that day" would drop
    // the memory the person was pointing at.
    let a = call(&conn, json!({"query": "payment provider", "project": p, "until": "2026-07-15"}));
    assert_eq!(eras_in(&a), vec!["old", "mid"], "{a}");

    let b = call(&conn, json!({"query": "payment provider", "project": p, "until": "2026-07-14"}));
    assert_eq!(eras_in(&b), vec!["old"], "{b}");
}

#[test]
fn a_memory_with_no_timestamp_is_still_inside_the_window() {
    let (_d, conn, p) = three_eras();
    // `old` and `mid` have a date and no `created_ts`, which is every memory
    // saved before that column existed. Filtering on `created_ts` alone
    // would leave the oldest part of a corpus permanently outside every
    // window — the part a question about the past is asking for.
    let a = call(&conn, json!({
        "query": "payment provider", "project": p,
        "since": "2026-02-01", "until": "2026-08-01"}));
    assert_eq!(eras_in(&a), vec!["old", "mid"], "{a}");
}

/// The shape `mem migrate` leaves: the original date in `created`, and a
/// `created_ts` stamped with the instant of the migration.
///
/// This is the case that decides which column a date bound compares against.
/// Through `coalesce(created_ts, created)` a whole imported corpus reads as
/// "written today", and `--until 2026-09-01` over it would answer "nothing
/// is filed under that window" about a corpus made entirely of older
/// memories — the one answer a question about the past must not get wrong.
#[test]
fn a_migrated_memory_is_found_by_the_date_it_carries_and_not_by_its_import() {
    let (_d, conn, p) = three_eras();
    dated(&conn, &p, "brought", "2026-04-10", "the payment provider was checked by finance");
    with_timestamp(&conn, "brought", &chrono::Local::now().to_rfc3339());

    let a = call(&conn, json!({
        "query": "payment provider", "project": p, "until": "2026-06-01"}));
    assert!(a.contains("finance"), "the memory date is April's, not today's:\n{a}");

    let b = call(&conn, json!({
        "query": "payment provider", "project": p, "since": "2026-06-01"}));
    assert!(!b.contains("finance"), "it cannot come in by the date it was imported:\n{b}");
}

#[test]
fn the_newest_one_is_reachable_by_its_own_day() {
    let (_d, conn, p) = three_eras();
    // This one does have a time of day. A date bound compares against a
    // timestamp here, which is where an off-by-one day shows up.
    let a = call(&conn, json!({"query": "payment provider", "project": p, "since": "2026-10-20"}));
    assert_eq!(eras_in(&a), vec!["new"], "{a}");

    let b = call(&conn, json!({
        "query": "payment provider", "project": p,
        "since": "2026-10-20", "until": "2026-10-20"}));
    assert_eq!(eras_in(&b), vec!["new"], "a single day has to contain itself: {b}");
}

#[test]
fn windows_narrow_monotonically() {
    let (_d, conn, p) = three_eras();
    // Nothing in the shorter window may be missing from the longer one. The
    // dates here are fixed and the windows are not, so this is the one
    // assertion that is about `now`.
    let wide = eras_in(&call(&conn, json!({"query": "payment provider", "project": p, "since": "3650d"})));
    let narrow = eras_in(&call(&conn, json!({"query": "payment provider", "project": p, "since": "7d"})));
    for e in &narrow {
        assert!(wide.contains(e), "{e} is in the short window and not in the long one");
    }
    assert_eq!(wide.len(), 3, "ten years have to cover everything: {wide:?}");
}

#[test]
fn a_date_and_a_scope_combine_in_either_order() {
    let (_d, conn, p) = three_eras();
    dated(&conn, "otro", "elsewhere", "2026-07-16", "the payment provider there is paypal");

    let a = call(&conn, json!({
        "query": "payment provider", "scope": [format!("project:{p}")], "until": "2026-08-01"}));
    assert_eq!(eras_in(&a), vec!["old", "mid"], "{a}");
    assert!(!a.contains("paypal"), "the scope has to keep filtering: {a}");

    let b = call(&conn, json!({
        "query": "payment provider", "project": p, "since": "2026-07-01", "until": "2026-08-01"}));
    assert_eq!(eras_in(&b), vec!["mid"], "{b}");
}

#[test]
fn an_empty_window_says_so_instead_of_blaming_the_query() {
    let (_d, conn, p) = three_eras();
    let a = call(&conn, json!({
        "query": "payment provider", "project": p,
        "since": "2026-01-01", "until": "2026-02-01"}));
    // The window is what came back empty, not the wording, and the answer
    // has to name it: otherwise somebody rephrases a query that was never
    // the problem.
    assert!(a.contains("since:2026-01-01"), "{a}");
    assert!(a.contains("until:2026-02-01"), "{a}");
}

#[test]
fn a_date_that_cannot_be_read_comes_back_explained() {
    let (_d, conn, p) = three_eras();
    // Never a filter quietly dropped: without the bound this query returns
    // three memories from three different years, which looks exactly like a
    // search that worked.
    let a = call(&conn, json!({"query": "payment provider", "project": p, "since": "ayer"}));
    assert!(a.starts_with("Invalid since"), "{a}");
    assert!(a.contains("2026-09-01"), "the error has to say what it does accept: {a}");
    assert!(!a.contains("stripe"), "it cannot answer as if it had filtered: {a}");

    let b = call(&conn, json!({"query": "payment provider", "project": p, "until": "2026-13-45"}));
    assert!(b.starts_with("Invalid until"), "{b}");
}

// ------------------------------------------------------------- the CLI

fn mem(dir: &Path, args: &[&str]) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(dir.join("t.db"))
        .args(args)
        .output()
        .unwrap();
    (o.status.success(), String::from_utf8_lossy(&o.stdout).to_string()
        + &String::from_utf8_lossy(&o.stderr))
}

#[test]
fn the_cli_spells_the_window_the_same_way() {
    let (d, conn, p) = three_eras();
    drop(conn);

    let (ok, out) = mem(d.path(), &["search", "payment provider", "--project", &p, "--until", "2026-07-15"]);
    assert!(ok, "{out}");
    assert!(out.contains("july"), "{out}");
    assert!(!out.contains("mercadopago"), "{out}");

    let (ok, out) = mem(d.path(), &["search", "payment provider", "--project", &p, "--since", "2026-10-01"]);
    assert!(ok, "{out}");
    assert!(out.contains("mercadopago"), "{out}");
    assert!(!out.contains("march"), "{out}");

    // An unreadable window stops the command instead of answering about the
    // wrong era.
    let (ok, out) = mem(d.path(), &["search", "payment provider", "--since", "ayer"]);
    assert!(!ok, "it had to fail: {out}");
    assert!(out.contains("cannot read the date"), "{out}");
}
