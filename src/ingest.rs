use crate::{chunk, model::NewMemory};
use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

pub const CHUNK_SIZE: usize = 500;
pub const CHUNK_STEP: usize = 400;

/// Must match the check constraint on `memory_refs.kind` in db::SCHEMA.
const VALID_REF_KINDS: [&str; 3] = ["ticket", "pr", "sha"];

pub fn insert(conn: &Connection, m: &NewMemory) -> Result<i64> {
    let tx = conn.unchecked_transaction()?;
    let id = insert_row(&tx, m, None)?;
    tx.commit()?;
    Ok(id)
}

/// A timestamp with the time of day for `memories.created_ts`.
///
/// `created` stores only the date and stays that way: the export, each
/// memory's filename and the migration from other systems all use it. This
/// is a separate column, so "what happened a short while ago" can be
/// answered without touching any of that.
fn now_with_time() -> String {
    chrono::Local::now().to_rfc3339()
}

/// The insert itself, opening and closing no transaction: the caller sets
/// the boundary. `insert` wraps it in its own; saving with a topic key runs
/// it inside the one that also covers the supersede, so both happen together
/// or neither does.
fn insert_row(tx: &Connection, m: &NewMemory, agent: Option<&str>) -> Result<i64> {
    tx.execute(
        "insert into memories
           (project, title, body, created, updated, event_start, event_end,
            source, ext_id, agent, created_ts)
         values (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![m.project, m.title, m.body, m.created,
                m.event_start, m.event_end, m.source, m.ext_id,
                agent, now_with_time()],
    )?;
    let id = tx.last_insert_rowid();

    for (ord, body) in chunk::split(&m.body, CHUNK_SIZE, CHUNK_STEP).iter().enumerate() {
        tx.execute(
            "insert into chunks (memory_id, ord, body) values (?1, ?2, ?3)",
            params![id, ord as i64, body],
        )?;
    }
    for t in &m.tags {
        tx.execute(
            "insert or ignore into memory_tags (memory_id, tag) values (?1, ?2)",
            params![id, t],
        )?;
    }
    for (kind, value) in &m.refs {
        if !VALID_REF_KINDS.contains(&kind.as_str()) {
            bail!(
                "invalid ref kind {kind:?} (value={value:?}); it has to be one of {VALID_REF_KINDS:?}"
            );
        }
        tx.execute(
            "insert or ignore into memory_refs (memory_id, kind, value) values (?1, ?2, ?3)",
            params![id, kind, value],
        )?;
    }
    Ok(id)
}

/// Explicit save of a new memory, shared by the CLI (`mem save`) and by
/// MCP's `save_memory` tool. It exists so the two paths cannot drift apart:
/// this used to be duplicated and the CLI branch neither classified nor
/// extracted references, so a memory saved from the terminal was born
/// without tags or tickets.
///
/// When `given_tags` arrives empty the text is classified; when it carries
/// something, that is respected as is and nothing is classified on top.
/// Returns the id and the tags applied.
/// A memory's title is its first non-empty line, trimmed to 120 characters.
/// It lives here, and not duplicated in each path, because the title derives
/// from the body: if one path computed it differently — or did not recompute
/// it when the body changed — the memory would end up showing a title that
/// no longer describes what it holds.
pub fn title_from(text: &str) -> String {
    text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .chars()
        .take(120)
        .collect()
}

pub fn save_explicit(
    conn: &Connection,
    text: &str,
    project: &str,
    given_tags: Vec<String>,
) -> Result<(i64, Vec<String>)> {
    let g = save_with_topic(conn, text, project, given_tags, None, None)?;
    Ok((g.id, g.tags))
}

/// The memory that a save with a topic key left `superseded`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replaced {
    pub id: i64,
    pub title: String,
}

/// The result of `save_with_topic`.
#[derive(Debug, Clone)]
pub struct Saved {
    pub id: i64,
    pub project: String,
    pub tags: Vec<String>,
    /// The key exactly as it was stored, already normalized.
    pub topic: Option<String>,
    /// The memory left superseded, whether by the topic key or by an
    /// explicit `supersedes`.
    pub replaced: Option<Replaced>,
    /// The key had no active memory before this save.
    pub key_is_new: bool,
    /// Only when the key turned out to be new: the other keys that do have
    /// an active memory in the project, so a typo shows.
    pub other_keys: Vec<String>,
}

