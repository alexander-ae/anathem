use crate::model::SearchHit;
use crate::pack::Pack;
use anyhow::Result;
use regex::Regex;
use rusqlite::{types::Value, Connection, OptionalExtension};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const DEFAULT_BUDGET_TOKENS: usize = 1200;
/// How many chunks each retriever may nominate before interpretation: the
/// pool that is deduplicated, fused and only then trimmed to the twenty the
/// agent sees. It was a constant; `retrieval_depth` in `config.json` moves
/// it, and `mem tune --depth` measures whether 200 is the right number.
pub const DEFAULT_RETRIEVAL_DEPTH: usize = 200;

/// The ceiling a `config.json` may set. It exists so a typo cannot ask the
/// ranking to hold a million candidates: the pool is work done on every
/// search, and beyond a few thousand it is memory spent for no new evidence.
pub const MAX_RETRIEVAL_DEPTH: usize = 2000;

/// Ceilings for what a caller may ask for (finding B-4).
///
/// `limit` and `budget` arrive straight off an MCP request and used to be
/// taken as given, so one call could ask for a billion candidates and a
/// budget to match. The caller is the local agent rather than a stranger,
/// so this is robustness and not a breach — but a model that writes
/// `limit: 100000` because it was feeling thorough should get a large
/// answer, not an unresponsive one.
///
/// `MAX_LIMIT` sits above the retrieval depth on purpose: the lexical pool
/// is capped there anyway, and pinned memories come in over and above it, so
/// a legitimate caller can still ask for more than the pool.
pub const MAX_LIMIT: usize = 500;
pub const MAX_BUDGET_TOKENS: usize = 100_000;

/// The most terms `to_match` will put into one FTS5 expression.
///
/// Every term becomes a quoted `OR` branch, so without a ceiling the size
/// of the query SQLite has to plan is set by the length of the text handed
/// in. Fifty is far past any real query: the measurements behind
/// `Config::candidates` were run on queries of a handful of words.
const MAX_TERMS: usize = 50;

/// Below this many chunks, document frequency says nothing about whether a
/// word is common. In a corpus of ten notes a word in six of them is not a
/// stopword, it is a small corpus, and discarding it would throw away the
/// only evidence there is. Real corpora are orders of magnitude past this,
/// so the floor only ever applies to a database that has just been created.
const MIN_CHUNKS_FOR_DF: usize = 20;

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
    /// Lower bound on when the memory was written. Like the other filters,
    /// it narrows the candidates and does not touch the ranking.
    pub since: Option<DateBound>,
    /// Upper bound on when the memory was written, exclusive at the instant
    /// it names — a bare date names the whole day, so the day is included.
    pub until: Option<DateBound>,
    /// Answers what was true on that date instead of what is true now: the
    /// filter is the memory's validity interval, not its status, so a version
    /// that has since been superseded comes back if it covered the day.
    pub as_of: Option<String>,
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
            since: None,
            until: None,
            as_of: None,
        }
    }
}

/// A bound on when a memory was written: what the person typed, and the
/// string SQL compares the stored timestamp against.
///
/// Both halves are kept because both are needed. The comparison string is
/// what the query runs on; the text is what a message about an empty window
/// has to say back, and `since:2026-11-03T09:12:44-05:00` is not an answer to
/// somebody who typed `7d`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateBound {
    /// As it was written: `7d`, `2026-09-01`, a full timestamp.
    pub text: String,
    /// The value the stored date is compared against.
    pub at: String,
    /// The SQL the comparison runs on. See `WHEN_IT_IS_FROM` and
    /// `WHEN_IT_ARRIVED` for why there are two of them.
    column: &'static str,
}

/// The date the memory itself claims.
///
/// What a date bound asks about. The two columns are not two precisions of
/// one fact: `created` is the date the memory is from and `created_ts` is
/// when the row was written here, and for anything that came from somewhere
/// else those are different days. `mem migrate` stamps `created_ts` with the
/// instant of the migration while keeping the original date in `created`, so
/// a whole imported corpus reads as "written today" through the timestamp —
/// and `mem search --until 2026-09-01` over it would answer "nothing", which
/// is the one answer a question about the past must not get wrong.
const WHEN_IT_IS_FROM: &str = "m.created";

/// When the row was written here, to the second.
///
/// What a window like `6h` or `90m` asks about: not "which era is this
/// memory from" but "what has happened around here lately", which is the
/// same question `mem recent` answers and the same expression it orders by.
/// `created_ts` is null for everything saved before the column existed and
/// for everything `mem import` brought in, hence the coalesce.
const WHEN_IT_ARRIVED: &str = "coalesce(m.created_ts, m.created)";

