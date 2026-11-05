use crate::model::SearchHit;
use crate::pack::Pack;
use anyhow::Result;
use regex::Regex;
use rusqlite::{types::Value, Connection, OptionalExtension};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const DEFAULT_BUDGET_TOKENS: usize = 1200;
const CANDIDATES: usize = 200;

/// Ceilings for what a caller may ask for (finding B-4).
///
/// `limit` and `budget` arrive straight off an MCP request and used to be
/// taken as given, so one call could ask for a billion candidates and a
/// budget to match. The caller is the local agent rather than a stranger,
/// so this is robustness and not a breach — but a model that writes
/// `limit: 100000` because it was feeling thorough should get a large
/// answer, not an unresponsive one.
///
/// `MAX_LIMIT` sits above `CANDIDATES` on purpose: the lexical pool is
/// capped at `CANDIDATES` anyway, and pinned memories come in over and
/// above it, so a legitimate caller can still ask for more than the pool.
pub const MAX_LIMIT: usize = 500;
pub const MAX_BUDGET_TOKENS: usize = 100_000;

/// The most terms `to_match` will put into one FTS5 expression.
///
/// Every term becomes a quoted `OR` branch, so without a ceiling the size
/// of the query SQLite has to plan is set by the length of the text handed
/// in. Fifty is far past any real query: the measurements behind
/// `Config::candidates` were run on queries of a handful of words.
const MAX_TERMS: usize = 50;

#[derive(Debug, Clone)]
pub struct Query {
    pub text: String,
    pub project: Option<String>,
    pub tags: Vec<String>,
    /// Narrows to one topic key. Like `project` and `tags`, it is a filter
    /// over `memories` and not a change to the ranking.
    pub topic: Option<String>,
    pub limit: usize,
    pub budget_tokens: usize,
    /// When `Some(n)`, each result's snippet is trimmed to `n` characters
    /// **before** its cost counts against `budget_tokens`. That way more
    /// short candidates fit in the same budget instead of a few full
    /// snippets. `None` keeps the long-standing behaviour: the whole snippet
    /// counts against the budget.
    pub excerpt: Option<usize>,
    /// Replaces a long memory's excerpt with a deterministic outline of the
    /// whole memory. Off by default, and below `OUTLINE_THRESHOLD`
    /// characters it changes nothing at all. See `outline`.
    pub outline: bool,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            text: String::new(),
            project: None,
            tags: Vec::new(),
            topic: None,
            limit: 10,
            budget_tokens: DEFAULT_BUDGET_TOKENS,
            excerpt: None,
            outline: false,
        }
    }
}

/// Below this many characters a memory is its own summary, and an outline
/// would spend tokens repeating what the excerpt already says. Above
/// `CHUNK_SIZE` on purpose: a memory that fits in one chunk has no "rest"
/// for the reader to wonder about.
pub const OUTLINE_THRESHOLD: usize = 800;

/// At most this many headings. A memory with forty sections would otherwise
/// cost more in outline than in body.
const OUTLINE_HEADINGS: usize = 6;

/// And at most this many characters of each heading.
const OUTLINE_HEADING_CHARS: usize = 60;

/// What a long memory looks like from outside: its first paragraph and the
/// headings underneath, computed from its own text.
///
/// The excerpt a candidate carries is a window on **one chunk**
/// (`ingest::CHUNK_SIZE`), so a memory with sections is represented by
/// whichever 500-character slice happened to match, and nothing in it says
/// whether the rest is relevant. This is the tier that would answer that,
/// and it is built the only way this project is willing to build it: no
/// model, nothing generated, nothing stored. The same body gives the same
/// outline forever, which is what keeps `mem export` reproducible.
///
/// Below `OUTLINE_THRESHOLD` the text comes back untouched: a short note is
/// already its own outline.
pub fn outline(body: &str, excerpt: usize) -> String {
    if body.chars().count() <= OUTLINE_THRESHOLD {
        return body.to_string();
    }
    let is_heading = |l: &str| l.trim_start().starts_with('#');
    let first: String = body
        .split("\n\n")
        .map(str::trim)
        .find(|p| !p.is_empty() && !is_heading(p))
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(excerpt)
        .collect();
    let headings: Vec<String> = body
        .lines()
        .filter(|l| is_heading(l))
        .map(|l| {
            l.trim_start_matches(['#', ' ']).trim().chars().take(OUTLINE_HEADING_CHARS).collect()
        })
        .filter(|h: &String| !h.is_empty())
        .take(OUTLINE_HEADINGS)
        .collect();
    if headings.is_empty() {
        // Nothing to index, so nothing is added: the acceptance criterion is
        // that a single-paragraph memory costs no more than it does today.
        return first;
    }
    format!("{first}\nsections: {}", headings.join(" / "))
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
        .take(MAX_TERMS)
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
    if let Some(t) = &q.topic {
        params.push(Value::Text(t.clone()));
        sql.push_str(&format!(" and m.topic = ?{}", params.len()));
    }
    sql
}

/// A narrowing written as `key:value`, which is what `--scope` and the MCP
/// `scope` argument both speak.
///
/// It adds no filter the database did not already support: `filters_for`
/// has built the project and tag `WHERE` since the vector side appeared,
/// and `topic` is one more column on the same table. What was missing was a
/// way to say it, and a way to compose several.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scopes {
    pub project: Option<String>,
    pub tags: Vec<String>,
    pub topic: Option<String>,
}

/// The keys a scope may name. Listed in every error, because a key that is
/// almost right is the likely mistake.
const SCOPE_KEYS: &str = "project, tag, topic";