/// Validates and normalizes a topic key: lowercase, trimmed at the edges
/// and with inner whitespace collapsed to one space (a newline counts as a
/// space). An empty key is rejected.
///
/// It is normalized because a model writes the key, and `Pagos`, `pagos` and
/// `pagos ` have to be the same topic: otherwise a typing variation saves as
/// a new topic and replaces nothing, with a message identical to that of a
/// legitimately new key. What normalizing does not fix (`stack-frontend`
/// against `stack-del-frontend`) is made visible by `Saved::other_keys`.
pub fn normalize_topic(key: &str) -> Result<String> {
    let t = key.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    if t.is_empty() {
        bail!("the topic key cannot be empty");
    }
    Ok(t)
}

/// Cap on how many keys are listed when reporting that a key is new.
const MAX_KEYS_LISTED: usize = 30;

/// An error for a request that cannot be fulfilled (a `supersedes` that does
/// not exist, is not active, or clashes with the key). Unlike a database
/// failure, it is the input's fault and can be told to the caller as is: MCP
/// returns it as the tool's text and not as an "internal error".
#[derive(Debug)]
pub struct BadRequest(pub String);

impl std::fmt::Display for BadRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BadRequest {}

/// `save_explicit` with an optional topic key and/or `supersedes`.
///
/// With neither of the two it is exactly the ordinary save.
///
/// **`topic`**: if the project already has an **active** memory under that
/// key, the new one supersedes it: it is inserted separately and the old one
/// is left `superseded`, linked by `supersedes_id`, instead of being edited
/// in place. The project keeps its history (`mem history` walks the chain,
/// the export includes what is hidden), and editing here would destroy the
/// previous version. The key is scoped per project: the same one in two
/// projects is two topics.
///
/// **`supersedes`**: the id of a specific memory this one corrects, even if
/// that one was saved without a key. It has to exist and be active. It is
/// the route for what was already saved; `topic` is for what will keep
/// evolving. Both can be given together when they point at the same memory
/// (or when the key is new): a new memory replaces at most one, so if the
/// key already has a current one other than `supersedes` it is rejected
/// instead of choosing silently.
///
/// The marking is always done by `supersede::link` and not by an `update` of
/// its own, so there is a single path deciding what a valid replacement is
/// (and rejecting cycles).
///
/// It all runs in an `immediate` transaction: finding the current one,
/// inserting and linking are one unit. With a deferred transaction, two
/// simultaneous saves under the same key would see the same current one and
/// leave two active memories for one topic; `immediate` takes the write lock
/// before reading and the second waits for the first.
pub fn save_with_topic(
    conn: &Connection,
    text: &str,
    project: &str,
    given_tags: Vec<String>,
    topic: Option<&str>,
    supersedes: Option<i64>,
) -> Result<Saved> {
    save_with_topic_by(conn, text, project, given_tags, topic, supersedes, None)
}

