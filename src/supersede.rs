use crate::{model::SearchHit, search};
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

/// Active memories from the same project that most resemble `body`.
///
/// It is called right after inserting the new memory, so that memory itself
/// shows up among the hits and has to be excluded. Filtering by "identical
/// body" (as the naive approach suggests) is not enough in general: if the
/// corpus held legitimate active duplicates from the same project with
/// identical bodies, and then a third one with that same text were saved,
/// excluding by content would discard *all* the matches — the pre-existing
/// duplicates included — instead of proposing them, which is exactly the
/// case this function exists to detect.
///
/// (In a real migrated corpus, the only truly byte-identical pair turned out
/// to live in two different projects, so `candidates()`, which always
/// filters to a single project, never saw them together in one call. A query
/// over that whole corpus for identical-body pairs within the same project
/// with both active returned no rows: there was no real case exercising the
/// argument above. This is a safeguard against a scenario that corpus did
/// not have yet, not a fix for an observed bug.)
///
/// That is why we exclude by id and not by content: among the hits whose
/// body is identical to the one being saved, the highest id is the one just
/// inserted (ids autoincrement and this function is called immediately after
/// the insert), so it is the only one dropped. Pre-existing duplicates, if
/// any, remain candidates.
pub fn candidates(conn: &Connection, body: &str, project: &str, k: usize) -> Result<Vec<SearchHit>> {
    let q = search::Query {
        text: body.chars().take(400).collect(),
        project: Some(project.to_string()),
        tags: Vec::new(),
        limit: k + 1,
        budget_tokens: usize::MAX,
        // This compares bodies to spot possible duplicates, it is not
        // building an answer to show: no trimming.
        excerpt: None,
    };
    // Called right after an insert, when the vector for the just-saved
    // memory may not be synced yet: the vector index would be incomplete in
    // precisely the case this function cares about. So it searches with bm25
    // alone, without the pack.
    let mut hits = search::run(conn, &q, &search::SearchContext::bm25_only())?;

    let self_id = hits
        .iter()
        .filter(|h| {
            let same: rusqlite::Result<String> = conn.query_row(
                "select body from memories where id = ?1",
                [h.memory_id],
                |r| r.get(0),
            );
            matches!(same, Ok(b) if b == body)
        })
        .map(|h| h.memory_id)
        .max();
    if let Some(self_id) = self_id {
        hits.retain(|h| h.memory_id != self_id);
    }

    hits.truncate(k);
    Ok(hits)
}

/// Sets `supersedes_id` on `new_id` and marks `old` as `superseded`, so it
/// stops showing up in `search::run` (which already filters by active
/// status).
///
/// It requires `new_id` to exist and be `active` before touching anything.
/// The first check keeps a nonexistent `new_id` from passing silently: the
/// first `update` below does `where id = new_id`, and if no row matches,
/// SQLite raises no error — only the second `update` would run, leaving
/// `old` superseded and pointing at nothing. The second check (requiring
/// `active`) closes the path to a cycle: if `new_id` is already superseded,
/// some other call to `link` already used it as `old` — letting it chain
/// again as `new_id` is how a cycle closes (A supersedes B, B supersedes A),
/// which leaves no active version at all and would be an infinite loop for
/// anyone walking the `supersedes_id` chain later. A nonexistent `old` was
/// already caught by the foreign key on `supersedes_id`, with no explicit
/// check needed.
///
/// If the caller already had a transaction open, `link` opens no other and
/// does not commit: both `update`s are part of theirs. If the second fails,
/// the first stays applied until the caller rolls back (or drops the
/// transaction without committing); `link` does not undo it on its own.
/// Whoever calls it inside a transaction has to propagate the error and not
/// commit.
pub fn link(conn: &Connection, newer: i64, older: i64) -> Result<()> {
    if newer == older {
        anyhow::bail!("una memoria no puede reemplazarse a sí misma");
    }
    let status_of_newer: Option<String> = conn
        .query_row("select status from memories where id = ?1", [newer], |r| r.get(0))
        .optional()?;
    match status_of_newer.as_deref() {
        None => anyhow::bail!("no existe la memoria nueva id:{newer}"),
        Some("active") => {}
        Some(otro) => anyhow::bail!(
            "la memoria nueva id:{newer} no está activa (status={otro}); no puede replace_file \
             a otra memoria -- si ya fue superseded, esto formaría un ciclo"
        ),
    }
    // If the caller already opened a transaction (saving with a topic key
    // inserts and supersedes as one unit), no other is opened: SQLite does
    // not nest `BEGIN`. So both `update`s land inside theirs, and if either
    // fails everything is undone together with their insert.
    let propia = if conn.is_autocommit() { Some(conn.unchecked_transaction()?) } else { None };
    conn.execute("update memories set supersedes_id = ?2 where id = ?1", params![newer, older])?;
    conn.execute(
        "update memories set status = 'superseded', updated = ?2 where id = ?1",
        params![older, chrono::Local::now().format("%Y-%m-%d").to_string()],
    )?;
    if let Some(tx) = propia {
        tx.commit()?;
    }
    Ok(())
}
