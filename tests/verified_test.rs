//! A fact somebody checked, told apart from one somebody typed.
//!
//! Until now the two were indistinguishable once saved: same shape, same
//! ranking, same weight when they reached the model. The agent reading them
//! back had nothing to tell them apart with.
//!
//! Two properties matter more than the mark itself and both are pinned here.
//! It is **additive**: every memory that existed before the column reads as
//! unverified, which is what it is, and nothing had to be rewritten. And it
//! **moves no ranking**: promoting what is verified would quietly bury a
//! correct hunch, and nothing in this project moves a ranking on something
//! that was not measured.

use anathem::{config::Config, db, mcp, model::NewMemory, search};
use serde_json::json;

fn insert(conn: &rusqlite::Connection, ext: &str, body: &str) -> i64 {
    anathem::ingest::insert(conn, &NewMemory {
        project: "p".into(),
        title: body.chars().take(30).collect(),
        body: body.into(),
        created: "2026-11-08".into(),
        event_start: None,
        event_end: None,
        source: "migrated".into(),
        ext_id: Some(ext.into()),
        tags: vec![],
        refs: vec![],
    })
    .unwrap()
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

fn db() -> (tempfile::TempDir, rusqlite::Connection) {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    (d, conn)
}

#[test]
fn a_memory_is_unverified_until_someone_says_otherwise() {
    let (_d, conn) = db();
    insert(&conn, "a", "the deployment pipeline runs nightly");
    let out = answer(&conn, "deployment");
    let line = hits(&out)[0];
    assert!(!line.contains("[verified]"), "nobody checked it yet:\n{line}");
}

#[test]
fn a_verified_memory_says_so() {
    let (_d, conn) = db();
    let id = insert(&conn, "a", "the deployment pipeline runs nightly");
    anathem::maint::verify(&conn, id, true).unwrap();
    let out = answer(&conn, "deployment");
    let line = hits(&out)[0];
    assert!(line.contains("[verified]"), "the mark did not reach the answer:\n{line}");
}

#[test]
fn the_mark_can_be_taken_away() {
    let (_d, conn) = db();
    let id = insert(&conn, "a", "the deployment pipeline runs nightly");
    anathem::maint::verify(&conn, id, true).unwrap();
    anathem::maint::verify(&conn, id, false).unwrap();
    let out = answer(&conn, "deployment");
    assert!(!hits(&out)[0].contains("[verified]"), "it stayed marked:\n{out}");
}

/// Confirming a note is not correcting it: the body, the date and the id
/// have to survive untouched, or `mem verify` would be a save in disguise.
#[test]
fn verifying_changes_nothing_but_the_mark() {
    let (_d, conn) = db();
    let id = insert(&conn, "a", "the deployment pipeline runs nightly");
    let before: (String, String, String) = conn
        .query_row("select body, created, status from memories where id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    anathem::maint::verify(&conn, id, true).unwrap();
    let after: (String, String, String) = conn
        .query_row("select body, created, status from memories where id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(before, after, "verifying rewrote something it should not have");
}

/// The property that keeps this honest. If marking a memory moved it up,
/// every mark would be a thumb on the scale and a correct hunch would sink.
#[test]
fn the_mark_moves_no_ranking() {
    let (_d, conn) = db();
    let a = insert(&conn, "a", "the deployment pipeline runs nightly on the build host");
    let b = insert(&conn, "b", "the deployment pipeline runs nightly on the spare host");
    let before: Vec<&str> = {
        let out = answer(&conn, "deployment pipeline nightly");
        hits(&out).iter().map(|l| if l.contains("build") { "a" } else { "b" }).collect()
    };
    // Mark whichever came second, which is the one a ranking boost would move.
    let segundo = if before[1] == "a" { a } else { b };
    anathem::maint::verify(&conn, segundo, true).unwrap();
    let after: Vec<&str> = {
        let out = answer(&conn, "deployment pipeline nightly");
        hits(&out).iter().map(|l| if l.contains("build") { "a" } else { "b" }).collect()
    };
    assert_eq!(before, after, "the mark changed the order, and it must not");
}

/// Additive: a database written before the column existed keeps working and
/// every memory in it reads as unverified, with no backfill.
#[test]
fn memories_saved_before_the_column_read_as_unverified() {
    let d = tempfile::tempdir().unwrap();
    let ruta = d.path().join("t.db");
    let conn = db::open(&ruta).unwrap();
    let id = insert(&conn, "a", "the deployment pipeline runs nightly");
    // Drop the column to stand in for a database from before it existed,
    // then reopen: `db::open` has to put it back without touching the rows.
    conn.execute("alter table memories drop column verified", []).unwrap();
    drop(conn);
    let conn = db::open(&ruta).unwrap();
    let v: i64 = conn
        .query_row("select verified from memories where id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(v, 0, "an old memory came back as something other than unverified");
    let out = answer(&conn, "deployment");
    assert!(!hits(&out).is_empty(), "the old memory stopped being findable:\n{out}");
}

#[test]
fn verifying_a_memory_that_does_not_exist_says_so() {
    let (_d, conn) = db();
    let e = anathem::maint::verify(&conn, 9999, true).unwrap_err();
    assert!(e.to_string().contains("9999"), "the error has to name the id: {e}");
}

/// Saving with `verified: true` from MCP marks it in one step, which is what
/// an agent that just ran the command and read the output would do.
#[test]
fn save_memory_can_mark_it_at_once() {
    let (_d, conn) = db();
    let ctx = search::SearchContext::bm25_only();
    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "save_memory",
                   "arguments": {"text": "the deployment pipeline runs nightly",
                                 "project": "p", "verified": true}}
    });
    mcp::handle(&conn, &ctx, &Config::default(), &req).unwrap().unwrap();
    let out = answer(&conn, "deployment");
    assert!(hits(&out)[0].contains("[verified]"), "saved verified, came back unmarked:\n{out}");
}

#[test]
fn save_memory_without_the_flag_does_not_mark_it() {
    let (_d, conn) = db();
    let ctx = search::SearchContext::bm25_only();
    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "save_memory",
                   "arguments": {"text": "the deployment pipeline runs nightly",
                                 "project": "p"}}
    });
    mcp::handle(&conn, &ctx, &Config::default(), &req).unwrap().unwrap();
    let out = answer(&conn, "deployment");
    assert!(!hits(&out)[0].contains("[verified]"), "it marked itself:\n{out}");
}
