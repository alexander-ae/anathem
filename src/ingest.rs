use crate::{chunk, model::NewMemory};
use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

pub const CHUNK_SIZE: usize = 500;
pub const CHUNK_STEP: usize = 400;

/// Debe coincidir con el check constraint de `memory_refs.kind` en db::SCHEMA.
const VALID_REF_KINDS: [&str; 3] = ["ticket", "pr", "sha"];

pub fn insert(conn: &Connection, m: &NewMemory) -> Result<i64> {
    let tx = conn.unchecked_transaction()?;
    let id = insertar(&tx, m)?;
    tx.commit()?;
    Ok(id)
}

/// El insert en sí, sin abrir ni cerrar transacción: quien llama pone el
/// límite. `insert` lo envuelve en la suya; el guardado con clave de tema lo
/// corre dentro de la que también cubre el supersede, para que ambas cosas
/// ocurran juntas o ninguna.
fn insertar(tx: &Connection, m: &NewMemory) -> Result<i64> {
    tx.execute(
        "insert into memories
           (project, title, body, created, updated, event_start, event_end,
            source, ext_id)
         values (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8)",
        params![m.project, m.title, m.body, m.created,
                m.event_start, m.event_end, m.source, m.ext_id],
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
                "ref kind inválido {kind:?} (value={value:?}); debe ser uno de {VALID_REF_KINDS:?}"
            );
        }
        tx.execute(
            "insert or ignore into memory_refs (memory_id, kind, value) values (?1, ?2, ?3)",
            params![id, kind, value],
        )?;
    }
    Ok(id)
}

/// Guardado explícito de una memoria nueva, compartido por la CLI (`mem save`)
/// y por la tool `save_memory` de MCP. Existe para que los dos caminos no
/// puedan divergir: antes estaba duplicado y la rama de la CLI no clasificaba
/// ni extraía referencias, así que una memoria guardada desde la terminal
/// nacía sin tags ni tickets.
///
/// Si `tags_dados` viene vacío se clasifica el texto; si trae algo, se respeta
/// tal cual y no se clasifica encima. Devuelve el id y los tags aplicados.
/// El título de una memoria es su primera línea no vacía, recortada a 120
/// caracteres. Vive acá, y no duplicado en cada camino, porque el título se
/// deriva del cuerpo: si un camino lo calcula distinto —o no lo recalcula al
/// cambiar el cuerpo— la memoria queda mostrando un título que ya no
/// describe lo que contiene.
pub fn titulo_de(texto: &str) -> String {
    texto
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .chars()
        .take(120)
        .collect()
}

pub fn guardar_explicito(
    conn: &Connection,
    texto: &str,
    project: &str,
    tags_dados: Vec<String>,
) -> Result<(i64, Vec<String>)> {
    let g = guardar_con_tema(conn, texto, project, tags_dados, None, None)?;
    Ok((g.id, g.tags))
}

/// La memoria que un guardado con clave de tema dejó `superseded`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reemplazada {
    pub id: i64,
    pub title: String,
}

/// Resultado de `guardar_con_tema`.
#[derive(Debug, Clone)]
pub struct Guardado {
    pub id: i64,
    pub project: String,
    pub tags: Vec<String>,
    /// La clave tal como quedó guardada, ya normalizada.
    pub topic: Option<String>,
    /// La memoria que quedó superseded, sea por la clave de tema o por un
    /// `supersedes` explícito.
    pub reemplazada: Option<Reemplazada>,
    /// La clave no tenía ninguna memoria activa antes de este guardado.
    pub clave_nueva: bool,
    /// Solo cuando la clave resultó nueva: las demás claves que tienen una
    /// memoria activa en el proyecto, para que un error de tipeo se note.
    pub otras_claves: Vec<String>,
}

/// Valida y normaliza una clave de tema: minúsculas, recortada en los bordes
/// y con los espacios internos colapsados a uno (un salto de línea cuenta
/// como espacio). Una clave vacía se rechaza.
///
/// Se normaliza porque la clave la escribe un modelo, y `Pagos`, `pagos` y
/// `pagos ` tienen que ser el mismo tema: si no, una variación de tipeo
/// guarda como tema nuevo y no reemplaza nada, con un mensaje idéntico al de
/// una clave legítimamente nueva. Lo que la normalización no arregla
/// (`stack-frontend` contra `stack-del-frontend`) lo hace visible
/// `Guardado::otras_claves`.
pub fn normalizar_tema(clave: &str) -> Result<String> {
    let t = clave.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    if t.is_empty() {
        bail!("la clave de tema no puede estar vacía");
    }
    Ok(t)
}

