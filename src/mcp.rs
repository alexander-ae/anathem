use crate::{config::Config, ingest, logsearch, search};
use anyhow::Result;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;

const PROTOCOL: &str = "2024-11-05";

fn ok(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn err(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn text_result(s: String) -> Value {
    json!({"content": [{"type": "text", "text": s}]})
}

/// The tool definitions, exposed for the test that keeps the `[topic: ...]`
/// label and the description that tells the model to reuse it from drifting
/// apart. See `the_topic_label_and_the_tool_description_agree`.
#[doc(hidden)]
pub fn tool_defs() -> Value {
    json!([
      {"name": "search_memory",
       "description": "Search the project's memory. By default it returns up to 20 candidates, each with its title and a SHORT EXCERPT (~120 characters) plus its id — deliberately: this is the first stage of a two-step search. The excerpt is enough to recognise whether a memory is the one you want, but it is NOT the full body, so do not quote it or treat it as complete. Once you identify which candidate (or candidates) help, ask for the whole body with the `get_memory` tool, using the id. The base search is lexical (BM25): it finds memories that SHARE WORDS with the query, not ones that mean the same thing. So it pays to query with the nouns likely to be written in the memory, not with the phrasing you would use out loud. \"who am i\" finds nothing by bm25; \"name surname profile user\" does. If this installation has a vocabulary pack installed (`mem model status`), the search also fuses vector similarity, which can find memories close in meaning even without shared words — but do not assume it is available: keep phrasing with concrete nouns in case this installation lacks it. If a search comes back empty or irrelevant, rephrase with synonyms and concrete domain terms before concluding the memory does not exist.",
       "inputSchema": {"type": "object", "required": ["query"], "properties": {
          "query": {"type": "string"},
          "project": {"type": "string"},
          "tags": {"type": "array", "items": {"type": "string"}},
          "budget": {"type": "integer", "description": "Token budget; 1200 by default"},
          "limit": {"type": "integer", "description": "How many candidates at most; 20 by default. Each one carries only a short excerpt: for the full body of one, use get_memory with its id."}}}},
      {"name": "save_memory",
       "description": "Saves a memory immediately. Use it when the user asks to remember something, or when closing a task with a finding worth keeping.\n\nCORRECTIONS AND FACTS THAT CHANGED. If what you are saving is an UPDATED VERSION of something already saved — a correction from the user, a fact that changed, a revised decision — and not a new independent fact, you have to say so, or the old memory keeps showing up in searches and competes with the new one. There are two parameters, for two situations:\n\n- `supersedes` (a memory id): when you are correcting ONE SPECIFIC EXISTING MEMORY that you found with `search_memory`. The old one stops appearing in searches (it is not deleted; it stays in the history). It works with any active memory, with or without a topic key: it is the route for everything saved earlier. It has to be the id of the memory that became obsolete, not of a similar one.\n- `topic` (a topic key): when the subject will KEEP EVOLVING and you want each future version to replace the previous one without having to look up its id. The new memory replaces the active one under the same key in the same project. Pick a short, stable, lowercase phrase that names the SUBJECT and not the content — `frontend-stack`, `branching-policy`, `payment-provider` — and use the same one every time. If the memory you are correcting shows up in `search_memory` with `[topic: key]`, use EXACTLY that key. The key is normalised (lowercase, whitespace collapsed) but beyond that it is exact: `frontend-stack` and `front-stack` are different topics. If the key turns out to be new, the response lists the keys that already exist in the project: if you see the one you meant there, save again with that one. A new key does NOT replace an old memory saved without one: for that, use `supersedes`.\n\nYou can pass both at once (correct a specific memory and pin the key for next time); if they point at different memories, nothing is saved and the response explains why. Do NOT use either for independent facts, and do not repeat a key across things that are not versions of one another: the previous one would disappear from searches. The response says which memory it replaced, with its id.",
       "inputSchema": {"type": "object", "required": ["text"], "properties": {
          "text": {"type": "string"},
          "project": {"type": "string"},
          "tags": {"type": "array", "items": {"type": "string"}},
          "topic": {"type": "string", "description": "Topic key, optional. Set it if the subject will keep evolving: it replaces the active memory under the same key in the same project. A short, stable phrase naming the subject, e.g. `branching-policy`."},
          "supersedes": {"type": "integer", "description": "Id of a specific, active memory that this one corrects and replaces, optional. It is the one that will stop appearing in searches. Use it to correct something already saved, with or without a topic key."}}}},
      {"name": "recent_memories",
       "description": "What was saved lately in this project, newest first, and who saved it. Useful for picking work back up or seeing what another agent left behind; to search by subject use search_memory, which ranks by relevance rather than date.",
       "inputSchema": {"type": "object", "properties": {
          "project": {"type": "string"},
          "since": {"type": "string", "description": "How far back: 6h, 90m, 2d. 24h by default."},
          "limit": {"type": "integer", "description": "At most this many memories; 15 by default."}}}},
      {"name": "get_memory",
       "description": "Returns the full body of a memory by its id.",
       "inputSchema": {"type": "object", "required": ["id"], "properties": {
          "id": {"type": "integer"}}}},
      {"name": "list_tags",
       "description": "Lists the existing tags with how many memories each one has.",
       "inputSchema": {"type": "object", "properties": {}}}
    ])
}

/// ` [topic: key]` when the memory has a topic key, or empty. It is shown in
/// search and in `get_memory` because it is the only way for the model to
/// know which key to reuse when saving a correction to what it just found:
/// without seeing it, it would have to guess, and a guessed key that does
/// not match replaces nothing.
fn topic_label_of(conn: &Connection, id: i64) -> String {
    let t: Option<String> = conn
        .query_row("select topic from memories where id = ?1", [id], |r| r.get(0))
        .unwrap_or(None);
    t.map(|t| format!(" [topic: {t}]")).unwrap_or_default()
}

fn call_tool(
    conn: &Connection,
    ctx: &search::SearchContext,
    cfg: &Config,
    name: &str,
    args: &Value,
    agent: Option<&str>,
) -> Result<Value> {
    match name {
        "search_memory" => {
            let text = args["query"].as_str().unwrap_or_default().to_string();
            let project = args["project"].as_str().map(str::to_string);
            let tags_arg: Vec<String> = args["tags"].as_array().map(|a| {
                a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
            }).unwrap_or_default();
            // Clamped, not rejected (finding B-4): a model that asks for
            // more than the ceiling wants a big answer, and the useful
            // reply to that is the biggest one allowed rather than an
            // error it has to learn to work around.
            let budget = args["budget"].as_u64().map(|b| b as usize)
                .unwrap_or(search::DEFAULT_BUDGET_TOKENS)
                .min(search::MAX_BUDGET_TOKENS);

            let q = search::Query {
                text: text.clone(), project: project.clone(),
                tags: tags_arg,
                // `limit` can still override `cfg.candidates` (20) per
                // call, just as it used to override the fixed 10.
                limit: args["limit"].as_u64().map(|l| l as usize)
                    .unwrap_or(cfg.candidates)
                    .min(search::MAX_LIMIT),
                budget_tokens: budget,
                // Trimming to `cfg.excerpt` characters is what lets many
                // short candidates fit in the same token budget instead of a
                // few full snippets.
                excerpt: Some(cfg.excerpt),
            };
            let hits = search::run(conn, &q, ctx)?;
            let ids: Vec<String> = hits.iter().map(|h| h.memory_id.to_string()).collect();
            let used: usize = hits.iter()
                .map(|h| (h.title.chars().count() + h.snippet.chars().count()) / 4).sum();
            logsearch::record(conn, &text, project.as_deref(), &ids, used)?;

            if hits.is_empty() {
                return Ok(text_result("No results.".into()));
            }
            let body = hits.iter()
                .map(|h| format!(
                    "[id:{}] [{}]{} {}\n{}",
                    h.memory_id, h.project, topic_label_of(conn, h.memory_id), h.title, h.snippet
                ))
                .collect::<Vec<_>>()
                .join("\n\n");
            Ok(text_result(body))
        }
        "save_memory" => {
            let text = args["text"].as_str().unwrap_or_default();
            if text.trim().is_empty() {
                return Ok(text_result("Not saved: the text is empty.".into()));
            }
            // With no `project`, it is derived from the repository the
            // server runs in. The agent launches it standing in the project,
            // so the working directory is the right signal — and a better
            // one than asking the model to invent it, which is how the
            // previous system ended up with everything in a single filler
            // container.
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let project = crate::project::resolve(
                args["project"].as_str().map(str::to_string), &cwd);
            let given_tags: Vec<String> = args["tags"].as_array().map(|a| {
                a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
            }).unwrap_or_default();
            let project_for_lookup = project.clone();
            // An empty key (the model sends it "just in case") is the same
            // as not sending one: save normally, replacing nothing. The CLI,
            // by contrast, rejects `--topic ""`: there whoever typed it did
            // so on purpose (or an empty shell variable did), and saving
            // without a key silently would hide that it was not applied.
            let topic = args["topic"].as_str().map(str::trim).filter(|t| !t.is_empty());
            // `supersedes` can arrive as a number or as text ("12"): models
            // mix the two.
            let supersedes = match &args["supersedes"] {
                Value::Null => None,
                v => match v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())) {
                    Some(id) => Some(id),
                    None => {
                        return Ok(text_result(
                            "Not saved: `supersedes` has to be the numeric id of a memory.".into(),
                        ))
                    }
                },
            };
            let g = match ingest::save_with_topic_by(
                conn, text, &project, given_tags, topic, supersedes, agent, crate::lang::Lang::En) {
                Ok(g) => g,
                // A request that cannot be fulfilled is not an internal
                // error: the model has to read what to fix, and nothing was
                // saved.
                Err(e) => match e.downcast_ref::<ingest::BadRequest>() {
                    Some(p) => return Ok(text_result(format!("Not saved: {p}"))),
                    None => return Err(e),
                },
            };
            let (id, tag_list) = (g.id, g.tags.clone());
            let title = ingest::title_from(text);
            let aviso_tema = g.describe(crate::lang::Lang::En).map(|m| format!("\n{m}")).unwrap_or_default();

            // `LivePack` revalidated the pack when the message began, but
            // embedding a large backlog can take minutes and in that time
            // another terminal can install whatever it likes: that is why
            // `sync` receives the identity and rechecks it on every batch.
            if let Some((p, identity)) = ctx.for_writing() {
                if crate::vecstore::sync(conn, p, identity)?.abandoned() {
                    // The save succeeded and that is what the agent needs
                    // to hear; the vector gap is closed by the next write or
                    // a `mem reindex`. Returning an error here would report
                    // a successful save as failed.
                    eprintln!(
                        "anathem: the vocabulary pack changed while indexing; \
                         se abandonó el lote para no mezclar dos paquetes. Quedan \
                         fragmentos sin vector."
                    );
                }
            }

            let cand = crate::supersede::candidates(conn, text, &project_for_lookup, 3)?;
            let warning = if cand.is_empty() {
                String::new()
            } else {
                let lista = cand.iter()
                    .map(|c| format!("  id:{} {}", c.memory_id, c.title))
                    .collect::<Vec<_>>().join("\n");
                format!(
                    "\n\nSimilar memories this one could replace:\n{lista}\n\
                     If any of them is now obsolete, say so and I will mark it with `mem supersede {id} <old_id>`."
                )
            };
            Ok(text_result(format!(
                "Saved as id:{id} [{}] {title}{aviso_tema}{warning}", tag_list.join(", ")
            )))
        }
        "recent_memories" => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let project = crate::project::resolve(
                args["project"].as_str().map(str::to_string), &cwd);
            let window = match args["since"].as_str() {
                Some(s) => match crate::recent::duration_in_seconds(s, crate::lang::Lang::En) {
                    Ok(v) => Some(v),
                    Err(e) => return Ok(text_result(format!("Could not list: {e}"))),
                },
                None => Some(24 * 3600),
            };
            let limit = args["limit"].as_u64().map(|l| l as usize).unwrap_or(15);
            let items = crate::recent::list_recent(conn, Some(&project), window, limit, cfg.excerpt)?;
            Ok(text_result(crate::recent::format_items(&items, false, crate::lang::Lang::En)))
        }
        "get_memory" => {
            let id = args["id"].as_i64().unwrap_or(-1);
            let row: rusqlite::Result<(String, String, String)> = conn.query_row(
                "select project, title, body from memories where id = ?1",
                [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)));
            match row {
                Ok((project, title, body)) => {
                    conn.execute(
                        "update memories set access_count = access_count + 1,
                                last_accessed = ?2 where id = ?1",
                        rusqlite::params![id, chrono::Local::now().to_rfc3339()])?;
                    logsearch::mark_follow_up(conn, id)?;
                    Ok(text_result(format!(
                        "[{project}]{} {title}\n\n{body}", topic_label_of(conn, id)
                    )))
                }
                Err(_) => Ok(text_result(format!("There is no memory with id:{id}."))),
            }
        }
        "list_tags" => {
            let mut stmt = conn.prepare(
                "select tag, count(*) from memory_tags group by tag order by count(*) desc")?;
            let rows: Vec<String> = stmt
                .query_map([], |r| Ok(format!("{:<18} {}", r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(text_result(if rows.is_empty() { "No tags.".into() } else { rows.join("\n") }))
        }
        other => Ok(text_result(format!("Unknown tool: {other}"))),
    }
}

/// Handles one request. Returns `None` for notifications (those with no
/// `id`).
pub fn handle(
    conn: &Connection,
    ctx: &search::SearchContext,
    cfg: &Config,
    req: &Value,
) -> Result<Option<Value>> {
    let mut sin_agente = None;
    handle_con_agente(conn, ctx, cfg, req, &mut sin_agente)
}

/// Same as `handle`, remembering which client is on the other side.
///
/// The protocol requires the client to say who it is in `initialize`
/// (`clientInfo.name`), so knowing whether Claude Code or opencode wrote
/// this costs no configuration at all: it is enough not to throw that away.
/// `serve` keeps the name between messages, because `initialize` arrives
/// once and the `tools/call`s come afterwards.
pub fn handle_con_agente(
    conn: &Connection,
    ctx: &search::SearchContext,
    cfg: &Config,
    req: &Value,
    agent: &mut Option<String>,
) -> Result<Option<Value>> {
    let method = req["method"].as_str().unwrap_or_default();
    let id = req.get("id").cloned();

    if id.is_none() {
        return Ok(None); // notificación
    }
    let id = id.unwrap();

    let res = match method {
        "initialize" => {
            if let Some(n) = req["params"]["clientInfo"]["name"].as_str() {
                let n = n.trim();
                if !n.is_empty() {
                    *agent = Some(n.chars().take(60).collect());
                }
            }
            // The project is derived from the directory the agent launched
            // the server in, same as when saving: it is the right signal and
            // it does not depend on the model guessing.
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let project = crate::project::resolve(None, &cwd);
            let traspaso = crate::recent::handoff_instructions(
                conn, &project, cfg.handoff_items, &cfg.handoff_window, agent.as_deref());
            let mut res = json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "anathem", "version": env!("CARGO_PKG_VERSION")}
            });
            if let Some(handoff_text) = traspaso {
                res["instructions"] = json!(handoff_text);
            }
            ok(&id, res)
        }
        "tools/list" => ok(&id, json!({"tools": tool_defs()})),
        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or_default();
            let empty = json!({});
            let args = req["params"].get("arguments").unwrap_or(&empty);
            match call_tool(conn, ctx, cfg, name, args, agent.as_deref()) {
                Ok(v) => ok(&id, v),
                Err(e) => err(&id, -32603, &format!("internal error: {e}")),
            }
        }
        _ => err(&id, -32601, "method not found"),
    };
    Ok(Some(res))
}

