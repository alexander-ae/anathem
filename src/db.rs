use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

const SCHEMA: &str = r#"
pragma journal_mode = wal;
pragma foreign_keys = on;

create table if not exists memories (
  id            integer primary key,
  project       text not null,
  title         text not null,
  body          text not null,
  created       text not null,
  updated       text not null,
  event_start   text,
  event_end     text,
  source        text not null check (source in ('explicit','suggested','migrated')),
  status        text not null default 'active'
                check (status in ('active','superseded','draft','forgotten')),
  pinned        integer not null default 0,
  last_accessed text,
  access_count  integer not null default 0,
  supersedes_id integer references memories(id),
  ext_id        text unique,
  topic         text,  -- topic key; see `ingest::save_with_topic`
  -- Who sent this memory, when it was not written here, and when it
  -- arrived. Null for everything local. See `db::add_provenance_columns`.
  received_from text,
  received_at   text
);

create table if not exists memory_tags (
  memory_id integer not null references memories(id) on delete cascade,
  tag       text not null,
  primary key (memory_id, tag)
);

create table if not exists memory_refs (
  memory_id integer not null references memories(id) on delete cascade,
  kind      text not null check (kind in ('ticket','pr','sha')),
  value     text not null,
  primary key (memory_id, kind, value)
);

create table if not exists chunks (
  id        integer primary key,
  memory_id integer not null references memories(id) on delete cascade,
  ord       integer not null,
  body      text not null,
  -- What gets indexed, which is not always what gets shown. For Latin text
  -- the two are identical; for Chinese and Japanese this holds the bigram
  -- form (see `chunk::segment_cjk`), because `unicode61` would otherwise
  -- index a whole clause as one token nobody can type.
  --
  -- It is a column and not a transformation in the trigger because
  -- `chunks_fts` is an external-content table: `insert into
  -- chunks_fts(chunks_fts) values ('rebuild')` re-reads this table, so a
  -- trigger-side transformation would be reverted by the next rebuild
  -- without a word.
  fts_body  text not null default ''
);

create virtual table if not exists chunks_fts using fts5(
  fts_body,
  content='chunks',
  content_rowid='id',
  tokenize="unicode61 remove_diacritics 2"
);

create table if not exists chunk_vectors (
  chunk_id integer primary key references chunks(id) on delete cascade,
  vec      blob not null   -- f32 little endian x dims, ya normalizado
);

create table if not exists meta (
  key text primary key,
  value text not null
);

create table if not exists search_log (
  id           integer primary key,
  ts           text not null,
  query        text not null,
  project      text,
  returned_ids text not null,
  budget_used  integer not null,
  followed_up  integer not null default 0,
  -- Which memory was opened, when it is known. Null in rows written before
  -- the column existed, where `followed_up` says that one was and not which.
  followed_up_id integer
);

create index if not exists idx_mem_project_status on memories(project, status);
create index if not exists idx_chunks_memory      on chunks(memory_id);
create index if not exists idx_tags_tag           on memory_tags(tag);
create index if not exists idx_refs_value         on memory_refs(value);

create trigger if not exists chunks_ai after insert on chunks begin
  insert into chunks_fts(rowid, fts_body) values (new.id, new.fts_body);
end;
create trigger if not exists chunks_ad after delete on chunks begin
  insert into chunks_fts(chunks_fts, rowid, fts_body) values ('delete', old.id, old.fts_body);
end;
create trigger if not exists chunks_au after update on chunks begin
  insert into chunks_fts(chunks_fts, rowid, fts_body) values ('delete', old.id, old.fts_body);
  insert into chunks_fts(rowid, fts_body) values (new.id, new.fts_body);
