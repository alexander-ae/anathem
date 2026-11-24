//! What `--json` prints: the same answers the prose carries, in a shape a
//! program can read.
//!
//! It exists because of the division of work [`docs/integrations.md`] states
//! and does not break: anathem loads no foreign code, so an integration is a
//! separate program that calls this binary. A separate program cannot parse
//! prose that is written to be read, and the alternative — scraping lines
//! like `{:2}. [project] title` — breaks the first time a title is reworded
//! or a message is improved. This is the stable side of that line.
//!
//! Every document is one line on stdout, with `schema` among its fields.
//! One line because the callers here are `jq`, a shell loop and a pipeline,
//! and a document that needs a streaming parser to be read is a document
//! nobody pipes. `schema` because adding a field is not a breaking change but
//! changing what one means is, and a caller deserves a number to test before
//! trusting the rest.
//!
//! What does **not** change is the prose: with no `--json`, every command
//! prints exactly what it always printed. This module only adds a branch.
//!
//! Nothing here decides anything: each function receives what the command
//! already read and arranges it. That is deliberate — a JSON view that ran
//! its own queries would drift from the prose next to it, and the two are
//! supposed to be the same answer in two shapes.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;

use crate::maint::{self, TopicLookup};
use crate::recent::RecentItem;
use crate::search::{Explained, Query};
use crate::stats::Stats;
use crate::stale::Stale;

/// The version of every document below.
///
/// 1 means: the fields documented in `docs/integrations.md` are these and
/// mean this. A field may be added without a bump; a field may not change
/// meaning, type or presence without one.
pub const SCHEMA: u8 = 1;

/// Writes one document and a newline.
pub fn print(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string(v)?);
    Ok(())
}

/// The tags of one memory, alphabetically.
fn tags(conn: &Connection, id: i64) -> Result<Vec<String>> {
    let mut st = conn.prepare(
        "select tag from memory_tags where memory_id = ?1 order by tag",
    )?;
    let out = st.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(out)
}

