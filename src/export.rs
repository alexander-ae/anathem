use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join("-")
}

/// Codifica `s` como un escalar YAML entre comillas dobles, válido en
/// cualquier contexto (bloque o flujo). El estilo "double-quoted" de YAML
/// soporta cualquier carácter con un juego de escapes acotado, así que no
/// hace falta decidir caso por caso si el valor "necesita" comillas — a
/// diferencia de la interpolación cruda de antes, que rompía apenas el texto
/// libre (típicamente `description`, el título de la memoria) contenía ": "
/// y YAML lo leía como una clave anidada.
fn yaml_dq(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Sanitiza un componente de ruta de un solo nivel (pensado para `project`,
/// que en producción llega sin validar desde argumentos MCP que un agente
/// completa a partir de la conversación — ver `save_memory` en `src/mcp.rs`).
/// Reemplaza separadores de ruta y recorta puntos líderes para que el
/// resultado no pueda ser `..`, `.`, ni contener `/` o `\`, y por lo tanto no
/// pueda salirse del directorio de exportación al usarse con `Path::join`.
fn safe_component(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c == '/' || c == '\\' { '-' } else { c })
        .collect();
    let trimmed = cleaned.trim_start_matches('.');
    let out = if trimmed.is_empty() { "-" } else { trimmed };
    out.to_string()
}

/// Una fila de `memories` con todo lo que el export necesita preservar.
struct Fila {
    id: i64,
    project: String,
    title: String,
    body: String,
    created: String,
    updated: String,
    status: String,
    source: String,
    pinned: i64,
    ext_id: Option<String>,
    supersedes_id: Option<i64>,
    event_start: Option<String>,
    event_end: Option<String>,
    topic: Option<String>,
}

/// Exporta cada memoria a un markdown con frontmatter. El formato está
/// documentado en `docs/formato-de-export.md` y es **completo**: lo que sale
/// alcanza para reconstruir la base en otra herramienta sin perder nada
/// salvo los vectores, que se recalculan.
///
/// Incluye las memorias `forgotten`. `forget` oculta, no borra, así que
/// dejarlas afuera —como hacía antes— convertía cada migración en una
/// pérdida de datos silenciosa. El campo `status` viaja en el frontmatter,
/// de modo que quien lea el export puede filtrarlas si quiere.
pub fn to_markdown(conn: &Connection, dir: &Path) -> Result<usize> {
    let rows: Vec<Fila> = conn
        .prepare(
            "select id, project, title, body, created, updated, status, source,
                    pinned, ext_id, supersedes_id, event_start, event_end, topic
               from memories order by id",
        )?
        .query_map([], |r| {
            Ok(Fila {
                id: r.get(0)?,
                project: r.get(1)?,
                title: r.get(2)?,
                body: r.get(3)?,
                created: r.get(4)?,
                updated: r.get(5)?,
                status: r.get(6)?,
                source: r.get(7)?,
                pinned: r.get(8)?,
                ext_id: r.get(9)?,
                supersedes_id: r.get(10)?,
                event_start: r.get(11)?,
                event_end: r.get(12)?,
                topic: r.get(13)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    // slug() trunca a las primeras 8 palabras, así que dos memorias con el mismo
    // proyecto, fecha de creación y título hasta la palabra 8 producirían el
    // mismo nombre de archivo (visto en un corpus real: dos memorias con el
    // mismo proyecto, misma fecha, y un título largo idéntico hasta la
    // palabra 8 con dos continuaciones distintas). El id es clave primaria y
    // por lo tanto único, así que sufijarlo garantiza un nombre de archivo
    // distinto por memoria sin sacrificar legibilidad.
    let mut written: HashSet<PathBuf> = HashSet::new();
    let mut n = 0;
    for f in rows {
        let id = f.id;
        let tags: Vec<String> = conn
            .prepare("select tag from memory_tags where memory_id = ?1 order by tag")?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let refs: Vec<(String, String)> = conn
            .prepare(
                "select kind, value from memory_refs where memory_id = ?1
                  order by kind, value",
            )?
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;

        let project_dir = safe_component(&f.project);
        let d = dir.join(&project_dir);
        // `safe_component` ya garantiza que `project_dir` no puede contener
        // separadores de ruta ni ser `..`/`.`, así que `d` no puede salirse de
        // `dir` — pero lo verificamos explícitamente en vez de confiar en la
        // invariante de una función vecina: si algo la cambia mañana y se
        // rompe, preferimos un error acá y no un archivo escrito fuera de
        // `dir` (hallazgo B: `--project "../../ESCAPADO"` escribía dos
        // niveles por encima del directorio de salida).
        if d.parent() != Some(dir) {
            anyhow::bail!(
                "mem export: el proyecto de la memoria {id} produjo una ruta fuera del directorio de salida: {}",
                d.display()
            );
        }
        std::fs::create_dir_all(&d)?;
        let name = format!("{}-{}-{id}.md", f.created, slug(&f.title));
        let path = d.join(&name);
        // El sufijo de id vuelve la colisión estructuralmente imposible (id es
        // primary key), pero si de todos modos ocurriera preferimos avisar y
        // no pisar un archivo ya escrito en esta misma corrida, en vez de
        // repetir en silencio el bug que esto reemplaza.
        if written.contains(&path) {
            eprintln!(
                "mem export: colisión de nombre de archivo pese al sufijo de id, se omite memoria {id}: {}",
                path.display()
            );
            continue;
        }
        let tags_yaml = tags.iter().map(|t| yaml_dq(t)).collect::<Vec<_>>().join(", ");
        // Los campos opcionales se omiten cuando no hay valor, en vez de
        // emitirse como `null`: un frontmatter sin la clave se lee igual en
        // cualquier parser y no invita a confundir "no tiene" con "tiene
        // nulo".
        let mut opcionales = String::new();
        if let Some(e) = &f.ext_id {
            opcionales.push_str(&format!("ext_id: {}\n", yaml_dq(e)));
        }
        if let Some(s) = f.supersedes_id {
            opcionales.push_str(&format!("supersedes: {s}\n"));
        }
        if let Some(t) = &f.topic {
            opcionales.push_str(&format!("topic: {}\n", yaml_dq(t)));
        }
        if let Some(e) = &f.event_start {
            opcionales.push_str(&format!("event_start: {}\n", yaml_dq(e)));
        }
        if let Some(e) = &f.event_end {
            opcionales.push_str(&format!("event_end: {}\n", yaml_dq(e)));
        }
        if !refs.is_empty() {
            opcionales.push_str("refs:\n");
            for (kind, value) in &refs {
                opcionales.push_str(&format!(
                    "  - kind: {}\n    value: {}\n",
                    yaml_dq(kind),
                    yaml_dq(value)
                ));
            }
        }
        let doc = format!(
            "---\nid: {id}\nname: {}\ndescription: {}\nmetadata:\n  type: project\nproject: {}\ntags: [{}]\ncreated: {}\nupdated: {}\nstatus: {}\nsource: {}\npinned: {}\n{}---\n\n{}\n",
            yaml_dq(&slug(&f.title)),
            yaml_dq(&f.title.replace('\n', " ")),
            yaml_dq(&f.project),
            tags_yaml,
            yaml_dq(&f.created),
            yaml_dq(&f.updated),
            yaml_dq(&f.status),
            yaml_dq(&f.source),
            if f.pinned != 0 { "true" } else { "false" },
            opcionales,
            f.body
        );
        std::fs::write(&path, doc)?;
        written.insert(path);
        n += 1;
    }
    Ok(n)
}
