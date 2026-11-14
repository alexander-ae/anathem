use anathem::{db, logsearch};
use rusqlite::params;

fn empty_conn() -> (tempfile::TempDir, rusqlite::Connection) {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    (dir, conn)
}

#[test]
fn mark_follow_up_does_not_match_by_substring() {
    let (_d, conn) = empty_conn();
    // returned_ids holds "131" and "132" but never the id "1" as an entry of its own.
    conn.execute(
        "insert into search_log (ts, query, project, returned_ids, budget_used)
         values ('2026-09-22T00:00:00', 'q', null, ?1, 0)",
        params!["131,101,132"],
    ).unwrap();

    logsearch::mark_follow_up(&conn, 1).unwrap();

    let followed_up: i64 = conn
        .query_row("select followed_up from search_log where id = 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(followed_up, 0, "id=1 should not match by substring inside 131/132");
}

#[test]
fn mark_follow_up_matches_a_genuine_id_in_the_list() {
    let (_d, conn) = empty_conn();
    conn.execute(
        "insert into search_log (ts, query, project, returned_ids, budget_used)
         values ('2026-09-22T00:00:00', 'q', null, ?1, 0)",
        params!["131,101,1,132"],
    ).unwrap();

    logsearch::mark_follow_up(&conn, 1).unwrap();

    let followed_up: i64 = conn
        .query_row("select followed_up from search_log where id = 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(followed_up, 1, "id=1 is genuinely in the list and has to be marked");
}

// ---------------------------------------------------------------------------
// Reading the log back. `record` and `mark_follow_up` have been writing the
// retrieval trajectory since the MCP server existed, and nothing read it.
// ---------------------------------------------------------------------------

use std::path::Path;
use std::process::Command;

fn mem(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(dir.join("t.db"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

fn mem_ok(dir: &Path, args: &[&str]) -> String {
    let (ok, out, err) = mem(dir, args);
    assert!(ok, "mem {args:?} failed: {err}");
    out
}

/// Writes a row by hand: the log is written only by the MCP server, and the
/// read path has to be exercised without standing a server up for it.
fn log_row(conn: &rusqlite::Connection, ts: &str, query: &str, project: Option<&str>, ids: &str) {
    conn.execute(
        "insert into search_log (ts, query, project, returned_ids, budget_used)
         values (?1, ?2, ?3, ?4, 1301)",
        params![ts, query, project, ids],
    )
    .unwrap();
}

#[test]
fn log_lists_the_searches_newest_first_with_what_came_back() {
    let (d, conn) = empty_conn();
    log_row(&conn, "2026-11-01T10:00:00-05:00", "la vieja", Some("p"), "7,8");
    log_row(&conn, "2026-11-04T10:00:00-05:00", "la nueva", Some("p"), "1,2,3");
    drop(conn);

    let out = mem_ok(d.path(), &["log", "--since", "all"]);
    let primera = out.lines().find(|l| l.contains("query=")).unwrap();
    assert!(primera.contains("la nueva"), "the newest goes first: {out}");
    assert!(out.contains("returned=3"), "how many came back: {out}");
    assert!(out.contains("tokens=1301"), "and what budget it cost: {out}");
    assert!(out.contains("project=p"), "{out}");
}

#[test]
fn log_on_a_database_never_searched_says_so_and_exits_zero() {
    let (d, _conn) = empty_conn();
    let (ok, out, _) = mem(d.path(), &["log"]);
    assert!(ok, "an empty log is not an error");
    assert!(
        out.to_lowercase().contains("mcp"),
        "y tiene que decir por qué puede estar vacío: solo se registran las búsquedas por MCP: {out}"
    );
}

#[test]
fn log_respects_the_window_and_the_project() {
    let (d, conn) = empty_conn();
    log_row(&conn, "2020-01-01T00:00:00-05:00", "antiquísima", Some("p"), "1");
    log_row(&conn, &chrono::Local::now().to_rfc3339(), "de recién", Some("p"), "2");
    log_row(&conn, &chrono::Local::now().to_rfc3339(), "de otro lado", Some("q"), "3");
    drop(conn);

    let out = mem_ok(d.path(), &["log", "--since", "24h", "--project", "p"]);
    assert!(out.contains("de recién"), "{out}");
    assert!(!out.contains("antiquísima"), "outside the window: {out}");
    assert!(!out.contains("de otro lado"), "from another project: {out}");
}

#[test]
fn why_names_the_queries_that_returned_a_memory_and_its_position() {
    let (d, conn) = empty_conn();
    log_row(&conn, "2026-11-04T10:00:00-05:00", "caída de la caché", Some("p"), "9,42,7");
    drop(conn);

    let out = mem_ok(d.path(), &["why", "42"]);
    assert!(out.contains("caída de la caché"), "{out}");
    assert!(out.contains("2 of 3") || out.contains("2 de 3"), "what position it came out in: {out}");
}

#[test]
fn why_on_a_memory_that_never_came_back_says_so() {
    // The interesting case, and the one an empty answer would hide.
    let (d, conn) = empty_conn();
    log_row(&conn, "2026-11-04T10:00:00-05:00", "otra cosa", Some("p"), "9,7");
    drop(conn);

    let (ok, out, _) = mem(d.path(), &["why", "42"]);
    assert!(ok);
    assert!(out.contains("42"), "{out}");
    assert!(out.to_lowercase().contains("never"), "it has to say it, not come out empty: {out}");
    assert!(out.to_lowercase().contains("mcp"), "and why it may be: {out}");
}

#[test]
fn a_search_then_a_get_memory_shows_in_the_log_as_followed_up() {
    use anathem::{config::Config, ingest, mcp, search};
    let (d, conn) = empty_conn();
    let (id, _tags) =
        ingest::save_explicit(&conn, "la caché se cae con 50 conexiones", "p", Vec::new()).unwrap();
    let ctx = search::SearchContext::bm25_only();
    let request = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"search_memory","arguments":{"query":"caché"}}});
    mcp::handle(&conn, &ctx, &Config::default(), &request).unwrap();
    let abrir = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"get_memory","arguments":{"id":id}}});
    mcp::handle(&conn, &ctx, &Config::default(), &abrir).unwrap();
    drop(conn);

    let out = mem_ok(d.path(), &["log", "--since", "all"]);
    assert!(out.contains(&format!("opened={id}")), "the follow-up has to be visible: {out}");

    let why = mem_ok(d.path(), &["why", &id.to_string()]);
    assert!(
        why.contains("opened=yes"),
        "y desde el otro lado también, diciendo que fue ESTA la que se abrió: {why}"
    );
}

