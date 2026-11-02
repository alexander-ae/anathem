//! `mem doctor`: everything one used to go look up by hand, in one command.
//!
//! It exists because three failures in a row were silent: an installed
//! binary that lagged behind the code, a vocabulary pack left uninstalled
//! while search kept running degraded, and a plugin that would not start.
//! Each took a while to discover, and each would have been visible at a
//! glance.
//!
//! Exit code: 0 when there are no problems (warnings and information do not
//! count), 1 when there are, 2 when doctor itself could not run.
//!
//! Two rules order the module:
//!
//! 1. **It is read-only.** It does not repair, migrate, or create either the
//!    database or its folder: it opens the database with
//!    `SQLITE_OPEN_READ_ONLY` and does *not* go through `db::open`, which
//!    runs the schema, creates missing tables and adds columns. A diagnostic
//!    that modifies what it diagnoses can hide the very problem it was asked
//!    to find. If something is wrong, it says which command fixes it and
//!    leaves a person to run it.
//! 2. **It never falls over on what it diagnoses.** Every check catches its
//!    own errors and turns them into a line; a broken file, a missing table
//!    or a database that is not even SQLite do not stop the other checks
//!    from running. A diagnostic that will not start when there is a problem
//!    is useless.
//!
//! One limit worth keeping in mind: the FTS5 index check verifies
//! **presence**, not content. It compares which `chunks` ids are in the
//! index and vice versa, but not that the indexed text is the one in
//! `chunks.body`: if the `body` changes without going through the trigger,
//! doctor says "in sync" and search does not find the new text. Checking
//! that (`integrity-check`) is an `insert`, and doctor does not write.

use crate::{config, pack::Pack, packstore, tags, vecstore};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

/// The crate's version, the one in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The commit it was built from, or `unknown` when there was no git
/// (see `build.rs`).
pub const COMMIT: &str = env!("ANATHEM_COMMIT");

/// What `mem --version` prints.
pub const FULL_VERSION: &str =
    concat!(env!("CARGO_PKG_VERSION"), " (commit ", env!("ANATHEM_COMMIT"), ")");

/// `mem doctor`'s exit code when it found problems.
pub const EXIT_PROBLEMS: i32 = 1;

/// `mem doctor`'s exit code when it could not run (a database path that
/// cannot be resolved, incorrect command-line usage). It differs from the
/// previous one so a script can tell "found problems" from "broke". With no
/// problems it exits 0.
pub const EXIT_BROKEN: i32 = 2;

/// How many concrete ids each check names before summarising with "y N
/// más": enough to go look at them without flooding the terminal.
const EJEMPLOS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// All good.
    Good,
    /// Information: something worth knowing that is not a problem.
    Info,
    /// Something odd that does not stop anything working; it does not
    /// change the exit code.
    Warn,
    /// A problem. It makes `mem doctor` exit non-zero.
    Bad,
}

