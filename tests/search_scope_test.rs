//! `search_memory` searches the project the session is in.
//!
//! `save_memory` and `recent_memories` worked out the project from the
//! working directory; `search_memory` did not, so memories were **filed** by
//! project and then **searched** as if they were not — every query against
//! the whole corpus, which is the regime where the ranking is measurably
//! worst. These cover the new default and, more importantly, every way out
//! of it: a default nobody can escape is worse than the bug it fixed.
//!
//! The expected project is taken from `project::detect`, the same function
//! the server uses, rather than written down here: a literal would be
//! asserting what this repository's git remote is called.

use anathem::{config::Config, db, mcp, model::NewMemory, search};
use serde_json::{json, Value};

/// The project a session rooted at the test's working directory belongs to.
/// `None` outside a repository, which is a case the server has an answer
/// for and so does this file.
fn here() -> Option<String> {
    anathem::project::detect(
        &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    )
}

fn insert(conn: &rusqlite::Connection, project: &str, ext: &str, body: &str) {
    anathem::ingest::insert(conn, &NewMemory {
        project: project.into(),
        title: body.chars().take(30).collect(),
        body: body.into(),
        created: "2026-11-06".into(),
        event_start: None,
        event_end: None,
        source: "migrated".into(),
        ext_id: Some(ext.into()),
        tags: vec![],
        refs: vec![],
    })
    .unwrap();
}

fn call(conn: &rusqlite::Connection, args: Value) -> String {
    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "search_memory", "arguments": args}
    });
    let res = mcp::handle(conn, &search::SearchContext::bm25_only(), &Config::default(), &req)
        .unwrap()
        .unwrap();
    res["result"]["content"][0]["text"].as_str().unwrap().to_string()
}

/// Two memories sharing every search term, one in this session's project and
/// one somewhere else. Only the project tells them apart, so anything that
/// separates them in an answer separated them by project.
fn two_projects() -> (tempfile::TempDir, rusqlite::Connection, String) {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let mine = here().unwrap_or_else(|| "no-project".into());
    insert(&conn, &mine, "mine", "the deployment pipeline runs nightly");
    insert(&conn, "somewhere-else", "theirs", "the deployment pipeline runs nightly");
    (d, conn, mine)
}

#[test]
fn a_search_stays_in_the_project_the_session_is_in() {
    let (_d, conn, mine) = two_projects();
    let out = call(&conn, json!({"query": "deployment pipeline"}));

    if here().is_none() {
        // Outside a repository there is no project to narrow to, and
        // narrowing to `no-project` would hide everything rather than
        // nothing. The documented behaviour is to search everything.
        assert!(out.contains("somewhere-else"), "outside a repo nothing should be hidden: {out}");
        return;
    }
    assert!(out.contains(&format!("[{mine}]")), "the session's own project is missing: {out}");
    assert!(
        !out.contains("somewhere-else"),
        "another project's memory came back from an unscoped search: {out}"
    );
}

#[test]
fn all_projects_looks_past_it() {
    let (_d, conn, mine) = two_projects();
    let out = call(&conn, json!({"query": "deployment pipeline", "all_projects": true}));
    assert!(out.contains("somewhere-else"), "all_projects did not widen: {out}");
    assert!(out.contains(&format!("[{mine}]")), "all_projects lost the local one: {out}");
}

/// A model writing JSON by hand produces `"true"` often enough, and the
/// failure is invisible from the answer: a search that was meant to be wide
/// comes back narrow and looks like it simply found less.
#[test]
fn all_projects_accepts_the_string_a_model_tends_to_write() {
    let (_d, conn, _) = two_projects();
    let out = call(&conn, json!({"query": "deployment pipeline", "all_projects": "true"}));
    assert!(out.contains("somewhere-else"), "the string form did not widen: {out}");
}

#[test]
fn naming_another_project_wins_over_the_implicit_one() {
    let (_d, conn, mine) = two_projects();
    let out = call(&conn, json!({"query": "deployment pipeline", "project": "somewhere-else"}));
    assert!(out.contains("somewhere-else"), "the named project is missing: {out}");
    if here().is_some() {
        assert!(!out.contains(&format!("[{mine}]")), "the implicit project leaked in: {out}");
    }
}

/// The collision this nearly shipped with: an implicit project and an
/// explicit `project:` scope are two different projects, and `parse_scopes`
/// refuses that pair rather than guessing. The explicit one has to win, or
/// a scope that used to work comes back as an error about two projects.
#[test]
fn a_project_scope_is_not_fought_by_the_implicit_one() {
    let (_d, conn, _) = two_projects();
    let out = call(&conn, json!({
        "query": "deployment pipeline",
        "scope": ["project:somewhere-else"]
    }));
    assert!(!out.contains("Invalid scope"), "the implicit project fought the scope: {out}");
    assert!(out.contains("somewhere-else"), "the scope did not select: {out}");
}

/// Narrowing must never be the reason something cannot be found. When the
/// project nobody asked for comes back empty, the search widens by itself
/// and says so.
#[test]
fn an_empty_project_widens_by_itself_and_says_so() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    insert(&conn, "somewhere-else", "theirs", "the deployment pipeline runs nightly");

    let out = call(&conn, json!({"query": "deployment pipeline"}));
    assert!(out.contains("somewhere-else"), "the memory was unreachable: {out}");
    if here().is_some() {
        assert!(
            out.contains("searched every project"),
            "it widened without saying so, which hides where the answer came from: {out}"
        );
    }
}

/// Widening is for the implicit scope only. A project the caller named and
/// that holds nothing is an answer, not a reason to go looking elsewhere.
#[test]
fn a_named_project_that_is_empty_does_not_widen() {
    let (_d, conn, _) = two_projects();
    let out = call(&conn, json!({"query": "deployment pipeline", "project": "nothing-here"}));
    assert!(
        !out.contains("somewhere-else"),
        "a named project widened on its own: {out}"
    );
}
