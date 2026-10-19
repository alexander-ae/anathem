use crate::{model::SearchHit, search};
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

/// Memorias activas del mismo proyecto más parecidas a `body`.
///
/// Se llama justo después de insertar la memoria nueva, así que ella misma
/// aparece entre los hits y hay que excluirla. Filtrar por "cuerpo idéntico"
/// (como sugiere el enfoque ingenuo) no alcanza en general: si el corpus
/// tuviera duplicados legítimos activos del mismo proyecto con cuerpo
/// idéntico entre sí, y luego se guardara un tercero con ese mismo texto,
/// excluir por contenido descartaría *todos* los que matchean -- los
/// duplicados preexistentes incluidos -- en vez de proponerlos, justo el
/// caso que esta función existe para detectar.
///
/// (En un corpus migrado real, el único par realmente byte-idéntico
/// resultó estar en dos proyectos distintos, así que `candidates()`, que
/// siempre filtra por un único proyecto, nunca los vio juntos en una misma
/// llamada. Una consulta sobre todo ese corpus por pares de cuerpo idéntico
/// dentro del mismo proyecto con ambos activos no devolvió filas: no había
/// un caso real que ejercitara el argumento de arriba. Es una salvaguarda
/// contra un escenario que ese corpus no tenía todavía, no una corrección de
/// un bug observado.)
///
/// Por eso excluimos por id en vez de por contenido: entre los hits cuyo
/// cuerpo es idéntico al que se está guardando, el de mayor id es el que se
/// acaba de insertar (los ids son autoincrementales y esta función se llama
/// inmediatamente después del insert), así que es el único que se descarta.
/// Los duplicados preexistentes, si los hubiera, quedan como candidatos.
pub fn candidates(conn: &Connection, body: &str, project: &str, k: usize) -> Result<Vec<SearchHit>> {
    let q = search::Query {
        text: body.chars().take(400).collect(),
        project: Some(project.to_string()),
        tags: Vec::new(),
        limit: k + 1,
        budget_tokens: usize::MAX,
        // Esto compara cuerpos para detectar posibles duplicados, no arma
        // una respuesta para mostrar: sin recorte.
        extracto: None,
    };
    // Se llama justo después de un insert, cuando el vector de la memoria
    // recién guardada puede no estar sincronizado todavía: el índice
    // vectorial estaría incompleto justo en el caso que le importa a esta
    // función. Por eso busca solo con bm25, sin el paquete.
    let mut hits = search::run(conn, &q, &search::Contexto::solo_bm25())?;

    let self_id = hits
        .iter()
        .filter(|h| {
            let same: rusqlite::Result<String> = conn.query_row(
                "select body from memories where id = ?1",
                [h.memory_id],
                |r| r.get(0),
            );
            matches!(same, Ok(b) if b == body)
        })
        .map(|h| h.memory_id)
        .max();
    if let Some(self_id) = self_id {
        hits.retain(|h| h.memory_id != self_id);
    }

    hits.truncate(k);
    Ok(hits)
}

/// Fija `supersedes_id` en `nueva` y marca `vieja` como `superseded`, para
/// que deje de aparecer en `search::run` (que ya filtra por status activo).
///
/// Exige que `nueva` exista y esté `active` antes de tocar nada. Lo primero
/// evita que un `nueva` inexistente pase silencioso: el primer `update` de
/// abajo hace `where id = nueva`, y si no matchea ninguna fila, SQLite no
/// levanta error -- solo el segundo `update` corría, dejando `vieja`
/// superseded y apuntando a nada. Lo segundo (exigir `active`) cierra el
/// camino a un ciclo: si `nueva` ya está superseded, es porque alguna otra
/// llamada a `link` ya la usó como `vieja` -- dejarla encadenar de nuevo como
/// `nueva` es como se cierra un ciclo (A supersede a B, B supersede a A),
/// que deja sin ninguna versión activa y sería un bucle infinito para quien
/// recorra la cadena de `supersedes_id` más adelante. Un `vieja` inexistente
/// ya lo atajaba la foreign key de `supersedes_id` sin necesidad de chequeo
/// explícito.
///
/// Si el llamador ya tenía una transacción abierta, `link` no abre otra y no
/// hace commit: los dos `update` son parte de la de él. Si el segundo falla,
/// el primero queda aplicado hasta que el llamador haga rollback (o suelte la
/// transacción sin commit); `link` no lo deshace por su cuenta. Quien la
/// llama dentro de una transacción tiene que propagar el error y no hacer
/// commit.
pub fn link(conn: &Connection, nueva: i64, vieja: i64) -> Result<()> {
    if nueva == vieja {
        anyhow::bail!("una memoria no puede reemplazarse a sí misma");
    }
    let estado_nueva: Option<String> = conn
        .query_row("select status from memories where id = ?1", [nueva], |r| r.get(0))
        .optional()?;
    match estado_nueva.as_deref() {
        None => anyhow::bail!("no existe la memoria nueva id:{nueva}"),
        Some("active") => {}
        Some(otro) => anyhow::bail!(
            "la memoria nueva id:{nueva} no está activa (status={otro}); no puede reemplazar \
             a otra memoria -- si ya fue superseded, esto formaría un ciclo"
        ),
    }
    // Si el llamador ya abrió una transacción (el guardado con clave de tema
    // inserta y supersede como una sola unidad), no se abre otra: SQLite no
    // anida `BEGIN`. Entonces los dos `update` quedan dentro de la de él, y
    // si alguno falla se deshace todo junto con su insert.
    let propia = if conn.is_autocommit() { Some(conn.unchecked_transaction()?) } else { None };
    conn.execute("update memories set supersedes_id = ?2 where id = ?1", params![nueva, vieja])?;
    conn.execute(
        "update memories set status = 'superseded', updated = ?2 where id = ?1",
        params![vieja, chrono::Local::now().format("%Y-%m-%d").to_string()],
    )?;
    if let Some(tx) = propia {
        tx.commit()?;
    }
    Ok(())
}
