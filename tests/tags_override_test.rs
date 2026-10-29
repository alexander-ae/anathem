use anathem::tags;

/// These tests exercise `load_rules`/`load_rules_from_file` directly
/// (without going through the global `init`/`classify` state), so they
/// interfere neither with each other nor with the rest of the suite.

#[test]
fn with_no_file_next_to_the_db_it_uses_the_default_rules() {
    let dir = tempfile::tempdir().unwrap();
    let rules = tags::load_rules(Some(dir.path())).unwrap();
    let tags: Vec<&str> = rules.iter().map(|(t, _)| t.as_str()).collect();
    assert!(tags.contains(&"code-review"), "faltan las reglas por defecto: {tags:?}");
    assert!(tags.contains(&"bug"), "faltan las reglas por defecto: {tags:?}");
}

#[test]
fn a_tags_json_next_to_the_db_replaces_the_default_rules_outright() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("tags.json"),
        r#"[
            {"tag": "urgente", "pattern": "(?i)\\burgente\\b|\\bASAP\\b"},
            {"tag": "cliente", "pattern": "(?i)\\bcliente\\b"}
        ]"#,
    )
    .unwrap();

    let rules = tags::load_rules(Some(dir.path())).unwrap();
    let tags: Vec<&str> = rules.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(tags, vec!["urgente", "cliente"], "el override debe reemplazar, no sumar");

    // An entry that would match the default "code-review" rule (reviewer)
    // must produce no tag at all: the default rules are no longer active.
    let sin_match: Vec<&(String, regex::Regex)> = rules
        .iter()
        .filter(|(_, re)| re.is_match("el revisor aprobó el PR"))
        .collect();
    assert!(sin_match.is_empty(), "una regla por defecto sigue activa pese al override: {sin_match:?}");

    // Las reglas nuevas sí deben funcionar.
    assert!(rules.iter().any(|(_, re)| re.is_match("esto es urgente, ASAP")));
}

#[test]
fn a_tags_json_with_an_invalid_regex_fails_naming_file_and_rule() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tags.json");
    std::fs::write(&path, r#"[{"tag": "roto", "pattern": "(no cierra"}]"#).unwrap();

    let err = tags::load_rules(Some(dir.path())).expect_err("un regex inválido debe fallar");
    assert!(err.contains(&path.display().to_string()), "el error debe nombrar el archivo: {err}");
    assert!(err.contains("roto"), "el error debe nombrar la regla ofensora: {err}");
}

#[test]
fn a_malformed_tags_json_fails_naming_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tags.json");
    std::fs::write(&path, "esto no es json").unwrap();

    let err = tags::load_rules(Some(dir.path())).expect_err("JSON inválido debe fallar");
    assert!(err.contains(&path.display().to_string()), "el error debe nombrar el archivo: {err}");
}

#[test]
fn an_empty_tags_json_fails_instead_of_leaving_no_rules() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tags.json");
    std::fs::write(&path, "[]").unwrap();

    let err = tags::load_rules(Some(dir.path())).expect_err("un archivo sin reglas debe fallar");
    assert!(err.contains(&path.display().to_string()), "el error debe nombrar el archivo: {err}");
}
