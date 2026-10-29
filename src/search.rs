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
    /// When `Some(n)`, each result's snippet is trimmed to `n` characters
    /// **before** its cost counts against `budget_tokens`. That way more
    /// short candidates fit in the same budget instead of a few full
    /// snippets. `None` keeps the long-standing behaviour: the whole snippet
    /// counts against the budget.
    pub excerpt: Option<usize>,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            text: String::new(),
            project: None,
            tags: Vec::new(),
            limit: 10,
            budget_tokens: DEFAULT_BUDGET_TOKENS,
            excerpt: None,
        }
    }
}

fn word_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\w+").unwrap())
}

/// Turns free text into a safe FTS5 MATCH expression. Each term is quoted
/// so characters like the `+` in "N+1" are not read as query syntax.
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

/// Builds and runs the candidate query against `chunks_fts`, with the
/// common filters (match, active status, project, tags) plus an optional
/// extra clause and limit.
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

    sql.push_str(&filters_for(q, &mut params));
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

/// A result with its raw signals exposed, for `--explain` and for anyone
/// who needs the number and not just the order. A result that got in by bm25
/// alone has no cosine, and one that got in by vector alone has no bm25.
#[derive(Debug, Clone)]
pub struct Explained {
    pub hit: SearchHit,
    pub bm25: Option<f64>,
    pub cosine: Option<f32>,
}

/// Which retrievers are available for this operation.
///
/// The fields are private and there is one constructor per use case because
/// the pack and the identity the database had it registered under cannot
/// come from two separate reads: pairing the pack from before with the
/// registry from after is precisely how one ends up writing a stale pack's
/// vectors. Whoever only searches uses `de_lectura` and has nothing to write
/// with; whoever also saves uses `de_servidor`, which receives the whole
/// pair exactly as `packstore::LivePack::current` hands it over.
pub struct SearchContext<'a> {
    pack: Option<&'a Pack>,
    identity: Option<&'a crate::vecstore::PackIdentity>,
    pub bm25_weight: f64,
}

impl SearchContext<'static> {
    /// Search with no vector layer: the long-standing behaviour.
    pub fn bm25_only() -> SearchContext<'static> {
        SearchContext { pack: None, identity: None, bm25_weight: 1.0 }
    }
}

impl<'a> SearchContext<'a> {
    /// For searching. Reading with the wrong pack gives a worse result, not
    /// a mixed index, so reads do not need the identity.
    pub fn for_reading(pack: Option<&'a Pack>, bm25_weight: f64) -> SearchContext<'a> {
        SearchContext { pack, identity: None, bm25_weight }
    }

    /// For the MCP server, which searches and also saves.
    pub fn for_server(
        active: Option<(&'a Pack, &'a crate::vecstore::PackIdentity)>,
        bm25_weight: f64,
    ) -> SearchContext<'a> {
        SearchContext { pack: active.map(|(p, _)| p), identity: active.map(|(_, i)| i), bm25_weight }
    }

    /// The pack to embed the query with, if there is one.
    pub fn pack(&self) -> Option<&Pack> {
        self.pack
    }

    /// The pair `vecstore::sync` demands. `None` in a read context even
    /// when a pack is present: there is nothing to write there.
    pub fn for_writing(&self) -> Option<(&Pack, &crate::vecstore::PackIdentity)> {
        match (self.pack, self.identity) {
            (Some(p), Some(i)) => Some((p, i)),
            _ => None,
        }
    }
}

/// The project and tags `WHERE`, shared by the lexical and the vector
/// search. It assumes the query has `memories` aliased as `m`.
///
/// It appends its parameters to `params` and returns the SQL fragment with
/// the `?N` numbering already resolved, so the caller can bring parameters
/// from before (the lexical search arrives with the `match` in `?1`).
///
/// It exists because this used to be written twice, in `search` and in
/// `vecstore`, each with its numbering done by hand. As long as they agreed
/// nothing showed. The day someone added a filter to `Query` and updated
/// only one, the vector side would have returned memories the lexical side
/// filters out, and no test would have seen it.
pub(crate) fn filters_for(q: &Query, params: &mut Vec<Value>) -> String {
    let mut sql = String::new();
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
    sql
}

