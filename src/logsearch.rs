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
        "update search_log set followed_up = 1, followed_up_id = ?2
          where id = (
            select max(id) from search_log
             where ',' || returned_ids || ',' like ?1
          )",
        params![format!("%,{memory_id},%"), memory_id],
    )?;
    Ok(())
}

/// One line of `mem log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub ts: String,
    pub query: String,
    pub project: Option<String>,
    /// The ids that came back, in the order they came back in.
    pub returned: Vec<String>,
    pub budget_used: i64,
    /// `Some(id)` when it is known which memory was opened afterwards,
    /// `Some(None)`-shaped as `Followed::Unrecorded` for a row written
    /// before the id was stored, `None` when nothing was opened.
    pub followed: Option<Followed>,
}

/// Whether the agent went on to open one of the results, and which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Followed {
    Memory(i64),
    /// The row predates `followed_up_id`: it says one was opened and not
    /// which one. Reporting this as "none" would be a lie about the past.
    Unrecorded,
}

fn followed_from(flag: i64, id: Option<i64>) -> Option<Followed> {
    match (flag, id) {
        (0, _) => None,
        (_, Some(i)) => Some(Followed::Memory(i)),
        (_, None) => Some(Followed::Unrecorded),
    }
}

/// Splits `returned_ids` the way it was joined. An empty field is no ids,
/// not one empty id.
fn split_ids(raw: &str) -> Vec<String> {
    raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// The searches in the log, newest first.
///
/// A null `project` looks across all of them; a null `window_seconds` puts
/// no bound on time. Same shape as `recent::list_recent`, and for the same
/// reason: these are the two commands that answer "what happened lately".
pub fn list(
    conn: &Connection,
    project: Option<&str>,
    window_seconds: Option<i64>,
    limit: usize,
) -> Result<Vec<LogEntry>> {
    let mut sql = String::from(
        "select ts, query, project, returned_ids, budget_used, followed_up, followed_up_id
           from search_log where 1 = 1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(p) = project {
        sql.push_str(" and project = ?");
        args.push(Box::new(p.to_string()));
    }
    if let Some(sec) = window_seconds {
        sql.push_str(" and ts >= ?");
        args.push(Box::new(crate::recent::since(sec)));
    }
    sql.push_str(" order by ts desc, id desc limit ?");
    args.push(Box::new(limit as i64));

    let mut st = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = st.query_map(refs.as_slice(), |r| {
        let raw: String = r.get(3)?;
        Ok(LogEntry {
            ts: r.get(0)?,
            query: r.get(1)?,
            project: r.get(2)?,
            returned: split_ids(&raw),
            budget_used: r.get(4)?,
            followed: followed_from(r.get(5)?, r.get(6)?),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// One time a memory came back from a search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Appearance {
    pub ts: String,
    pub query: String,
    pub project: Option<String>,
    /// 1-based, as a person counts positions in a list of results.
    pub position: usize,
    pub of: usize,
    /// Whether this memory — not merely some memory of that search — was the
    /// one opened afterwards.
    pub opened: bool,
}

/// Every logged search that returned `memory_id`, newest first.
///
/// The match is against delimited entries for the same reason
/// `mark_follow_up` does it: id 1 must not match inside 131.
pub fn why(conn: &Connection, memory_id: i64, limit: usize) -> Result<Vec<Appearance>> {
    let mut st = conn.prepare(
        "select ts, query, project, returned_ids, followed_up, followed_up_id
           from search_log
          where ',' || returned_ids || ',' like ?1
          order by ts desc, id desc limit ?2",
    )?;
    let rows = st.query_map(params![format!("%,{memory_id},%"), limit as i64], |r| {
        let ids = split_ids(&r.get::<_, String>(3)?);
        let wanted = memory_id.to_string();
        Ok(Appearance {
            ts: r.get(0)?,
            query: r.get(1)?,
            project: r.get(2)?,
            position: ids.iter().position(|i| *i == wanted).map_or(0, |p| p + 1),
            of: ids.len(),
            opened: followed_from(r.get(4)?, r.get(5)?) == Some(Followed::Memory(memory_id)),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// How much log there is: rows, and the span it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSize {
    pub rows: i64,
    pub oldest: String,
    pub newest: String,
}

/// Nothing bounds this table, so `doctor` says how big it has got. `None`
/// when it is empty, which is the normal state of a database nobody has
/// searched through MCP.
pub fn size(conn: &Connection) -> Result<Option<LogSize>> {
    let (rows, oldest, newest): (i64, Option<String>, Option<String>) = conn.query_row(
        "select count(*), min(ts), max(ts) from search_log",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    match (rows, oldest, newest) {
        (0, _, _) => Ok(None),
        (rows, Some(oldest), Some(newest)) => Ok(Some(LogSize { rows, oldest, newest })),
        _ => Ok(None),
    }
}

/// Removes rows older than `window_seconds`, and says how many went.
///
/// There is no default window and no trimming on write: retrieval history is
/// deleted because a person asked for it with a number, never because the
/// table got big on its own.
pub fn prune(conn: &Connection, project: Option<&str>, window_seconds: i64) -> Result<usize> {
    let cut = crate::recent::since(window_seconds);
    let n = match project {
        Some(p) => conn.execute("delete from search_log where ts < ?1 and project = ?2", params![cut, p])?,
        None => conn.execute("delete from search_log where ts < ?1", params![cut])?,
    };
    Ok(n)
}