/// A moment named as a window back from now, as a date, or as a timestamp.
enum Moment {
    /// A whole day. Its start is what `--since` means by it and the start of
    /// the next day is what `--until` means, so that `--until 2026-09-01`
    /// includes the first of September rather than stopping at its midnight.
    Day(chrono::NaiveDate),
    /// An exact instant, already in the format the stored timestamps use.
    Instant(String),
}

/// Reads `7d`, `2026-09-01` or a full RFC 3339 timestamp.
///
/// One parser for both ends on purpose: somebody who learns `--since 7d`
/// should not find out that `--until 7d` is a usage error, and the other way
/// round. A value it cannot read is an error and never a filter quietly
/// dropped — a dropped date filter answers with the wrong era and looks like
/// it worked, which is the same reasoning `parse_scopes` follows.
fn moment(text: &str) -> Result<Moment> {
    let t = text.trim();
    if let Ok(d) = chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        return Ok(Moment::Day(d));
    }
    if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(t) {
        return Ok(Moment::Instant(ts.to_rfc3339()));
    }
    let seconds = crate::recent::duration_in_seconds(t).map_err(|_| {
        anyhow::anyhow!(
            "cannot read the date «{text}»; try a window like 7d, 6h or 90m, \
             a date like 2026-09-01, or a full timestamp"
        )
    })?;
    Ok(Moment::Instant(crate::recent::since(seconds)))
}

/// The keys that mean "no bound at all", the same two `mem recent` accepts.
const UNBOUNDED: [&str; 2] = ["all", "todo"];

/// The lower bound `--since` names, or `None` when it names no bound.
pub fn since_bound(text: &str) -> Result<Option<DateBound>> {
    let t = text.trim();
    if UNBOUNDED.iter().any(|k| t.eq_ignore_ascii_case(k)) {
        return Ok(None);
    }
    let (at, column) = match moment(t)? {
        Moment::Day(d) => (d.format("%Y-%m-%d").to_string(), WHEN_IT_IS_FROM),
        Moment::Instant(s) => (s, WHEN_IT_ARRIVED),
    };
    Ok(Some(DateBound { text: t.to_string(), at, column }))
}

/// The upper bound `--until` names, exclusive, or `None` for no bound.
pub fn until_bound(text: &str) -> Result<Option<DateBound>> {
    let t = text.trim();
    if UNBOUNDED.iter().any(|k| t.eq_ignore_ascii_case(k)) {
        return Ok(None);
    }
    let (at, column) = match moment(t)? {
        // The start of the next day, so the day that was named is inside the
        // window. `--until 2026-09-01` is read as "up to and including the
        // first of September", which is what it says out loud.
        Moment::Day(d) => (
            d.succ_opt()
                .ok_or_else(|| anyhow::anyhow!("the date «{text}» has no next day"))?
                .format("%Y-%m-%d")
                .to_string(),
            WHEN_IT_IS_FROM,
        ),
        Moment::Instant(s) => (s, WHEN_IT_ARRIVED),
    };
    Ok(Some(DateBound { text: t.to_string(), at, column }))
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
/// `None` when there is nothing to outline, and then the candidate keeps the
/// excerpt it would have had. That happens below `OUTLINE_THRESHOLD` — a
/// short note is already its own outline — and, more importantly, when the
/// memory has no headings at all.
///
/// That second case is the one a measurement settled. Over LongMemEval-S,
/// 98.3% of memories clear the threshold and only 1.5% of those carry a
/// single heading: they are chat transcripts, not documents. Returning the
/// first paragraph for the rest would have replaced the text that **matched
/// the query** with the memory's opening lines, at the same token cost and
/// with less to go on. An outline is worth paying for when there is
/// structure to report; where there is none, the chunk that won is still
/// the best thing to show.
pub fn outline(body: &str, excerpt: usize) -> Option<String> {
    if body.chars().count() <= OUTLINE_THRESHOLD {
        return None;
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
        return None;
    }
    Some(format!("{first}\nsections: {}", headings.join(" / ")))
}

fn word_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\w+").unwrap())
}

/// The query's distinct terms, each already quoted as its own FTS5 MATCH
/// expression. Quoted so characters like the `+` in "N+1" are not read as
/// query syntax.
///
/// Distinct by the lowercased term: asking for `de de de` is asking about
/// one word, and counting it three times would make a query look better
/// matched the more it repeated itself. The index also folds diacritics
/// (`remove_diacritics 2`) and this does not, so `política` and `politica`
/// survive here as two terms where the index has one. That is the harmless
/// direction: it can only make a query look longer than it is, never make a
/// word that matched nothing look like it matched.
pub fn match_terms(text: &str) -> Vec<String> {
    // The index holds CJK as bigrams (`chunk::segment_cjk`), so the query
    // has to become bigrams too. For text with no CJK in it this is the
    // identity and the expression is exactly what it always was.
    let text = &crate::chunk::segment_cjk(text);
    let mut seen = std::collections::HashSet::new();
    word_re()
        .find_iter(text)
        .take(MAX_TERMS)
        .map(|m| m.as_str().replace('"', ""))
        .filter(|t| seen.insert(t.to_lowercase()))
        .map(|t| format!("\"{t}\""))
        .collect()
}