impl Level {
    fn simbolo(self) -> &'static str {
        match self {
            Level::Good => "✓",
            Level::Info => "i",
            Level::Warn => "!",
            Level::Bad => "✗",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Line {
    pub level: Level,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Section {
    pub title: &'static str,
    pub lines: Vec<Line>,
}

impl Section {
    fn new(title: &'static str) -> Self {
        Section { title, lines: Vec::new() }
    }
    fn add(&mut self, level: Level, text: impl Into<String>) {
        self.lines.push(Line { level, text: text.into() });
    }
    fn bien(&mut self, text: impl Into<String>) {
        self.add(Level::Good, text);
    }
    fn info(&mut self, text: impl Into<String>) {
        self.add(Level::Info, text);
    }
    fn warning(&mut self, text: impl Into<String>) {
        self.add(Level::Warn, text);
    }
    fn mal(&mut self, text: impl Into<String>) {
        self.add(Level::Bad, text);
    }
}

#[derive(Debug, Clone)]
pub struct Informe {
    pub sections: Vec<Section>,
}

impl Informe {
    fn count_at(&self, level: Level) -> usize {
        self.sections.iter().flat_map(|s| &s.lines).filter(|l| l.level == level).count()
    }

    /// How many checks came out badly. It is what decides the exit code:
    /// warnings and information do not count.
    pub fn problems(&self) -> usize {
        self.count_at(Level::Bad)
    }

    pub fn warnings(&self) -> usize {
        self.count_at(Level::Warn)
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        for sec in &self.sections {
            s.push_str(sec.title);
            s.push('\n');
            for l in &sec.lines {
                s.push_str(&format!("  {} {}\n", l.level.simbolo(), l.text));
            }
            s.push('\n');
        }
        let (p, a) = (self.problems(), self.warnings());
        s.push_str(&match (p, a) {
            (0, 0) => "All good.\n".to_string(),
            (0, a) => format!("No problems ({a} {}).\n", plural(a, "warning", "warnings")),
            (p, 0) => format!("{p} {}.\n", plural(p, "problem", "problems")),
            (p, a) => format!(
                "{p} {} y {a} {}.\n",
                plural(p, "problem", "problems"),
                plural(a, "warning", "warnings")
            ),
        });
        s
    }
}

fn plural(n: usize, uno: &'static str, several: &'static str) -> &'static str {
    if n == 1 {
        uno
    } else {
        several
    }
}

/// Runs every check against the database at `db` and the directory holding
/// it. It writes nothing and does not fail: whatever cannot be checked is
/// stated in the report.
pub fn diagnose(db: &Path) -> Informe {
    // `parent()` of a path with no folder (`mem.db`) is `""`, which joined
    // with a name gives a path relative to the current directory: the right
    // thing.
    let dir = db.parent().unwrap_or(Path::new("."));
    let mut sections = vec![binary_section()];
    let (database_sec, conn) = database_section(db);
    sections.push(database_sec);
    if let Some(conn) = &conn {
        sections.push(vector_section(conn, dir));
    }
    sections.push(config_section(dir));
    Informe { sections }
}

fn binary_section() -> Section {
    let mut s = Section::new("Binary");
    s.bien(format!("mem {VERSION}"));
    if COMMIT == "unknown" {
        s.warning(
            "unknown commit: this was built without git at hand, so there is no way to \
             tell whether this binary is up to date with the code",
        );
    } else {
        s.bien(format!("built from commit {COMMIT}"));
    }
    s
}

/// `count(*)` and friends: a query returning one integer.
fn entero(conn: &Connection, sql: &str) -> rusqlite::Result<i64> {
    conn.query_row(sql, [], |r| r.get(0))
}

fn has_table(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    Ok(conn.query_row(
        "select count(*) from sqlite_master where type in ('table', 'view') and name = ?1",
        [name],
        |r| r.get::<_, i64>(0),
    )? > 0)
}

/// The vector layer's tables, which a database predating it does not have.
/// It is the same case as a database predating topic keys: nothing is
/// broken, `db::open` creates them (`create table if not exists`) the next
/// time a command opens the database for writing.
const TABLAS_VECTORIALES: [&str; 2] = ["chunk_vectors", "meta"];

fn missing_vector_tables(conn: &Connection) -> rusqlite::Result<Vec<&'static str>> {
    let mut v = Vec::new();
    for t in TABLAS_VECTORIALES {
        if !has_table(conn, t)? {
            v.push(t);
        }
    }
    Ok(v)
}

/// Lists ids as `1, 2, 3 y 4 más`, without going past `EJEMPLOS`.
/// The Spanish in the output is deliberate: it is user-facing text.
fn name_list(ids: &[String]) -> String {
    if ids.len() <= EJEMPLOS {
        ids.join(", ")
    } else {
        format!("{} and {} more", ids[..EJEMPLOS].join(", "), ids.len() - EJEMPLOS)
    }
}

/// Returns each row's first column as text.
fn texts(conn: &Connection, sql: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare(sql)?.query_map([], |r| r.get::<_, String>(0))?.collect()
}

/// Wraps a check that can fail: if it fails, it does not fall over, it
/// says so.
fn guard(s: &mut Section, what: &str, f: impl FnOnce(&mut Section) -> anyhow::Result<()>) {
    if let Err(e) = f(s) {
        s.mal(format!("could not check {what}: {e:#}"));
    }
}

/// Opens the database unable to write it, and without leaving a trace in
/// the directory.
///
/// `SQLITE_OPEN_READ_ONLY` alone is not enough: on a WAL-mode database,
/// SQLite creates `mem.db-wal` and `mem.db-shm` even for a read-only
/// connection, and since it cannot checkpoint on close, it leaves them
/// behind. That is not content, but it is a diagnostic dirtying the very
/// folder it diagnoses.
///
/// When there is no `-wal` it is because no connection has the database open
/// (the last one to close deletes it after checkpointing) and everything
/// written is already in the main file: there it opens `immutable=1`, which
/// neither takes locks nor touches the WAL. If the `-wal` exists, someone is
/// writing or a write was left unconsolidated, and ignoring it would show a
/// stale picture: there it opens normally, which reads it and creates
/// nothing new.
fn open_read_only(db: &Path) -> rusqlite::Result<Connection> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut wal = db.as_os_str().to_os_string();
        wal.push("-wal");
        if !Path::new(&wal).exists() {
            let mut uri = String::from("file:");
            for &b in db.as_os_str().as_bytes() {
                match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                        uri.push(b as char)
                    }
                    _ => uri.push_str(&format!("%{b:02X}")),
                }
            }
            uri.push_str("?immutable=1");
            return Connection::open_with_flags(
                uri,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            );
        }
    }
    Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