end;
"#;

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    // Several processes share the database: two `mem save` from two
    // terminals, the MCP server and the CLI at once, a `mem model install`
    // while something searches. Without this, the second writer gets
    // `database is locked` instantly instead of waiting its turn.
    //
    // Five seconds. Write transactions are short on purpose — `vecstore::sync`
    // embeds outside and writes in batches of 256 — so five seconds cover
    // dozens of other processes' batches back to back, far more than any
    // real contention lasts. Higher would make a genuinely stuck lock look
    // like a hung command; lower would go back to failing on a
    // millisecond-long race.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(SCHEMA)?;
    add_topic_column(&conn)?;
    add_handoff_columns(&conn)?;
    add_follow_up_column(&conn)?;
    add_provenance_columns(&conn)?;
    add_verified_column(&conn)?;
    migrate_indexed_body(&conn)?;
    migrate_project_name(&conn)?;
    restringir(path);
    Ok(conn)
}

/// Leaves the database and its directory readable only by their owner.
///
/// It holds every note the agents saved, and the default umask gives it
/// `0644`: on a shared machine any other local user reads the lot (finding
/// B-1). It runs on **every** open and not only on creation, because the
/// databases worth protecting are precisely the ones an earlier version
/// already left at `0644`.
///
/// It runs after `SCHEMA`, not before: that batch is what switches the
/// journal to WAL, and the `-wal`/`-shm` files — which carry the same
/// content as the database — do not exist until then. SQLite gives them the
/// database's own mode from that point on, so narrowing all three here is
/// enough for every later run.
///
/// Nothing here can fail the open. A database on a filesystem with no Unix
/// modes works perfectly well, and refusing to open it over a permission
/// that cannot exist would be trading a real feature for an imaginary
/// guarantee; `restrict_to_owner` warns on stderr instead.
fn restringir(path: &Path) {
    if let Some(parent) = path.parent() {
        if parent.as_os_str().is_empty() {
            // `--db mem.db`: the parent is the working directory, which is
            // not ours to narrow.
        } else {
            let _ = crate::permissions::restrict_to_owner(parent);
        }
    }
    let anexo = |suffix: &str| {
        let mut s = path.as_os_str().to_os_string();
        s.push(suffix);
        std::path::PathBuf::from(s)
    };
    for p in [path.to_path_buf(), anexo("-wal"), anexo("-shm")] {
        if p.exists() {
            let _ = crate::permissions::restrict_to_owner(&p);
        }
    }
}

