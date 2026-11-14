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

/// Runs the grid, returning `(weight, mrr, recall@5)` per point.
///
/// The pack is required: `bm25_weight` only exists to balance the lexical
/// ranking against the vector one, so with no vector layer every weight
/// gives the same answer and sweeping measures nothing.
pub fn sweep(
    conn: &Connection,
    cases: &[Case],
    pack: &crate::pack::Pack,
) -> Result<Vec<(f64, f64, f64)>> {
    let mut out = Vec::new();
    for &w in WEIGHTS {
        out.push(punto(conn, cases, pack, w)?);
    }
    Ok(out)
}

fn punto(
    conn: &Connection,
    cases: &[Case],
    pack: &crate::pack::Pack,
    w: f64,
) -> Result<(f64, f64, f64)> {
    let ctx = search::SearchContext::for_reading(Some(pack), w);
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
) -> Result<Vec<(f64, f64, f64)>> {
    let mut out = sweep(conn, cases, pack)?;
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
        let p = punto(conn, cases, pack, next)?;
        if next > high {
            out.push(p);
        } else {
            out.insert(0, p);
        }
    }
}
