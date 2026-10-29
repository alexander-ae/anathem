//! Reading back what `mem export` wrote.
//!
//! Until now the export was one-way: it served to leave anathem, but not to
//! come back. That left three everyday things unanswered — restoring a
//! backup, moving the memory to another machine, and syncing two computers
//! by hand with git — none of which need a server or an account, only being
//! able to read what you wrote yourself.
//!
//! Nothing runs unless it is invoked, so it costs the rest neither memory
//! nor time.

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, TransactionBehavior};
use std::collections::HashMap;
use std::path::Path;

/// A memory as it comes from the file, before touching the database.
#[derive(Debug, Default, Clone)]
pub struct ParsedMemory {
    pub old_id: Option<i64>,
    pub project: String,
    pub title: String,
    pub body: String,
    pub created: String,
    pub updated: String,
    pub status: String,
    pub source: String,
    pub pinned: bool,
    pub topic: Option<String>,
    pub ext_id: Option<String>,
    pub supersedes: Option<i64>,
    pub event_start: Option<String>,
    pub event_end: Option<String>,
    pub tags: Vec<String>,
    pub refs: Vec<(String, String)>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub inserted: usize,
    pub skipped: usize,
    pub chains: usize,
}

/// Strips the quotes from a YAML value written by `export::yaml_dq`.
fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        v[1..v.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        v.to_string()
    }
}

/// A single-line YAML list: `["a", "b"]`.
fn inline_list(v: &str) -> Vec<String> {
    let v = v.trim().trim_start_matches('[').trim_end_matches(']');
    v.split(',').map(unquote).filter(|s| !s.is_empty()).collect()
}

/// Parses one file from the export. Returns `None` when it has no
/// frontmatter, so `--desde` can be pointed at a folder with other things
/// inside without blowing up.
pub fn parse(texto: &str) -> Option<ParsedMemory> {
    let resto = texto.strip_prefix("---\n")?;
    let fin = resto.find("\n---\n")?;
    let (cabecera, cuerpo) = (&resto[..fin], &resto[fin + 5..]);

    let mut m = ParsedMemory { body: cuerpo.trim_start_matches('\n').trim_end().to_string(), ..Default::default() };
    m.status = "active".into();
    m.source = "explicit".into();

    let lines_: Vec<&str> = cabecera.lines().collect();
    let mut i = 0;
    while i < lines_.len() {
        let l = lines_[i];
        i += 1;
        // Refs are a list of pairs on following lines, indented.
        if l.trim_start().starts_with("- kind:") {
            let kind = unquote(l.split_once("kind:").map(|x| x.1).unwrap_or(""));
            let value = lines_
                .get(i)
                .and_then(|x| x.split_once("value:"))
                .map(|x| unquote(x.1))
                .unwrap_or_default();
            if !kind.is_empty() && !value.is_empty() {
                m.refs.push((kind, value));
                i += 1;
            }
            continue;
        }
        let Some((k, v)) = l.split_once(':') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "id" => m.old_id = v.parse().ok(),
            "project" => m.project = unquote(v),
            "description" => {
                if m.title.is_empty() {
                    m.title = unquote(v);
                }
            }
            "created" => m.created = unquote(v),
            "updated" => m.updated = unquote(v),
            "status" => m.status = unquote(v),
            "source" => m.source = unquote(v),
            "pinned" => m.pinned = v == "true",
            "topic" => m.topic = Some(unquote(v)).filter(|s| !s.is_empty()),
            "ext_id" => m.ext_id = Some(unquote(v)).filter(|s| !s.is_empty()),
            "supersedes" => m.supersedes = v.parse().ok(),
            "event_start" => m.event_start = Some(unquote(v)).filter(|s| !s.is_empty()),
            "event_end" => m.event_end = Some(unquote(v)).filter(|s| !s.is_empty()),
            "tags" => m.tags = inline_list(v),
            _ => {}
        }
    }
    if m.project.is_empty() || m.body.is_empty() {
        return None;
    }
    if m.title.is_empty() {
        m.title = crate::ingest::title_from(&m.body);
    }
    if m.updated.is_empty() {
        m.updated = m.created.clone();
    }
    Some(m)
}

