use crate::model::SearchHit;
use crate::pack::Pack;
use anyhow::Result;
use regex::Regex;
use rusqlite::{types::Value, Connection};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const DEFAULT_BUDGET_TOKENS: usize = 1200;
const CANDIDATES: usize = 200;

#[derive(Debug, Clone)]
pub struct Query {
    pub text: String,
    pub project: Option<String>,
    pub tags: Vec<String>,
    pub limit: usize,
    pub budget_tokens: usize,
    /// Si es `Some(n)`, el fragmento de cada resultado se recorta a `n`
    /// caracteres **antes** de contar su costo contra `budget_tokens`. Así
    /// entran más candidatos breves en el mismo presupuesto en vez de pocos
    /// fragmentos completos. `None` conserva el comportamiento de siempre:
    /// el fragmento entero cuenta contra el presupuesto.
    pub extracto: Option<usize>,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            text: String::new(),
            project: None,
            tags: Vec::new(),
            limit: 10,
            budget_tokens: DEFAULT_BUDGET_TOKENS,
            extracto: None,
        }
    }
}

fn word_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\w+").unwrap())
}

/// Convierte texto libre en una expresión MATCH de FTS5 segura.
/// Cada término va entre comillas para que caracteres como `+` de "N+1"
/// no se interpreten como sintaxis de consulta.
pub fn to_match(text: &str) -> String {
    let terms: Vec<String> = word_re()
        .find_iter(text)
        .map(|m| format!("\"{}\"", m.as_str().replace('"', "")))
        .collect();
    terms.join(" OR ")
}

fn est_tokens(s: &str) -> usize {
    s.chars().count() / 4
}

type Row = (i64, Option<String>, String, String, String, f64, i64);

/// Arma y ejecuta la consulta de candidatos contra `chunks_fts`, con los
/// filtros comunes (match, status activo, proyecto, tags) más una cláusula
/// extra y un límite opcionales.
fn candidate_rows(
    conn: &Connection,
    match_expr: &str,
    q: &Query,
    extra_where: &str,
    limit: Option<usize>,
) -> Result<Vec<Row>> {
    let mut sql = String::from(
        "select m.id, m.ext_id, m.project, m.title, c.body, bm25(chunks_fts) as score, m.pinned
           from chunks_fts
           join chunks c on c.id = chunks_fts.rowid
           join memories m on m.id = c.memory_id
          where chunks_fts match ?1
            and m.status = 'active'",
    );
    let mut params: Vec<Value> = vec![Value::Text(match_expr.to_string())];

    if let Some(p) = &q.project {
        params.push(Value::Text(p.clone()));
        sql.push_str(&format!(" and m.project = ?{}", params.len()));
    }
    for t in &q.tags {
        params.push(Value::Text(t.clone()));
        sql.push_str(&format!(
            " and exists (select 1 from memory_tags mt where mt.memory_id = m.id and mt.tag = ?{})",
            params.len()
        ));
    }
    sql.push_str(extra_where);
    sql.push_str(" order by score");
    if let Some(l) = limit {
        params.push(Value::Integer(l as i64));
        sql.push_str(&format!(" limit ?{}", params.len()));
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, f64>(5)?,
            r.get::<_, i64>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Resultado con las señales crudas expuestas, para `--explain` y para quien
/// necesite el número y no solo el orden. Un resultado que entró solo por
/// bm25 no tiene coseno, y uno que entró solo por vector no tiene bm25.
#[derive(Debug, Clone)]
pub struct Explained {
    pub hit: SearchHit,
    pub bm25: Option<f64>,
    pub coseno: Option<f32>,
}

/// Qué recuperadores están disponibles para esta operación.
///
/// Los campos son privados y hay un constructor por caso de uso porque el
/// paquete y la identidad con la que la base lo tenía registrado no pueden
/// venir de dos lecturas distintas: emparejar el paquete de antes con el
/// registro de después es justamente la forma de terminar escribiendo
/// vectores de un paquete caducado. Quien solo busca usa `de_lectura` y no
/// tiene con qué escribir; quien además guarda usa `de_servidor`, que
/// recibe el par entero tal como lo entrega `paquete::PaqueteVivo::actual`.
pub struct Contexto<'a> {
    pack: Option<&'a Pack>,
    identidad: Option<&'a crate::vecstore::Identidad>,
    pub peso_bm25: f64,
}

impl Contexto<'static> {
    /// Búsqueda sin capa vectorial: el comportamiento histórico.
    pub fn solo_bm25() -> Contexto<'static> {
        Contexto { pack: None, identidad: None, peso_bm25: 1.0 }
    }
}

