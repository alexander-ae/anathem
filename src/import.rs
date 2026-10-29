//! Leer de vuelta lo que escribió `mem export`.
//!
//! Hasta ahora el export era de ida nomás: servía para irse de anathem, pero
//! no para volver. Eso dejaba sin respuesta tres cosas de todos los días —
//! restaurar un respaldo, mover la memoria a otra máquina, y sincronizar dos
//! computadoras a mano con git— que no necesitan servidor ni cuenta, solo
//! poder leer lo que uno mismo escribió.
//!
//! No corre nada mientras no se lo invoque, así que no cuesta memoria ni
//! tiempo al resto.

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, TransactionBehavior};
use std::collections::HashMap;
use std::path::Path;

/// Una memoria tal como viene del archivo, todavía sin tocar la base.
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

/// Saca las comillas de un valor YAML escrito por `export::yaml_dq`.
fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        v[1..v.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        v.to_string()
    }
}

/// Lista YAML en una línea: `["a", "b"]`.
fn inline_list(v: &str) -> Vec<String> {
    let v = v.trim().trim_start_matches('[').trim_end_matches(']');
    v.split(',').map(unquote).filter(|s| !s.is_empty()).collect()
}

/// Lee un archivo del export. Devuelve `None` si no tiene frontmatter, para
/// poder apuntar `--desde` a una carpeta con otras cosas adentro sin que
/// explote.
pub fn parse(texto: &str) -> Option<ParsedMemory> {
    let resto = texto.strip_prefix("---\n")?;
    let fin = resto.find("\n---\n")?;
    let (cabecera, cuerpo) = (&resto[..fin], &resto[fin + 5..]);

    let mut m = ParsedMemory { body: cuerpo.trim_start_matches('\n').trim_end().to_string(), ..Default::default() };
    m.status = "active".into();
    m.source = "explicit".into();

    let lineas: Vec<&str> = cabecera.lines().collect();
    let mut i = 0;
    while i < lineas.len() {
        let l = lineas[i];
        i += 1;
        // Las refs son una lista de pares en líneas siguientes, con sangría.
        if l.trim_start().starts_with("- kind:") {
            let kind = unquote(l.split_once("kind:").map(|x| x.1).unwrap_or(""));
            let value = lineas
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

/// Importa todo lo que haya bajo `dir`.
///
/// `fusionar` permite importar sobre una base que ya tiene memorias; sin eso
/// se exige que esté vacía. La diferencia no es de comodidad: importar sobre
/// una base con contenido duplica todo lo que ya estaba, y descubrirlo
/// después cuesta más que escribir una bandera.
///
/// Se omite lo que ya está, comparando proyecto y cuerpo: es la identidad que
/// sobrevive a un export y vuelta, porque los ids se reasignan.
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
    // Lo que ya está, para no duplicarlo.
    let mut existing: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    {
        let mut st = tx.prepare("select project, body from memories")?;
        let filas = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for f in filas {
            existing.insert(f?);
        }
    }

    // Primero todas las filas, sin `supersedes_id`: el id al que apunta puede
    // ser el de una memoria que todavía no se insertó, y resolverlo sobre la
    // marcha dejaría cadenas rotas según el orden de los archivos.
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

    // Recién ahora las cadenas, con todos los ids nuevos ya conocidos.
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

/// `Transaction::new_unchecked` con `Immediate`, igual que el guardado: si dos
/// procesos importan a la vez, el segundo espera en vez de fallar a mitad.
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