/// Identifier-looking spans: alphanumeric runs joined by a separator that
/// `unicode61` treats as a word boundary. `OP-204`, `v0.8.0`, `n+1`,
/// `mem_eval`, a URL.
fn identifier_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[A-Za-z0-9]+(?:[-._+/:@][A-Za-z0-9]+)+").unwrap())
}

/// Parameters for the optional rescoring pass.
///
/// FTS5's `bm25()` hardcodes `k1 = 1.2` and `b = 0.75`. When any of these is
/// set in `config.json`, the candidate pool is rescored with the formula
/// below instead, which is the only way to vary them without leaving FTS5.
/// The default is `None`, and then FTS5 keeps scoring exactly as it did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bm25Params {
    pub k1: f64,
    pub b: f64,
    /// BM25+'s lower bound on the saturated term weight. Zero is plain BM25.
    pub delta: f64,
}

impl Default for Bm25Params {
    fn default() -> Self {
        Self { k1: 1.2, b: 0.75, delta: 0.0 }
    }
}

/// The average length, in tokens, of a chunk. Chunks are cut at 500
/// characters with a 400-character step, so this is the one corpus statistic
/// the rescoring pass does not have to measure by scanning every row.
const AVG_CHUNK_TOKENS: f64 = 125.0;

/// Fold the accents `unicode61 remove_diacritics 2` folds, so a query and a
/// body compare equal on the same letters. It covers the Latin-1 range this
/// corpus is written in; the candidate set still comes from FTS5, so a
/// character this misses costs ranking precision and never recall.
fn fold_diacritic(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'ē' => 'e',
        'í' | 'ì' | 'î' | 'ï' | 'ī' => 'i',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ō' => 'o',
        'ú' | 'ù' | 'û' | 'ü' | 'ū' => 'u',
        'ñ' => 'n',
        'ç' => 'c',
        'ý' | 'ÿ' => 'y',
        _ => c,
    }
}

/// The candidate's tokens and their counts, folded the same way the query is.
fn token_counts(text: &str) -> (HashMap<String, usize>, usize) {
    let mut counts = HashMap::new();
    let mut total = 0usize;
    for token in text.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()) {
        let folded: String = token.to_lowercase().chars().map(fold_diacritic).collect();
        *counts.entry(folded).or_insert(0) += 1;
        total += 1;
    }
    (counts, total)
}

/// The query's terms with their IDF, or `None` when there is nothing to
/// score. Document frequency comes from `fts5vocab`; `N` is the number of
/// chunks, the unit the index is built in.
fn bm25_query_terms(
    conn: &Connection,
    text: &str,
) -> Result<Option<(Vec<(String, f64)>, HashMap<String, i64>)>> {
    let chunks: i64 = conn.query_row("select count(*) from chunks", [], |r| r.get(0))?;
    if chunks <= 0 {
        return Ok(None);
    }
    let n = chunks as f64;
    let cap = if (chunks as usize) >= MIN_CHUNKS_FOR_DF { chunks / 2 + 1 } else { i64::MAX };
    let mut seen = std::collections::HashSet::new();
    let mut terms = Vec::new();
    let mut frequencies = HashMap::new();
    for quoted in match_terms(text) {
        let raw = quoted.trim_matches('"').to_lowercase();
        let folded: String = raw.chars().map(fold_diacritic).collect();
        if folded.is_empty() || !seen.insert(folded.clone()) {
            continue;
        }
        // The vocabulary answers by looking the term up, which is cheaper
        // than walking its postings; the capped scan is the fallback for an
        // index the module cannot read.
        let df = match doc_freq(conn, &folded) {
            Some(n) => n,
            None => count_chunks_with(conn, &quoted, cap)?,
        };
        frequencies.insert(raw, df);
        if df > chunks / 2 {
            continue;
        }
        let df = df as f64;
        let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
        terms.push((folded, idf));
    }
    if terms.is_empty() {
        Ok(None)
    } else {
        Ok(Some((terms, frequencies)))
    }
}

