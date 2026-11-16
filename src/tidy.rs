//! Memories that carry no project, and where they probably belong.
//!
//! A project is derived from the directory the agent was in, and there is
//! none when that directory is not a repository. The result is a small pile
//! of active memories under `no-project` that do belong somewhere: on the
//! installation this was written for it was 7 of 67, and five of the seven
//! named their project in the title.
//!
//! So this **reports and never moves anything**, for the reason
//! [`crate::dupes`] reports and never merges: the evidence below is a word
//! that appears in the memory, and a word is not a decision. `mem project`
//! is the command that acts, and it is a person's or an agent's to run.
//!
//! What it is not: a semantic classifier. A note about GA4 that never
//! mentions the site it measures has no signal here, and saying so is more
//! useful than guessing. The agent already in the session can read three
//! of those in a few hundred tokens; a model bundled into this binary would
//! be a second one to pay for, in a project whose whole promise is not
//! having one.

use anyhow::Result;
use rusqlite::Connection;

use crate::project::NO_PROJECT;

/// One memory under `no-project`, with the best guess at where it belongs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: i64,
    pub title: String,
    /// What the body starts with, so a person can decide without opening it.
    pub preview: String,
    /// An existing project whose name appears in the memory, when one does.
    pub suggestion: Option<String>,
    /// The word that suggested it. Without this the suggestion is a guess
    /// that cannot be argued with.
    pub evidence: Option<String>,
}

/// Every active memory filed under `no-project`, newest first.
pub fn candidates(conn: &Connection) -> Result<Vec<Candidate>> {
    let projects: Vec<String> = conn
        .prepare(
            "select distinct project from memories
              where status = 'active' and project <> ?1 order by project",
        )?
        .query_map([NO_PROJECT], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut st = conn.prepare(
        "select id, title, body from memories
          where status = 'active' and project = ?1
          order by id desc",
    )?;
    let rows = st.query_map([NO_PROJECT], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (id, title, body) = row?;
        let (suggestion, evidence) = best_match(&projects, &title, &body);
        out.push(Candidate {
            id,
            preview: body
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(120)
                .collect(),
            title,
            suggestion,
            evidence,
        });
    }
    Ok(out)
}

/// The project whose name the memory mentions, and where it was mentioned.
///
/// The title counts more than the body, and not merely as a tie-break: a note
/// titled «DROPLET DE DIGITALOCEAN» whose body happens to name a website is
/// about the droplet. So a mention in the title decides outright, and the
/// body is only read when the title names no project.
///
/// And when the body names **more than one** project there is no answer: a
/// note that mentions three sites is one a person reads. Reporting the first
/// one alphabetically would be a guess wearing the clothes of a result — the
/// two failure directions are not symmetric, and a wrong suggestion is worse
/// than none, because it is one click away from being applied.
///
/// It compares the **stem** of each project name — everything before the dot
/// — because that is what people write: `colegios.pe` is mentioned as
/// `colegios`. Case-insensitive and word-bounded: `anathem` inside
/// `anathemas` is not a mention of the project.
pub fn best_match(
    projects: &[String],
    title: &str,
    body: &str,
) -> (Option<String>, Option<String>) {
    let title = title.to_lowercase();
    let body = body.to_lowercase();

    let in_title: Vec<(&String, String)> = projects
        .iter()
        .filter_map(|p| {
            let stem = stem_of(p);
            mentions(&title, &stem).then(|| (p, stem))
        })
        .collect();
    if in_title.len() == 1 {
        let (project, stem) = &in_title[0];
        return (Some((*project).clone()), Some(format!("«{stem}» in the title")));
    }
    if in_title.len() > 1 {
        return (None, Some(format!("{} projects in the title", in_title.len())));
    }

    let in_body: Vec<(&String, String)> = projects
        .iter()
        .filter_map(|p| {
            let stem = stem_of(p);
            mentions(&body, &stem).then(|| (p, stem))
        })
        .collect();
    match in_body.len() {
        1 => {
            let (project, stem) = &in_body[0];
            (Some((*project).clone()), Some(format!("«{stem}» in the body")))
        }
        0 => (None, None),
        n => (None, Some(format!("{n} projects in the body"))),
    }
}

/// `colegios.pe` -> `colegios`. Everything before the first dot, lowercased.
fn stem_of(project: &str) -> String {
    project.split('.').next().unwrap_or(project).to_lowercase()
}

/// Whether `needle` appears as a word, not as a substring of a longer one.
fn mentions(haystack: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(at) = haystack[from..].find(needle) {
        let start = from + at;
        let end = start + needle.len();
        let before_ok = haystack[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after_ok = haystack[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        from = end;
        if from >= haystack.len() {
            break;
        }
    }
    false
}

/// Moves one memory to a project, refusing when the move would break the one
/// invariant the schema has about topic keys: one active memory per
/// (project, topic).
///
/// The body, the chunks, the index and the vectors are untouched: a project
/// is a label the search filters on, not part of what a memory says.
pub fn move_to(conn: &Connection, id: i64, project: &str) -> Result<()> {
    let project = crate::project::canonical(project);
    let (current, topic): (String, Option<String>) = conn
        .query_row(
            "select project, topic from memories where id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| anyhow::anyhow!("there is no memory with id:{id}"))?;

    if let Some(topic) = topic {
        let taken: Option<i64> = conn
            .query_row(
                "select id from memories
                  where project = ?1 and topic = ?2 and status = 'active' and id <> ?3",
                rusqlite::params![project, topic, id],
                |r| r.get(0),
            )
            .ok();
        if let Some(other) = taken {
            anyhow::bail!(
                "id:{id} carries topic «{topic}» and {project} already has an active memory \
                 with that key (id:{other}). Moving it would leave two active versions of the \
                 same subject. Supersede one of them first, or choose another project."
            );
        }
    }

    if current == project {
        return Ok(());
    }
    conn.execute(
        "update memories set project = ?2 where id = ?1",
        rusqlite::params![id, project],
    )?;
    Ok(())
}
