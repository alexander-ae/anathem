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
             saved itself, since those have no ext_id. See the README.",
            evalset.display()))?;
    let set: EvalSet = serde_json::from_str(&raw).context("eval set with invalid JSON")?;
    run_cases(conn, &set.cases, system, ctx)
}

/// The measurement itself, over cases already in hand.
///
/// `run` reads a file and calls this; `tune` builds its cases out of the
/// topic keys and calls it too, so both report the same numbers computed the
/// same way rather than two implementations that drift.
pub fn run_cases(
    conn: &Connection,
    cases: &[Case],
    system: &str,
    ctx: &search::SearchContext,
) -> Result<EvalResult> {
    let set = EvalSet { cases: cases.to_vec() };

    let (mut h5, mut h50) = (0usize, 0usize);
    let (mut rr, mut lat, mut per_case) = (Vec::new(), Vec::new(), Vec::new());

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
        per_case.push(CaseResult {
            id: c.id.clone(),
            kind: c.kind.clone(),
            hit_at_5: hit5,
            first_rank: rank.map(|r| r + 1),
            returned: got.into_iter().take(10).collect(),
            parafrastica: c.parafrastica,
            words: c.words,
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
        per_case,
    })
}
