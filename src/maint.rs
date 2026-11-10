use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

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

/// What deleting a memory takes with it, and what it leaves behind.
///
/// Read before the delete and printed whether or not the delete goes ahead,
/// because the point of the command is that somebody sees the cost before
/// paying it. `--force` prints the same thing; it does not quieten it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteImpact {
    pub id: i64,
    pub project: String,
    pub title: String,
    pub status: String,
    pub topic: Option<String>,
    /// The older memory this one supersedes, if any. After the delete it
    /// stays where it is, with nothing left pointing at it.
    pub supersedes: Option<i64>,
    /// The memories whose `supersedes_id` points here. These are what the
    /// refusal is about: deleting leaves them pointing at an id that no
    /// longer exists, and `mem history` stops at the hole.
    pub superseded_by: Vec<i64>,
    pub chunks: i64,
    pub tags: i64,
    pub refs: i64,
    /// Rows in `search_log` that name this id. They are not deleted — see
    /// `delete`.
    pub log_rows: i64,
}

impl DeleteImpact {
    /// Whether anything points at this memory, which is what makes the plain
    /// `delete` refuse.
    pub fn is_referenced(&self) -> bool {
        !self.superseded_by.is_empty()
    }
}

/// What `delete` would do to this memory, without doing any of it.
pub fn delete_impact(conn: &Connection, id: i64) -> Result<Option<DeleteImpact>> {
    let row: Option<(String, String, String, Option<String>, Option<i64>)> = conn
        .query_row(
            "select project, title, status, topic, supersedes_id from memories where id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((project, title, status, topic, supersedes)) = row else {
        return Ok(None);
    };
    let superseded_by = conn
        .prepare("select id from memories where supersedes_id = ?1 order by id")?
        .query_map([id], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let count = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, [id], |r| r.get(0))?) };
    Ok(Some(DeleteImpact {
        id,
        project,
        title,
        status,
        topic,
        supersedes,
        superseded_by,
        chunks: count("select count(*) from chunks where memory_id = ?1")?,
        tags: count("select count(*) from memory_tags where memory_id = ?1")?,
        refs: count("select count(*) from memory_refs where memory_id = ?1")?,
        // The log stores ids in a comma-separated list, so this is a
        // substring match with the separators written in to keep `1` from
        // matching `12`. It is a count for a sentence, not a filter.
        log_rows: conn.query_row(
            "select count(*) from search_log
              where ',' || returned_ids || ',' like '%,' || ?1 || ',%'
                 or followed_up_id = ?1",
            [id],
            |r| r.get(0),
        )?,
    }))
}

