use crate::search;
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Instant};

#[derive(Deserialize)]
struct EvalSet {
    cases: Vec<Case>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Case {
    pub id: String,
    pub query: String,
    pub expect_docs: Vec<String>,
    /// The project this case's haystack lives in. A benchmark that gives
    /// every question its own history needs the search confined to it:
    /// pooling the haystacks and searching all of them measures something
    /// harder than the benchmark defines. `None` searches everything, which
    /// is what an eval set over a single corpus wants.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub parafrastica: Option<bool>,
    #[serde(default)]
    pub words: Option<u64>,
    /// Documents that carry a **superseded** version of the fact this case
    /// asks about, when a version-agnostic question would still match them.
    ///
    /// Empty means the case says nothing about currency, and then
    /// `contamination@5` reports "not measured" instead of zero: a zero it
    /// did not earn is the kind of number that gets quoted.
    #[serde(default)]
    pub stale_docs: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct CaseResult {
    pub id: String,
    pub kind: Option<String>,
    pub hit_at_5: bool,
    pub first_rank: Option<usize>,
    pub returned: Vec<String>,
    pub parafrastica: Option<bool>,
    pub words: Option<u64>,
    /// Share of the top 5 that holds a superseded version of the asked
    /// fact: StaleBench's `contamination@5`, per case. `None` when the case
    /// carries no currency labels.
    #[serde(default)]
    pub contamination_at_5: Option<f64>,
}

#[derive(Serialize, Debug)]
pub struct EvalResult {
    pub system: String,
    pub date: String,
    pub n_cases: usize,
    pub recall_at_5: f64,
    pub recall_at_50: f64,
    pub mrr: f64,
    pub p50_ms: f64,
    /// Mean `contamination@5` over the cases that label their stale
    /// versions. `None` when none does.
    #[serde(default)]
    pub contamination_at_5: Option<f64>,
    pub per_case: Vec<CaseResult>,
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// How an `expect_docs` entry is compared with a hit.
///
/// A bare entry is an `ext_id`. `id:<number>` is a `memories.id`, which is
/// the only way to name a memory that `save_memory` wrote, since those carry
/// no ext_id. Keeping the prefix means a bare "42" still means the ext_id
/// "42" and an existing eval set measures exactly what it measured before.
fn matches(expected: &str, h: &crate::model::SearchHit) -> bool {
    match expected.strip_prefix("id:") {
        Some(n) => n.trim().parse::<i64>().is_ok_and(|n| n == h.memory_id),
        None => h.ext_id.as_deref() == Some(expected),
    }
}

/// What a hit is called in `per_case[].returned`, in the same spelling an
/// `expect_docs` entry would use for it.
fn name_of(h: &crate::model::SearchHit) -> String {
    match &h.ext_id {
        Some(e) => e.clone(),
        None => format!("id:{}", h.memory_id),
    }
}

pub fn run(
    conn: &Connection,
    evalset: &Path,
    system: &str,
    ctx: &search::SearchContext,
) -> Result<EvalResult> {
    let set = read_evalset(evalset)?;
    run_cases(conn, &set.cases, system, ctx)
}

/// One case, with the rank each arm gave the expected memory.
#[derive(Serialize, Debug)]
pub struct CompareCase {
    pub id: String,
    pub query: String,
    pub expected: Vec<String>,
    /// 1-based position of the expected memory, or `null` when the arm did
    /// not return it.
    pub fused_rank: Option<usize>,
    pub bm25_rank: Option<usize>,
    pub vector_rank: Option<usize>,
    /// The fused ranking put the expected memory below the better of the two
    /// components that found it. This is the fusion tax, per case.
    pub below_better: bool,
}

/// Three arms over one eval set, and the per-query cost of fusing them.
#[derive(Serialize, Debug)]
pub struct CompareResult {
    pub date: String,
    pub n_cases: usize,
    pub vector_available: bool,
    pub fused: EvalResult,
    pub bm25: EvalResult,
    pub vector: Option<EvalResult>,
    /// Cases where fusion ranked the expected memory below BM25 alone.
    pub fused_below_bm25: usize,
    /// Cases where fusion ranked it below the vector arm alone.
    pub fused_below_vector: usize,
    /// Cases where fusion ranked it below the better of the two components.
    pub fused_below_better: usize,
    pub per_case: Vec<CompareCase>,
}

/// True when `a` is worse than `b`: not returned at all counts as worse than
/// any position. Two missing ranks are not a tax; they are a case neither arm
/// answered.
fn below(a: Option<usize>, b: Option<usize>) -> bool {
    match (a, b) {
        (None, Some(_)) => true,
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// Runs the same cases through the three arms. No ranking changes: this is
/// the instrument #77 asks for, and the fused arm is the one search uses.
pub fn compare(
    conn: &Connection,
    evalset: &Path,
    label: &str,
    fused_ctx: &search::SearchContext,
    bm25_ctx: &search::SearchContext,
    vector_ctx: Option<&search::SearchContext>,
) -> Result<CompareResult> {
    let set = read_evalset(evalset)?;
    let fused = run_cases(conn, &set.cases, &format!("{label} fused"), fused_ctx)?;
    let bm25 = run_cases(conn, &set.cases, &format!("{label} bm25"), bm25_ctx)?;
    let vector = match vector_ctx {
        Some(ctx) => Some(run_cases(conn, &set.cases, &format!("{label} vector"), ctx)?),
        None => None,
    };

    let (mut below_bm25, mut below_vector, mut below_better) = (0usize, 0usize, 0usize);
    let mut per_case = Vec::with_capacity(set.cases.len());
    for (i, case) in set.cases.iter().enumerate() {
        let fused_rank = fused.per_case[i].first_rank;
        let bm25_rank = bm25.per_case[i].first_rank;
        let vector_rank = vector.as_ref().and_then(|v| v.per_case[i].first_rank);
        let better = match (bm25_rank, vector_rank) {
            (Some(b), Some(v)) => Some(b.min(v)),
            (Some(b), None) => Some(b),
            (None, Some(v)) => Some(v),
            (None, None) => None,
        };
        if below(fused_rank, bm25_rank) {
            below_bm25 += 1;
        }
        if below(fused_rank, vector_rank) {
            below_vector += 1;
        }
        let tax = below(fused_rank, better);
        if tax {
            below_better += 1;
        }
        per_case.push(CompareCase {
            id: case.id.clone(),
            query: case.query.clone(),
            expected: case.expect_docs.clone(),
            fused_rank,
            bm25_rank,
            vector_rank,
            below_better: tax,
        });
    }

    Ok(CompareResult {
        date: chrono::Local::now().format("%Y-%m-%d").to_string(),
        n_cases: set.cases.len(),
        vector_available: vector.is_some(),
        fused,
        bm25,
        vector,
        fused_below_bm25: below_bm25,
        fused_below_vector: below_vector,
        fused_below_better: below_better,
        per_case,
    })
}

/// Reads the eval set, and says how to write one when it cannot.
fn read_evalset(evalset: &Path) -> Result<EvalSet> {
    let raw = std::fs::read_to_string(evalset)
        .with_context(|| format!(
            "could not parse the eval set at {}.\n\n\
             An eval set is a JSON file with the queries to measure retrieval\n\
             against, and which memories they should find. Without it there is\n\
             nothing to measure. Minimal format:\n\
             \n\
             {{\"cases\": [{{\"id\": \"q1\", \"query\": \"...\", \"expect_docs\": [\"<ext_id>\"], \"kind\": \"factual\"}}]}}\n\
             \n\
             An `expect_docs` entry is a `memories.ext_id`, or `id:<number>`\n\
             for a `memories.id` — which is how you name a memory anathem\n\
             saved itself, since those have no ext_id. Add \"stale_docs\" with\n\
             the entries that hold a SUPERSEDED version of the same fact and\n\
             the run also reports contamination@5: the share of the top five\n\
             carrying a value that is no longer true. Without that field the\n\
             column is not measured, which is not the same as zero. See the\n\
             README.",
            evalset.display()))?;
    serde_json::from_str(&raw).context("eval set with invalid JSON")
}

/// `run_cases_as_mcp` over a file, the same way `run` is `run_cases`.
pub fn run_as_mcp(
    conn: &Connection,
    evalset: &Path,
    system: &str,
    ctx: &search::SearchContext,
    cfg: &crate::config::Config,
) -> Result<PayloadResult> {
    let set = read_evalset(evalset)?;
    run_cases_as_mcp(conn, &set.cases, system, ctx, cfg)
}

/// The cases of an eval set, for a caller that runs them more than once with
/// different settings. `mem tune --depth` is the caller; reading the file
/// once keeps the four points on exactly the same questions.
pub fn cases_from_file(evalset: &Path) -> Result<Vec<Case>> {
    Ok(read_evalset(evalset)?.cases)
}

/// The measurement itself, over cases already in hand.
///
/// `run` reads a file and calls this; `tune` builds its cases out of the
/// topic keys and calls it too, so both report the same numbers computed the
/// same way rather than two implementations that drift.
/// What the MCP answer actually puts in front of the model.
///
/// `run_cases` measures retrieval: unlimited budget, no excerpt, fifty
/// candidates, because that is the ranking and trimming it would falsify the
/// figures. This measures the other question — whether the memory the answer
/// needed is among the ones `search_memory` would have shown, at the settings
/// a real installation uses.
pub struct PayloadResult {
    pub system: String,
    pub n_cases: usize,
    /// Cases whose expected memory made it into the returned candidates.
    pub found: f64,
    /// Cases where it came first.
    pub first: f64,
    /// Median tokens the answer spent, as `search_log` counts them.
    pub median_tokens: usize,
}

pub fn run_cases_as_mcp(
    conn: &Connection,
    cases: &[Case],
    system: &str,
    ctx: &search::SearchContext,
    cfg: &crate::config::Config,
) -> Result<PayloadResult> {
    let mut found = 0usize;
    let mut first = 0usize;
    let mut spent: Vec<usize> = Vec::new();

    for c in cases {
        let q = search::Query {
            text: c.query.clone(),
            project: c.project.clone(),
            limit: cfg.candidates,
            budget_tokens: cfg.budget,
            excerpt: Some(cfg.excerpt),
            outline: cfg.outline,
            ..Default::default()
        };
        let hits = search::run_explained(conn, &q, ctx)?;
        // The same arithmetic `mcp.rs` uses for the entry it writes to the
        // log, so the number here is the number a real search costs.
        let used: usize = hits
            .iter()
            .map(|e| (e.hit.title.chars().count() + e.hit.snippet.chars().count()) / 4)
            .sum();
        spent.push(used);

        if let Some(pos) = hits
            .iter()
            .position(|e| c.expect_docs.iter().any(|d| matches(d, &e.hit)))
        {
            found += 1;
            if pos == 0 {
                first += 1;
            }
        }
    }

    spent.sort_unstable();
    let median_tokens = spent.get(spent.len() / 2).copied().unwrap_or(0);
    let n = cases.len().max(1);
    Ok(PayloadResult {
        system: system.to_string(),
        n_cases: cases.len(),
        found: found as f64 / n as f64,
        first: first as f64 / n as f64,
        median_tokens,
    })
}

pub fn run_cases(
    conn: &Connection,
    cases: &[Case],
    system: &str,
    ctx: &search::SearchContext,
) -> Result<EvalResult> {
    let set = EvalSet { cases: cases.to_vec() };

    let (mut h5, mut h50) = (0usize, 0usize);
    let (mut rr, mut lat, mut per_case) = (Vec::new(), Vec::new(), Vec::new());
    let mut contams: Vec<f64> = Vec::new();

    for c in &set.cases {
        let q = search::Query {
            text: c.query.clone(),
            project: c.project.clone(),
            limit: 50,
            budget_tokens: usize::MAX,
            // The eval measures retrieval, not injection: trimming the
            // snippet would falsify its figures. It is already the default,
            // but it is left explicit so it does not depend on `Default`
            // staying put.
            excerpt: None,
            ..Default::default()
        };
        let t0 = Instant::now();
        let hits = search::run(conn, &q, ctx)?;
        lat.push(t0.elapsed().as_secs_f64() * 1000.0);

        // The rank is computed over the whole `hits` list, so a hit that
        // cannot match still occupies its position instead of being removed
        // and pulling up the rank of everything that follows (I1).
        //
        // An expectation names either an `ext_id` or, with the `id:` prefix,
        // a `memories.id`. The prefix is what keeps the two namespaces
        // apart: a bare "42" is the ext_id "42" and always was, so adding
        // this cannot change what an existing eval set measures.
        let rank = hits.iter().position(|h| c.expect_docs.iter().any(|d| matches(d, h)));
        let got: Vec<String> = hits.iter().map(name_of).collect();
        let hit5 = rank.map_or(false, |r| r < 5);
        if hit5 {
            h5 += 1;
        }
        if rank.is_some() {
            h50 += 1;
        }
        rr.push(rank.map_or(0.0, |r| 1.0 / (r as f64 + 1.0)));
        // Currency is a different question from recall: the right entity
        // with the wrong value matches the query exactly as well as the
        // current one, which is why a stale fact needs its own column.
        //
        // The definition is StaleBench's frozen one: contamination is
        // whether the top five holds BOTH a current and a stale version of
        // the asked fact, averaged over the cases that label it. It is not
        // the share of stale entries — a context with one stale row and no
        // current one is not contaminated, it is simply wrong in a different
        // way, and `current@k` already reports that.
        let contam = if c.stale_docs.is_empty() {
            None
        } else {
            let top: Vec<_> = hits.iter().take(5).collect();
            let has_current = top.iter().any(|h| {
                c.expect_docs.iter().any(|d| matches(d, h))
            });
            let has_stale = top.iter().any(|h| {
                c.stale_docs.iter().any(|d| matches(d, h))
            });
            Some(if has_current && has_stale { 1.0 } else { 0.0 })
        };
        if let Some(v) = contam {
            contams.push(v);
        }
        per_case.push(CaseResult {
            id: c.id.clone(),
            kind: c.kind.clone(),
            hit_at_5: hit5,
            first_rank: rank.map(|r| r + 1),
            returned: got.into_iter().take(10).collect(),
            parafrastica: c.parafrastica,
            words: c.words,
            contamination_at_5: contam,
        });
    }

    let n = set.cases.len().max(1);
    Ok(EvalResult {
        system: system.to_string(),
        date: chrono::Local::now().format("%Y-%m-%d").to_string(),
        n_cases: set.cases.len(),
        recall_at_5: h5 as f64 / n as f64,
        recall_at_50: h50 as f64 / n as f64,
        mrr: rr.iter().sum::<f64>() / n as f64,
        p50_ms: median(lat),
        contamination_at_5: if contams.is_empty() {
            None
        } else {
            Some(contams.iter().sum::<f64>() / contams.len() as f64)
        },
        per_case,
    })
}