fn database_section(db: &Path) -> (Section, Option<Connection>) {
    let mut s = Section::new("Database");
    if !db.exists() {
        // A warning and not information: with a mistyped `--db`, a
        // different `$HOME` or an unmounted volume, the summary cannot claim
        // everything is in order when no database was checked at all. The
        // exit code stays 0 because "I have not installed anything yet" is
        // legitimate.
        s.warning(format!(
            "{} does not exist yet: there is no database to check. It is created by \
             `mem init` or by saving the first memory; if you expected to find it here, \
             check `--db`",
            db.display()
        ));
        return (s, None);
    }
    // Read-only, and deliberately not `db::open`: see the module's
    // documentation.
    let conn = match open_read_only(db) {
        Ok(c) => c,
        Err(e) => {
            s.mal(format!("could not open {}: {e}", db.display()));
            return (s, None);
        }
    };
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    // Opening reads nothing: a file that is not SQLite only complains on
    // the first query.
    match entero(&conn, "select count(*) from sqlite_master where name = 'memories'") {
        Err(e) => {
            s.mal(format!(
                "{} could not be read as a SQLite database: {e}. Is this the right file? \
                 If it is corrupt, restore a copy.",
                db.display()
            ));
            return (s, None);
        }
        Ok(0) => {
            s.mal(format!(
                "{} exists but does not have anathem's schema; run `mem init`",
                db.display()
            ));
            return (s, None);
        }
        Ok(_) => s.bien(format!("{} opens fine", db.display())),
    }
    check_database(&conn, &mut s);
    (s, Some(conn))
}

fn check_database(conn: &Connection, s: &mut Section) {
    match missing_vector_tables(conn) {
        Ok(v) if !v.is_empty() => s.info(format!(
            "this database predates the vector layer (the tables {} are missing); it \
             updates itself the next time a command opens it to write",
            v.join(" y ")
        )),
        Ok(_) => {}
        Err(e) => s.mal(format!("could not check the tables: {e}")),
    }
    guard(s, "the counts", |s| counts(conn, s));
    guard(s, "the FTS5 index", |s| fts_index(conn, s));
    guard(s, "the orphans", |s| orphans(conn, s));
    guard(s, "the supersession chains", |s| supersession(conn, s));
    guard(s, "the topic keys", |s| topic_keys(conn, s));
}

fn counts(conn: &Connection, s: &mut Section) -> anyhow::Result<()> {
    let mut by_status = [0i64; 4];
    let mut stmt = conn.prepare("select status, count(*) from memories group by status")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    for f in rows {
        let (status_, n) = f?;
        match status_.as_str() {
            "active" => by_status[0] = n,
            "superseded" => by_status[1] = n,
            "draft" => by_status[2] = n,
            "forgotten" => by_status[3] = n,
            // The table's check constraint prevents it; if an older
            // database carried another, it would add to no column and the
            // total would not add up with no explanation.
            other => s.warning(format!("{n} memories with an unknown status: {other}")),
        }
    }
    s.bien(format!(
        "memories: {} active, {} superseded, {} draft, {} forgotten",
        by_status[0], by_status[1], by_status[2], by_status[3]
    ));
    s.bien(format!(
        "chunks: {}, tags: {}, references: {}",
        entero(conn, "select count(*) from chunks")?,
        entero(conn, "select count(*) from memory_tags")?,
        entero(conn, "select count(*) from memory_refs")?,
    ));
    Ok(())
}