/// BM25 (or BM25+ when `delta > 0`) over one candidate.
///
/// `text` is the chunk in indexed form: CJK already bigrammed and, when the
/// database was written with stemming, its stems appended. That is what
/// makes the tokens comparable with the ones the query produced.
fn bm25_score(text: &str, terms: &[(String, f64)], params: Bm25Params) -> f64 {
    let (counts, dl) = token_counts(text);
    let dl = dl as f64;
    let mut score = 0.0;
    for (term, idf) in terms {
        let tf = *counts.get(term).unwrap_or(&0) as f64;
        if tf == 0.0 {
            continue;
        }
        let saturated = (tf * (params.k1 + 1.0))
            / (tf + params.k1 * (1.0 - params.b + params.b * dl / AVG_CHUNK_TOKENS));
        score += idf * (saturated + params.delta);
    }
    score
}

/// The span as an FTS5 phrase, or `None` when it has too few or too many
/// parts to be worth one.
///
/// `unicode61` splits `OP-204` into `op` and `204`, so an `OR` of the parts
/// lets a note that repeats `204` outrank the note carrying the identifier.
/// The phrase requires the tokens to be adjacent, which is what "this
/// identifier is written here" means to the index.
fn identifier_phrase(span: &str) -> Option<String> {
    let parts: Vec<&str> = span
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 2 || parts.len() > 8 {
        return None;
    }
    Some(format!("\"{}\"", parts.join(" ")))
}

/// The query's distinct identifier phrases, already quoted.
///
/// This is the query-side half of the fix: no reindex and no change to the
/// index format, because the tokens are already adjacent in `fts_body`. It
/// is capped because every phrase is another branch of the MATCH expression,
/// and the expression has a ceiling (`MAX_TERMS`).
pub fn identifier_phrases(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    identifier_re()
        .find_iter(text)
        .filter_map(|m| identifier_phrase(m.as_str()))
        .filter(|p| seen.insert(p.to_lowercase()))
        .take(8)
        .collect()
}

/// Turns free text into a safe FTS5 MATCH expression.
///
/// The identifier phrases are appended after the terms, so trimming the
/// terms to fit `MAX_TERMS` cannot drop them: the exact identifier is the
/// narrower, stronger signal, and the individual terms still cover the rest
/// of the prose.
pub fn to_match(text: &str) -> String {
    let phrases = identifier_phrases(text);
    let mut terms = match_terms(text);
    terms.truncate(MAX_TERMS.saturating_sub(phrases.len()));
    terms.extend(phrases);
    terms.join(" OR ")
}

fn est_tokens(s: &str) -> usize {
    s.chars().count() / 4
}

type Row = (i64, Option<String>, String, String, String, f64, i64);

/// The span of chunk ids a project occupies, for narrowing the FTS scan.
///
/// `chunks_fts.rowid` is `chunks.id`, and FTS5 turns a `rowid BETWEEN` into a
/// constraint on the index instead of a filter over every posting. A project's
/// chunks are usually one contiguous block, so a scoped search stops scoring
/// the whole corpus to keep the handful of rows that project owns: on a
/// 23,854-memory corpus, a scoped query over a common word went from 526 ms to
/// 25 ms.
///
/// The range is a superset — chunk ids are not guaranteed to be contiguous —
/// and the `m.project = ?` filter stays, so this changes the work and not the
/// answer.
fn project_chunk_range(conn: &Connection, project: &str) -> Result<Option<(i64, i64)>> {
    let (lo, hi) = conn.query_row(
        "select min(c.id), max(c.id)
           from chunks c join memories m on m.id = c.memory_id
          where m.project = ?1",
        [project],
        |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?)),
    )?;
    Ok(match (lo, hi) {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    })
}

