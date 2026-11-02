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
    assert!(e.contains("config.json"), "el error debe nombrar el archivo: {e}");
}

#[test]
fn a_negative_weight_is_rejected() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"bm25_weight": -1.0}"#).unwrap();
    assert!(config::load(d.path()).is_err());
}

#[test]
fn with_no_file_it_uses_twenty_candidates_and_a_excerpt_of_one_twenty() {
    // These are the values that make `search_memory` return many short
    // candidates rather than a few whole chunks.
    let d = tempfile::tempdir().unwrap();
    let cfg = config::load(d.path()).unwrap();
    assert_eq!(cfg.candidates, 20);
    assert_eq!(cfg.excerpt, 120);
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
    assert!(e.contains("config.json"), "el error debe nombrar el archivo: {e}");
    assert!(e.contains("candidates"), "el error debe nombrar el campo: {e}");
}

#[test]
fn excerpt_at_zero_is_rejected_naming_file_and_field() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"excerpt": 0}"#).unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("config.json"), "el error debe nombrar el archivo: {e}");
    assert!(e.contains("excerpt"), "el error debe nombrar el campo: {e}");
}

#[test]
fn bm25_weight_negative_is_rejected_naming_the_real_key() {
    // The three validation messages used to name keys that do not exist:
    // `peso_bm25`, `candidatos`, `extracto`. Whoever set an invalid value
    // got told to fix a key that `deny_unknown_fields` would reject.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"bm25_weight": -1.0}"#).unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("bm25_weight"), "el error debe nombrar la clave real: {e}");
}