/// Compares the chunks against what the FTS5 index actually holds.
///
/// **`select count(*) from chunks_fts` cannot be used**: `chunks_fts` is an
/// external-content table (`content='chunks'`) and a full scan reads the
/// rows of `chunks`, not of the index, so it would always give the same
/// number as `chunks` and never detect anything. What does reflect what was
/// really indexed is the shadow table `chunks_fts_docsize`, which has one
/// row per indexed document.
fn fts_index(conn: &Connection, s: &mut Section) -> anyhow::Result<()> {
    let chunks = entero(conn, "select count(*) from chunks")?;
    let indexed = entero(conn, "select count(*) from chunks_fts_docsize")?;
    let unindexed = entero(
        conn,
        "select count(*) from chunks c
          where not exists (select 1 from chunks_fts_docsize d where d.id = c.id)",
    )?;
    let extras = entero(
        conn,
        "select count(*) from chunks_fts_docsize d
          where not exists (select 1 from chunks c where c.id = d.id)",
    )?;
    if unindexed == 0 && extras == 0 {
        s.bien(format!("FTS5 index in sync ({indexed} of {chunks} chunks)"));
    } else {
        s.mal(format!(
            "FTS5 index out of sync: {chunks} chunks and {indexed} indexed \
             ({unindexed} unindexed, {extras} indexed that no longer exist). \
             Run `mem reindex`"
        ));
    }
    Ok(())
}

fn orphans(conn: &Connection, s: &mut Section) -> anyhow::Result<()> {
    let chunks_no_memory = entero(
        conn,
        "select count(*) from chunks c
          where not exists (select 1 from memories m where m.id = c.memory_id)",
    )?;
    // Without the table (a database predating the vector layer) there are
    // no vectors that could be orphaned.
    let vectors_no_chunk = if has_table(conn, "chunk_vectors")? {
        entero(
            conn,
            "select count(*) from chunk_vectors v
              where not exists (select 1 from chunks c where c.id = v.chunk_id)",
        )?
    } else {
        0
    };
    if chunks_no_memory == 0 && vectors_no_chunk == 0 {
        s.bien("no orphans");
    } else {
        // The cascade prevents it, so if it happened there is a bug: worth
        // knowing.
        s.mal(format!(
            "orphans (there should be none, the cascade prevents it: this is a bug worth \
             reporting) — chunks with no memory: {chunks_no_memory}, vectors with no chunk: \
             {vectors_no_chunk}"
        ));
    }
    Ok(())
}

fn supersession(conn: &Connection, s: &mut Section) -> anyhow::Result<()> {
    let broken = texts(
        conn,
        "select 'memoria ' || m.id || ' reemplaza a la ' || m.supersedes_id
           from memories m
          where m.supersedes_id is not null
            and not exists (select 1 from memories p where p.id = m.supersedes_id)
          order by m.id",
    )?;
    if broken.is_empty() {
        s.bien("supersession chains intact");
    } else {
        s.mal(format!(
            "broken supersession chain: {}, which does not exist. There is no command \
             that repairs it; `mem history <id>` shows how far the chain reaches",
            name_list(&broken)
        ));
    }
    Ok(())
}

fn topic_keys(conn: &Connection, s: &mut Section) -> anyhow::Result<()> {
    // A database from before topic keys has no such column, and there is
    // nothing to say about an invariant that does not exist yet. It is not
    // added here: fixing that is `db::open`'s job, which it does on the
    // first command that writes.
    let has_column = entero(
        conn,
        "select count(*) from pragma_table_info('memories') where name = 'topic'",
    )? > 0;
    if !has_column {
        s.info(
            "this database predates topic keys; it updates itself the next time a \
             command opens it to write",
        );
        return Ok(());
    }
    let duplicated = texts(
        conn,
        "select '(' || project || ', ' || topic || '): memorias ' || group_concat(id, ', ')
           from memories
          where status = 'active' and topic is not null
          group by project, topic
         having count(*) > 1
          order by project, topic",
    )?;
    if duplicated.is_empty() {
        s.bien("exactly one active memory per (project, topic key)");
    } else {
        s.mal(format!(
            "more than one active memory under the same topic key, which the unique \
             index should prevent: {}. Leave one with `mem supersede <new> <old>` or \
             `mem forget <id>`",
            name_list(&duplicated)
        ));
    }
    Ok(())
}

fn vector_section(conn: &Connection, dir: &Path) -> Section {
    let mut s = Section::new("Vector layer");
    // The tables' absence was already reported in the database section;
    // here it is only a matter of not tripping over it.
    if missing_vector_tables(conn).map_or(false, |v| !v.is_empty()) {
        s.info("no vocabulary pack: this database has no vector layer yet");
        stray_files(&mut s, dir);
        return s;
    }
    match vecstore::registered_pack(conn) {
        Err(e) => s.mal(format!("could not read the registered pack: {e:#}")),
        Ok(None) => s.info(
            "no vocabulary pack: search is lexical (bm25), which is the normal mode. \
             One is installed with `mem model install <file>`",
        ),
        Ok(Some((name, _sha, dims))) => {
            s.bien(format!("pack {name}, {dims} dimensions"));
            pack_file_section(&mut s, dir, &name, dims);
            guard(&mut s, "the coverage", |s| coverage(conn, s, dims));
        }
    }
    // The half-finished install mark belongs to the database, not the file:
    // it is always checked.
    match vecstore::pending_install(conn) {
        Ok(Some(n)) => s.warning(format!(
            "there is a half-done install of {n}: the file on disk may not be the one \
             this database indexed with. The next search reindexes on its own"
        )),
        Ok(None) => {}
        Err(e) => s.mal(format!("could not read the install mark: {e:#}")),
    }
    stray_files(&mut s, dir);
    s
}

