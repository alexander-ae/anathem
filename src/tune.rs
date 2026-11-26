//! Finds a better `bm25_weight` for *this* corpus, out of the labelling the
//! corpus already contains.
//!
//! Why it exists. Measured on LongMemEval the best weight is 1.0; measured on
//! a corpus of real technical notes it is 8.0, and the distance between them
//! is 0.03 to 0.06 of MRR in both directions. No single default serves both,
//! because what decides it is the shape of the corpus — prose, where the
//! vector ranking has meaning to catch, against notes dense in identifiers,
//! where bm25 is already confident at rank 1 and fusing dilutes it.
//!
//! So the number has to be found per corpus, and finding it needs an eval
//! set, which normally costs somebody an afternoon of writing questions and
//! labelling answers. The topic keys are that labelling, already done: a
//! `--topic` is somebody declaring "this memory is the current answer about
//! X", which is a query and its expected document.
//!
//! What this cannot do, and the output says so before the table: a topic key
//! is lexical by construction — it names the subject in keywords — so an
//! eval set built this way over-represents exactly the queries bm25 already
//! wins, and would flatter it if read as a quality figure. It is good for
//! **choosing between configurations**, where the bias applies equally to
//! all of them, and worthless as a number to report. Validated: against a
//! hand-labelled set of 50 queries over the same corpus, the two curves rank
//! the weights the same way (Spearman 0.86) and both put the default third
//! of seven.

use crate::{evalrun::Case, search};
use anyhow::{bail, Result};
use rusqlite::Connection;

/// Below this many topic keys the recommendation would be noise with a
/// straight face, so `tune` refuses instead of producing one.
pub const MINIMUM_CASES: usize = 20;

/// The grid. It spans the two optima found so far (1.0 on LongMemEval, 8.0
/// on a real corpus) with room past both, so a corpus that wants something
/// more extreme than either shows it by landing on an end — which is itself
/// worth seeing, and the output flags it.
pub const WEIGHTS: &[f64] = &[0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0];

/// One case per active memory that carries a topic key.
///
/// The key is the query, with its hyphens opened out, because it is written
/// as a slug and asked as words. The expectation is `id:`, since
/// `save_memory` assigns no ext_id and these are precisely the memories it
/// wrote.
pub fn cases_from_topics(conn: &Connection) -> Result<Vec<Case>> {
    let mut st = conn.prepare(
        "select id, topic, project from memories
          where status = 'active' and topic is not null and trim(topic) <> ''
          order by id",
    )?;
    let rows = st.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    })?;
    let mut cases = Vec::new();
    for row in rows {
        let (id, topic, project) = row?;
        cases.push(Case {
            id: format!("topic-{id}"),
            query: topic.replace('-', " ").replace('_', " "),
            expect_docs: vec![format!("id:{id}")],
            // Confined to the memory's own project, which is what a search
            // from inside that repository would be.
            project: Some(project),
            kind: Some("topic".into()),
            parafrastica: Some(false),
            words: None,
            stale_docs: Vec::new(),
        });
    }
    Ok(cases)
}

/// Refuses a corpus too small to conclude anything from, naming both
/// numbers so the answer is "come back later", not "something went wrong".
pub fn check_enough(found: usize) -> Result<()> {
    if found < MINIMUM_CASES {
        bail!(
            "only {found} topic keys here, and tuning wants at least {MINIMUM_CASES}.\n\
             A topic key is what `mem save --topic <key>` writes, and each one is a\n\
             question this can measure. With {found} the recommendation would be noise.\n\
             `mem topics` lists the ones you have."
        );
    }
    Ok(())
}

/// The weight with the best MRR. Ties go to the lower weight, which is the
/// smaller departure from whatever is configured now.
pub fn best_weight(scored: &[(f64, f64)]) -> f64 {
    scored
        .iter()
        .fold(None::<(f64, f64)>, |best, &(w, m)| match best {
            Some((bw, bm)) if bm >= m => Some((bw, bm)),
            _ => Some((w, m)),
        })
        .map(|(w, _)| w)
        .unwrap_or(1.0)
}

