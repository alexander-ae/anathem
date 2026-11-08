//! The memories nobody has opened, and where each one was written down.
//!
//! Two mechanical halves of a consolidation pass that happens somewhere
//! else. Deciding whether a note is *wrong* takes a model: it means reading
//! what the note claims, finding what actually happened, and judging whether
//! they agree. A binary that called a model for that would need an API key
//! of its own, which is the thing this project promises not to ask for.
//!
//! So the judgement stays in the agent session the user already has open,
//! spending the model they are already paying for, and what lives here is
//! only what a binary can do honestly: pick the candidates, and say where
//! the evidence is. Neither function calls anything.

use anyhow::Result;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// The longest a fragment taken from a body may be before it is used as a
/// needle. Long enough to be unique in a gigabyte of logs, short enough to
/// survive the line having been re-wrapped on its way into the transcript.
const NEEDLE: usize = 70;

/// A line shorter than this is a heading, a bullet marker or a date, and
/// matches half the corpus. The needle has to come from prose.
const MIN_LINE: usize = 60;

/// Where transcripts are looked for when nobody says otherwise.
///
/// Claude Code writes one JSONL file per session under here. opencode keeps
/// about a hundred kilobytes of session metadata instead of transcripts, so
/// there is nothing of ours to find in it and it is not a default; anyone
/// whose agent writes logs elsewhere passes the directory in.
pub fn default_roots() -> Vec<PathBuf> {
    match std::env::var_os("HOME") {
        Some(h) => vec![PathBuf::from(h).join(".claude").join("projects")],
        None => Vec::new(),
    }
}

/// One memory nobody has opened, or opened long ago.
#[derive(Debug, Clone)]
pub struct Stale {
    pub id: i64,
    pub project: String,
    pub title: String,
    /// When its full body was last fetched, or `None` for never.
    pub last_accessed: Option<String>,
    pub created: String,
}

/// Active memories ordered by how long ago anyone last read them in full,
/// never-read first.
///
/// **What `last_accessed` does and does not say.** It is written by
/// `get_memory`, so it records that somebody asked for the whole body. It
/// does **not** record that an agent saw the memory in a list of search
/// results and took what it needed from the excerpt — which is the common
/// case, since the two-stage protocol exists precisely so that most answers
/// never need the body. So "never accessed" overstates neglect, and this
/// list is a place to start looking rather than a list of neglected notes.
/// Whatever prints it has to say so.
pub fn never_read(conn: &Connection, project: Option<&str>, limit: usize) -> Result<Vec<Stale>> {
    let mut sql = String::from(
        "select id, project, title, last_accessed, created
           from memories
          where status = 'active'",
    );
    let mut params: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(p) = project {
        params.push(rusqlite::types::Value::Text(crate::project::canonical(p).to_string()));
        sql.push_str(" and project = ?1");
    }
    // Nulls first: a memory nobody ever opened is the one worth looking at
    // before one that was opened a year ago.
    sql.push_str(" order by last_accessed is not null, last_accessed asc, id asc");
    params.push(rusqlite::types::Value::Integer(limit as i64));
    sql.push_str(&format!(" limit ?{}", params.len()));

    let mut st = conn.prepare(&sql)?;
    let rows = st.query_map(rusqlite::params_from_iter(params.iter()), |r| {
        Ok(Stale {
            id: r.get(0)?,
            project: r.get(1)?,
            title: r.get(2)?,
            last_accessed: r.get(3)?,
            created: r.get(4)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Where a memory was written down: the transcript file and the line.
#[derive(Debug, Clone, PartialEq)]
pub struct Origin {
    pub file: PathBuf,
    /// 1-based, so it can be pasted after a colon and opened.
    pub line: usize,
}

/// A fragment of `body` distinctive enough to find it in a log.
///
/// The first line of real prose. Not the title: a title is the first line
/// truncated, and truncation is exactly what makes a needle stop matching.
pub fn needle(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|l| l.chars().count() >= MIN_LINE)
        .map(|l| l.chars().take(NEEDLE).collect())
}

/// Finds the transcripts that contain `needle`, with the line each one
/// matched on.
///
/// Saving a memory is a tool call, and a tool call is recorded with its
/// arguments, so a memory saved by an agent is in the log verbatim — whether
/// it went through the MCP server or through `mem save` in a shell the agent
/// was driving. One written by a person in their own terminal is in no log
/// at all, and that is not an error: it is the honest answer.
///
/// Reading the files line by line rather than whole keeps the memory this
/// uses flat: the largest transcript measured was 28.5 MB and there are
/// gigabytes of them.
pub fn find_origin(roots: &[PathBuf], needle: &str) -> Result<Vec<Origin>> {
    let mut out = Vec::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        buscar_en(root, needle, &mut out)?;
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(out)
}

fn buscar_en(dir: &Path, needle: &str, out: &mut Vec<Origin>) -> Result<()> {
    let entradas = match std::fs::read_dir(dir) {
        Ok(e) => e,
        // A directory that cannot be read is skipped rather than fatal:
        // this walks somebody's home, where a stray permission is normal.
        Err(_) => return Ok(()),
    };
    for e in entradas.flatten() {
        let p = e.path();
        if p.is_dir() {
            buscar_en(&p, needle, out)?;
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            if let Some(n) = linea_con(&p, needle) {
                out.push(Origin { file: p, line: n });
            }
        }
    }
    Ok(())
}

fn linea_con(file: &Path, needle: &str) -> Option<usize> {
    use std::io::BufRead;
    let f = std::fs::File::open(file).ok()?;
    let r = std::io::BufReader::new(f);
    for (i, linea) in r.lines().enumerate() {
        // A transcript holds whatever was typed or pasted, so a line that is
        // not valid UTF-8 is possible and is not worth failing over.
        let Ok(l) = linea else { continue };
        if l.contains(needle) {
            return Some(i + 1);
        }
    }
    None
}