#[test]
fn log_prune_needs_a_window_and_removes_only_what_is_older() {
    let (d, conn) = empty_conn();
    log_row(&conn, "2020-01-01T00:00:00-05:00", "antiquísima", Some("p"), "1");
    log_row(&conn, &chrono::Local::now().to_rfc3339(), "de recién", Some("p"), "2");
    drop(conn);

    // Without a window it cannot run: deleting retrieval history is
    // something a person asks for, with a number.
    let (ok, _, _) = mem(d.path(), &["log", "--prune"]);
    assert!(!ok, "--prune with no window cannot delete anything");

    let out = mem_ok(d.path(), &["log", "--prune", "30d"]);
    assert!(out.contains('1'), "it has to say how many it removed: {out}");

    let quedan = mem_ok(d.path(), &["log", "--since", "all"]);
    assert!(quedan.contains("de recién"), "{quedan}");
    assert!(!quedan.contains("antiquísima"), "{quedan}");
}

#[test]
fn doctor_reports_how_big_the_search_log_is() {
    let (d, conn) = empty_conn();
    log_row(&conn, "2026-11-01T10:00:00-05:00", "una", Some("p"), "1");
    log_row(&conn, "2026-11-04T10:00:00-05:00", "otra", Some("p"), "2");
    drop(conn);

    let (_ok, out, _) = mem(d.path(), &["doctor"]);
    assert!(out.contains("search log") || out.contains("search_log"), "{out}");
    assert!(out.contains('2'), "how many rows it has: {out}");
    assert!(out.contains("2026-11-01"), "and since when it has been growing: {out}");
}
