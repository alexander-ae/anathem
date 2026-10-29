//! Merging two rankings with Reciprocal Rank Fusion.
//!
//! A pure function: it takes two lists of `memory_id`, each already ordered
//! best to worst, and returns a single one. It touches neither the database
//! nor the model, so the ordering decision can be tested with hand-written
//! lists.

use std::cmp::Ordering;
use std::collections::HashMap;

/// RRF damping constant. The value 60 is the one from the original paper and
/// the one used in every measurement behind the design.
pub const K: f64 = 60.0;

/// Merges `bm25` and `vector`. `bm25_weight` scales the lexical ranking's
/// contribution: at 1.0 both count the same, below 1.0 the vector side wins.
///
/// On why the default is 1.0, and why no single value wins in both
/// directions, see `docs/superpowers/medicion-corpus-real.md`.
pub fn rrf(bm25: &[i64], vector: &[i64], bm25_weight: f64) -> Vec<i64> {
    let mut score: HashMap<i64, f64> = HashMap::new();
    for (i, id) in bm25.iter().enumerate() {
        *score.entry(*id).or_insert(0.0) += bm25_weight / (K + (i + 1) as f64);
    }
    for (i, id) in vector.iter().enumerate() {
        *score.entry(*id).or_insert(0.0) += 1.0 / (K + (i + 1) as f64);
    }
    let mut ids: Vec<(i64, f64)> = score.into_iter().collect();
    // Highest score first; on a tie, lowest id. Breaking ties by id is not
    // cosmetic: `score` is a HashMap, and without it two tied documents would
    // come out in a different order across identical runs.
    ids.sort_by(|a, b| {
        b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal).then(a.0.cmp(&b.0))
    });
    ids.into_iter().map(|(id, _)| id).collect()
}
