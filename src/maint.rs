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
        anyhow::bail!("there is no memory with id:{id}");
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
        anyhow::bail!("there is no memory with id:{id}");
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

/// One version of a topic key, as the chain under `mem show --topic` lists
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicVersion {
    pub id: i64,
    /// `created_ts` when it is there, the date in `created` otherwise — the
    /// same value `mem recent` orders by.
    pub when: String,
    pub status: String,
    pub title: String,
}

/// The active version of a topic: the part a person is actually after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentTopic {
    pub id: i64,
    pub body: String,
    pub when: String,
    /// Who saved it, when that was known. Null for everything saved from the
    /// CLI; see `db::add_column`.
    pub agent: Option<String>,
}

/// What a topic says today, and what it said before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicState {
    pub project: String,
    pub topic: String,
    /// `None` when every version of the key was forgotten. The key still
    /// exists and still has a history; nothing answers for it today. That is
    /// a different answer from a key that was never used, and telling the
    /// two apart is most of why this command exists.
    pub current: Option<CurrentTopic>,
    /// Every version, newest first, the active one included.
    pub chain: Vec<TopicVersion>,
}

/// The answer to "what does this topic say", or the keys that project does
/// have when it does not have this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopicLookup {
    Found(Box<TopicState>),
    /// Ordered, as `topics` returns them.
    Unknown(Vec<String>),
}

/// Reads a topic key's state in one project.
///
/// The chain is every memory carrying the key, newest id first, and not the
/// walk of `supersedes_id` that `history` does. They differ: once the
/// current version of a key is forgotten, the next save under that key
/// supersedes nothing (see `ingest::save_with_topic`), so the key's history
/// is several disjoint chains and walking one of them would quietly report
/// a shorter past than `mem topics` counts in the same breath.
pub fn topic_state(conn: &Connection, project: &str, topic: &str) -> Result<TopicLookup> {
    let key = crate::ingest::normalize_topic(topic)?;
    let mut stmt = conn.prepare(
        "select id, coalesce(created_ts, created), status, title, body, agent
           from memories
          where project = ?1 and topic = ?2
          order by id desc",
    )?;
    let versions = stmt
        .query_map(params![project, key], |r| {
            Ok((
                TopicVersion { id: r.get(0)?, when: r.get(1)?, status: r.get(2)?, title: r.get(3)? },
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    if versions.is_empty() {
        // The courtesy `save_memory` already extends when a key turns out to
        // be new: a typo and a genuinely new subject look identical, and the
        // list is what separates them.
        let keys =
            topics(conn)?.into_iter().filter(|t| t.project == project).map(|t| t.topic).collect();
        return Ok(TopicLookup::Unknown(keys));
    }

    // At most one can be active: the unique index over (project, topic) for
    // active rows is what guarantees it, and `doctor` reports a database
    // where that stopped being true.
    let current = versions.iter().find(|(v, _, _)| v.status == "active").map(|(v, body, agent)| {
        CurrentTopic { id: v.id, body: body.clone(), when: v.when.clone(), agent: agent.clone() }
    });
    Ok(TopicLookup::Found(Box::new(TopicState {
        project: project.to_string(),
        topic: key,
        current,
        chain: versions.into_iter().map(|(v, _, _)| v).collect(),
    })))
}
