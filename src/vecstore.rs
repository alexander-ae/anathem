//! Persisting vectors and searching by cosine.
//!
//! The search is brute force, with no approximate index and no SQLite
//! extensions. A dot product against 100,000 chunks is 25 million
//! operations, a few milliseconds, and it avoids a compiled extension that
//! would break the single-binary promise. Worth revisiting past half a
//! million chunks.

use crate::{pack::Pack, search::Query};
use anyhow::Result;
use rusqlite::{params, types::Value, Connection, Transaction, TransactionBehavior};
use std::collections::HashMap;

/// How many chunks `sync` embeds and writes per round.
///
/// Embedding is the expensive part — a hundred thousand chunks is minutes —
/// and it does not touch the database, so it stays outside the transaction.
/// What remains inside are short batches: a concurrent writer waits tens of
/// milliseconds instead of minutes. Two hundred and fifty-six is the
/// compromise between paying the transaction's startup over and over and
/// holding the lock for a long time.
const LOTE: usize = 256;

/// Opens a write transaction, taking the lock up front.
///
/// `begin immediate` instead of the `begin deferred` that
/// `unchecked_transaction` gives: with deferred, two processes can start at
/// once and only discover the conflict on write, when it is already too late
/// for `busy_timeout` to make them wait their turn.
fn escritura(conn: &Connection) -> rusqlite::Result<Transaction<'_>> {
    Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorHit {
    pub memory_id: i64,
    pub chunk_id: i64,
    pub cosine: f32,
}

