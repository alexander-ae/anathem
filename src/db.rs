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
  topic         text   -- clave de topic_label; ver `ingest::save_with_topic`
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
  body      text not null
);

create virtual table if not exists chunks_fts using fts5(
  body,
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
  followed_up  integer not null default 0
);

create index if not exists idx_mem_project_status on memories(project, status);
create index if not exists idx_chunks_memory      on chunks(memory_id);
create index if not exists idx_tags_tag           on memory_tags(tag);
create index if not exists idx_refs_value         on memory_refs(value);

create trigger if not exists chunks_ai after insert on chunks begin
  insert into chunks_fts(rowid, body) values (new.id, new.body);
end;
create trigger if not exists chunks_ad after delete on chunks begin
  insert into chunks_fts(chunks_fts, rowid, body) values ('delete', old.id, old.body);
end;
create trigger if not exists chunks_au after update on chunks begin
  insert into chunks_fts(chunks_fts, rowid, body) values ('delete', old.id, old.body);
  insert into chunks_fts(rowid, body) values (new.id, new.body);
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
/// guarantee; `restringir_a_dueno` warns on stderr instead.
fn restringir(path: &Path) {
    if let Some(parent) = path.parent() {
        if parent.as_os_str().is_empty() {
            // `--db mem.db`: the parent is the working directory, which is
            // not ours to narrow.
        } else {
            let _ = crate::permisos::restringir_a_dueno(parent);
        }
    }
    let anexo = |sufijo: &str| {
        let mut s = path.as_os_str().to_os_string();
        s.push(sufijo);
        std::path::PathBuf::from(s)
    };
    for p in [path.to_path_buf(), anexo("-wal"), anexo("-shm")] {
        if p.exists() {
            let _ = crate::permisos::restringir_a_dueno(&p);
        }
    }
}

fn has_column(conn: &Connection, name: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "select count(*) from pragma_table_info('memories') where name = ?1",
        [name],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

fn has_topic_column(conn: &Connection) -> Result<bool> {
    has_column(conn, "topic")
}

/// Adds a column that may be missing from an older database, tolerating
/// another process winning the race. Same reasoning as `add_topic_column`:
/// the CLI and the MCP server can open the same database at once, both see
/// the column missing, and the second `alter` fails with "duplicate column
/// name". If the column is there after the failure, the result is the one
/// we wanted.
fn add_column(conn: &Connection, name: &str, tipo: &str) -> Result<()> {
    if has_column(conn, name)? {
        return Ok(());
    }
    let sql = format!("alter table memories add column {name} {tipo}");
    if let Err(e) = conn.execute(&sql, []) {
        if !has_column(conn, name)? {
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
fn add_handoff_columns(conn: &Connection) -> Result<()> {
    rename_columns_to_english(conn)?;
    add_column(conn, "agent", "text")?;
    add_column(conn, "created_ts", "text")?;
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
