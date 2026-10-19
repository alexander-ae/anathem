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
      {"name": "get_memory",
       "description": "Devuelve el cuerpo completo de una memoria por su id.",
       "inputSchema": {"type": "object", "required": ["id"], "properties": {
          "id": {"type": "integer"}}}},
      {"name": "list_tags",
       "description": "Lista los tags existentes con cuántas memorias tiene cada uno.",
       "inputSchema": {"type": "object", "properties": {}}}
    ])
}

/// ` [tema: clave]` si la memoria tiene clave de tema, o vacío. Se muestra en
/// la búsqueda y en `get_memory` porque es la única manera de que el modelo
/// sepa qué clave reutilizar al guardar la corrección de lo que acaba de
/// encontrar: sin verla tendría que adivinarla, y una clave adivinada que no
/// coincide no reemplaza nada.
fn etiqueta_de_tema(conn: &Connection, id: i64) -> String {
    let t: Option<String> = conn
        .query_row("select topic from memories where id = ?1", [id], |r| r.get(0))
        .unwrap_or(None);
    t.map(|t| format!(" [tema: {t}]")).unwrap_or_default()
}

fn call_tool(
    conn: &Connection,
    ctx: &search::Contexto,
    cfg: &Config,
    name: &str,
    args: &Value,
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
                // `limit` sigue pudiendo pisar por llamada el default de
                // `cfg.candidatos` (20), igual que antes pisaba al 10 fijo.
                limit: args["limit"].as_u64().map(|l| l as usize).unwrap_or(cfg.candidatos),
                budget_tokens: budget,
                // El recorte a `cfg.extracto` caracteres es lo que hace que
                // entren muchos candidatos breves en el mismo presupuesto de
                // tokens en vez de pocos fragmentos completos.
                extracto: Some(cfg.extracto),
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
                    h.memory_id, h.project, etiqueta_de_tema(conn, h.memory_id), h.title, h.snippet
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
            // Sin `project`, se deriva del repositorio donde corre el
            // servidor. El agente lo lanza parado en el proyecto, así que el
            // directorio de trabajo es la señal correcta — y es mejor que
            // pedirle al modelo que lo invente, que es como el sistema
            // anterior terminó con todo en un único contenedor de relleno.
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let project = crate::proyecto::resolver(
                args["project"].as_str().map(str::to_string), &cwd);
            let tags_dados: Vec<String> = args["tags"].as_array().map(|a| {
                a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
            }).unwrap_or_default();
            let project_for_lookup = project.clone();
            // Una clave vacía (el modelo la manda "por las dudas") es lo
            // mismo que no mandarla: guardar normal, sin reemplazar nada. La
            // CLI, en cambio, rechaza `--topic ""`: ahí quien lo escribe lo
            // puso a propósito (o una variable de shell vacía) y guardar sin
            // clave en silencio sería ocultarle que no se aplicó.
            let topic = args["topic"].as_str().map(str::trim).filter(|t| !t.is_empty());
            // `supersedes` puede llegar como número o como texto ("12"): los
            // modelos mezclan los dos.
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
            let g = match ingest::guardar_con_tema(conn, text, &project, tags_dados, topic, supersedes) {
                Ok(g) => g,
                // Un pedido que no se puede cumplir no es un error interno:
                // el modelo tiene que leer qué corregir, y no se guardó nada.
                Err(e) => match e.downcast_ref::<ingest::PedidoInvalido>() {
                    Some(p) => return Ok(text_result(format!("No se guardó: {p}"))),
                    None => return Err(e),
                },
            };
            let (id, tag_list) = (g.id, g.tags.clone());
            let title = ingest::titulo_de(text);
            let aviso_tema = g.describir().map(|m| format!("\n{m}")).unwrap_or_default();

            // `PaqueteVivo` revalidó el paquete al empezar el mensaje, pero
            // embeber un backlog grande puede llevar minutos y en ese rato
            // otra terminal instala lo que quiera: por eso `sincronizar`
            // recibe la identidad y la vuelve a comprobar en cada lote.
            if let Some((p, identidad)) = ctx.para_escribir() {
                if crate::vecstore::sincronizar(conn, p, identidad)?.abandonada() {
                    // El guardado salió bien y eso es lo que el agente tiene
                    // que oír; el hueco de vectores lo cierra la próxima
                    // escritura o un `mem reindex`. Devolver un error acá
                    // sería reportar como fallido un guardado exitoso.
                    eprintln!(
                        "anathem: el paquete de vocabulario cambió mientras se indexaba; \
                         se abandonó el lote para no mezclar dos paquetes. Quedan \
                         fragmentos sin vector."
                    );
                }
            }

            let cand = crate::supersede::candidates(conn, text, &project_for_lookup, 3)?;
            let aviso = if cand.is_empty() {
                String::new()
            } else {
                let lista = cand.iter()
                    .map(|c| format!("  id:{} {}", c.memory_id, c.title))
                    .collect::<Vec<_>>().join("\n");
                format!(
                    "\n\nMemorias parecidas que esta podría reemplazar:\n{lista}\n\
                     Si alguna quedó obsoleta, decilo y la marco con `mem supersede {id} <id_vieja>`."
                )
            };
            Ok(text_result(format!(
                "Guardada como id:{id} [{}] {title}{aviso_tema}{aviso}", tag_list.join(", ")
            )))
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
                        "[{project}]{} {title}\n\n{body}", etiqueta_de_tema(conn, id)
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

/// Procesa una petición. Devuelve `None` para notificaciones (sin `id`).
pub fn handle(
    conn: &Connection,
    ctx: &search::Contexto,
    cfg: &Config,
    req: &Value,
) -> Result<Option<Value>> {
    let method = req["method"].as_str().unwrap_or_default();
    let id = req.get("id").cloned();

    if id.is_none() {
        return Ok(None); // notificación
    }
    let id = id.unwrap();

    let res = match method {
        "initialize" => ok(&id, json!({
            "protocolVersion": PROTOCOL,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "anathem", "version": env!("CARGO_PKG_VERSION")}
        })),
        "tools/list" => ok(&id, json!({"tools": tool_defs()})),
        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or_default();
            let empty = json!({});
            let args = req["params"].get("arguments").unwrap_or(&empty);
            match call_tool(conn, ctx, cfg, name, args) {
                Ok(v) => ok(&id, v),
                Err(e) => err(&id, -32603, &format!("error interno: {e}")),
            }
        }
        _ => err(&id, -32601, "método no encontrado"),
    };
    Ok(Some(res))
}