/// Builds and runs the candidate query against `chunks_fts`, with the
/// common filters (match, active status, project, tags) plus an optional
/// extra clause and limit.
fn candidate_rows(
    conn: &Connection,
    match_expr: &str,
    q: &Query,
    range: Option<(i64, i64)>,
    extra_where: &str,
    limit: Option<usize>,
) -> Result<Vec<Row>> {
    let mut sql = String::from(
        "select m.id, m.ext_id, m.project, m.title, c.body, bm25(chunks_fts) as score, m.pinned
           from chunks_fts
           join chunks c on c.id = chunks_fts.rowid
           join memories m on m.id = c.memory_id
          where chunks_fts match ?1
            and ({status})",
    );
    let mut params: Vec<Value> = vec![Value::Text(match_expr.to_string())];
    // With `--as-of` the interval decides, so a superseded memory has to be
    // able to come back: what it said then is what the question is about.
    let status = if q.as_of.is_some() {
        "m.status in ('active','superseded')"
    } else {
        "m.status = 'active'"
    };
    sql = sql.replace("{status}", status);

    sql.push_str(&filters_for(q, &mut params));
    if let Some((lo, hi)) = range {
        sql.push_str(&format!(
            " and chunks_fts.rowid between ?{} and ?{}",
            params.len() + 1,
            params.len() + 2
        ));
        params.push(Value::Integer(lo));
        params.push(Value::Integer(hi));
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

/// The same pool when the query has no filters and there is no memory
/// outside `active`. The FTS scoring runs alone, over the index, and the
/// joins that fetch the bodies happen after the limit: joining every match
/// to `memories` was most of the cost of an unscoped prose question.
fn candidate_rows_unscoped(
    conn: &Connection,
    match_expr: &str,
    limit: usize,
) -> Result<Vec<Row>> {
    let sql = "select m.id, m.ext_id, m.project, m.title, c.body, s.score, m.pinned
                 from (
                   select rowid as cid, bm25(chunks_fts) as score
                     from chunks_fts
                    where chunks_fts match ?1
                    order by score
                    limit ?2
                 ) s
                 join chunks c on c.id = s.cid
                 join memories m on m.id = c.memory_id
                order by s.score";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params![match_expr, limit as i64], |r| {
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
    /// How many of `words_total` this memory actually contains.
    pub words_matched: usize,
    /// How many of the query's terms carry evidence at all — see
    /// `word_evidence`. Zero means the query had nothing to match with, and
    /// then `words_matched` is zero too and says nothing.
    pub words_total: usize,
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
    /// Whether the lexical ranking runs at all. False is the vector-only
    /// arm of `mem eval --compare`; it is not a config value, because a
    /// weight of zero means the lexical side contributes no score but its
    /// hits still occupy the tail of the merged list, and the comparison
    /// wants what the vector side returns on its own.
    lexical: bool,
    /// How many chunks each retriever may nominate before deduplication and
    /// fusion. See `DEFAULT_RETRIEVAL_DEPTH`.
    depth: usize,
    /// When set, the lexical pool is rescored with these parameters instead
    /// of FTS5's hardcoded ones. `None` keeps the old behaviour exactly.
    bm25: Option<Bm25Params>,
}

impl SearchContext<'static> {
    /// Search with no vector layer: the long-standing behaviour.
    pub fn bm25_only() -> SearchContext<'static> {
        SearchContext {
            pack: None,
            identity: None,
            bm25_weight: 1.0,
            lexical: true,
            depth: DEFAULT_RETRIEVAL_DEPTH,
            bm25: None,
        }
    }
}

impl<'a> SearchContext<'a> {
    /// For searching. Reading with the wrong pack gives a worse result, not
    /// a mixed index, so reads do not need the identity.
    pub fn for_reading(pack: Option<&'a Pack>, bm25_weight: f64) -> SearchContext<'a> {
        SearchContext {
            pack,
            identity: None,
            bm25_weight,
            lexical: true,
            depth: DEFAULT_RETRIEVAL_DEPTH,
            bm25: None,
        }
    }

    /// The vector ranking alone, for `mem eval --compare`. When there is no
    /// pack this answers nothing, which is the honest result: there is no
    /// vector arm to compare.
    pub fn vector_only(pack: &'a Pack) -> SearchContext<'a> {
        SearchContext {
            pack: Some(pack),
            identity: None,
            bm25_weight: 1.0,
            lexical: false,
            depth: DEFAULT_RETRIEVAL_DEPTH,
            bm25: None,
        }
    }

    /// The same context with a different retrieval depth. Clamped to the
    /// interval `config::load` accepts, so a caller cannot construct the
    /// pathological pool the configuration refuses.
    pub fn with_depth(mut self, depth: usize) -> SearchContext<'a> {
        self.depth = depth.clamp(1, MAX_RETRIEVAL_DEPTH);
        self
    }

    /// The same context with the rescoring pass enabled. `None` is FTS5's
    /// own scorer.
    pub fn with_bm25(mut self, bm25: Option<Bm25Params>) -> SearchContext<'a> {
        self.bm25 = bm25;
        self
    }

    /// The rescoring parameters, when this context asks for them.
    pub fn bm25(&self) -> Option<Bm25Params> {
        self.bm25
    }