/// Same as `save_with_topic`, recording which agent saved it.
///
/// It exists as a separate function and not as one more parameter on the
/// other so its twenty-odd call sites, almost all tests, are left untouched:
/// only the MCP server knows the agent, since it is the only one reading
/// `clientInfo` from `initialize`. Anything saved from the CLI has no agent,
/// and that is as it should be.
#[allow(clippy::too_many_arguments)]
pub fn save_with_topic_by(
    conn: &Connection,
    text: &str,
    project: &str,
    given_tags: Vec<String>,
    topic: Option<&str>,
    supersedes: Option<i64>,
    agent: Option<&str>,
) -> Result<Saved> {
    let topic = topic.map(normalize_topic).transpose()?;
    let tags = if given_tags.is_empty() {
        crate::tags::classify(text).map_err(|e| anyhow::anyhow!(e))?
    } else {
        given_tags
    };
    let m = NewMemory {
        project: project.to_string(),
        title: title_from(text),
        body: text.to_string(),
        created: chrono::Local::now().format("%Y-%m-%d").to_string(),
        event_start: None,
        event_end: None,
        source: "explicit".into(),
        ext_id: None,
        tags: tags.clone(),
        refs: crate::tags::extract_refs(text),
    };

    if topic.is_none() && supersedes.is_none() {
        let tx = conn.unchecked_transaction()?;
        let id = insert_row(&tx, &m, agent)?;
        tx.commit()?;
        return Ok(Saved {
            id, project: project.to_string(), tags, topic: None,
            replaced: None, key_is_new: false, other_keys: Vec::new(),
        });
    }

    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let current_one: Option<(i64, String)> = match &topic {
        Some(t) => tx
            .query_row(
                "select id, title from memories
                  where project = ?1 and topic = ?2 and status = 'active'
                  order by id desc limit 1",
                params![project, t],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?,
        None => None,
    };
    let explicit_one: Option<(i64, String)> = match supersedes {
        Some(sid) => {
            let row: Option<(String, String)> = tx
                .query_row("select status, title from memories where id = ?1", [sid], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?;
            match row {
                None => return Err(BadRequest(format!("there is no memory with id:{sid}")).into()),
                Some((st, _)) if st != "active" => {
                    return Err(BadRequest(format!(
                            "memory id:{sid} is not active (status={st}); there is nothing to replace"
                        ))
                    .into())
                }
                Some((_, title)) => Some((sid, title)),
            }
        }
        None => None,
    };
    if let (Some((v, _)), Some((e, _))) = (&current_one, &explicit_one) {
        if v != e {
            let key = topic.as_deref().unwrap_or_default();
            return Err(BadRequest(format!(
                    "key «{key}» already has the active memory id:{v} and supersedes points at \
                     id:{e}: a new memory replaces exactly one. Leave only `topic` to update the \
                     current one, or only `supersedes` to correct that other one."
                ))
            .into());
        }
    }

    let key_is_new = topic.is_some() && current_one.is_none();
    let goal = explicit_one.or(current_one);
    let id = insert_row(&tx, &m, agent)?;
    if let Some((older, _)) = &goal {
        crate::supersede::link(&tx, id, *older)?;
    }
    if let Some(t) = &topic {
        tx.execute("update memories set topic = ?2 where id = ?1", params![id, t])?;
    }
    let other_keys = match (&topic, &goal) {
        (Some(t), None) => active_topic_keys(&tx, project, t)?,
        _ => Vec::new(),
    };
    tx.commit()?;
    Ok(Saved {
        id,
        project: project.to_string(),
        tags,
        topic,
        replaced: goal.map(|(id, title)| Replaced { id, title }),
        key_is_new,
        other_keys,
    })
}

/// The topic keys with an active memory in `project`, minus `except`,
/// sorted. Trimmed to `MAX_CLAVES_LISTADAS`.
fn active_topic_keys(conn: &Connection, project: &str, excepto: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "select distinct topic from memories
          where project = ?1 and topic is not null and topic <> ?2 and status = 'active'
          order by topic limit ?3",
    )?;
    let keys = stmt
        .query_map(params![project, excepto, MAX_KEYS_LISTED as i64 + 1], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(keys)
}

impl Saved {
    /// What happened with the replacement and the topic key, in Spanish, or
    /// `None` when neither occurred. The CLI and MCP both use it so they say
    /// the same thing.
    ///
    /// When it replaced something, it names the memory by id: a silent
    /// replacement is the kind of magic nobody can account for later. When
    /// the key was new and nothing was replaced, it lists the keys already in
    /// the project: a typo produces the same "there was nothing" as a
    /// legitimately new key, and without the list nobody can tell them apart.
    /// It costs text only in that case.
    pub fn describe(&self) -> Option<String> {
        let reemplazo = self.replaced.as_ref().map(|r| format!(
                "replaces memory {} («{}»), which is left superseded. `mem history {}` \
                 shows the chain.",
                r.id, r.title, self.id
            ));
        let Some(key) = self.topic.as_deref() else {
            return reemplazo.map(|r| capitalize(&r));
        };
        let topic_word = "Topic";
        match (reemplazo, self.key_is_new) {
            (Some(r), false) => Some(format!("{topic_word} «{key}»: {r}")),
            (Some(r), true) => Some(format!(
                "{} {topic_word} «{key}»: {}",
                capitalize(&r),
                "new key; the next memory under it will replace this one.",
            )),
            (None, _) => {
                let mut msg = format!(
                    "{topic_word} «{key}»: {}",
                    "there was no active memory under that key, so it was saved as a new one.",
                );
                if self.other_keys.is_empty() {
                    msg.push_str(&format!(" There are no other topic keys in {}.", self.project));
                } else {
                    let has_more = self.other_keys.len() > MAX_KEYS_LISTED;
                    let lista: Vec<&str> = self.other_keys.iter()
                        .take(MAX_KEYS_LISTED).map(String::as_str).collect();
                    let puntos = if has_more { ", …" } else { "" };
                    msg.push_str(&format!(
                            " Keys that already exist in {}: {}{}. If you meant to update one \
                             of them, the key did not match: save again with the exact one.",
                            self.project, lista.join(", "), puntos
                        ));
                }
                Some(msg)
            }
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(p) => p.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
