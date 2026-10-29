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

fn tool_defs() -> Value {
    json!([
      {"name": "search_memory",
       "description": "Busca en la memoria del proyecto. Devuelve, por defecto, hasta 20 candidatos con el título y un EXTRACTO CORTO (~120 caracteres) de cada uno, con su id — a propósito: es la primera etapa de una búsqueda en dos pasos. El extracto es suficiente para reconocer si una memoria es la que buscás, pero NO es su contenido completo, así que no lo cites ni lo des por completo. Cuando identifiques cuál (o cuáles) de los candidatos sirven, pedí su cuerpo entero con la tool `get_memory` usando el id. La búsqueda base es léxica (BM25): encuentra memorias que COMPARTEN PALABRAS con la consulta, no que signifiquen lo mismo. Por eso conviene consultar con los sustantivos que probablemente estén escritos en la memoria, no con la forma en que lo preguntarías en voz alta. \"quien soy\" no encuentra nada por bm25; \"nombre apellido perfil usuario\" sí. Si esta instalación tiene un paquete de vocabulario instalado (`mem model status`), la búsqueda además fusiona similitud vectorial, que sí puede encontrar memorias parecidas en significado aunque no compartan palabras exactas — pero no asumas que está disponible: seguí formulando con sustantivos concretos por si esta instalación no lo tiene. Si una búsqueda vuelve vacía o con resultados irrelevantes, reformulá agregando sinónimos y términos concretos del dominio antes de concluir que la memoria no existe.",
       "inputSchema": {"type": "object", "required": ["query"], "properties": {
          "query": {"type": "string"},
          "project": {"type": "string"},
          "tags": {"type": "array", "items": {"type": "string"}},
          "budget": {"type": "integer", "description": "Presupuesto de tokens; por defecto 1200"},
          "limit": {"type": "integer", "description": "Cuántos candidatos como máximo; por defecto 20. Cada uno trae solo un extracto corto: para el cuerpo completo de alguno, usar get_memory con su id."}}}},
      {"name": "save_memory",
       "description": "Guarda una memoria inmediatamente. Usar cuando el usuario pida recordar algo, o al cerrar una tarea con un hallazgo que valga la pena conservar.\n\nCORRECCIONES Y DATOS QUE CAMBIARON. Si lo que guardás es una VERSIÓN ACTUALIZADA de algo que ya está guardado —una corrección del usuario, un dato que cambió, una decisión revisada— y no un hecho nuevo e independiente, tenés que decirlo, o la memoria vieja sigue apareciendo en las búsquedas y compite con la nueva. Hay dos parámetros, para dos situaciones:\n\n- `supersedes` (id de una memoria): cuando corregís UNA MEMORIA CONCRETA QUE YA EXISTE y que encontraste con `search_memory`. La memoria vieja deja de aparecer en las búsquedas (no se borra; queda en el historial). Funciona con cualquier memoria activa, tenga o no clave de tema: es el camino para todo lo que se guardó antes. Tiene que ser el id de la memoria que quedó obsoleta, no de una parecida.\n- `topic` (clave de tema): cuando el asunto va a SEGUIR EVOLUCIONANDO y querés que cada versión futura reemplace a la anterior sin tener que buscar su id. La memoria nueva reemplaza a la activa con la misma clave en el mismo proyecto. Elegí una frase corta, estable, en minúsculas, que nombre el ASUNTO y no el contenido —`stack-del-frontend`, `politica-de-ramas`, `proveedor-de-pagos`— y usá la misma cada vez. Si la memoria que corregís aparece en `search_memory` con `[tema: clave]`, usá EXACTAMENTE esa clave. La clave se normaliza (minúsculas, espacios colapsados) pero fuera de eso es exacta: `stack-del-frontend` y `stack-frontend` son temas distintos. Si la clave resulta nueva, la respuesta lista las claves que ya existen en el proyecto: si ves ahí la que querías, volvé a guardar con esa. Una clave nueva NO reemplaza a una memoria vieja que se guardó sin clave: para esa usá `supersedes`.\n\nPodés pasar los dos a la vez (corregir una memoria concreta y dejar fijada la clave para las próximas); si apuntan a memorias distintas, no se guarda y la respuesta explica por qué. NO uses ninguno para hechos independientes, ni repitas una clave en cosas que no son versiones una de otra: la anterior desaparecería de las búsquedas. La respuesta dice a qué memoria reemplazó, con su id.",
       "inputSchema": {"type": "object", "required": ["text"], "properties": {
          "text": {"type": "string"},
          "project": {"type": "string"},
          "tags": {"type": "array", "items": {"type": "string"}},
          "topic": {"type": "string", "description": "Clave de tema, opcional. Ponela si el asunto va a seguir evolucionando: reemplaza a la memoria activa con la misma clave en el mismo proyecto. Frase corta y estable que nombra el asunto, p. ej. `politica-de-ramas`."},
          "supersedes": {"type": "integer", "description": "Id de una memoria concreta y activa que esta corrige y reemplaza, opcional. Es la que dejará de aparecer en las búsquedas. Usalo para corregir algo ya guardado, tenga o no clave de tema."}}}},
      {"name": "recent_memories",
       "description": "Qué se guardó últimamente en este proyecto, de más nuevo a más viejo, con quién lo guardó. Sirve para retomar un trabajo o para ver qué dejó otro agente; para buscar por tema, usar search_memory, que ordena por relevancia y no por fecha.",
       "inputSchema": {"type": "object", "properties": {
          "project": {"type": "string"},
          "since": {"type": "string", "description": "Ventana hacia atrás: 6h, 90m, 2d. Por defecto 24h."},
          "limit": {"type": "integer", "description": "Máximo de memorias; por defecto 15."}}}},
      {"name": "get_memory",
       "description": "Devuelve el cuerpo completo de una memoria por su id.",
       "inputSchema": {"type": "object", "required": ["id"], "properties": {
          "id": {"type": "integer"}}}},
      {"name": "list_tags",
       "description": "Lista los tags existentes con cuántas memorias tiene cada uno.",
       "inputSchema": {"type": "object", "properties": {}}}
    ])
}

