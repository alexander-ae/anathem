use anathem::config;

#[test]
fn with_no_file_it_uses_the_default_weight() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(config::load(d.path()).unwrap().bm25_weight, 1.0);
}

#[test]
fn it_reads_the_weight_from_the_file() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"bm25_weight": 0.25}"#).unwrap();
    assert_eq!(config::load(d.path()).unwrap().bm25_weight, 0.25);
}

#[test]
fn a_broken_file_fails_loudly() {
    // Same as tags.json: an invalid configuration cannot silently degrade to
    // the defaults, because the user would believe their setting is in
    // effect.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), "{esto no es json").unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("config.json"), "the error has to name the file: {e}");
}

#[test]
fn a_negative_weight_is_rejected() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"bm25_weight": -1.0}"#).unwrap();
    assert!(config::load(d.path()).is_err());
}

#[test]
fn with_no_file_it_uses_twenty_candidates_and_a_three_hundred_char_excerpt() {
    // These are the values that make `search_memory` return many short
    // candidates rather than a few whole chunks.
    let d = tempfile::tempdir().unwrap();
    let cfg = config::load(d.path()).unwrap();
    assert_eq!(cfg.candidates, 20);
    assert_eq!(cfg.excerpt, 300);
}

#[test]
fn it_reads_candidates_and_excerpt_from_the_file() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("config.json"),
        r#"{"candidates": 30, "excerpt": 80}"#,
    ).unwrap();
    let cfg = config::load(d.path()).unwrap();
    assert_eq!(cfg.candidates, 30);
    assert_eq!(cfg.excerpt, 80);
}

#[test]
fn candidates_at_zero_is_rejected_naming_file_and_field() {
    // Same policy as bm25_weight: an invalid config.json fails loudly, it
    // does not silently degrade to the defaults.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"candidates": 0}"#).unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("config.json"), "the error has to name the file: {e}");
    assert!(e.contains("candidates"), "the error has to name the field: {e}");
}

#[test]
fn excerpt_at_zero_is_rejected_naming_file_and_field() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"excerpt": 0}"#).unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("config.json"), "the error has to name the file: {e}");
    assert!(e.contains("excerpt"), "the error has to name the field: {e}");
}

#[test]
fn bm25_weight_negative_is_rejected_naming_the_real_key() {
    // The three validation messages used to name keys that do not exist:
    // `peso_bm25`, `candidatos`, `extracto`. Whoever set an invalid value
    // got told to fix a key that `deny_unknown_fields` would reject.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"bm25_weight": -1.0}"#).unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("bm25_weight"), "the error has to name the real key: {e}");
}

// --- the config keys moved to English without breaking old files ---

#[test]
fn the_old_spanish_keys_still_load() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"),
        r#"{"traspaso_items": 3, "traspaso_ventana": "6h", "soltar_paquete_tras": 60}"#).unwrap();
    let c = config::load(d.path()).unwrap();
    assert_eq!(c.handoff_items, 3);
    assert_eq!(c.handoff_window, "6h");
    assert_eq!(c.release_pack_after, 60);
}

#[test]
fn the_new_english_keys_load() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"),
        r#"{"handoff_items": 3, "handoff_window": "6h", "release_pack_after": 60}"#).unwrap();
    let c = config::load(d.path()).unwrap();
    assert_eq!(c.handoff_items, 3);
    assert_eq!(c.handoff_window, "6h");
    assert_eq!(c.release_pack_after, 60);
}

#[test]
fn the_old_and_new_key_together_is_an_error() {
    // Silently picking one would leave the user believing the one that was
    // not applied is the one that was.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"),
        r#"{"traspaso_items": 3, "handoff_items": 9}"#).unwrap();
    assert!(config::load(d.path()).is_err());
}

#[test]
fn an_unknown_key_is_still_rejected() {
    // El alias no puede haber aflojado deny_unknown_fields.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"inventada": 1}"#).unwrap();
    assert!(config::load(d.path()).is_err());
}

#[test]
fn with_no_file_it_uses_the_default_budget() {
    // 1200 is what `search_memory` has always spent when nobody said
    // otherwise. Moving it into the file does not move the number.
    let d = tempfile::tempdir().unwrap();
    assert_eq!(config::load(d.path()).unwrap().budget, 1200);
}

#[test]
fn it_reads_the_budget_from_the_file() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"budget": 4000}"#).unwrap();
    assert_eq!(config::load(d.path()).unwrap().budget, 4000);
}

#[test]
fn a_zero_budget_is_rejected() {
    // Like `candidates` and `excerpt`: zero is not a setting, it is a search
    // that can never return anything, and finding that out from an empty
    // result is worse than finding it out from an error.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"budget": 0}"#).unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("budget"), "the error has to name the key: {e}");
}

#[test]
fn with_no_file_the_retrieval_depth_is_two_hundred() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(config::load(d.path()).unwrap().retrieval_depth, 200);
}

#[test]
fn it_reads_the_retrieval_depth_without_touching_candidates() {
    // They are different numbers: `candidates` is what the agent sees,
    // `retrieval_depth` is the pool the ranking may consider. Setting one
    // must not move the other.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("config.json"),
        r#"{"candidates": 12, "retrieval_depth": 600}"#,
    ).unwrap();
    let cfg = config::load(d.path()).unwrap();
    assert_eq!(cfg.candidates, 12);
    assert_eq!(cfg.retrieval_depth, 600);
}

#[test]
fn retrieval_depth_at_zero_is_rejected_naming_file_and_field() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"retrieval_depth": 0}"#).unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("config.json"), "the error has to name the file: {e}");
    assert!(e.contains("retrieval_depth"), "the error has to name the field: {e}");
}

#[test]
fn retrieval_depth_past_the_ceiling_is_rejected_instead_of_clamped() {
    let d = tempfile::tempdir().unwrap();
    let too_deep = anathem::search::MAX_RETRIEVAL_DEPTH + 1;
    std::fs::write(
        d.path().join("config.json"),
        format!(r#"{{"retrieval_depth": {too_deep}}}"#),
    ).unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("retrieval_depth"), "the error has to name the field: {e}");
    assert!(e.contains(&too_deep.to_string()), "and the value it read: {e}");
}

#[test]
fn bm25_parameters_are_absent_by_default_and_partial_ones_take_the_rest() {
    let d = tempfile::tempdir().unwrap();
    assert!(config::load(d.path()).unwrap().bm25_params().is_none());

    std::fs::write(d.path().join("config.json"), r#"{"bm25_k1": 3.0}"#).unwrap();
    let p = config::load(d.path()).unwrap().bm25_params().unwrap();
    assert_eq!(p.k1, 3.0);
    assert_eq!(p.b, 0.75, "the absent b keeps FTS5's value");
    assert_eq!(p.delta, 0.0);
}

#[test]
fn an_out_of_range_bm25_b_is_rejected_naming_the_field() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"bm25_b": 1.5}"#).unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("bm25_b"), "the error has to name the field: {e}");
}

#[test]
fn a_negative_bm25_k1_is_rejected_naming_the_field() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"bm25_k1": -1.0}"#).unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("bm25_k1"), "the error has to name the field: {e}");
}