/// Parses `key:value` scopes, the same way for the CLI and for MCP.
///
/// Nothing here is lenient. A scope that cannot be understood is a usage
/// error and never a filter quietly dropped: a dropped filter returns more
/// than was asked for, which looks like a working search, and whoever wrote
/// it never finds out. The same goes for two `project:` scopes — no memory
/// can be in two projects, so an AND of both matches nothing, and failing is
/// more useful than an empty answer whose reason is invisible.
pub fn parse_scopes(raw: &[String]) -> anyhow::Result<Scopes> {
    let mut out = Scopes::default();
    for one in raw {
        let one = one.trim();
        let Some((key, value)) = one.split_once(':') else {
            anyhow::bail!("scope \"{one}\" has no `:`; it is written key:value, with key one of {SCOPE_KEYS}");
        };
        let (key, value) = (key.trim(), value.trim());
        if value.is_empty() {
            anyhow::bail!("scope \"{one}\" has no value after the `:`");
        }
        match key {
            "project" => {
                if out.project.as_deref().is_some_and(|p| p != value) {
                    anyhow::bail!(
                        "two project scopes cannot both be satisfied: a memory belongs to one project"
                    );
                }
                out.project = Some(value.to_string());
            }
            "topic" => {
                if out.topic.as_deref().is_some_and(|t| t != value) {
                    anyhow::bail!(
                        "two topic scopes cannot both be satisfied: a memory carries one topic key"
                    );
                }
                out.topic = Some(value.to_string());
            }
            "tag" => out.tags.push(value.to_string()),
            other => anyhow::bail!("unknown scope key \"{other}\"; it is one of {SCOPE_KEYS}"),
        }
    }
    Ok(out)
}

impl Scopes {
    /// Lays the scopes over a query, keeping the filters it already carries.
    ///
    /// A `project` or `topic` already set another way — `--project`, the MCP
    /// `project` argument — and a scope naming a different one cannot both
    /// hold. Choosing one of them silently would make a flag do nothing, so
    /// it fails and says which two disagree.
    pub fn onto(self, q: Query) -> anyhow::Result<Query> {
        let project = match (q.project, self.project) {
            (Some(a), Some(b)) if a != b => anyhow::bail!(
                "the search asks for project \"{a}\" and the scope for \"{b}\"; a memory belongs to one project"
            ),
            (a, b) => b.or(a),
        };
        let topic = match (q.topic, self.topic) {
            (Some(a), Some(b)) if a != b => anyhow::bail!(
                "the search asks for topic \"{a}\" and the scope for \"{b}\"; a memory carries one topic key"
            ),
            (a, b) => b.or(a),
        };
        let mut tags = q.tags;
        tags.extend(self.tags);
        Ok(Query { project, topic, tags, ..q })
    }
}

/// The query's filters written back in scope syntax, for a message that has
/// to name what came out empty.
pub fn describe_scope(q: &Query) -> String {
    let mut parts = Vec::new();
    if let Some(p) = &q.project {
        parts.push(format!("project:{p}"));
    }
    for t in &q.tags {
        parts.push(format!("tag:{t}"));
    }
    if let Some(t) = &q.topic {
        parts.push(format!("topic:{t}"));
    }
    parts.join(", ")
}

/// Whether the filters alone, with no query text, match any active memory.
///
/// "No results for this query" and "that scope is empty" are different
/// answers, and only one of them means rephrasing would help.
pub fn scope_is_empty(conn: &Connection, q: &Query) -> Result<bool> {
    if describe_scope(q).is_empty() {
        return Ok(false);
    }
    let mut params: Vec<Value> = Vec::new();
    let filters = filters_for(q, &mut params);
    let sql = format!("select count(*) from memories m where m.status = 'active'{filters}");
    let n: i64 = conn.query_row(&sql, rusqlite::params_from_iter(params), |r| r.get(0))?;
    Ok(n == 0)
}

/// Builds a `SearchHit` around one specific chunk. The merge uses it for
/// memories the vector side found and bm25 did not: for those we have no row
/// from the lexical pool, but we do know which chunk won.
fn hit_from_chunk(conn: &Connection, chunk_id: i64) -> Result<Option<(SearchHit, i64)>> {
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
        // The cap is `CANDIDATES` and not `None` (finding B-4). The
        // guarantee this query exists for is unchanged in practice —
        // pinning is a deliberate act, so no real corpus has two hundred
        // pinned memories matching one query — but a query with no `LIMIT`
        // at all makes the work SQLite does depend on the size of the
        // corpus rather than on what was asked for.
        rows.extend(candidate_rows(conn, &m, q, " and m.pinned = 1", Some(CANDIDATES))?);

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
                    if let Some(pair) = hit_from_chunk(conn, a.chunk_id)? {
                        best.insert(a.memory_id, pair);
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
        if q.outline {
            // From the whole memory and not from the chunk that won: the
            // point is to say what the rest holds, and the chunk is the
            // part we already know matched.
            if let Some(body) = memory_body(conn, hit.memory_id)? {
                hit.snippet = outline(&body, q.excerpt.unwrap_or(usize::MAX));
            }
        }
        if let Some(n) = q.excerpt {
            // An outline is already bounded by its own rules; trimming it
            // here would cut the headings off the end, which is the half
            // that is not in the excerpt to begin with.
            if !q.outline {
                hit.snippet = hit.snippet.chars().take(n).collect();
            }
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

/// A memory's whole body, for the outline. One row per candidate, which is
/// twenty rows on the default settings.
fn memory_body(conn: &Connection, memory_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row("select body from memories where id = ?1", [memory_id], |r| r.get(0))
        .optional()?)
}

pub fn run(conn: &Connection, q: &Query, ctx: &SearchContext) -> Result<Vec<SearchHit>> {
    Ok(run_explained(conn, q, ctx)?.into_iter().map(|e| e.hit).collect())
}
