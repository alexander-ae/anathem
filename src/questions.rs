//! The questions a memory was written to answer, so a paraphrase can find it.
//!
//! MeMo (arXiv 2605.15156) gets its distance from a retrieval stack by
//! exposing a corpus under many query variations — its *reflections* — and
//! training a model on them. The half that needs no training is this: whoever
//! writes the note, which is the agent and not anathem, also writes two or
//! three questions it answers. They are indexed with the memory, never shown
//! in an excerpt, and they are plain text, so the whole promise of this tool
//! (deterministic, model-free, one file) still holds.
//!
//! Why it is worth a table: recall on questions that *describe* a situation
//! was measured at 19 % against 88 % for the ones that name the thing. The
//! words a person uses in the question are not the words in the note, and no
//! amount of BM25 tunig invents them.
use anyhow::Result;
use rusqlite::{params, Connection};

/// Stores the questions and puts them in the index. Returns how many were new.
pub fn add(conn: &Connection, memory_id: i64, questions: &[String]) -> Result<usize> {
    let mut nuevas = 0;
    for q in questions {
        let q = q.split_whitespace().collect::<Vec<_>>().join(" ");
        if q.is_empty() {
            continue;
        }
        nuevas += conn.execute(
            "insert or ignore into memory_questions (memory_id, question) values (?1, ?2)",
            params![memory_id, q],
        )?;
    }
    if nuevas > 0 {
        append_to_index(conn, memory_id)?;
    }
    Ok(nuevas)
}

pub fn list(conn: &Connection, memory_id: i64) -> Result<Vec<String>> {
    let mut st = conn.prepare(
        "select question from memory_questions where memory_id = ?1 order by rowid")?;
    let rows = st.query_map([memory_id], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Rebuilds the indexed form of the memory's first chunk as *body plus the
/// questions*, which makes the call idempotent: appending twice would double
/// every term and quietly promote the memory in the ranking.
///
/// `chunks.body` — what an excerpt shows — is never touched. A question is
/// bait for the index, not text the user saved.
pub fn append_to_index(conn: &Connection, memory_id: i64) -> Result<()> {
    let preguntas: String = conn.query_row(
        "select coalesce(group_concat(question, ' '), '') from memory_questions where memory_id = ?1",
        [memory_id],
        |r| r.get(0),
    )?;
    if preguntas.trim().is_empty() {
        return Ok(());
    }
    let body: Option<String> = conn
        .query_row(
            "select body from chunks where memory_id = ?1 and ord = 0",
            [memory_id],
            |r| r.get(0),
        )
        .ok();
    let Some(body) = body else { return Ok(()) };
    let mut indexed = crate::stem::indexed_form(&body, crate::stem::enabled(conn));
    indexed.push(' ');
    indexed.push_str(&crate::stem::indexed_form(&preguntas, crate::stem::enabled(conn)));
    conn.execute(
        "update chunks set fts_body = ?2 where memory_id = ?1 and ord = 0",
        params![memory_id, indexed],
    )?;
    Ok(())
}
