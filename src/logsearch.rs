use anyhow::Result;
use rusqlite::{params, Connection};

pub fn record(
    conn: &Connection,
    query: &str,
    project: Option<&str>,
    ids: &[String],
    budget_used: usize,
) -> Result<i64> {
    conn.execute(
        "insert into search_log (ts, query, project, returned_ids, budget_used)
         values (?1, ?2, ?3, ?4, ?5)",
        params![
            chrono::Local::now().to_rfc3339(),
            query,
            project,
            ids.join(","),
            budget_used as i64
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Records that the agent asked for a memory's full body after searching for
/// it: a sign that the injected snippet was not enough.
///
/// `returned_ids` is a comma-separated list of ids (e.g. "131,101,1,132").
/// The match has to be against delimited entries, not a substring: comparing
/// against `',' || returned_ids || ','` with the pattern `%,id,%` keeps id 1
/// from matching inside 131 or 132.
pub fn mark_follow_up(conn: &Connection, memory_id: i64) -> Result<()> {
    conn.execute(
        "update search_log set followed_up = 1
          where id = (
            select max(id) from search_log
             where ',' || returned_ids || ',' like ?1
          )",
        params![format!("%,{memory_id},%")],
    )?;
    Ok(())
}