impl<'a> Contexto<'a> {
    /// Para buscar. Leer con el paquete equivocado da un resultado peor, no
    /// un índice mezclado, así que la lectura no necesita la identidad.
    pub fn de_lectura(pack: Option<&'a Pack>, peso_bm25: f64) -> Contexto<'a> {
        Contexto { pack, identidad: None, peso_bm25 }
    }

    /// Para el servidor MCP, que busca y también guarda.
    pub fn de_servidor(
        activo: Option<(&'a Pack, &'a crate::vecstore::Identidad)>,
        peso_bm25: f64,
    ) -> Contexto<'a> {
        Contexto { pack: activo.map(|(p, _)| p), identidad: activo.map(|(_, i)| i), peso_bm25 }
    }

    /// El paquete con el que embeber la consulta, si hay.
    pub fn pack(&self) -> Option<&Pack> {
        self.pack
    }

    /// El par que exige `vecstore::sincronizar`. `None` en un contexto de
    /// lectura aunque tenga paquete: ahí no hay nada que escribir.
    pub fn para_escribir(&self) -> Option<(&Pack, &crate::vecstore::Identidad)> {
        match (self.pack, self.identidad) {
            (Some(p), Some(i)) => Some((p, i)),
            _ => None,
        }
    }
}

/// Trae un `SearchHit` armado alrededor de un fragmento concreto. Lo usa la
/// fusión para las memorias que encontró el vector y bm25 no: de esas no
/// tenemos fila del pool léxico, pero sí sabemos qué fragmento ganó.
fn hit_de_chunk(conn: &Connection, chunk_id: i64) -> Result<Option<(SearchHit, i64)>> {
    let r = conn.query_row(
        "select m.id, m.ext_id, m.project, m.title, c.body, m.pinned
           from chunks c join memories m on m.id = c.memory_id
          where c.id = ?1",
        [chunk_id],
        |r| {
            Ok((
                SearchHit {
                    memory_id: r.get(0)?,
                    ext_id: r.get(1)?,
                    project: r.get(2)?,
                    title: r.get(3)?,
                    snippet: r.get(4)?,
                    score: 0.0,
                },
                r.get::<_, i64>(5)?,
            ))
        },
    );
    match r {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn run_explained(
    conn: &Connection,
    q: &Query,
    ctx: &Contexto,
) -> Result<Vec<Explained>> {
    let m = to_match(&q.text);

    // --- recuperación léxica: idéntica a como era ---
    let mut best: HashMap<i64, (SearchHit, i64)> = HashMap::new();
    let mut orden_bm25: Vec<i64> = Vec::new();
    if !m.is_empty() {
        // Pool principal: los CANDIDATES mejores chunks por bm25, sin importar pinned.
        let mut rows = candidate_rows(conn, &m, q, "", Some(CANDIDATES))?;
        // Garantía aparte, sin límite: toda memoria pinned que matchee el query es
        // candidata siempre, aunque su mejor chunk caiga fuera del top-CANDIDATES
        // por bm25 puro. Sin esto, `order by score limit N` puede dejar afuera a
        // una pinned relevante pero verbosa cuando el pool de candidatos supera N
        // (en el corpus real, un conector común como "que" ya matchea >700 chunks).
        rows.extend(candidate_rows(conn, &m, q, " and m.pinned = 1", None)?);

        // bm25() de SQLite devuelve valores negativos, más negativo es mejor.
        // Deduplicamos quedándonos con el mejor (más negativo) chunk por memoria.
        // Las dos consultas de arriba vienen concatenadas, así que no alcanza con
        // quedarse con la primera aparición: hay que comparar explícitamente.
        for (id, ext_id, project, title, body, score, pinned) in rows {
            match best.entry(id) {
                Entry::Occupied(mut e) => {
                    if score < e.get().0.score {
                        e.get_mut().0.snippet = body;
                        e.get_mut().0.score = score;
                    }
                }
                Entry::Vacant(e) => {
                    e.insert((
                        SearchHit { memory_id: id, ext_id, project, title, snippet: body, score },
                        pinned,
                    ));
                }
            }
        }
        let mut por_score: Vec<(i64, f64)> =
            best.iter().map(|(id, (h, _))| (*id, h.score)).collect();
        // bm25() devuelve negativos: más negativo es mejor.
        por_score.sort_by(|a, b| {
            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0))
        });
        orden_bm25 = por_score.into_iter().map(|(id, _)| id).collect();
    }

    // --- recuperación vectorial, si hay paquete ---
    let mut cosenos: HashMap<i64, f32> = HashMap::new();
    let mut orden_vec: Vec<i64> = Vec::new();
    if let Some(p) = ctx.pack() {
        if !q.text.trim().is_empty() {
            let cv = p.embeber(&q.text)?;
            // Un vector nulo (consulta sin tokens conocidos por el paquete:
            // solo puntuación, un emoji, una escritura que no cubre) tiene
            // coseno 0 contra todo, así que `vecstore::buscar` devolvería
            // hasta CANDIDATES memorias arbitrarias empatadas en 0 -- basura
            // con apariencia de resultado. Sin señal real, no hay nada que
            // recuperar por vector.
            let nulo = cv.iter().all(|&x| x == 0.0);
            let vec_hits = if nulo { Vec::new() } else { crate::vecstore::buscar(conn, &cv, q, CANDIDATES)? };
            for a in vec_hits {
                cosenos.insert(a.memory_id, a.coseno);
                orden_vec.push(a.memory_id);
                if !best.contains_key(&a.memory_id) {
                    if let Some(par) = hit_de_chunk(conn, a.chunk_id)? {
                        best.insert(a.memory_id, par);
                    }
                }
            }
        }
    }

    if best.is_empty() {
        return Ok(Vec::new());
    }

    // --- orden final ---
    let fusionado: Vec<i64> = if orden_vec.is_empty() {
        orden_bm25
    } else {
        crate::fusion::rrf(&orden_bm25, &orden_vec, ctx.peso_bm25)
    };

    // Las fijadas siguen primero, como siempre: la fusión ordena dentro de
    // cada grupo, no lo atraviesa.
    let mut fijadas: Vec<i64> = Vec::new();
    let mut resto: Vec<i64> = Vec::new();
    for id in fusionado {
        match best.get(&id) {
            Some((_, 1)) => fijadas.push(id),
            Some(_) => resto.push(id),
            None => {}
        }
    }
    fijadas.extend(resto);

    let mut out = Vec::new();
    let mut spent = 0usize;
    for id in fijadas {
        if out.len() >= q.limit {
            break;
        }
        let (mut hit, _) = match best.remove(&id) {
            Some(v) => v,
            None => continue,
        };
        // El recorte va antes de calcular `cost`: si se hiciera después de
        // este bucle, el presupuesto ya habría cortado la lista al tamaño
        // que daban los fragmentos completos, y devolver más candidatos
        // breves no cambiaría nada. Recortar por caracteres, no bytes: el
        // corpus es en español con acentos y un corte a mitad de un UTF-8
        // rompería el fragmento (ver `chunk::split`).
        if let Some(n) = q.extracto {
            hit.snippet = hit.snippet.chars().take(n).collect();
        }
        let cost = est_tokens(&hit.title) + est_tokens(&hit.snippet);
        if !out.is_empty() && spent + cost > q.budget_tokens {
            break;
        }
        spent += cost;
        let bm25 = if hit.score < 0.0 { Some(hit.score) } else { None };
        let coseno = cosenos.get(&id).copied();
        out.push(Explained { hit, bm25, coseno });
    }
    Ok(out)
}

pub fn run(conn: &Connection, q: &Query, ctx: &Contexto) -> Result<Vec<SearchHit>> {
    Ok(run_explained(conn, q, ctx)?.into_iter().map(|e| e.hit).collect())
}
