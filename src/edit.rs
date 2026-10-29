use crate::{chunk, ingest};
use anyhow::Result;
use rusqlite::{params, Connection};

fn rechunk(conn: &Connection, memory_id: i64, body: &str) -> Result<()> {
    // The chunks_ad trigger cleans chunks_fts on delete.
    conn.execute("delete from chunks where memory_id = ?1", [memory_id])?;
    for (ord, c) in chunk::split(body, ingest::CHUNK_SIZE, ingest::CHUNK_STEP).iter().enumerate() {
        conn.execute(
            "insert into chunks (memory_id, ord, body) values (?1, ?2, ?3)",
            params![memory_id, ord as i64, c],
        )?;
    }
    Ok(())
}

/// Replaces a memory's body, and **recomputes its title**. The title
/// derives from the body, so leaving it untouched on edit leaves the memory
/// describing itself with text that is no longer inside it.
pub fn replace_body(conn: &Connection, id: i64, body: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "update memories set body = ?2, title = ?4, updated = ?3 where id = ?1",
        params![
            id,
            body,
            chrono::Local::now().format("%Y-%m-%d").to_string(),
            ingest::title_from(body)
        ],
    )?;
    rechunk(&tx, id, body)?;
    tx.commit()?;
    Ok(())
}

/// Rebuilds chunks and the FTS5 index from `memories.body`.
pub fn reindex(conn: &Connection) -> Result<usize> {
    let rows: Vec<(i64, String)> = conn
        .prepare("select id, body from memories")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let tx = conn.unchecked_transaction()?;
    for (id, body) in &rows {
        rechunk(&tx, *id, body)?;
    }
    tx.execute("insert into chunks_fts(chunks_fts) values ('rebuild')", [])?;
    tx.commit()?;
    Ok(rows.len())
}