/// Really deletes a memory: the row, its chunks and their index entries, its
/// tags, its refs and its vectors.
///
/// This is not `forget`, and the two are different promises. `forget` sets a
/// status, which takes the memory out of search and leaves the body readable
/// by id, in `mem show` and in `mem export`. That is an audit trail, and it
/// is the right default. It is not an answer to somebody who asks that
/// something stop existing here, and until this command there was no answer
/// to give them but "delete the database".
///
/// Three things it does **not** do, each on purpose:
///
/// - **It does not touch `search_log`.** The log holds ids and queries, not
///   bodies, and rewriting it would hide that a search happened. The id
///   stays there, pointing at nothing.
/// - **It does not reach copies.** A `mem export` taken earlier still
///   contains the memory, and so does any machine it was sent to.
/// - **It does not vacuum.** The pages are zeroed (see `secure_delete`
///   below), not handed back to the filesystem.
///
/// The order of the deletes matters, and not for tidiness. `chunks` has
/// `on delete cascade`, so dropping the memory row would take the chunks
/// with it — but SQLite only fires triggers for cascaded deletes when
/// `recursive_triggers` is on, and it is off. The `chunks_ad` trigger is
/// what removes the terms from `chunks_fts`, so a cascaded delete would
/// leave the body's words in the index. Deleting the chunks explicitly, and
/// first, fires it.
pub fn delete(conn: &Connection, id: i64, force: bool) -> Result<DeleteImpact> {
    let Some(impact) = delete_impact(conn, id)? else {
        anyhow::bail!("there is no memory with id:{id}");
    };
    if impact.is_referenced() && !force {
        let who: Vec<String> = impact.superseded_by.iter().map(|i| i.to_string()).collect();
        anyhow::bail!(
            "memory id:{id} is superseded by {}, which would be left pointing at an id that \
             no longer exists; `mem delete {id} --force` deletes it anyway",
            who.join(", ")
        );
    }

    // Overwrites the freed pages with zeros instead of leaving the body
    // legible in them. It is per-connection and set here rather than in
    // `db::open` so only this command pays for it: every other write would
    // be doing the extra work to protect content it is not removing.
    conn.pragma_update(None, "secure_delete", true)?;

    let tx = conn.unchecked_transaction()?;
    if force {
        // Not a cascade: whatever superseded this one is a memory somebody
        // still wants, and deleting it would be deleting far more than was
        // asked — the failure the unlearning benchmarks measure as often as
        // the opposite one.
        tx.execute("update memories set supersedes_id = null where supersedes_id = ?1", [id])?;
    }
    tx.execute(
        "delete from chunk_vectors where chunk_id in (select id from chunks where memory_id = ?1)",
        [id],
    )?;
    tx.execute("delete from chunks where memory_id = ?1", [id])?;
    tx.execute("delete from memory_tags where memory_id = ?1", [id])?;
    tx.execute("delete from memory_refs where memory_id = ?1", [id])?;
    tx.execute("delete from memories where id = ?1", [id])?;
    // The trigger above takes the memory *out of the answers* — a MATCH for
    // one of its words returns nothing. It does not take the words out of
    // the file: FTS5 records a deletion as a marker in a new segment and
    // leaves the old segment, terms and all, until something merges them.
    // For an ordinary edit that is a reasonable trade and `edit.rs` makes
    // the same call for the same reason; for a command whose whole promise
    // is that the content is gone, a readable copy in the index is the
    // promise broken. Rebuilding re-reads `chunks`, which no longer has the
    // row, and the old segments are freed — zeroed, by the pragma above.
    tx.execute_batch("insert into chunks_fts(chunks_fts) values ('rebuild');")?;
    tx.commit()?;

    // Without this the zeroed pages sit in the write-ahead log until some
    // later connection checkpoints it, and the old bytes stay in `mem.db`
    // meanwhile. A delete whose guarantee arrives at an unspecified time is
    // not the guarantee that was asked for.
    conn.execute_batch("pragma wal_checkpoint(truncate);")?;
    Ok(impact)
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

/// Marks a memory as checked against the world, or takes the mark away.
///
/// Separate from saving on purpose: confirming a note is not correcting it.
/// Going through `--topic` or `mem save` again would create a new version
/// and leave the old one superseded, which says the content changed when
/// only its standing did — and it would lose the original's date.
///
/// It is deliberately not restricted to active memories. A superseded one
/// can be marked too: whoever walks `mem history` to see what a topic used
/// to say is better served knowing which of those versions had been
/// confirmed.
pub fn verify(conn: &Connection, id: i64, on: bool) -> Result<()> {
    let n = conn.execute("update memories set verified=?2 where id=?1",
        params![id, if on { 1 } else { 0 }])?;
    if n == 0 {
        anyhow::bail!("there is no memory with id:{id}");
    }
    Ok(())
}

/// Promotes a received memory from `draft` to `active`, which is what makes
/// it visible to search.
///
/// Only a draft can be accepted. Accepting something already active is not a
/// no-op worth being quiet about — it means whoever ran it had the wrong id,
/// and the useful answer is to say so.
pub fn accept(conn: &Connection, id: i64) -> Result<String> {
    let row: Option<(String, Option<String>)> = conn
        .query_row("select status, received_from from memories where id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let Some((status, from)) = row else {
        anyhow::bail!("there is no memory with id:{id}");
    };
    if status != "draft" {
        anyhow::bail!("memory id:{id} is {status}, not a draft waiting to be accepted");
    }
    conn.execute(
        "update memories set status = 'active', updated = ?2 where id = ?1",
        params![id, chrono::Local::now().format("%Y-%m-%d").to_string()],
    )?;
    Ok(from.unwrap_or_else(|| "an unrecorded sender".to_string()))
}

/// Promotes every draft received from one sender, and says how many.
///
/// It takes a sender rather than accepting everything waiting: "let in
/// whatever arrived, from wherever" is precisely the decision this gate
/// exists to make somebody take deliberately.
pub fn accept_all_from(conn: &Connection, from: &str) -> Result<usize> {
    Ok(conn.execute(
        "update memories set status = 'active', updated = ?2
          where status = 'draft' and received_from = ?1",
        params![from.trim(), chrono::Local::now().format("%Y-%m-%d").to_string()],
    )?)
}

/// How many received memories are waiting, and from how many senders.
pub fn waiting_to_be_accepted(conn: &Connection) -> Result<(i64, i64)> {
    Ok(conn.query_row(
        "select count(*), count(distinct received_from) from memories
          where status = 'draft' and received_from is not null",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

/// One memory, whole, as `mem show <id>` prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Memory {
    pub id: i64,
    pub project: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub when: String,
    pub agent: Option<String>,
    pub topic: Option<String>,
    pub received_from: Option<String>,
}

/// Reads one memory by id, whatever its status.
///
/// Deliberately not filtered by `status = 'active'` like search is: the
/// reason this exists is to read a draft before deciding whether to accept
/// it, and a reviewer who cannot see what they are reviewing is not one.
pub fn memory(conn: &Connection, id: i64) -> Result<Option<Memory>> {
    Ok(conn
        .query_row(
            "select id, project, title, body, status, coalesce(created_ts, created),
                    agent, topic, received_from
               from memories where id = ?1",
            [id],
            |r| {
                Ok(Memory {
                    id: r.get(0)?,
                    project: r.get(1)?,
                    title: r.get(2)?,
                    body: r.get(3)?,
                    status: r.get(4)?,
                    when: r.get(5)?,
                    agent: r.get(6)?,
                    topic: r.get(7)?,
                    received_from: r.get(8)?,
                })
            },
        )
        .optional()?)
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
