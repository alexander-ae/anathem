use anathem::fusion::rrf;

#[test]
fn with_no_vector_ranking_it_keeps_the_bm25_one() {
    assert_eq!(rrf(&[10, 20, 30], &[], 0.25), vec![10, 20, 30]);
}

#[test]
fn with_no_bm25_ranking_it_keeps_the_vector_one() {
    assert_eq!(rrf(&[], &[30, 20, 10], 0.25), vec![30, 20, 10]);
}

#[test]
fn at_a_low_weight_the_vector_side_beats_bm25() {
    // Each contributes a different document at position 1. At a weight of
    // 0.25 bm25's is worth a quarter, so the vector one rules.
    assert_eq!(rrf(&[10], &[20], 0.25), vec![20, 10]);
}

#[test]
fn at_weight_one_they_tie_and_the_id_breaks_it() {
    // Same case, equal weights: the scores are identical and the stable
    // tiebreak by id puts the lower one first. Without that tiebreak the
    // order would depend on the HashMap's traversal and vary between runs.
    assert_eq!(rrf(&[10], &[20], 1.0), vec![10, 20]);
}

#[test]
fn appearing_in_both_rankings_beats_leading_one() {
    // 20 is in both (position 2 in bm25, 1 in the vector one); 30 only in
    // the vector one at position 2; 10 only in bm25 at position 1.
    assert_eq!(rrf(&[10, 20], &[20, 30], 0.25), vec![20, 30, 10]);
}

#[test]
fn it_does_not_repeat_documents() {
    let r = rrf(&[1, 2, 3], &[3, 2, 1], 0.5);
    let mut u = r.clone();
    u.sort_unstable();
    u.dedup();
    assert_eq!(r.len(), u.len(), "there are duplicates in {r:?}");
    assert_eq!(u, vec![1, 2, 3]);
}

#[test]
fn breaking_ties_by_id_is_deterministic() {
    // 10 at position 0 in bm25, position 1 in the vector one
    // 20 at position 1 in bm25, position 0 in the vector one
    // With bm25_weight=1.0 both have the same total score.
    // The stable tiebreak by id has to put 10 first, always.
    let expected = vec![10, 20];
    for _ in 0..20 {
        assert_eq!(rrf(&[10, 20], &[20, 10], 1.0), expected);
    }
}

#[test]
fn peso_cero_bm25_pone_solo_vectorial_adelante() {
    // At weight zero, bm25 contributes nothing. The documents that are only
    // in bm25 (10, 30) fall behind the ones in the vector list (20, 40), and
    // the vector list's internal order is preserved.
    assert_eq!(rrf(&[10, 20, 30, 40], &[20, 40], 0.0), vec![20, 40, 10, 30]);
}

#[test]
fn with_both_empty_it_returns_empty() {
    assert!(rrf(&[], &[], 0.25).is_empty());
}
