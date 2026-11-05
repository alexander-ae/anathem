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

/// Encodes `s` as a double-quoted YAML scalar, valid in any context (block
/// or flow). YAML's "double-quoted" style supports any character with a
/// bounded set of escapes, so there is no need to decide case by case
/// whether a value "needs" quoting — unlike the raw interpolation it
/// replaces, which broke as soon as the free text (typically `description`,
/// the memory's title) contained ": " and YAML read it as a nested key.
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

/// Sanitizes a single-level path component (meant for `project`, which in
/// production arrives unvalidated from MCP arguments an agent fills in from
/// the conversation — see `save_memory` in `src/mcp.rs`). It replaces path
/// separators and trims leading dots so the result cannot be `..`, `.`, nor
/// contain `/` or `\`, and therefore cannot escape the export directory
/// when used with `Path::join`.
fn safe_component(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c == '/' || c == '\\' { '-' } else { c })
        .collect();
    let trimmed = cleaned.trim_start_matches('.');
    let out = if trimmed.is_empty() { "-" } else { trimmed };
    out.to_string()
}

/// One row of `memories` with everything the export has to preserve.
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
    received_from: Option<String>,
    received_at: Option<String>,
}

/// Exports each memory to a markdown file with frontmatter. The format is
/// documented in `docs/export-format.md` and is **complete**: what comes
/// out is enough to rebuild the database in another tool without losing
/// anything except the vectors, which are recomputed.
///
/// It includes `forgotten` memories. `forget` hides, it does not delete, so
/// leaving them out — as it used to — turned every migration into silent
/// data loss. The `status` field travels in the frontmatter, so whoever
/// reads the export can filter them if they want.
pub fn to_markdown(conn: &Connection, dir: &Path) -> Result<usize> {
    let rows: Vec<Fila> = conn
        .prepare(
            "select id, project, title, body, created, updated, status, source,
                    pinned, ext_id, supersedes_id, event_start, event_end, topic,
                    received_from, received_at
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
                received_from: r.get(14)?,
                received_at: r.get(15)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    // slug() truncates to the first 8 words, so two memories with the same
    // project, creation date and title up to word 8 would produce the same
    // filename (seen in a real corpus: two memories with the same project,
    // the same date, and a long title identical up to word 8 with two
    // different continuations). The id is the primary key and therefore
    // unique, so suffixing it guarantees a distinct filename per memory
    // without sacrificing readability.
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
        // `safe_component` already guarantees `project_dir` cannot contain
        // path separators nor be `..`/`.`, so `d` cannot escape `dir` — but
        // we check it explicitly instead of relying on a neighbouring
        // function's invariant: if something changes it tomorrow and it
        // breaks, we would rather have an error here than a file written
        // outside `dir` (finding B: `--project "../../ESCAPADO"` wrote two
        // levels above the output directory).
        if d.parent() != Some(dir) {
            anyhow::bail!(
                "mem export: the project of memory {id} produced a path outside the output directory: {}",
                d.display()
            );
        }
        std::fs::create_dir_all(&d)?;
        // `created` gets the same treatment as `project`, and for the same
        // reason: it is not always generated here. `import::parse` reads it
        // verbatim out of a `.md` frontmatter and `migrate::run` out of the
        // export's JSONL, so a hostile bundle can put `../..` in it and the
        // next `mem export` would write outside `dir` — the directory guard
        // above never looks at the file name (finding M-1). `slug()`
        // already keeps the title to alphanumerics.
        let name = format!("{}-{}-{id}.md", safe_component(&f.created), slug(&f.title));
        let path = d.join(&name);
        // Same defence in depth as the directory check: state the invariant
        // rather than trusting the neighbouring function to keep holding
        // it. The whole route, and not just its parent, has to stay inside
        // the output directory.
        if path.parent() != Some(d.as_path()) {
            anyhow::bail!(
                "mem export: memory {id} produced a file name outside the output directory: {}",
                path.display()
            );
        }
        // The id suffix makes a collision structurally impossible (id is
        // the primary key), but should one happen anyway we would rather
        // warn and not overwrite a file already written in this same run,
        // instead of silently repeating the bug this replaces.
        if written.contains(&path) {
            eprintln!(
                "mem export: file name collision despite the id suffix, skipping memory {id}: {}",
                path.display()
            );
            continue;
        }
        let tags_yaml = tags.iter().map(|t| yaml_dq(t)).collect::<Vec<_>>().join(", ");
        // Optional fields are omitted when there is no value, rather than
        // emitted as `null`: frontmatter without the key reads the same in
        // any parser and does not invite confusing "has none" with "has
        // null".
        let mut optional = String::new();
        if let Some(e) = &f.ext_id {
            optional.push_str(&format!("ext_id: {}\n", yaml_dq(e)));
        }
        if let Some(s) = f.supersedes_id {
            optional.push_str(&format!("supersedes: {s}\n"));
        }
        if let Some(t) = &f.topic {
            optional.push_str(&format!("topic: {}\n", yaml_dq(t)));
        }
        // Provenance travels, or an export of a received memory comes back
        // looking like one this machine wrote.
        if let Some(r) = &f.received_from {
            optional.push_str(&format!("received_from: {}\n", yaml_dq(r)));
        }
        if let Some(r) = &f.received_at {
            optional.push_str(&format!("received_at: {}\n", yaml_dq(r)));
        }
        if let Some(e) = &f.event_start {
            optional.push_str(&format!("event_start: {}\n", yaml_dq(e)));
        }
        if let Some(e) = &f.event_end {
            optional.push_str(&format!("event_end: {}\n", yaml_dq(e)));
        }
        if !refs.is_empty() {
            optional.push_str("refs:\n");
            for (kind, value) in &refs {
                optional.push_str(&format!(
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
            optional,
            f.body
        );
        std::fs::write(&path, doc)?;
        written.insert(path);
        n += 1;
    }
    Ok(n)
}