/// Bucle de servidor sobre stdin/stdout. Todo diagnóstico va a stderr.
///
/// El paquete **no** se resuelve una sola vez al arrancar. El servidor vive
/// horas, y en ese rato otra terminal puede correr `mem model install` o
/// `mem model remove`: un servidor que se quedara con el paquete del
/// arranque escribiría vectores del paquete viejo junto a los del nuevo, y
/// con el formato fijando 256 dimensiones para todo paquete nada en el blob
/// los distinguiría. `PaqueteVivo` revalida el registro antes de cada
/// operación que use el paquete —una consulta a `meta`— y solo reabre el
/// archivo, que cuesta entre 40 y 120 ms y unos 54 MB de heap del
/// tokenizador, cuando la identidad cambió de verdad.
///
/// Esa apertura tampoco pasa al arrancar ni en cada mensaje: `search_memory`
/// y `save_memory` son las únicas tools que usan el paquete
/// (`get_memory`, `list_tags`, `initialize` y `tools/list` no), así que acá
/// se le pide el paquete a `vivo` solo cuando el mensaje es una llamada a una
/// de esas dos. Una sesión que nunca busca ni guarda nunca paga ese heap.
pub fn serve(conn: &Connection, dir: &Path, cfg: &Config) -> Result<()> {
    let mut vivo = crate::paquete::PaqueteVivo::nuevo(dir);
    // El paquete se abre recién cuando alguien busca o guarda, así que una
    // sesión que nunca lo use no vería que desapareció. Este chequeo cuesta
    // un `stat` y cubre ese caso al arrancar.
    crate::paquete::avisar_si_falta_el_archivo(conn, dir);
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
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
        let necesita_paquete = req["method"] == "tools/call"
            && matches!(
                req["params"]["name"].as_str(),
                Some("search_memory") | Some("save_memory")
            );
        let ctx = if necesita_paquete {
            search::Contexto::de_servidor(vivo.actual(conn), cfg.peso_bm25)
        } else {
            search::Contexto::de_servidor(None, cfg.peso_bm25)
        };
        if let Some(res) = handle(conn, &ctx, cfg, &req)? {
            writeln!(stdout, "{}", serde_json::to_string(&res)?)?;
            stdout.flush()?;
        }
    }
    Ok(())
}
