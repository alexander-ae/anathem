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
    // Igual que tags.json: una configuración inválida no puede degradarse en
    // silencio a los valores por defecto, porque el usuario creería que su
    // ajuste está aplicado.
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
    // Estos son los valores que hacen que `search_memory` devuelva muchos
    // candidatos breves en vez de pocos fragmentos completos.
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
    // Misma política que peso_bm25: un config.json inválido falla
    // ruidosamente, no se degrada en silencio a los valores por defecto.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"candidates": 0}"#).unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("config.json"), "el error debe nombrar el archivo: {e}");
    assert!(e.contains("candidatos"), "el error debe nombrar el campo: {e}");
}

#[test]
fn excerpt_at_zero_is_rejected_naming_file_and_field() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"excerpt": 0}"#).unwrap();
    let e = config::load(d.path()).unwrap_err().to_string();
    assert!(e.contains("config.json"), "el error debe nombrar el archivo: {e}");
    assert!(e.contains("extracto"), "el error debe nombrar el campo: {e}");
}