/// Tope de claves que se listan al avisar que una clave es nueva.
const MAX_CLAVES_LISTADAS: usize = 30;

/// Error por un pedido que no se puede cumplir (un `supersedes` que no existe,
/// que no está activo, o que choca con la clave). A diferencia de un fallo de
/// la base, es culpa de la entrada y se puede contar tal cual a quien llamó:
/// MCP lo devuelve como texto de la tool y no como "error interno".
#[derive(Debug)]
pub struct PedidoInvalido(pub String);

impl std::fmt::Display for PedidoInvalido {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PedidoInvalido {}

/// `guardar_explicito` con clave de tema y/o `supersedes` opcionales.
///
/// Sin ninguno de los dos es exactamente el guardado de siempre.
///
/// **`topic`**: si el proyecto ya tiene una memoria **activa** con esa clave,
/// la nueva la supersede: se inserta aparte y la vieja queda `superseded`,
/// enlazada por `supersedes_id`, en vez de editarse en el lugar. El proyecto
/// conserva historia (`mem history` recorre la cadena, el export incluye lo
/// oculto), y editar acá destruiría la versión anterior. La clave se acota por
/// proyecto: la misma en dos proyectos son dos temas.
///
/// **`supersedes`**: el id de una memoria concreta que esta corrige, aunque se
/// haya guardado sin clave. Tiene que existir y estar activa. Es el camino para
/// lo que ya estaba guardado; `topic` es para lo que va a seguir evolucionando.
/// Pueden darse juntos si apuntan a la misma memoria (o si la clave es nueva):
/// una memoria nueva reemplaza como mucho a una, así que si la clave ya tiene
/// una vigente distinta de `supersedes` se rechaza en vez de elegir en silencio.
///
/// El marcado lo hace siempre `supersede::link` y no un `update` propio, para
/// que haya un solo camino que decida qué es un reemplazo válido (y que
/// rechace los ciclos).
///
/// Todo corre en una transacción `immediate`: buscar la vigente, insertar y
/// enlazar son una unidad. Con una transacción diferida, dos guardados
/// simultáneos con la misma clave verían la misma vigente y dejarían dos
/// memorias activas para un tema; `immediate` toma el lock de escritura antes
/// de leer y el segundo espera al primero.
pub fn guardar_con_tema(
    conn: &Connection,
    texto: &str,
    project: &str,
    tags_dados: Vec<String>,
    topic: Option<&str>,
    supersedes: Option<i64>,
) -> Result<Guardado> {
    let topic = topic.map(normalizar_tema).transpose()?;
    let tags = if tags_dados.is_empty() {
        crate::tags::classify(texto).map_err(|e| anyhow::anyhow!(e))?
    } else {
        tags_dados
    };
    let m = NewMemory {
        project: project.to_string(),
        title: titulo_de(texto),
        body: texto.to_string(),
        created: chrono::Local::now().format("%Y-%m-%d").to_string(),
        event_start: None,
        event_end: None,
        source: "explicit".into(),
        ext_id: None,
        tags: tags.clone(),
        refs: crate::tags::extract_refs(texto),
    };

    if topic.is_none() && supersedes.is_none() {
        let id = insert(conn, &m)?;
        return Ok(Guardado {
            id, project: project.to_string(), tags, topic: None,
            reemplazada: None, clave_nueva: false, otras_claves: Vec::new(),
        });
    }

    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let vigente: Option<(i64, String)> = match &topic {
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
    let explicita: Option<(i64, String)> = match supersedes {
        Some(sid) => {
            let fila: Option<(String, String)> = tx
                .query_row("select status, title from memories where id = ?1", [sid], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?;
            match fila {
                None => return Err(PedidoInvalido(format!("no existe la memoria id:{sid}")).into()),
                Some((st, _)) if st != "active" => {
                    return Err(PedidoInvalido(format!(
                        "la memoria id:{sid} no está activa (status={st}); no hay nada que reemplazar"
                    ))
                    .into())
                }
                Some((_, title)) => Some((sid, title)),
            }
        }
        None => None,
    };
    if let (Some((v, _)), Some((e, _))) = (&vigente, &explicita) {
        if v != e {
            return Err(PedidoInvalido(format!(
                "la clave «{}» ya tiene la memoria activa id:{v} y supersedes apunta a id:{e}: \
                 una memoria nueva reemplaza a una sola. Dejá solo `topic` para actualizar la \
                 vigente, o solo `supersedes` para corregir esa otra.",
                topic.as_deref().unwrap_or_default()
            ))
            .into());
        }
    }

    let clave_nueva = topic.is_some() && vigente.is_none();
    let objetivo = explicita.or(vigente);
    let id = insertar(&tx, &m)?;
    if let Some((vieja, _)) = &objetivo {
        crate::supersede::link(&tx, id, *vieja)?;
    }
    if let Some(t) = &topic {
        tx.execute("update memories set topic = ?2 where id = ?1", params![id, t])?;
    }
    let otras_claves = match (&topic, &objetivo) {
        (Some(t), None) => claves_activas(&tx, project, t)?,
        _ => Vec::new(),
    };
    tx.commit()?;
    Ok(Guardado {
        id,
        project: project.to_string(),
        tags,
        topic,
        reemplazada: objetivo.map(|(id, title)| Reemplazada { id, title }),
        clave_nueva,
        otras_claves,
    })
}

/// Las claves de tema con una memoria activa en `project`, menos `excepto`,
/// ordenadas. Se recorta a `MAX_CLAVES_LISTADAS`.
fn claves_activas(conn: &Connection, project: &str, excepto: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "select distinct topic from memories
          where project = ?1 and topic is not null and topic <> ?2 and status = 'active'
          order by topic limit ?3",
    )?;
    let claves = stmt
        .query_map(params![project, excepto, MAX_CLAVES_LISTADAS as i64 + 1], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(claves)
}

impl Guardado {
    /// Lo que pasó con el reemplazo y la clave de tema, en español, o `None`
    /// si no hubo ni una cosa ni la otra. Lo usan la CLI y MCP para que digan
    /// lo mismo.
    ///
    /// Si reemplazó, nombra la memoria con su id: un reemplazo silencioso es
    /// el tipo de magia que después nadie entiende. Si la clave era nueva y no
    /// reemplazó nada, lista las que ya existen en el proyecto: un error de
    /// tipeo produce el mismo "no había nada" que una clave legítimamente
    /// nueva, y sin la lista nadie puede distinguirlos. Cuesta texto solo en
    /// ese caso.
    pub fn describir(&self) -> Option<String> {
        let reemplazo = self.reemplazada.as_ref().map(|r| {
            format!(
                "reemplaza a la memoria {} («{}»), que queda superseded. `mem history {}` \
                 muestra la cadena.",
                r.id, r.title, self.id
            )
        });
        let Some(clave) = self.topic.as_deref() else {
            return reemplazo.map(|r| capitalizar(&r));
        };
        match (reemplazo, self.clave_nueva) {
            (Some(r), false) => Some(format!("Tema «{clave}»: {r}")),
            (Some(r), true) => Some(format!(
                "{} Tema «{clave}»: clave nueva; la próxima memoria con esa clave reemplazará a esta.",
                capitalizar(&r)
            )),
            (None, _) => {
                let mut msg = format!(
                    "Tema «{clave}»: no había una memoria activa con esa clave, se guardó como nueva."
                );
                if self.otras_claves.is_empty() {
                    msg.push_str(&format!(" No hay otras claves de tema en {}.", self.project));
                } else {
                    let hay_mas = self.otras_claves.len() > MAX_CLAVES_LISTADAS;
                    let lista: Vec<&str> = self.otras_claves.iter()
                        .take(MAX_CLAVES_LISTADAS).map(String::as_str).collect();
                    msg.push_str(&format!(
                        " Claves que ya existen en {}: {}{}. Si querías actualizar una de ellas, \
                         la clave no coincidió: guardá de nuevo con la exacta.",
                        self.project, lista.join(", "), if hay_mas { ", …" } else { "" }
                    ));
                }
                Some(msg)
            }
        }
    }
}

fn capitalizar(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(p) => p.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