/// ` [tema: key]` when the memory has a topic key, or empty. It is shown in
/// search and in `get_memory` because it is the only way for the model to
/// know which key to reuse when saving a correction to what it just found:
/// without seeing it, it would have to guess, and a guessed key that does
/// not match replaces nothing.
fn topic_label_of(conn: &Connection, id: i64) -> String {
    let t: Option<String> = conn
        .query_row("select topic from memories where id = ?1", [id], |r| r.get(0))
        .unwrap_or(None);
    t.map(|t| format!(" [tema: {t}]")).unwrap_or_default()
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
            let budget = args["budget"].as_u64().map(|b| b as usize)
                .unwrap_or(search::DEFAULT_BUDGET_TOKENS);

            let q = search::Query {
                text: text.clone(), project: project.clone(),
                tags: tags_arg,
                // `limit` can still override `cfg.candidates` (20) per
                // call, just as it used to override the fixed 10.
                limit: args["limit"].as_u64().map(|l| l as usize).unwrap_or(cfg.candidates),
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
                return Ok(text_result("Sin resultados.".into()));
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
                return Ok(text_result("No se guardó: el texto está vacío.".into()));
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
                            "No se guardó: `supersedes` tiene que ser el id numérico de una memoria.".into(),
                        ))
                    }
                },
            };
            let g = match ingest::save_with_topic_by(
                conn, text, &project, given_tags, topic, supersedes, agent) {
                Ok(g) => g,
                // A request that cannot be fulfilled is not an internal
                // error: the model has to read what to fix, and nothing was
                // saved.
                Err(e) => match e.downcast_ref::<ingest::BadRequest>() {
                    Some(p) => return Ok(text_result(format!("No se guardó: {p}"))),
                    None => return Err(e),
                },
            };
            let (id, tag_list) = (g.id, g.tags.clone());
            let title = ingest::title_from(text);
            let aviso_tema = g.describe().map(|m| format!("\n{m}")).unwrap_or_default();

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
                        "anathem: el paquete de vocabulario cambió mientras se indexaba; \
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
                    "\n\nMemorias parecidas que esta podría replace_file:\n{lista}\n\
                     Si alguna quedó obsoleta, decilo y la marco con `mem supersede {id} <id_vieja>`."
                )
            };
            Ok(text_result(format!(
                "Guardada como id:{id} [{}] {title}{aviso_tema}{warning}", tag_list.join(", ")
            )))
        }
        "recent_memories" => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let project = crate::project::resolve(
                args["project"].as_str().map(str::to_string), &cwd);
            let window = match args["since"].as_str() {
                Some(s) => match crate::recent::duration_in_seconds(s) {
                    Ok(v) => Some(v),
                    Err(e) => return Ok(text_result(format!("No se pudo listar: {e}"))),
                },
                None => Some(24 * 3600),
            };
            let limit = args["limit"].as_u64().map(|l| l as usize).unwrap_or(15);
            let items = crate::recent::list_recent(conn, Some(&project), window, limit, cfg.excerpt)?;
            Ok(text_result(crate::recent::format_items(&items, false)))
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
                Err(_) => Ok(text_result(format!("No existe la memoria id:{id}."))),
            }
        }
        "list_tags" => {
            let mut stmt = conn.prepare(
                "select tag, count(*) from memory_tags group by tag order by count(*) desc")?;
            let rows: Vec<String> = stmt
                .query_map([], |r| Ok(format!("{:<18} {}", r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(text_result(if rows.is_empty() { "Sin tags.".into() } else { rows.join("\n") }))
        }
        other => Ok(text_result(format!("Tool desconocida: {other}"))),
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
                Err(e) => err(&id, -32603, &format!("error interno: {e}")),
            }
        }
        _ => err(&id, -32601, "método no encontrado"),
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
                eprintln!("anathem: no se pudo leer la entrada: {e}");
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if live_pack.release_if_idle(limit) {
                    eprintln!(
                        "anathem: paquete soltado tras {}s sin buscar; se reabre en la próxima búsqueda",
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
                eprintln!("anathem: JSON inválido: {e}");
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