fn has_column(conn: &Connection, table: &str, name: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "select count(*) from pragma_table_info(?1) where name = ?2",
        [table, name],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

fn has_topic_column(conn: &Connection) -> Result<bool> {
    has_column(conn, "memories", "topic")
}

/// Adds a column that may be missing from an older database, tolerating
/// another process winning the race. Same reasoning as `add_topic_column`:
/// the CLI and the MCP server can open the same database at once, both see
/// the column missing, and the second `alter` fails with "duplicate column
/// name". If the column is there after the failure, the result is the one
/// we wanted.
fn add_column(conn: &Connection, table: &str, name: &str, tipo: &str) -> Result<()> {
    if has_column(conn, table, name)? {
        return Ok(());
    }
    let sql = format!("alter table {table} add column {name} {tipo}");
    if let Err(e) = conn.execute(&sql, []) {
        if !has_column(conn, table, name)? {
            return Err(e.into());
        }
    }
    Ok(())
}

/// The columns and tables born with Spanish names, before the project
/// unified its code in English. They are renamed once; afterwards this does
/// nothing.
///
/// It uses `alter table ... rename column`, which SQLite supports since 3.25
/// and which preserves the data: nothing is copied and no table is rebuilt,
/// so there is no window in which rows could be lost. If the old column is
/// already gone — the case for every new database — nothing is touched.
fn rename_columns_to_english(conn: &Connection) -> Result<()> {
    for (tabla, old, new_id) in [
        ("memories", "agente", "agent"),
        ("memories", "creado_ts", "created_ts"),
        ("meta", "clave", "key"),
        ("meta", "valor", "value"),
    ] {
        let existe: i64 = conn.query_row(
            "select count(*) from pragma_table_info(?1) where name = ?2",
            [tabla, old],
            |r| r.get(0),
        )?;
        if existe == 0 {
            continue;
        }
        // The old index references the column by its previous name; it is
        // dropped before renaming and recreated below with the new one.
        conn.execute("drop index if exists idx_mem_project_creado", [])?;
        let sql = format!("alter table \"{tabla}\" rename column \"{old}\" to \"{new_id}\"");
        if let Err(e) = conn.execute(&sql, []) {
            // Another process may have won the race, same as with the new
            // columns: if it is already renamed, the result is the one we
            // wanted.
            let ya: i64 = conn.query_row(
                "select count(*) from pragma_table_info(?1) where name = ?2",
                [tabla, new_id],
                |r| r.get(0),
            )?;
            if ya == 0 {
                return Err(e.into());
            }
        }
    }
    Ok(())
}

/// The two columns that make it possible to tell what happened lately and
/// who wrote it. They are optional in the strong sense: a database where
/// nobody uses them behaves exactly as before, and older memories have them
/// null.
///
/// `created_ts` exists because `created` stores only the date. To know what
/// the other agent did a short while ago, a day of resolution is not enough.
///
/// `agent` is filled by the MCP server with the name the client sends in
/// `initialize`; anything saved from the CLI is left without an agent, which
/// is the truth.
/// Which memory was opened after a search, and not only that one was.
///
/// `followed_up` has been a flag since the log existed, and a flag cannot
/// answer the question `mem why <id>` asks — "was *this* memory the one the
/// agent opened" — when a search returned twenty candidates. Rows written
/// before this column keep their flag and leave the id null, which reads as
/// "yes, and which one was not recorded": the truth about them.
fn add_follow_up_column(conn: &Connection) -> Result<()> {
    add_column(conn, "search_log", "followed_up_id", "integer")
}

/// Moves an existing database onto the indexed-body column.
///
/// A database written before this indexed `chunks.body` directly, so every
/// Chinese or Japanese memory in it is one token and unreachable. `create
/// ... if not exists` cannot change a table that exists, so the FTS table
/// and its three triggers are dropped and rebuilt, and `fts_body` is filled
/// in from `body` — which for Latin text copies it unchanged.
///
/// It detects the old shape by looking at the stored DDL rather than keeping
/// a version number: the question is literally "does this index fts_body",
/// and `sqlite_master` answers it.
fn migrate_indexed_body(conn: &Connection) -> Result<()> {
    add_column(conn, "chunks", "fts_body", "text not null default ''")?;
    let ddl: String = conn
        .query_row(
            "select coalesce(sql, '') from sqlite_master where name = 'chunks_fts'",
            [],
            |r| r.get(0),
        )
        .unwrap_or_default();
    if ddl.contains("fts_body") {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(
        "drop trigger if exists chunks_ai;
         drop trigger if exists chunks_ad;
         drop trigger if exists chunks_au;
         drop table if exists chunks_fts;",
    )?;
    {
        let mut st = tx.prepare("select id, body from chunks")?;
        let rows: Vec<(i64, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut up = tx.prepare("update chunks set fts_body = ?2 where id = ?1")?;
        for (id, body) in rows {
            up.execute(rusqlite::params![id, crate::chunk::segment_cjk(&body)])?;
        }
    }
    tx.execute_batch(SCHEMA)?;
    tx.execute_batch("insert into chunks_fts(chunks_fts) values ('rebuild');")?;
    tx.commit()?;
    Ok(())
}

/// Moves memories off the Spanish name for "no project at all".
///
/// It is a value in an indexed column, so this is one `UPDATE` and not a
/// schema change, and it has to happen rather than only being read around:
/// a corpus with some rows under each spelling would filter wrong in both
/// directions. Writing the old name is folded on insert as well, so an
/// import of an older export does not bring it back.
fn migrate_project_name(conn: &Connection) -> Result<()> {
    conn.execute(
        "update memories set project = ?1 where project = ?2",
        rusqlite::params![crate::project::NO_PROJECT, crate::project::NO_PROJECT_WAS],
    )?;
    Ok(())
}

/// Where a memory came from, when it did not come from here.
///
/// `received_from` is a free label naming the agent or machine that sent it
/// — `claude-code@alexander-lenovo` — and `received_at` when it arrived.
/// Null means a local memory, which is every memory written before this and
/// every one anybody writes on their own machine, so the columns cost two
/// nulls and change nothing for them.
///
/// They are columns and not a value in `source`, which already distinguishes
/// `explicit` from `migrated`: `source` carries a CHECK constraint, and
/// changing a CHECK in SQLite means rebuilding the table and copying every
/// row. The cheap-looking option was the expensive one.
fn add_provenance_columns(conn: &Connection) -> Result<()> {
    add_column(conn, "memories", "received_from", "text")?;
    add_column(conn, "memories", "received_at", "text")
}

/// Whether someone checked this memory against the world, or only wrote it
/// down.
///
/// An integer and not a text with a CHECK constraint, for the same reason
/// `received_from` is a column: changing a CHECK in SQLite means rebuilding
/// the table and copying every row, and this needs room to grow a third
/// value without that cost. Today it holds 0 and 1.
///
/// `default 0` is what makes this additive. Every memory that existed before
/// the column reads as unverified, which is exactly what it is: nobody
/// checked it, because there was no way to say so. No backfill, and nothing
/// a reader has to be told about their old notes.
fn add_verified_column(conn: &Connection) -> Result<()> {
    add_column(conn, "memories", "verified", "integer not null default 0")
}

fn add_handoff_columns(conn: &Connection) -> Result<()> {
    rename_columns_to_english(conn)?;
    add_column(conn, "memories", "agent", "text")?;
    add_column(conn, "memories", "created_ts", "text")?;
    // Serves `mem recent`'s query, which filters by project and orders by
    // date descending.
    conn.execute(
        "create index if not exists idx_mem_project_created on memories(project, created_ts)",
        [],
    )?;
    Ok(())
}

/// A database created before topic keys has no `memories.topic`, and
/// `create table if not exists` does not touch it: it has to be added with
/// `alter table`. SQLite has no `add column if not exists`, so it is asked
/// first. The index is created here and not in `SCHEMA` because in an older
/// database the column does not exist yet when that batch runs.
///
/// Two processes can open the same older database at once (the CLI and the
/// MCP server): both see the column missing and the second `alter` fails
/// with "duplicate column name". If the column is there after the failure,
/// the other won the race and the result is the one we wanted.
fn add_topic_column(conn: &Connection) -> Result<()> {
    if !has_topic_column(conn)? {
        if let Err(e) = conn.execute("alter table memories add column topic text", []) {
            if !has_topic_column(conn)? {
                return Err(e.into());
            }
        }
    }
    conn.execute(
        "create index if not exists idx_mem_project_topic on memories(project, topic)",
        [],
    )?;
    // A topic key has at most one active memory per project: the current
    // one. Saving with a key already maintains that (it finds the current
    // one, supersedes it and only then sets the key on the new one, see
    // `ingest::save_with_topic`), but an invariant that depends on every
    // path remembering to uphold it is the one somebody breaks by accident.
    // With the index, a path that left two active fails in the database
    // instead of leaving `search` returning both.
    //
    // The only paths touching `status` are `forget` (to `forgotten`) and
    // `supersede::link` (to `superseded`); neither returns to `active`, so
    // there is no way to revive a memory and collide with the current one.
    // Rows with a null `topic` do not count: in a unique index nulls are all
    // distinct.
    conn.execute(
        "create unique index if not exists idx_mem_topic_activa
           on memories(project, topic)
           where status = 'active' and topic is not null",
        [],
    )?;
    Ok(())
}