/// Builds a `SearchHit` around one specific chunk. The merge uses it for
/// memories the vector side found and bm25 did not: for those we have no row
/// from the lexical pool, but we do know which chunk won.
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
    ctx: &SearchContext,
) -> Result<Vec<Explained>> {
    let m = to_match(&q.text);

    // --- recuperación léxica: idéntica a como era ---
    let mut best: HashMap<i64, (SearchHit, i64)> = HashMap::new();
    let mut orden_bm25: Vec<i64> = Vec::new();
    if !m.is_empty() {
        // Main pool: the CANDIDATES best chunks by bm25, pinned or not.
        let mut rows = candidate_rows(conn, &m, q, "", Some(CANDIDATES))?;
        // A separate, unlimited guarantee: every pinned memory matching the
        // query is always a candidate, even when its best chunk falls
        // outside the top CANDIDATES by pure bm25. Without this, `order by
        // score limit N` can leave out a relevant but verbose pinned memory
        // once the candidate pool exceeds N (in the real corpus, a common
        // connective like "que" already matches >700 chunks).
        rows.extend(candidate_rows(conn, &m, q, " and m.pinned = 1", None)?);

        // SQLite's bm25() returns negative values; more negative is better.
        // We deduplicate by keeping the best (most negative) chunk per
        // memory. The two queries above come concatenated, so keeping the
        // first appearance is not enough: they have to be compared
        // explicitly.
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
        // bm25() returns negatives: more negative is better.
        por_score.sort_by(|a, b| {
            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0))
        });
        orden_bm25 = por_score.into_iter().map(|(id, _)| id).collect();
    }

    // --- vector retrieval, when there is a pack ---
    let mut cosines: HashMap<i64, f32> = HashMap::new();
    let mut orden_vec: Vec<i64> = Vec::new();
    if let Some(p) = ctx.pack() {
        if !q.text.trim().is_empty() {
            let cv = p.embed(&q.text)?;
            // A null vector (a query with no tokens the pack knows: only
            // punctuation, an emoji, a script it does not cover) has cosine
            // 0 against everything, so `vecstore::search_vectors` would
            // return up to CANDIDATES arbitrary memories all tied at 0 —
            // garbage shaped like a result. With no real signal, there is
            // nothing to retrieve by vector.
            let nulo = cv.iter().all(|&x| x == 0.0);
            let vec_hits = if nulo { Vec::new() } else { crate::vecstore::search_vectors(conn, &cv, q, CANDIDATES)? };
            for a in vec_hits {
                cosines.insert(a.memory_id, a.cosine);
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
    let merged: Vec<i64> = if orden_vec.is_empty() {
        orden_bm25
    } else {
        crate::fusion::rrf(&orden_bm25, &orden_vec, ctx.bm25_weight)
    };

    // Pinned ones still come first, as always: the merge orders within each
    // group, it does not cut across them.
    let mut pinned_ids: Vec<i64> = Vec::new();
    let mut resto: Vec<i64> = Vec::new();
    for id in merged {
        match best.get(&id) {
            Some((_, 1)) => pinned_ids.push(id),
            Some(_) => resto.push(id),
            None => {}
        }
    }
    pinned_ids.extend(resto);

    let mut out = Vec::new();
    let mut spent = 0usize;
    for id in pinned_ids {
        if out.len() >= q.limit {
            break;
        }
        let (mut hit, _) = match best.remove(&id) {
            Some(v) => v,
            None => continue,
        };
        // The trim goes before computing `cost`: done after this loop, the
        // budget would already have cut the list down to the size full
        // snippets gave, and returning more short candidates would change
        // nothing. Trim by characters, not bytes: the corpus is in Spanish,
        // with accents, and cutting mid UTF-8 would break the snippet (see
        // `chunk::split`).
        if let Some(n) = q.excerpt {
            hit.snippet = hit.snippet.chars().take(n).collect();
        }
        let cost = est_tokens(&hit.title) + est_tokens(&hit.snippet);
        if !out.is_empty() && spent + cost > q.budget_tokens {
            break;
        }
        spent += cost;
        let bm25 = if hit.score < 0.0 { Some(hit.score) } else { None };
        let cosine = cosines.get(&id).copied();
        out.push(Explained { hit, bm25, cosine });
    }
    Ok(out)
}

pub fn run(conn: &Connection, q: &Query, ctx: &SearchContext) -> Result<Vec<SearchHit>> {
    Ok(run_explained(conn, q, ctx)?.into_iter().map(|e| e.hit).collect())
}