    /// For the MCP server, which searches and also saves.
    pub fn for_server(
        active: Option<(&'a Pack, &'a crate::vecstore::PackIdentity)>,
        bm25_weight: f64,
    ) -> SearchContext<'a> {
        SearchContext {
            pack: active.map(|(p, _)| p),
            identity: active.map(|(_, i)| i),
            bm25_weight,
            lexical: true,
            depth: DEFAULT_RETRIEVAL_DEPTH,
            bm25: None,
        }
    }

    /// The pack to embed the query with, if there is one.
    pub fn pack(&self) -> Option<&Pack> {
        self.pack
    }

    /// The pool each retriever nominates. Exposed so `mem tune --depth` and
    /// the tests can say which setting they measured.
    pub fn depth(&self) -> usize {
        self.depth
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
    if let Some(b) = &q.since {
        params.push(Value::Text(b.at.clone()));
        sql.push_str(&format!(" and {} >= ?{}", b.column, params.len()));
    }
    if let Some(b) = &q.until {
        params.push(Value::Text(b.at.clone()));
        sql.push_str(&format!(" and {} < ?{}", b.column, params.len()));
    }
    if let Some(day) = &q.as_of {
        // Day granularity: `--as-of 2026-10-01` means that day. A memory with
        // no interval yet (an older database, or one never superseded) falls
        // back to the day it was created, which is what the backfill says.
        params.push(Value::Text(day.clone()));
        let n = params.len();
        sql.push_str(&format!(
            " and date(coalesce(m.valid_from, m.created)) <= date(?{n}) \
              and (m.valid_until is null or date(m.valid_until) > date(?{n}))"
        ));
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
    // Not `--scope` keys, but filters all the same, and the message this
    // builds exists to name what came back empty. A window with nothing in
    // it is the likeliest reason an otherwise good query finds nothing, so
    // leaving it out would send somebody rephrasing a query that was never
    // the problem.
    if let Some(b) = &q.since {
        parts.push(format!("since:{}", b.text));
    }
    if let Some(b) = &q.until {
        parts.push(format!("until:{}", b.text));
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

/// For each memory in `ids`, how many of the query's terms it contains —
/// counting only the terms that could tell one memory from another.
///
/// Returns `(terms that carry evidence, count per memory)`.
///
/// **Which terms count.** A term that matches more than half the chunks
/// separates nothing: BM25's own IDF goes to zero and past it right around
/// there, so the ranker already treats such a word as saying nothing, and
/// `de` in a Spanish corpus is in every chunk there is. The test is document
/// frequency and deliberately not a stopword list: the index is `unicode61`
/// with no stopwords, in any language, and a list would have to be written
/// per language and would still call `api` or `ci` short enough to drop.
/// Frequency needs no list and no language.
///
/// **What it costs.** One FTS5 lookup per term for the document frequency —
/// `fts5vocab` answers it by looking the term up instead of walking its
/// postings — and, for the terms that survive the cap, one walk over the
/// matches inside `range`, which is the same span the candidate query read.
/// Terms are counted in chunks rather than memories, the unit the index is
/// built in; a memory that repeats a word across many chunks therefore pushes
/// it towards common, which is the direction that errs on the side of saying
/// less.
fn word_evidence(
    conn: &Connection,
    terms: &[String],
    ids: &[i64],
    frequencies: Option<&HashMap<String, i64>>,
) -> Result<(usize, HashMap<i64, usize>)> {
    let mut per: HashMap<i64, usize> = HashMap::new();
    if terms.is_empty() || ids.is_empty() {
        return Ok((0, per));
    }
    let chunks: i64 = conn.query_row("select count(*) from chunks", [], |r| r.get(0))?;
    let cap: i64 = if (chunks as usize) >= MIN_CHUNKS_FOR_DF { chunks / 2 } else { i64::MAX };
    let stemming = crate::stem::enabled(conn);

    // Only the candidates can carry evidence, and there are at most a few
    // dozen of them: reading their bodies once is cheaper than walking the
    // postings of every query term over the whole corpus, which is what this
    // did when the same query was unscoped.
    let marks = vec!["?"; ids.len()].join(",");
    let sql = format!("select id, body from memories where id in ({marks})");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut bodies: HashMap<i64, std::collections::HashSet<String>> = HashMap::new();
    for row in rows {
        let (id, body) = row?;
        let indexed = crate::stem::indexed_form(&body, stemming);
        let (counts, _) = token_counts(&indexed);
        bodies.insert(id, counts.into_keys().collect());
    }

    let mut total = 0usize;
    for term in terms {
        let key = term.trim_matches('"').to_lowercase();
        let df = match frequencies.and_then(|m| m.get(&key).copied()) {
            Some(n) => n,
            None => match doc_freq(conn, term) {
                Some(n) => n,
                None => count_chunks_with(conn, term, cap)?,
            },
        };
        if df > cap {
            continue;
        }
        total += 1;
        let needle: String = key.chars().map(fold_diacritic).collect();
        for (id, terms_in_body) in &bodies {
            if terms_in_body.contains(&needle) {
                *per.entry(*id).or_insert(0) += 1;
            }
        }
    }
    Ok((total, per))
}

/// Document frequency of one indexed token, read from the FTS5 vocabulary.
///
/// The scan below has to walk a term's postings to learn how many there are,
/// and a common word has hundreds of thousands of them. `fts5vocab` answers
/// the same question by looking the term up, which is the difference between
/// milliseconds and seconds on a corpus of 600,000 chunks.
///
/// `None` means the lookup did not answer — a build without the module, a token
/// this index does not hold, a term that is not a single word — and the caller
/// then counts by scanning, as it always did.
fn doc_freq(conn: &Connection, term: &str) -> Option<i64> {
    conn.execute_batch(
        "create virtual table if not exists temp.chunks_vocab
           using fts5vocab('main', 'chunks_fts', 'row')",
    )
    .ok()?;
    conn.query_row(
        "select doc from temp.chunks_vocab where term = ?1",
        [term.trim_matches('"').to_lowercase()],
        |r| r.get(0),
    )
    .ok()
}

/// How many chunks hold one term, stopped one past the cap.
///
/// The fallback for when the vocabulary cannot answer, and the `limit` is what
/// stops it from walking a whole corpus to learn that a word is everywhere.
fn count_chunks_with(conn: &Connection, term: &str, cap: i64) -> Result<i64> {
    Ok(conn.query_row(
        "select count(*) from (select 1 from chunks_fts where chunks_fts match ?1 limit ?2)",
        rusqlite::params![term, cap.saturating_add(1)],
        |r| r.get(0),
    )?)
}

/// The match expression without the terms that carry no ranking signal.
///
/// A term present in more than half the chunks has a BM25 idf at or past
/// zero: it contributes nothing to the order, but it still widens the match
/// set, and widening it is what makes an unscoped search score 600,000
/// chunks to keep twenty. On the LongMemEval-S corpus "the" alone is
/// 515,043 of 623,648.
///
/// `None` means "keep what you had": nothing was dropped, or everything was,
/// and a query made only of common words has to return what it always
/// returned rather than nothing.
fn without_common_terms(conn: &Connection, terms: &[String]) -> Option<Vec<String>> {
    if terms.len() < 2 {
        return None;
    }
    let chunks: i64 = conn.query_row("select count(*) from chunks", [], |r| r.get(0)).ok()?;
    if (chunks as usize) < MIN_CHUNKS_FOR_DF {
        return None;
    }
    // Half the corpus is where BM25's idf reaches zero: dropping those terms
    // cannot reorder anything. It was measured on an unscoped eval of 1,536
    // questions: recall@5 84.9 % before and after, MRR 0.719, one first_rank
    // moved out of 1,536. Tighter thresholds buy more time and cost recall —
    // a quarter took 0.6 points and a tenth took 4.2 — so the line stays at
    // the zero point and the rest of the cost is left where it is.
    let cap = chunks / 2;
    let kept: Vec<String> = terms
        .iter()
        .filter(|term| doc_freq(conn, term).map_or(true, |df| df <= cap))
        .cloned()
        .collect();
    if kept.is_empty() || kept.len() == terms.len() {
        return None;
    }
    Some(kept)
}

pub fn run_explained(
    conn: &Connection,
    q: &Query,
    ctx: &SearchContext,
) -> Result<Vec<Explained>> {
    // The query goes through the same transformation the index did, or the
    // two sides tokenize differently and nothing ever matches. Which
    // transformation that was is recorded in the database, not in a
    // configuration file, precisely so the two cannot drift apart.
    let texto_indexado;
    let text = if crate::stem::enabled(conn) {
        texto_indexado = crate::stem::augment(&q.text);
        &texto_indexado
    } else {
        &q.text
    };
    let original = to_match(text);
    let mut m = original.clone();
    let mut podado = false;

    // The chunks a scoped search may read, computed once and handed to both
    // queries below: `chunks_fts.rowid` is `chunks.id`, and FTS5 turns a
    // `rowid BETWEEN` into a constraint on the index instead of a filter over
    // every posting. Without it, a scoped search scores the whole corpus to
    // keep the rows its project owns (issue #67).
    let chunk_range = match &q.project {
        Some(p) => project_chunk_range(conn, p)?,
        None => None,
    };
    if chunk_range.is_none() {
        // Only the unscoped search pays the whole-corpus scan, and only it
        // is treated: a scoped one is already 20x faster, and pruning there
        // would change what its benchmarks measure for no gain.
        if let Some(pruned) = without_common_terms(conn, &match_terms(text)) {
            let mut kept = pruned;
            kept.extend(identifier_phrases(text));
            kept.truncate(MAX_TERMS);
            m = kept.join(" OR ");
            podado = true;
        }
    }

    // An unscoped query with no inactive memories needs no filter join
    // before the limit; the same holds for every filter-free search on a
    // database that has never superseded anything.
    let unfiltered = q.project.is_none() && q.tags.is_empty() && q.topic.is_none()
        && q.since.is_none() && q.until.is_none() && q.as_of.is_none();
    let fast_unscoped = chunk_range.is_none() && unfiltered && !conn.query_row(
        "select exists(select 1 from memories where status <> 'active')", [], |r| r.get::<_, i64>(0),
    )? .eq(&1);

    // --- lexical retrieval: identical to how it was ---
    let mut bm25_frequencies: Option<HashMap<String, i64>> = None;
    let mut best: HashMap<i64, (SearchHit, i64)> = HashMap::new();
    let mut orden_bm25: Vec<i64> = Vec::new();
    if ctx.lexical && !m.is_empty() {
        // Main pool: the best chunks by bm25, pinned or not.
        let mut rows = if fast_unscoped {
            candidate_rows_unscoped(conn, &m, ctx.depth)?
        } else {
            candidate_rows(conn, &m, q, chunk_range, "", Some(ctx.depth))?
        };
        if rows.is_empty() && podado {
            // The terms that were dropped still had the power to bring the
            // corpus back: a query of one rare word and one stopword used to
            // answer with everything the stopword matched, and pruning it to
            // nothing is a regression, not an optimisation.
            m = original.clone();
            rows = candidate_rows(conn, &m, q, chunk_range, "", Some(ctx.depth))?;
        }
        // A separate, unlimited guarantee: every pinned memory matching the
        // query is always a candidate, even when its best chunk falls
        // outside the top `depth` by pure bm25. Without this, `order by
        // score limit N` can leave out a relevant but verbose pinned memory
        // once the candidate pool exceeds N (in the real corpus, a common
        // connective like "que" already matches >700 chunks).
        // The cap is `ctx.depth` and not `None` (finding B-4). The
        // guarantee this query exists for is unchanged in practice —
        // pinning is a deliberate act, so no real corpus has two hundred
        // pinned memories matching one query — but a query with no `LIMIT`
        // at all makes the work SQLite does depend on the size of the
        // corpus rather than on what was asked for.
        let any_pinned: i64 = conn.query_row(
            "select exists(select 1 from memories where pinned = 1)", [], |r| r.get(0),
        )?;
        if any_pinned > 0 {
            rows.extend(candidate_rows(
                conn, &m, q, chunk_range, " and m.pinned = 1", Some(ctx.depth),
            )?);
        }

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
        // FTS5 scores the pool with its hardcoded k1 and b. When the
        // configuration asks for different ones, the pool is rescored here;
        // FTS5 still decided who is in it.
        if let Some(params) = ctx.bm25() {
            if let Some((terms, frequencies)) = bm25_query_terms(conn, text)? {
                bm25_frequencies = Some(frequencies);
                let stemming = crate::stem::enabled(conn);
                for (_, (hit, _)) in best.iter_mut() {
                    let indexed = crate::stem::indexed_form(&hit.snippet, stemming);
                    hit.score = -bm25_score(&indexed, &terms, params);
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
            // return up to `depth` arbitrary memories all tied at 0 —
            // garbage shaped like a result. With no real signal, there is
            // nothing to retrieve by vector.
            let nulo = cv.iter().all(|&x| x == 0.0);
            let vec_hits = if nulo { Vec::new() } else { crate::vecstore::search_vectors(conn, &cv, q, ctx.depth)? };
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
    let mut rest: Vec<i64> = Vec::new();
    for id in merged {
        match best.get(&id) {
            Some((_, 1)) => pinned_ids.push(id),
            Some(_) => rest.push(id),
            None => {}
        }
    }
    pinned_ids.extend(rest);

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
        // From the whole memory and not from the chunk that won: the point
        // is to say what the rest holds, and the chunk is the part already
        // known to match. `None` means this memory has nothing to outline,
        // and then the excerpt below stands.
        let outlined = if q.outline {
            memory_body(conn, hit.memory_id)?
                .and_then(|b| outline(&b, q.excerpt.unwrap_or(usize::MAX)))
        } else {
            None
        };
        match outlined {
            // An outline is bounded by its own rules; trimming it again here
            // would cut the headings off the end, which is the half that is
            // not in the excerpt to begin with.
            Some(o) => hit.snippet = o,
            None => {
                if let Some(n) = q.excerpt {
                    hit.snippet = hit.snippet.chars().take(n).collect();
                }
            }
        }
        let cost = est_tokens(&hit.title) + est_tokens(&hit.snippet);
        if !out.is_empty() && spent + cost > q.budget_tokens {
            break;
        }
        spent += cost;
        let bm25 = if hit.score < 0.0 { Some(hit.score) } else { None };
        let cosine = cosines.get(&id).copied();
        out.push(Explained { hit, bm25, cosine, words_matched: 0, words_total: 0 });
    }

    // Counted once the list is final, so the work is proportional to what
    // is returned and not to the candidate pool.
    let ids: Vec<i64> = out.iter().map(|e| e.hit.memory_id).collect();
    let (words_total, per) = word_evidence(
        conn, &match_terms(text), &ids, bm25_frequencies.as_ref(),
    )?;
    for e in out.iter_mut() {
        e.words_total = words_total;
        e.words_matched = per.get(&e.hit.memory_id).copied().unwrap_or(0);
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