/// Imports everything under `dir`.
///
/// `merge` allows importing into a database that already holds memories;
/// without it, the database must be empty. The difference is not
/// convenience: importing on top of existing content duplicates everything
/// already there, and finding that out later costs more than typing a flag.
///
/// What is already present is skipped, comparing project and body: that is
/// the identity surviving an export round trip, because ids get reassigned.
pub fn import_dir(conn: &Connection, dir: &Path, merge: bool) -> Result<Summary> {
    if !dir.is_dir() {
        bail!("{} no es una carpeta", dir.display());
    }
    let had: i64 = conn.query_row("select count(*) from memories", [], |r| r.get(0))?;
    if had > 0 && !merge {
        bail!(
            "la base ya tiene {had} memorias. Importar encima duplicaría lo que ya está.\n   \
             Usá --fusionar para agregar solo lo que falte, o apuntá --db a una base nueva."
        );
    }

    let mut files: Vec<std::path::PathBuf> = Vec::new();
    collect_files(dir, &mut files)?;
    files.sort();

    let mut parsed: Vec<ParsedMemory> = Vec::new();
    for a in &files {
        let txt = std::fs::read_to_string(a)
            .with_context(|| format!("no se pudo leer {}", a.display()))?;
        if let Some(m) = parse(&txt) {
            parsed.push(m);
        }
    }

    let tx = Transaction_inmediata(conn)?;
    // What is already there, so it is not duplicated.
    let mut existing: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    {
        let mut st = tx.prepare("select project, body from memories")?;
        let filas = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for f in filas {
            existing.insert(f?);
        }
    }

    // All the rows first, without `supersedes_id`: the id it points at may
    // belong to a memory not inserted yet, and resolving it on the fly would
    // leave chains broken depending on the order of the files.
    let mut map_: HashMap<i64, i64> = HashMap::new();
    let mut resumen = Summary::default();
    for m in &parsed {
        if existing.contains(&(m.project.clone(), m.body.clone())) {
            resumen.skipped += 1;
            continue;
        }
        tx.execute(
            "insert into memories
               (project, title, body, created, updated, event_start, event_end,
                source, status, pinned, ext_id, topic)
             values (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                m.project, m.title, m.body, m.created, m.updated, m.event_start,
                m.event_end, m.source, m.status, m.pinned as i64, m.ext_id, m.topic
            ],
        )?;
        let new_id = tx.last_insert_rowid();
        if let Some(old) = m.old_id {
            map_.insert(old, new_id);
        }
        existing.insert((m.project.clone(), m.body.clone()));
        for (ord, cuerpo) in
            crate::chunk::split(&m.body, crate::ingest::CHUNK_SIZE, crate::ingest::CHUNK_STEP)
                .iter()
                .enumerate()
        {
            tx.execute(
                "insert into chunks (memory_id, ord, body) values (?1, ?2, ?3)",
                params![new_id, ord as i64, cuerpo],
            )?;
        }
        for t in &m.tags {
            tx.execute(
                "insert or ignore into memory_tags (memory_id, tag) values (?1, ?2)",
                params![new_id, t],
            )?;
        }
        for (kind, value) in &m.refs {
            tx.execute(
                "insert or ignore into memory_refs (memory_id, kind, value) values (?1, ?2, ?3)",
                params![new_id, kind, value],
            )?;
        }
        resumen.inserted += 1;
    }

    // Only now the chains, with every new id already known.
    for m in &parsed {
        let (Some(old), Some(apunta)) = (m.old_id, m.supersedes) else { continue };
        let (Some(&new_id), Some(&target)) = (map_.get(&old), map_.get(&apunta)) else {
            continue;
        };
        tx.execute("update memories set supersedes_id = ?2 where id = ?1", params![new_id, target])?;
        resumen.chains += 1;
    }
    tx.commit()?;
    Ok(resumen)
}

/// `Transaction::new_unchecked` with `Immediate`, same as saving: if two
/// processes import at once, the second waits instead of failing halfway.
#[allow(non_snake_case)]
fn Transaction_inmediata(conn: &Connection) -> Result<rusqlite::Transaction<'_>> {
    Ok(rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?)
}

fn collect_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            collect_files(&p, out)?;
        } else if p.extension().is_some_and(|x| x == "md") {
            out.push(p);
        }
    }
    Ok(())
}
