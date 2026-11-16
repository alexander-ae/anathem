//! `mem stats` answers "has this been earning its place", out of the log the
//! MCP server has been writing all along.
//!
//! What it must never do is imply more than the log holds. In one real
//! installation only 19 of 669 searches were followed by an open and not one
//! row recorded *which* memory, so an MRR computed from that would be a
//! number with nothing behind it.

use anathem::{db, stats};
use rusqlite::{params, Connection};

fn log(conn: &Connection, ts: &str, query: &str, returned: &str, budget: i64, opened: Option<i64>) {
    conn.execute(
        "insert into search_log (ts, query, project, returned_ids, budget_used, followed_up, followed_up_id)
         values (?1, ?2, 'p', ?3, ?4, ?5, ?6)",
        params![ts, query, returned, budget, opened.is_some() as i64, opened],
    ).unwrap();
}

/// Same as `log`, but naming the project and the agent, which is what the
/// breakdown groups by.
fn log_as(
    conn: &Connection,
    ts: &str,
    query: &str,
    project: Option<&str>,
    agent: Option<&str>,
    returned: &str,
    budget: i64,
) {
    conn.execute(
        "insert into search_log (ts, query, project, returned_ids, budget_used, agent)
         values (?1, ?2, ?3, ?4, ?5, ?6)",
        params![ts, query, project, returned, budget, agent],
    ).unwrap();
}

fn db_with_log() -> (tempfile::TempDir, Connection) {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    log(&conn, "2026-11-01T10:00:00+00:00", "payment provider", "7,3,9", 400, Some(3));
    log(&conn, "2026-11-01T11:00:00+00:00", "branching policy", "1,2", 300, None);
    log(&conn, "2026-11-02T10:00:00+00:00", "nothing like this", "", 10, None);
    log(&conn, "2026-11-03T10:00:00+00:00", "cache breakdown", "5,6,7,8", 500, Some(8));
    (d, conn)
}

#[test]
fn it_counts_the_searches_and_the_ones_that_came_back_empty() {
    let (_d, conn) = db_with_log();
    let s = stats::collect(&conn, None, None).unwrap();
    assert_eq!(s.searches, 4);
    assert_eq!(s.empty, 1);
}

#[test]
fn it_counts_how_often_a_search_led_to_opening_a_memory() {
    let (_d, conn) = db_with_log();
    let s = stats::collect(&conn, None, None).unwrap();
    assert_eq!(s.followed_up, 2);
}

#[test]
fn it_reads_the_rank_of_what_was_opened_from_the_result_list() {
    // The one signal close to ground truth: the memory was opened, so it was
    // relevant, and its position says where the ranking put it.
    let (_d, conn) = db_with_log();
    let s = stats::collect(&conn, None, None).unwrap();
    // id 3 sat second in "7,3,9"; id 8 sat fourth in "5,6,7,8".
    assert_eq!(s.ranks, vec![2, 4]);
}

#[test]
fn a_follow_up_whose_id_was_never_recorded_is_not_counted_as_a_rank() {
    // Rows written before `followed_up_id` existed say that one was opened
    // and not which. Counting them as rank 1 would invent data.
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    conn.execute(
        "insert into search_log (ts, query, project, returned_ids, budget_used, followed_up)
         values ('2026-11-01T10:00:00+00:00', 'q', 'p', '4,5', 100, 1)", []).unwrap();
    let s = stats::collect(&conn, None, None).unwrap();
    assert_eq!(s.followed_up, 1);
    assert!(s.ranks.is_empty(), "no id recorded means no rank: {:?}", s.ranks);
}

#[test]
fn it_adds_up_what_the_searches_cost() {
    let (_d, conn) = db_with_log();
    let s = stats::collect(&conn, None, None).unwrap();
    assert_eq!(s.tokens_total, 1210);
}

#[test]
fn it_lists_the_queries_that_found_nothing() {
    let (_d, conn) = db_with_log();
    let s = stats::collect(&conn, None, None).unwrap();
    assert_eq!(s.empty_queries, vec!["nothing like this".to_string()]);
}

#[test]
fn a_cutoff_leaves_out_what_is_older() {
    let (_d, conn) = db_with_log();
    let s = stats::collect(&conn, Some("2026-11-02T00:00:00+00:00"), None).unwrap();
    assert_eq!(s.searches, 2, "only the 2nd and the 3rd");
    assert_eq!(s.empty, 1);
}

// ------------------------------------------------------- the breakdown

/// `mem stats --by project` and `--by agent` were added because "is this
/// earning its place" is a question about one project, not about the
/// database. The answer has to add up to the totals, or the table is a
/// second opinion and not the same measurement.
#[test]
fn the_breakdown_counts_what_it_says_it_counts() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    log_as(&conn, "2026-11-01T10:00:00+00:00", "a", Some("anathem"), Some("opencode"), "1,2", 400);
    log_as(&conn, "2026-11-01T11:00:00+00:00", "b", Some("anathem"), Some("opencode"), "3", 300);
    log_as(&conn, "2026-11-01T12:00:00+00:00", "c", Some("kipin.pe"), Some("claude-code"), "", 50);
    log_as(&conn, "2026-11-01T13:00:00+00:00", "d", None, None, "9", 60);

    let s = stats::collect(&conn, None, Some(stats::By::Project)).unwrap();
    assert_eq!(s.searches, 4);
    let names: Vec<&str> = s.by_group.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, vec!["(no project)", "anathem", "kipin.pe"],
        "a row with no project is said, not left blank: {names:?}");
    let anathem = s.by_group.iter().find(|g| g.name == "anathem").unwrap();
    assert_eq!((anathem.searches, anathem.followed_up, anathem.empty), (2, 0, 0));
    let kipin = s.by_group.iter().find(|g| g.name == "kipin.pe").unwrap();
    assert_eq!((kipin.searches, kipin.empty), (1, 1));
    assert_eq!(s.by_group.iter().map(|g| g.searches).sum::<usize>(), 4,
        "the rows have to add up to the total, or the table measures something else");

    let a = stats::collect(&conn, None, Some(stats::By::Agent)).unwrap();
    let claude = a.by_group.iter().find(|g| g.name == "claude-code").unwrap();
    assert_eq!(claude.searches, 1);
    assert!(a.by_group.iter().any(|g| g.name == "(no agent)"));
}

/// Asking for nothing gives the report it always gave: no table, and the
/// same totals.
#[test]
fn without_by_there_is_no_breakdown() {
    let (_d, conn) = db_with_log();
    let s = stats::collect(&conn, None, None).unwrap();
    assert!(s.by_group.is_empty());
    assert_eq!(s.searches, 4);
}

/// A value of `--by` that is not one of the two is refused, named, and
/// never quietly ignored: a grouping that silently does nothing reads as
/// "there is nothing to group by".
#[test]
fn only_two_things_can_be_grouped_by() {
    assert!(stats::By::parse("project").is_some());
    assert!(stats::By::parse("agent").is_some());
    assert!(stats::By::parse("model").is_none());
    assert!(stats::By::parse("").is_none());
}