fn a_blob(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

fn de_blob(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

fn record_identity(
    conn: &Connection,
    name: &str,
    sha: &str,
    dims: usize,
) -> rusqlite::Result<()> {
    for (k, v) in [
        ("pack_nombre", name.to_string()),
        ("pack_sha256", sha.to_string()),
        ("pack_dims", dims.to_string()),
    ] {
        conn.execute(
            "insert into meta (key, value) values (?1, ?2)
             on conflict(key) do update set value = ?2",
            params![k, v],
        )?;
    }
    Ok(())
}

pub fn register_pack(conn: &Connection, name: &str, sha: &str, dims: usize) -> Result<()> {
    record_identity(conn, name, sha, dims)?;
    Ok(())
}

/// Leaves the database ready to index with `name`: it takes away the
/// previous pack's vectors and records the new one's identity **in the same
/// transaction**.
///
/// Splitting that in two steps — as it used to be — opens a window where the
/// vectors belong to one pack and the registry names another. If the process
/// dies there, the database is left describing an index it does not have,
/// and nothing in the blobs gives it away when the dimensions match. Either
/// both happen or neither does.
/// It also clears the in-progress install mark, in that same transaction:
/// the mark exists to cover the stretch that ends right here, and leaving it
/// alive an instant longer would cost an unnecessary reindex.
pub fn adopt_pack(conn: &Connection, name: &str, sha: &str, dims: usize) -> Result<()> {
    let tx = escritura(conn)?;
    tx.execute("delete from chunk_vectors", [])?;
    record_identity(&tx, name, sha, dims)?;
    tx.execute("delete from meta where key = ?1", [INSTALANDO])?;
    tx.commit()?;
    Ok(())
}

/// The `meta` key that says "there is an install halfway through".
///
/// The pack file is put in place with a `rename`, which is atomic, but the
/// registry describing it is written afterwards: between the two there is a
/// window where `paquetes/{name}.anathem-pack` is the new build and `meta`
/// still describes the old one. A power cut or a SIGKILL there leaves that
/// lie on disk forever, and when neither name nor dimensions change —
/// updating the `en-es` build, the most common reinstall — the identity
/// guard does not see it, because the registered sha is discarded on purpose
/// so 22 MB are not hashed on every startup. Result: queries embedded with
/// the new pack against the old one's vectors, indefinitely, with `model
/// status` reporting full coverage.
///
/// The mark is written **before** the rename and cleared in the same
/// transaction that adopts the pack, so its existence is exactly equivalent
/// to "the file may not be the one `meta` describes". It does not shorten
/// the window: it makes it declared.
const INSTALANDO: &str = "pack_instalando";

/// Records that the file for pack `name` is about to be replaced.
pub fn mark_install(conn: &Connection, name: &str) -> Result<()> {
    let tx = escritura(conn)?;
    tx.execute(
        "insert into meta (key, value) values (?1, ?2)
         on conflict(key) do update set value = ?2",
        params![INSTALANDO, name],
    )?;
    tx.commit()?;
    Ok(())
}

/// El nombre del paquete cuya instalación quedó a medias, si quedó alguna.
pub fn pending_install(conn: &Connection) -> Result<Option<String>> {
    match conn.query_row("select value from meta where key = ?1", [INSTALANDO], |r| {
        r.get::<_, String>(0)
    }) {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Clears the mark. For when the install failed before touching the file —
/// the rename never happened — or for an orphaned mark already known to
/// describe no risk.
pub fn clear_install_mark(conn: &Connection) -> Result<()> {
    let tx = escritura(conn)?;
    tx.execute("delete from meta where key = ?1", [INSTALANDO])?;
    tx.commit()?;
    Ok(())
}

/// How the database identifies the pack it indexed with: name, sha256 and
/// dimensions. Two different packs do not agree on all three, and the sha is
/// the only one of the three that tells apart two builds with the same name
/// and the same dimensions — the case of updating `en-es` — so the three
/// always travel together.
pub type PackIdentity = (String, String, usize);

pub fn registered_pack(conn: &Connection) -> Result<Option<PackIdentity>> {
    // Do not use `.ok()`: that would turn any real sqlite error (missing
    // table, corrupt database) into `None`, indistinguishable from "no pack
    // registered". Degrading to bm25 is fine; swallowing a real error is
    // not.
    let parse = |k: &str| -> Result<Option<String>> {
        match conn.query_row("select value from meta where key = ?1", [k], |r| {
            r.get::<_, String>(0)
        }) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    };
    match (parse("pack_nombre")?, parse("pack_sha256")?, parse("pack_dims")?) {
        (Some(n), Some(s), Some(d)) => Ok(Some((n, s, d.parse()?))),
        _ => Ok(None),
    }
}

pub fn forget_pack(conn: &Connection) -> Result<()> {
    let tx = escritura(conn)?;
    tx.execute("delete from chunk_vectors", [])?;
    tx.execute(
        "delete from meta
          where key in ('pack_nombre','pack_sha256','pack_dims','pack_instalando')",
        [],
    )?;
    tx.commit()?;
    Ok(())
}

/// How a run of `sync` ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOutcome {
    /// No chunk was left without a vector. `n` new ones were written.
    Complete(usize),
    /// Another process changed the registered pack while this one was
    /// embedding. The batch in flight was discarded without writing it and
    /// chunks are left without a vector; `n` are the ones that did get
    /// written while the identity was still the expected one, and that the
    /// other process's `adopt_pack` has already deleted.
    ///
    /// **This is not an error.** Whoever saved a memory saved it all the
    /// same, and ending up with fewer vectors than there should be is
    /// exactly the acceptable degradation; the unacceptable one would have
    /// been writing that batch.
    Abandoned(usize),
}

impl SyncOutcome {
    /// How many vectors were written, however it ended.
    pub fn written(self) -> usize {
        match self {
            SyncOutcome::Complete(n) | SyncOutcome::Abandoned(n) => n,
        }
    }

    pub fn abandonada(self) -> bool {
        matches!(self, SyncOutcome::Abandoned(_))
    }
}

/// Embeds the chunks that still have no vector and returns how many.
///
/// It is idempotent on purpose: the write paths (save, edit, migrate,
/// reindex) call it when they finish instead of each embedding on its own,
/// so they cannot drift apart.
///
/// `expected` is the identity the database had `pack` registered under when
/// the caller opened it, and it is mandatory: without it this function
/// writes vectors for a pack that may have stopped being the registered one
/// a while ago.
///
/// That is the real case, not a contrived one: embedding a large backlog —
/// memories saved without a pack, or an interrupted `install` — takes
/// minutes, and in that time another terminal can run `mem model install`.
/// `adopt_pack` deletes the vectors and records the new pack in one
/// transaction, but that does not stop the process already embedding with
/// the old one: its later batches were written all the same, and the `on
/// conflict do nothing` stored them silently beside the new pack's. With
/// equal dimensions — the real case, since the format fixes 256 for every
/// pack — neither `search_vectors` nor `coverage` tells them apart, and
/// since `sync` skips chunks that already have a vector, no later `mem
/// reindex` corrects them: the mixture is permanent.
///
/// That is why the check goes **inside** each batch's `immediate`
/// transaction, which already holds the write lock: rereading `meta` there
/// is one `select` per batch, and it guarantees that no vector is written
/// unless the identity it was computed with is, at that very instant and
/// under that very lock, the registered one. Batches already written need no
/// undoing: whoever changes the identity goes through `adopt_pack`, which
/// deletes `chunk_vectors` entirely.
pub fn sync(conn: &Connection, pack: &Pack, expected: &PackIdentity) -> Result<SyncOutcome> {
    let mut total = 0usize;
    loop {
        // The pending set is recomputed each round: between batches,
        // another process may have embedded part of what was missing.
        let pending: Vec<(i64, String)> = conn
            .prepare(
                "select c.id, c.body from chunks c
                   left join chunk_vectors v on v.chunk_id = c.id
                  where v.chunk_id is null
                  order by c.id
                  limit ?1",
            )?
            .query_map([LOTE as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        if pending.is_empty() {
            break;
        }
        let mut vectores: Vec<(i64, Vec<u8>)> = Vec::with_capacity(pending.len());
        for (id, body) in &pending {
            vectores.push((*id, a_blob(&pack.embed(body)?)));
        }
        let tx = escritura(conn)?;
        // With the write lock already held: is the pack this batch was
        // embedded with still the registered one? If not, it is abandoned
        // without writing anything. The transaction dies without `commit`
        // and leaves no trace.
        if registered_pack(&tx)?.as_ref() != Some(expected) {
            return Ok(SyncOutcome::Abandoned(total));
        }
        for (id, blob) in &vectores {
            // Two things may have happened since the `select` above, and
            // neither is a fault of whoever saved a memory:
            //
            // - Another process embedded the same chunk. `do nothing`: the
            //   first one wins. Without this, three simultaneous `mem save`
            //   left two exiting with code 1 and `UNIQUE constraint failed`
            //   even though the memory had been saved, and over MCP the
            //   agent saw `-32603 internal error` for a successful save.
            // - A `mem edit` rechunked and the chunk no longer exists. The
            //   `where exists` keeps the foreign key from failing the insert
            //   of a chunk nobody is going to look for.
            total += tx.execute(
                "insert into chunk_vectors (chunk_id, vec)
                 select ?1, ?2 where exists (select 1 from chunks where id = ?1)
                 on conflict(chunk_id) do nothing",
                params![id, blob],
            )?;
        }
        tx.commit()?;
        if pending.len() < LOTE {
            break;
        }
    }
    Ok(SyncOutcome::Complete(total))
}

/// (chunks with a **usable** vector, total chunks). A gap means there are
/// memories the vector layer cannot see yet.
///
/// "Usable" is literal, and that is why the active pack's dimensions have to
/// be passed in: `search_vectors` discards as incompatible every vector that
/// is not `dims` floats long, so counting it would report perfect health
/// exactly when the hole is total. With a bare `count(*)`, the only
/// diagnostic that exists to make the gap visible said 2/2 while vector
/// search returned absolutely nothing.
pub fn coverage(conn: &Connection, dims: usize) -> Result<(usize, usize)> {
    let con: i64 = conn.query_row(
        "select count(*) from chunk_vectors where length(vec) = ?1",
        [(dims * 4) as i64],
        |r| r.get(0),
    )?;
    let tot: i64 = conn.query_row("select count(*) from chunks", [], |r| r.get(0))?;
    Ok((con as usize, tot as usize))
}

/// The `limit` memories most similar to `query`, under the same filters the
/// lexical search applies. Each memory appears once, with its closest
/// chunk.
pub fn search_vectors(
    conn: &Connection,
    query_: &[f32],
    q: &Query,
    limit: usize,
) -> Result<Vec<VectorHit>> {
    let mut sql = String::from(
        "select m.id, v.chunk_id, v.vec
           from chunk_vectors v
           join chunks c on c.id = v.chunk_id
           join memories m on m.id = c.memory_id
          where m.status = 'active'",
    );
    let mut ps: Vec<Value> = Vec::new();
    // The same filter the lexical search uses, from a single source: if the
    // two sides filtered differently, the vector side would return memories
    // bm25 discards and the merge would mix them in with nothing noticing.
    sql.push_str(&crate::search::filters_for(q, &mut ps));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ps.iter()), |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Vec<u8>>(2)?))
    })?;

    let mut mejor: HashMap<i64, (i64, f32)> = HashMap::new();
    for fila in rows {
        let (mid, cid, blob) = fila?;
        let v = de_blob(&blob);
        if v.len() != query_.len() {
            continue; // dimensiones distintas: vector de otro paquete
        }
        let cos: f32 = v.iter().zip(query_).map(|(a, b)| a * b).sum();
        let e = mejor.entry(mid).or_insert((cid, f32::NEG_INFINITY));
        if cos > e.1 {
            *e = (cid, cos);
        }
    }
    let mut out: Vec<VectorHit> = mejor
        .into_iter()
        .map(|(memory_id, (chunk_id, cosine))| VectorHit { memory_id, chunk_id, cosine })
        .collect();
    // Highest cosine first; on a tie, lowest id, for the same determinism
    // reason as in `fusion::rrf` and `search::run_explained`.
    out.sort_by(|a, b| {
        b.cosine
            .partial_cmp(&a.cosine)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.memory_id.cmp(&b.memory_id))
    });
    out.truncate(limit);
    Ok(out)
}