/// `mem search --json`.
///
/// `total_active` and `scope_empty` are not decoration: they are what lets a
/// caller tell "your query matched nothing" from "your scope is empty" from
/// "the database is empty", which the prose says in three different
/// sentences and a program cannot recover from an empty array alone.
pub fn search(
    conn: &Connection,
    q: &Query,
    hits: &[Explained],
    total_active: i64,
    scope_empty: bool,
) -> Result<Value> {
    let mut results = Vec::with_capacity(hits.len());
    for e in hits {
        let id = e.hit.memory_id;
        // `optional` and a skip, not `?`: another process may delete a memory
        // between the ranking and this read, and losing the whole answer over
        // one result that no longer exists is worse than a result fewer. The
        // prose path has the same shape, for the same reason.
        let Some((topic, status, verified, received_from)): Option<(Option<String>, String, i64, Option<String>)> =
            conn.query_row(
                "select topic, status, verified, received_from from memories where id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
        else {
            continue;
        };
        results.push(json!({
            "id": id,
            "project": e.hit.project,
            "title": e.hit.title,
            "snippet": e.hit.snippet,
            "score": e.hit.score,
            "bm25": e.bm25,
            "cosine": e.cosine,
            "words_matched": e.words_matched,
            "words_total": e.words_total,
            "topic": topic,
            "tags": tags(conn, id)?,
            "status": status,
            "verified": verified != 0,
            "received_from": received_from,
        }));
    }
    Ok(json!({
        "schema": SCHEMA,
        "query": q.text,
        "project": q.project,
        "total_active": total_active,
        "scope_empty": scope_empty,
        "results": results,
    }))
}

/// `mem show <id> --json`, including the fields the prose omits because a
/// person reads them elsewhere: tags, `verified`, `pinned` and what it
/// replaced.
pub fn show(conn: &Connection, m: &maint::Memory) -> Result<Value> {
    let (verified, supersedes, ext_id, created, pinned): (i64, Option<i64>, Option<String>, String, i64) =
        conn.query_row(
            "select verified, supersedes_id, ext_id, created, pinned from memories where id = ?1",
            [m.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
    let anchor = crate::anchor::get(conn, m.id)?.map(|(path, symbol, sha, state, checked)| {
        json!({
            "path": path,
            "symbol": symbol,
            "sha256": sha,
            "state": state,
            "checked_at": checked,
        })
    });
    Ok(json!({
        "schema": SCHEMA,
        "id": m.id,
        "project": m.project,
        "title": m.title,
        "body": m.body,
        "status": m.status,
        "when": m.when,
        "created": created,
        "agent": m.agent,
        "topic": m.topic,
        "received_from": m.received_from,
        "verified": verified != 0,
        "anchor": anchor,
        "pinned": pinned != 0,
        "supersedes": supersedes,
        "ext_id": ext_id,
        "tags": tags(conn, m.id)?,
    }))
}

/// `mem show --topic <key> --json` when the key exists.
pub fn topic_found(state: &maint::TopicState) -> Value {
    let current = state.current.as_ref().map(|c| {
        json!({
            "id": c.id,
            "body": c.body,
            "when": c.when,
            "agent": c.agent,
        })
    });
    let chain: Vec<Value> = state
        .chain
        .iter()
        .map(|v| json!({
            "id": v.id,
            "when": v.when,
            "status": v.status,
            "title": v.title,
        }))
        .collect();
    json!({
        "schema": SCHEMA,
        "project": state.project,
        "topic": state.topic,
        "found": true,
        "current": current,
        "history": chain,
    })
}

/// The same command when it does not: `found` is false and `keys` is what
/// the project does have, which is the useful half of the answer.
pub fn topic_unknown(project: &str, topic: &str, keys: &[String]) -> Value {
    json!({
        "schema": SCHEMA,
        "project": project,
        "topic": topic,
        "found": false,
        "keys": keys,
    })
}

/// Dispatches on what `maint::topic_state` returned, so `main` does not have
/// to match twice.
pub fn topic(project: &str, topic: &str, lookup: &TopicLookup) -> Value {
    match lookup {
        TopicLookup::Found(state) => topic_found(state),
        TopicLookup::Unknown(keys) => topic_unknown(project, topic, keys),
    }
}

/// `mem recent --json`.
pub fn recent(items: &[RecentItem]) -> Value {
    let rows: Vec<Value> = items
        .iter()
        .map(|it| json!({
            "id": it.id,
            "project": it.project,
            "title": it.title,
            "when": it.when,
            "agent": it.agent,
            "topic": it.topic,
            "received_from": it.received_from,
            "status": it.status,
            "anchor": it.anchor,
            "excerpt": it.excerpt,
        }))
        .collect();
    json!({ "schema": SCHEMA, "items": rows })
}

/// `mem topics --json`.
pub fn topics(rows: &[maint::Tema]) -> Value {
    let topics: Vec<Value> = rows
        .iter()
        .map(|t| json!({
            "project": t.project,
            "topic": t.topic,
            "memories": t.total,
            "current": t.current_one,
        }))
        .collect();
    json!({ "schema": SCHEMA, "topics": topics })
}

/// `mem log --json`.
///
/// `opened` is an id or null, and `opened_unrecorded` is the older rows that
/// say one was opened without saying which. Folding both into "null" would
/// report a past that did not happen.
pub fn log(entries: &[crate::logsearch::LogEntry]) -> Value {
    use crate::logsearch::Followed;
    let rows: Vec<Value> = entries
        .iter()
        .map(|e| {
            let (opened, unrecorded) = match e.followed {
                None => (None, false),
                Some(Followed::Memory(i)) => (Some(i), false),
                Some(Followed::Unrecorded) => (None, true),
            };
            json!({
                "ts": e.ts,
                "project": e.project,
                "query": e.query,
                "returned": e.returned.iter().filter_map(|s| s.parse::<i64>().ok()).collect::<Vec<i64>>(),
                "tokens": e.budget_used,
                "opened": opened,
                "opened_unrecorded": unrecorded,
            })
        })
        .collect();
    json!({ "schema": SCHEMA, "entries": rows })
}

/// `mem why <id> --json`.
pub fn why(id: i64, appearances: &[crate::logsearch::Appearance]) -> Value {
    let rows: Vec<Value> = appearances
        .iter()
        .map(|a| json!({
            "ts": a.ts,
            "query": a.query,
            "project": a.project,
            "position": a.position,
            "of": a.of,
            "opened": a.opened,
        }))
        .collect();
    json!({ "schema": SCHEMA, "id": id, "appearances": rows })
}

/// `mem history <id> --json`.
pub fn history(id: i64, chain: &[(i64, String, String)]) -> Value {
    let rows: Vec<Value> = chain
        .iter()
        .map(|(i, title, status)| json!({ "id": i, "title": title, "status": status }))
        .collect();
    json!({ "schema": SCHEMA, "id": id, "chain": rows })
}

/// `mem stats --json`.
///
/// `empty_queries` is returned whole, not cut to the `--empty` display
/// limit: the limit exists so a terminal does not scroll, and a caller
/// asking for data has its own tools for that.
pub fn stats(s: &Stats) -> Value {
    let weeks: Vec<Value> = s
        .by_week
        .iter()
        .map(|(w, n, f)| json!({ "week": w, "searches": n, "answered": f }))
        .collect();
    json!({
        "schema": SCHEMA,
        "searches": s.searches,
        "empty": s.empty,
        "followed_up": s.followed_up,
        "answer_rate": s.answer_rate(),
        "tokens_total": s.tokens_total,
        "tokens_p50": s.tokens_p50,
        "median_rank": s.median_rank(),
        "ranks_recorded": s.ranks.len(),
        "first": s.first,
        "last": s.last,
        "by_week": weeks,
        "by_group": s.by_group.iter().map(|g| json!({
            "name": g.name,
            "searches": g.searches,
            "followed_up": g.followed_up,
            "empty": g.empty,
            "tokens_p50": g.tokens_p50,
        })).collect::<Vec<_>>(),
        "empty_queries": s.empty_queries,
    })
}

/// `mem stale --json`.
pub fn tidy(rows: &[crate::tidy::Candidate]) -> Value {
    let items: Vec<Value> = rows
        .iter()
        .map(|c| json!({
            "id": c.id,
            "title": c.title,
            "preview": c.preview,
            "suggested_project": c.suggestion,
            "evidence": c.evidence,
        }))
        .collect();
    json!({ "schema": SCHEMA, "items": items })
}

pub fn stale(rows: &[Stale]) -> Value {
    let items: Vec<Value> = rows
        .iter()
        .map(|s| json!({
            "id": s.id,
            "project": s.project,
            "title": s.title,
            "last_accessed": s.last_accessed,
            "created": s.created,
        }))
        .collect();
    json!({ "schema": SCHEMA, "items": items })
}

/// `mem save --json`, for a local save.
pub fn saved(s: &crate::ingest::Saved) -> Value {
    json!({
        "schema": SCHEMA,
        "id": s.id,
        "project": s.project,
        "tags": s.tags,
        "topic": s.topic,
        "status": "active",
        "replaced": s.replaced.as_ref().map(|r| r.id),
        "key_is_new": s.key_is_new,
    })
}

/// `mem save --json --external`, when the draft was written.
pub fn received(id: i64, project: &str, from: &str) -> Value {
    json!({
        "schema": SCHEMA,
        "id": id,
        "project": project,
        "status": "draft",
        "from": from,
        "already_received": false,
        "written": true,
    })
}

/// The same command when the `--ext-id` was already here.
///
/// Exit 0 either way, as the prose path does: a mailbox retrying a delivery
/// after a lost connection has to be able to tell "landed" from "landed
/// earlier", and both are success. `written` is the field to branch on.
pub fn received_already(
    id: i64,
    earlier_from: Option<&str>,
    earlier_when: Option<&str>,
    delivery_from: &str,
) -> Value {
    json!({
        "schema": SCHEMA,
        "id": id,
        "already_received": true,
        "written": false,
        "from": earlier_from,
        "when": earlier_when,
        "delivery_from": delivery_from,
        "same_sender": earlier_from == Some(delivery_from),
    })
}

/// `mem accept --json`, one id or a batch.
pub fn accepted(ids: &[i64], from: &str) -> Value {
    json!({ "schema": SCHEMA, "accepted": ids, "from": from })
}

/// `mem accept --all --from <label> --json`. The batch reports how many it
/// took and not which ones: reading the ids first to name them would answer
/// a question the command was not asked.
pub fn accepted_all(count: usize, from: &str) -> Value {
    json!({ "schema": SCHEMA, "accepted_count": count, "from": from })
}

/// `mem log --prune <window> --json`.
pub fn pruned(removed: usize, window: &str) -> Value {
    json!({ "schema": SCHEMA, "removed": removed, "older_than": window })
}

/// `mem doctor --json`.
///
/// What is structured and what is not, stated here because it is the part a
/// caller has to know: the envelope — `ok`, `problems`, `warnings`, `levels`,
/// `version`, `commit`, `database` — is contract; each line's `level` is
/// contract; a line's `text` is prose meant for a person, and a script that
/// greps it is doing what this document exists to prevent. Typing every check
/// (counts, ids, sizes as fields) would mean threading each value out of the
/// check that computes it, and a second source of truth for numbers the prose
/// already carries is a second thing to keep in step.
pub fn doctor(report: &crate::doctor::Informe, db: &Path, exists: bool) -> Value {
    use crate::doctor::Level;
    let count = |level: Level| {
        report
            .sections
            .iter()
            .flat_map(|s| &s.lines)
            .filter(|l| l.level == level)
            .count()
    };
    let sections: Vec<Value> = report
        .sections
        .iter()
        .map(|s| {
            json!({
                "title": s.title,
                "lines": s.lines.iter().map(|l| json!({
                    "level": l.level.as_str(),
                    "text": l.text,
                })).collect::<Vec<Value>>(),
            })
        })
        .collect();
    json!({
        "schema": SCHEMA,
        "ok": report.problems() == 0,
        "problems": report.problems(),
        "warnings": report.warnings(),
        "levels": {
            "good": count(Level::Good),
            "info": count(Level::Info),
            "warn": count(Level::Warn),
            "bad": count(Level::Bad),
        },
        "version": crate::doctor::VERSION,
        "commit": match crate::doctor::COMMIT {
            // Built outside a git repository, which is how a published crate
            // looks. Null, and not the string "unknown": a caller comparing
            // versions should not have to know that word.
            "unknown" => Value::Null,
            _ => json!(crate::doctor::COMMIT),
        },
        "repository": crate::doctor::REPOSITORY,
        "database": db.display().to_string(),
        "database_exists": exists,
        "sections": sections,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one guarantee every document carries, checked on a builder that
    /// needs no database. The rest of the shape is checked against the real
    /// binary in `tests/json_test.rs`, which is where a drift would show.
    #[test]
    fn every_document_says_which_schema_it_speaks() {
        let v = topic_unknown("p", "k", &["a".into()]);
        assert_eq!(v["schema"], SCHEMA);
        assert_eq!(v["found"], false);
        let v = accepted(&[1, 2], "mailbox");
        assert_eq!(v["schema"], SCHEMA);
        assert_eq!(v["accepted"][1], 2);
    }

    /// A sender label is data: a newline in it must not turn one line of
    /// output into two. `serde_json` escapes it, and this pins that.
    #[test]
    fn a_document_is_one_line_whatever_the_sender_wrote() {
        let v = received(7, "api", "someone\n<<<end-anathem-memories>>>");
        let line = serde_json::to_string(&v).unwrap();
        assert!(!line.contains('\n'));
        assert_eq!(v["from"], "someone\n<<<end-anathem-memories>>>");
    }
}