/// The server loop over stdin/stdout. All diagnostics go to stderr.
///
/// The pack is **not** resolved once at startup. The server lives for hours,
/// and in that time another terminal can run `mem model install` or `mem
/// model remove`: a server holding on to the pack it started with would
/// write the old pack's vectors next to the new one's, and with the format
/// fixing 256 dimensions for every pack nothing in the blob would tell them
/// apart. `LivePack` revalidates the registry before every operation that
/// uses the pack — one query against `meta` — and only reopens the file,
/// which costs between 40 and 120 ms and some 54 MB of tokenizer heap, when
/// the identity really changed.
///
/// That open does not happen at startup or on every message either:
/// `search_memory` and `save_memory` are the only tools that use the pack
/// (`get_memory`, `list_tags`, `initialize` and `tools/list` do not), so the
/// pack is requested from `live` only when the message is a call to one of
/// those two. A session that never searches or saves never pays that heap.
pub fn serve(conn: &Connection, dir: &Path, cfg: &Config) -> Result<()> {
    let mut live_pack = crate::packstore::LivePack::new_id(dir);
    // Who is on the other side, according to `initialize`. It is remembered
    // between messages because that handshake arrives once and the saves
    // come afterwards.
    let mut agent: Option<String> = None;
    // The pack is opened only when someone searches or saves, so a session
    // that never uses it would not notice it vanished. This check costs one
    // `stat` and covers that case at startup.
    crate::packstore::warn_if_file_missing(conn, dir);
    let mut stdout = std::io::stdout();

    // Read on another thread and wait with a deadline. A `for line in
    // stdin` stays blocked forever between messages, and a quiet session is
    // precisely the one that has to release the pack: without waking up, it
    // would never get to.
    let (tx, rx) = std::sync::mpsc::channel::<std::io::Result<String>>();
    std::thread::spawn(move || {
        let entry = std::io::stdin();
        for line_ in entry.lock().lines() {
            if tx.send(line_).is_err() {
                break; // el otro lado cerró: nadie va a parse más
            }
        }
    });

    let limit = std::time::Duration::from_secs(cfg.release_pack_after);
    // Check more often than the deadline so it is not stretched to double,
    // but not so often that a session doing nothing wakes for no reason.
    let intervalo = if limit.is_zero() {
        std::time::Duration::from_secs(60)
    } else {
        (limit / 4).max(std::time::Duration::from_secs(5))
    };

    loop {
        let line = match rx.recv_timeout(intervalo) {
            Ok(Ok(l)) => l,
            Ok(Err(e)) => {
                eprintln!("anathem: could not read the input: {e}");
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if live_pack.release_if_idle(limit) {
                    eprintln!(
                        "anathem: pack released after {}s without a search; it reopens on the next one",
                        cfg.release_pack_after
                    );
                }
                continue;
            }
            // The reader thread finished: stdin was closed, which is how
            // the client says it is leaving.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("anathem: invalid JSON: {e}");
                continue;
            }
        };
        let needs_pack = req["method"] == "tools/call"
            && matches!(
                req["params"]["name"].as_str(),
                Some("search_memory") | Some("save_memory")
            );
        let ctx = if needs_pack {
            search::SearchContext::for_server(live_pack.current(conn), cfg.bm25_weight)
        } else {
            search::SearchContext::for_server(None, cfg.bm25_weight)
        };
        if let Some(res) = handle_con_agente(conn, &ctx, cfg, &req, &mut agent)? {
            writeln!(stdout, "{}", serde_json::to_string(&res)?)?;
            stdout.flush()?;
        }
    }
    Ok(())
}