/// How far the grid may be extended past its ends before giving up.
///
/// A weight of 256 means the lexical ranking decides the order on its own
/// and the vector one only breaks ties; past that the number stops meaning
/// anything, so there is nothing to find further out. The same reasoning
/// downwards bottoms out well before 1/256.
pub const MAX_WEIGHT: f64 = 256.0;
pub const MIN_WEIGHT: f64 = 1.0 / 256.0;

/// The depths `mem tune --depth` compares. 200 is the default; the two
/// below it check whether the pool is paying for itself, and 1000 checks
/// whether it is truncating evidence the agent would have used.
pub const DEPTHS: &[usize] = &[100, 200, 500, 1000];

/// The one-axis grids `mem tune --bm25` walks. It is a coordinate descent
/// rather than the full product: 8 × 5 × 3 = 120 points over 470 questions
/// is an afternoon, and the axes interact little enough that two rounds
/// find the same neighbourhood.
pub const K1S: &[f64] = &[0.6, 1.2, 2.0, 3.0, 5.0, 10.0, 16.0, 25.0];
pub const BS: &[f64] = &[0.0, 0.4, 0.6, 0.75, 1.0];
pub const DELTAS: &[f64] = &[0.0, 0.5, 1.0];

/// One row of `mem tune --bm25`.
#[derive(Debug, Clone)]
pub struct Bm25Point {
    pub k1: f64,
    pub b: f64,
    pub delta: f64,
    pub recall_at_5: f64,
    pub recall_at_50: f64,
    pub mrr: f64,
    pub p50_ms: f64,
}

fn bm25_point(
    conn: &Connection,
    cases: &[Case],
    depth: usize,
    k1: f64,
    b: f64,
    delta: f64,
) -> Result<Bm25Point> {
    let params = search::Bm25Params { k1, b, delta };
    let ctx = search::SearchContext::bm25_only()
        .with_depth(depth)
        .with_bm25(Some(params));
    let r = crate::evalrun::run_cases(
        conn,
        cases,
        &format!("k1={k1} b={b} delta={delta}"),
        &ctx,
    )?;
    Ok(Bm25Point {
        k1,
        b,
        delta,
        recall_at_5: r.recall_at_5,
        recall_at_50: r.recall_at_50,
        mrr: r.mrr,
        p50_ms: r.p50_ms,
    })
}