fn pack_file_section(s: &mut Section, dir: &Path, name: &str, dims: usize) {
    let pack_path = packstore::pack_path(dir, name);
    if !pack_path.is_file() {
        s.mal(format!(
            "the file {} is missing: search is degraded to bm25. Reinstall it with \
             `mem model install <file>`",
            pack_path.display()
        ));
        return;
    }
    match Pack::open_pack(&pack_path) {
        Err(e) => s.mal(format!(
            "the file {} could not be opened ({e:#}): search is degraded to bm25. \
             Reinstall it with `mem model install <file>`",
            pack_path.display()
        )),
        Ok(p) if p.name() != name || p.dims() != dims => s.warning(format!(
            "the file {} is the pack {} ({} dimensions) and this database indexed with \
             {name} ({dims} dimensions): the next search reindexes",
            pack_path.display(),
            p.name(),
            p.dims()
        )),
        Ok(_) => s.bien(format!("file present and readable: {}", pack_path.display())),
    }
}

fn coverage(conn: &Connection, s: &mut Section, dims: usize) -> anyhow::Result<()> {
    let (covered, tot) = vecstore::coverage(conn, dims)?;
    let vectors = entero(conn, "select count(*) from chunk_vectors")? as usize;
    let unusable = vectors.saturating_sub(covered);
    let note = if unusable > 0 {
        format!(" ({unusable} vectors are not {dims} dimensions and go unused)")
    } else {
        String::new()
    };
    if covered == tot {
        s.bien(format!("coverage: {covered}/{tot} chunks with a usable vector{note}"));
    } else {
        s.mal(format!(
            "coverage: {covered}/{tot} chunks with a usable vector; {} are missing{note}. \
             Run `mem reindex`",
            tot - covered
        ));
    }
    Ok(())
}

/// Leftovers from an interrupted install in the packs directory: the file
/// `packstore::replace_file` sets aside while putting the new one in
/// (`*.reemplazando-*`) and `packstore::install`'s temporary
/// (`.instalando-*`).
fn stray_files(s: &mut Section, dir: &Path) {
    let folder = packstore::packs_dir(dir);
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return;
    };
    let mut extras: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".reemplazando-") || n.starts_with(".instalando-"))
        .collect();
    extras.sort();
    if extras.is_empty() {
        s.bien("no leftover sibling files in the packs directory");
    } else {
        s.warning(format!(
            "leftover sibling files in {}: {}. They are remains of an interrupted \
             install; if none is running, they can be deleted",
            folder.display(),
            name_list(&extras)
        ));
    }
}

fn config_section(dir: &Path) -> Section {
    let mut s = Section::new("Configuration");

    let pack_path = dir.join("config.json");
    // `cargar` fails on an invalid file: here it is caught and reported.
    match config::load(dir) {
        Ok(c) => {
            let values = format!(
                "bm25_weight={:?}, candidates={}, excerpt={}",
                c.bm25_weight, c.candidates, c.excerpt
            );
            if pack_path.exists() {
                s.bien(format!("{} valid: {values}", pack_path.display()));
            } else {
                s.bien(format!("no config.json: defaults ({values})"));
            }
        }
        Err(e) => s.mal(format!(
            "config.json inválido: {e:#}. `mem search`, `mem eval` y `mem serve` no \
             arrancan mientras tanto; corregilo o borralo"
        )),
    }

    let pack_path = dir.join("tags.json");
    match tags::load_rules(Some(dir)) {
        Ok(rules) => {
            if pack_path.exists() {
                s.bien(format!("{} valid: {} rules", pack_path.display(), rules.len()));
            } else {
                s.bien(format!(
                    "no tags.json: default taxonomy ({} rules)",
                    rules.len()
                ));
            }
        }
        Err(e) => s.mal(format!("invalid tags.json: {e}. Fix it or delete it")),
    }
    s
}
