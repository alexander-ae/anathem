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

/// The commit it was built from, or `desconocido` when there was no git
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
    fn agregar(&mut self, level: Level, text: impl Into<String>) {
        self.lines.push(Line { level, text: text.into() });
    }
    fn bien(&mut self, text: impl Into<String>) {
        self.agregar(Level::Good, text);
    }
    fn info(&mut self, text: impl Into<String>) {
        self.agregar(Level::Info, text);
    }
    fn warning(&mut self, text: impl Into<String>) {
        self.agregar(Level::Warn, text);
    }
    fn mal(&mut self, text: impl Into<String>) {
        self.agregar(Level::Bad, text);
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
            (0, 0) => "Todo en orden.\n".to_string(),
            (0, a) => format!("Sin problemas ({a} {}).\n", plural(a, "aviso", "avisos")),
            (p, 0) => format!("{p} {}.\n", plural(p, "problema", "problemas")),
            (p, a) => format!(
                "{p} {} y {a} {}.\n",
                plural(p, "problema", "problemas"),
                plural(a, "aviso", "avisos")
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
    let (seccion_base, conn) = database_section(db);
    sections.push(seccion_base);
    if let Some(conn) = &conn {
        sections.push(vector_section(conn, dir));
    }
    sections.push(config_section(dir));
    Informe { sections }
}

fn binary_section() -> Section {
    let mut s = Section::new("Binario");
    s.bien(format!("mem {VERSION}"));
    if COMMIT == "desconocido" {
        s.warning(
            "commit desconocido: se compiló sin git a mano, así que no se puede saber \
             si este binario está al día con el código",
        );
    } else {
        s.bien(format!("compilado desde el commit {COMMIT}"));
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
        format!("{} y {} más", ids[..EJEMPLOS].join(", "), ids.len() - EJEMPLOS)
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
        s.mal(format!("no se pudo comprobar {what}: {e:#}"));
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
    let mut s = Section::new("Base de datos");
    if !db.exists() {
        // A warning and not information: with a mistyped `--db`, a
        // different `$HOME` or an unmounted volume, the summary cannot claim
        // everything is in order when no database was checked at all. The
        // exit code stays 0 because "I have not installed anything yet" is
        // legitimate.
        s.warning(format!(
            "{} no existe todavía: no hay base que revisar. Se crea con `mem init` o al \
             guardar la primera memoria; si esperabas encontrarla acá, revisá `--db`",
            db.display()
        ));
        return (s, None);
    }
    // Read-only, and deliberately not `db::open`: see the module's
    // documentation.
    let conn = match open_read_only(db) {
        Ok(c) => c,
        Err(e) => {
            s.mal(format!("no se pudo abrir {}: {e}", db.display()));
            return (s, None);
        }
    };
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    // Opening reads nothing: a file that is not SQLite only complains on
    // the first query.
    match entero(&conn, "select count(*) from sqlite_master where name = 'memories'") {
        Err(e) => {
            s.mal(format!(
                "{} no se pudo parse como base SQLite: {e}. ¿Es el archivo correcto? \
                 Si está corrupto, restaurá una copia.",
                db.display()
            ));
            return (s, None);
        }
        Ok(0) => {
            s.mal(format!(
                "{} existe pero no tiene el esquema de anathem; corré `mem init`",
                db.display()
            ));
            return (s, None);
        }
        Ok(_) => s.bien(format!("{} abre bien", db.display())),
    }
    check_database(&conn, &mut s);
    (s, Some(conn))
}

fn check_database(conn: &Connection, s: &mut Section) {
    match missing_vector_tables(conn) {
        Ok(v) if !v.is_empty() => s.info(format!(
            "la base es anterior a la capa vectorial (faltan las tablas {}); se actualiza \
             sola la próxima vez que un comando la abra para escribir",
            v.join(" y ")
        )),
        Ok(_) => {}
        Err(e) => s.mal(format!("no se pudo comprobar las tablas: {e}")),
    }
    guard(s, "los conteos", |s| counts(conn, s));
    guard(s, "el índice FTS5", |s| fts_index(conn, s));
    guard(s, "los huérfanos", |s| orphans(conn, s));
    guard(s, "las cadenas de supersession", |s| supersession(conn, s));
    guard(s, "las claves de tema", |s| topic_keys(conn, s));
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
            otro => s.warning(format!("{n} memorias con un estado desconocido: {otro}")),
        }
    }
    s.bien(format!(
        "memorias: {} activas, {} superseded, {} draft, {} forgotten",
        by_status[0], by_status[1], by_status[2], by_status[3]
    ));
    s.bien(format!(
        "fragmentos: {}, tags: {}, referencias: {}",
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
        s.bien(format!("índice FTS5 sincronizado ({indexed} de {chunks} fragmentos)"));
    } else {
        s.mal(format!(
            "índice FTS5 desincronizado: {chunks} fragmentos y {indexed} indexados \
             ({unindexed} sin indexar, {extras} indexados que ya no existen). \
             Corré `mem reindex`"
        ));
    }
    Ok(())
}

fn orphans(conn: &Connection, s: &mut Section) -> anyhow::Result<()> {
    let sin_memoria = entero(
        conn,
        "select count(*) from chunks c
          where not exists (select 1 from memories m where m.id = c.memory_id)",
    )?;
    // Without the table (a database predating the vector layer) there are
    // no vectors that could be orphaned.
    let sin_fragmento = if has_table(conn, "chunk_vectors")? {
        entero(
            conn,
            "select count(*) from chunk_vectors v
              where not exists (select 1 from chunks c where c.id = v.chunk_id)",
        )?
    } else {
        0
    };
    if sin_memoria == 0 && sin_fragmento == 0 {
        s.bien("sin huérfanos");
    } else {
        // The cascade prevents it, so if it happened there is a bug: worth
        // knowing.
        s.mal(format!(
            "huérfanos (no debería haber ninguno, el cascade lo impide: hay un bug que \
             reportar) — fragmentos sin memoria: {sin_memoria}, vectores sin fragmento: \
             {sin_fragmento}"
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
        s.bien("cadenas de supersession íntegras");
    } else {
        s.mal(format!(
            "cadena de supersession rota: {}, que no existe. No hay un comando que lo \
             repare; `mem history <id>` muestra hasta dónde llega la cadena",
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
            "la base es anterior a las claves de tema; se actualiza sola la próxima vez \
             que un comando la abra para escribir",
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
        s.bien("una sola memoria activa por (proyecto, clave de tema)");
    } else {
        s.mal(format!(
            "más de una memoria activa para la misma clave de tema, que el índice único \
             debería impedir: {}. Dejá una con `mem supersede <nueva> <vieja>` o \
             `mem forget <id>`",
            name_list(&duplicated)
        ));
    }
    Ok(())
}

fn vector_section(conn: &Connection, dir: &Path) -> Section {
    let mut s = Section::new("Capa vectorial");
    // The tables' absence was already reported in the database section;
    // here it is only a matter of not tripping over it.
    if missing_vector_tables(conn).map_or(false, |v| !v.is_empty()) {
        s.info("sin paquete de vocabulario: la base todavía no tiene capa vectorial");
        stray_files(&mut s, dir);
        return s;
    }
    match vecstore::registered_pack(conn) {
        Err(e) => s.mal(format!("no se pudo leer el paquete registrado: {e:#}")),
        Ok(None) => s.info(
            "sin paquete de vocabulario: la búsqueda es léxica (bm25), que es el modo \
             normal. Se instala uno con `mem model install <archivo>`",
        ),
        Ok(Some((name, _sha, dims))) => {
            s.bien(format!("paquete {name}, {dims} dimensiones"));
            pack_file_section(&mut s, dir, &name, dims);
            guard(&mut s, "la cobertura", |s| coverage(conn, s, dims));
        }
    }
    // The half-finished install mark belongs to the database, not the file:
    // it is always checked.
    match vecstore::pending_install(conn) {
        Ok(Some(n)) => s.warning(format!(
            "hay una instalación de {n} a medias: el archivo en disco puede no ser el que \
             indexó la base. La próxima búsqueda reindexa sola"
        )),
        Ok(None) => {}
        Err(e) => s.mal(format!("no se pudo leer la marca de instalación: {e:#}")),
    }
    stray_files(&mut s, dir);
    s
}

fn pack_file_section(s: &mut Section, dir: &Path, name: &str, dims: usize) {
    let pack_path = packstore::pack_path(dir, name);
    if !pack_path.is_file() {
        s.mal(format!(
            "falta el archivo {}: la búsqueda está degradada a bm25. Reinstalalo con \
             `mem model install <archivo>`",
            pack_path.display()
        ));
        return;
    }
    match Pack::open_pack(&pack_path) {
        Err(e) => s.mal(format!(
            "el archivo {} no se pudo open_pack ({e:#}): la búsqueda está degradada a bm25. \
             Reinstalalo con `mem model install <archivo>`",
            pack_path.display()
        )),
        Ok(p) if p.name() != name || p.dims() != dims => s.warning(format!(
            "el archivo {} es el paquete {} ({} dimensiones) y la base indexó con {name} \
             ({dims} dimensiones): la próxima búsqueda reindexa",
            pack_path.display(),
            p.name(),
            p.dims()
        )),
        Ok(_) => s.bien(format!("archivo presente y legible: {}", pack_path.display())),
    }
}

fn coverage(conn: &Connection, s: &mut Section, dims: usize) -> anyhow::Result<()> {
    let (con, tot) = vecstore::coverage(conn, dims)?;
    let vectores = entero(conn, "select count(*) from chunk_vectors")? as usize;
    let inservibles = vectores.saturating_sub(con);
    let nota = if inservibles > 0 {
        format!(" ({inservibles} vectores no miden {dims} dimensiones y no se usan)")
    } else {
        String::new()
    };
    if con == tot {
        s.bien(format!("cobertura: {con}/{tot} fragmentos con vector usable{nota}"));
    } else {
        s.mal(format!(
            "coverage: {con}/{tot} fragmentos con vector usable; faltan {}{nota}. \
             Corré `mem reindex`",
            tot - con
        ));
    }
    Ok(())
}

/// Leftovers from an interrupted install in the packs directory: the file
/// `packstore::replace_file` sets aside while putting the new one in
/// (`*.reemplazando-*`) and `packstore::install`'s temporary
/// (`.instalando-*`).
fn stray_files(s: &mut Section, dir: &Path) {
    let carpeta = packstore::packs_dir(dir);
    let Ok(entries) = std::fs::read_dir(&carpeta) else {
        return;
    };
    let mut extras: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".reemplazando-") || n.starts_with(".instalando-"))
        .collect();
    extras.sort();
    if extras.is_empty() {
        s.bien("sin archivos laterales sobrantes en el directorio de paquetes");
    } else {
        s.warning(format!(
            "archivos laterales sobrantes en {}: {}. Son restos de una instalación \
             interrumpida; si no hay ninguna en curso, se pueden borrar",
            carpeta.display(),
            name_list(&extras)
        ));
    }
}

fn config_section(dir: &Path) -> Section {
    let mut s = Section::new("Configuración");

    let pack_path = dir.join("config.json");
    // `cargar` fails on an invalid file: here it is caught and reported.
    match config::load(dir) {
        Ok(c) => {
            let values = format!(
                "peso_bm25={:?}, candidatos={}, extracto={}",
                c.bm25_weight, c.candidates, c.excerpt
            );
            if pack_path.exists() {
                s.bien(format!("{} válido: {values}", pack_path.display()));
            } else {
                s.bien(format!("sin config.json: valores por defecto ({values})"));
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
                s.bien(format!("{} válido: {} reglas", pack_path.display(), rules.len()));
            } else {
                s.bien(format!(
                    "sin tags.json: taxonomía por defecto ({} reglas)",
                    rules.len()
                ));
            }
        }
        Err(e) => s.mal(format!("tags.json inválido: {e}. Corregilo o borralo")),
    }
    s
}
