use anyhow::Result;
use rusqlite::{params, Connection};

/// Marks a memory as `forgotten`. It does not delete the row: `search::run`
/// filters by `status = 'active'`, so changing the status is enough to take
/// it out of search without losing the content or breaking references
/// (`supersedes_id`, `memory_refs`, etc.) other rows may hold to it.
///
/// `UPDATE ... WHERE id = ?` does not fail in SQLite when it matches no row
/// — it returns 0 affected rows and carries on. We check that count
/// explicitly and fail with `bail!` naming the id, just as `supersede::link`
/// already does with a nonexistent `new_id`: a maintenance command whose
/// only purpose is to mutate state and report it cannot report success over
/// a row that does not exist.
pub fn forget(conn: &Connection, id: i64) -> Result<()> {
    let n = conn.execute("update memories set status='forgotten', updated=?2 where id=?1",
        params![id, chrono::Local::now().format("%Y-%m-%d").to_string()])?;
    if n == 0 {
        anyhow::bail!("no existe la memoria id:{id}");
    }
    Ok(())
}

/// Sets or clears a memory's `pinned` flag. See `forget` on why the
/// affected-row count is checked instead of trusting that `execute` returned
/// no error.
pub fn pin(conn: &Connection, id: i64, on: bool) -> Result<()> {
    let n = conn.execute("update memories set pinned=?2 where id=?1",
        params![id, if on { 1 } else { 0 }])?;
    if n == 0 {
        anyhow::bail!("no existe la memoria id:{id}");
    }
    Ok(())
}

/// Returns (id, title, status) from `id` backwards along the supersession
/// chain.
///
/// `supersede::link` already rejects any `link` whose `new_id` is not
/// `active`, which makes closing a cycle structurally impossible by that
/// route: the closing edge of a k-node cycle would always have a `new_id`
/// already left `superseded` by an earlier turn of the same cycle. So a
/// cycle should be unreachable walking this code.
///
/// We keep the cut-off guard (`out.len() > 50`) anyway because data can
/// arrive by routes that do not pass through `link`: a hand-edited database,
/// a future import, a bug in a later task. A guard that never fires costs
/// one comparison; its absence would cost an infinite loop.
pub fn history(conn: &Connection, id: i64) -> Result<Vec<(i64, String, String)>> {
    let mut out = Vec::new();
    let mut cur = Some(id);
    while let Some(c) = cur {
        let row: rusqlite::Result<(i64, String, String, Option<i64>)> = conn.query_row(
            "select id, title, status, supersedes_id from memories where id=?1",
            [c], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)));
        match row {
            Ok((i, t, s, prev)) => { out.push((i, t, s)); cur = prev; }
            Err(_) => break,
        }
        if out.len() > 50 { break; } // corta ciclos
    }
    Ok(out)
}

/// One topic key as `mem topics` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tema {
    pub project: String,
    pub topic: String,
    /// How many memories the key had in total, counting the ones already
    /// replaced and the forgotten ones: it is the length of its history.
    pub total: i64,
    /// The active memory that answers for the key today. `None` when all of
    /// them were forgotten: the key exists but there is no current
    /// version.
    pub current_one: Option<i64>,
}

/// The topic keys that exist, ordered by project and key. The same key in
/// two projects is two rows, because they are two topics.
pub fn topics(conn: &Connection) -> Result<Vec<Tema>> {
    let mut stmt = conn.prepare(
        "select project, topic, count(*),
                max(case when status = 'active' then id end)
           from memories
          where topic is not null
          group by project, topic
          order by project, topic",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Tema {
                project: r.get(0)?,
                topic: r.get(1)?,
                total: r.get(2)?,
                current_one: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}