fn best_mrr(points: &[Bm25Point]) -> Option<&Bm25Point> {
    points.iter().max_by(|a, c| {
        a.mrr.partial_cmp(&c.mrr).unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// Walks `k1`, then `b`, then `delta`, twice, keeping the best MRR at each
/// step. The baseline (1.2, 0.75, 0) is always included so the table has the
/// value FTS5 uses to compare against.
pub fn sweep_bm25(
    conn: &Connection,
    cases: &[Case],
    depth: usize,
) -> Result<Vec<Bm25Point>> {
    let mut points: Vec<Bm25Point> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut evaluate = |k1: f64, b: f64, delta: f64, points: &mut Vec<Bm25Point>| -> Result<()> {
        let key = format!("{k1:.6}|{b:.6}|{delta:.6}");
        if seen.insert(key) {
            points.push(bm25_point(conn, cases, depth, k1, b, delta)?);
        }
        Ok(())
    };

    let (mut k1, mut b, mut delta) = (1.2, 0.75, 0.0);
    evaluate(k1, b, delta, &mut points)?;
    for _ in 0..2 {
        for &candidate in K1S {
            evaluate(candidate, b, delta, &mut points)?;
        }
        if let Some(p) = best_mrr(&points) {
            k1 = p.k1;
            b = p.b;
            delta = p.delta;
        }
        for &candidate in BS {
            evaluate(k1, candidate, delta, &mut points)?;
        }
        if let Some(p) = best_mrr(&points) {
            k1 = p.k1;
            b = p.b;
            delta = p.delta;
        }
        for &candidate in DELTAS {
            evaluate(k1, b, candidate, &mut points)?;
        }
        if let Some(p) = best_mrr(&points) {
            k1 = p.k1;
            b = p.b;
            delta = p.delta;
        }
    }
    Ok(points)
}

/// One row of `mem tune --depth`.
#[derive(Debug)]
pub struct DepthPoint {
    pub depth: usize,
    pub recall_at_5: f64,
    pub recall_at_50: f64,
    pub mrr: f64,
    pub p50_ms: f64,
    /// Median tokens of the `search_memory` payload at the same settings.
    pub tokens_p50: usize,
}

/// Runs the same cases at each depth in `DEPTHS`. The pack is optional:
/// depth is a lexical question too, and a machine without one still has
/// something to measure.
pub fn sweep_depth(
    conn: &Connection,
    cases: &[Case],
    pack: Option<&crate::pack::Pack>,
    weight: f64,
    cfg: &crate::config::Config,
) -> Result<Vec<DepthPoint>> {
    let mut out = Vec::with_capacity(DEPTHS.len());
    for &depth in DEPTHS {
        let ctx = search::SearchContext::for_reading(pack, weight).with_depth(depth);
        let label = format!("depth={depth}");
        let r = crate::evalrun::run_cases(conn, cases, &label, &ctx)?;
        let payload = crate::evalrun::run_cases_as_mcp(conn, cases, &label, &ctx, cfg)?;
        out.push(DepthPoint {
            depth,
            recall_at_5: r.recall_at_5,
            recall_at_50: r.recall_at_50,
            mrr: r.mrr,
            p50_ms: r.p50_ms,
            tokens_p50: payload.median_tokens,
        });
    }
    Ok(out)
}

/// Runs the grid, returning `(weight, mrr, recall@5)` per point.
///
/// The pack is required: `bm25_weight` only exists to balance the lexical
/// ranking against the vector one, so with no vector layer every weight
/// gives the same answer and sweeping measures nothing.
pub fn sweep(
    conn: &Connection,
    cases: &[Case],
    pack: &crate::pack::Pack,
    depth: usize,
    bm25: Option<search::Bm25Params>,
) -> Result<Vec<(f64, f64, f64)>> {
    let mut out = Vec::new();
    for &w in WEIGHTS {
        out.push(punto(conn, cases, pack, w, depth, bm25)?);
    }
    Ok(out)
}

fn punto(
    conn: &Connection,
    cases: &[Case],
    pack: &crate::pack::Pack,
    w: f64,
    depth: usize,
    bm25: Option<search::Bm25Params>,
) -> Result<(f64, f64, f64)> {
    let ctx = search::SearchContext::for_reading(Some(pack), w)
        .with_depth(depth)
        .with_bm25(bm25);
    let r = crate::evalrun::run_cases(conn, cases, &format!("w={w}"), &ctx)?;
    Ok((w, r.mrr, r.recall_at_5))
}

/// The grid, extended past whichever end the best value landed on until the
/// optimum is interior or the extension runs out of room.
///
/// A best value sitting on an end does not mean the end is good; it means
/// the grid stopped too early and nobody looked further. Reporting that and
/// leaving the user to re-run something with a wider range is work the tool
/// can do itself — and it is the difference between a recommendation and a
/// measurement.
///
/// It doubles or halves, so each extra point costs one more sweep and the
/// range grows geometrically: from 16 to the ceiling is four more sweeps,
/// not a hundred.
pub fn sweep_until_interior(
    conn: &Connection,
    cases: &[Case],
    pack: &crate::pack::Pack,
    depth: usize,
    bm25: Option<search::Bm25Params>,
) -> Result<Vec<(f64, f64, f64)>> {
    let mut out = sweep(conn, cases, pack, depth, bm25)?;
    loop {
        let pares: Vec<(f64, f64)> = out.iter().map(|(w, m, _)| (*w, *m)).collect();
        let best = best_weight(&pares);
        let high = out.last().map(|(w, _, _)| *w).unwrap_or(0.0);
        let low = out.first().map(|(w, _, _)| *w).unwrap_or(0.0);
        let next = if best == high && high * 2.0 <= MAX_WEIGHT {
            high * 2.0
        } else if best == low && low / 2.0 >= MIN_WEIGHT {
            low / 2.0
        } else {
            return Ok(out);
        };
        let p = punto(conn, cases, pack, next, depth, bm25)?;
        if next > high {
            out.push(p);
        } else {
            out.insert(0, p);
        }
    }
}
