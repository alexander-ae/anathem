use anathem::tags;

/// Estas pruebas ejercitan `load_rules`/`load_rules_from_file` de forma
/// directa (sin pasar por el estado global `init`/`classify`), así que no
/// interfieren entre sí ni con el resto de la suite.

#[test]
fn sin_archivo_junto_a_la_db_usa_las_reglas_por_defecto() {
    let dir = tempfile::tempdir().unwrap();
    let rules = tags::load_rules(Some(dir.path())).unwrap();
    let tags: Vec<&str> = rules.iter().map(|(t, _)| t.as_str()).collect();
    assert!(tags.contains(&"code-review"), "faltan las reglas por defecto: {tags:?}");
    assert!(tags.contains(&"bug"), "faltan las reglas por defecto: {tags:?}");
}

#[test]
fn un_tags_json_junto_a_la_db_reemplaza_por_completo_las_reglas_por_defecto() {
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

    // Una entrada que matchearía la regla por defecto "code-review" (revisor)
    // no debe producir ningún tag: las reglas por defecto ya no están activas.
    let sin_match: Vec<&(String, regex::Regex)> = rules
        .iter()
        .filter(|(_, re)| re.is_match("el revisor aprobó el PR"))
        .collect();
    assert!(sin_match.is_empty(), "una regla por defecto sigue activa pese al override: {sin_match:?}");

    // Las reglas nuevas sí deben funcionar.
    assert!(rules.iter().any(|(_, re)| re.is_match("esto es urgente, ASAP")));
}

#[test]
fn tags_json_con_regex_invalido_falla_nombrando_archivo_y_regla() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tags.json");
    std::fs::write(&path, r#"[{"tag": "roto", "pattern": "(no cierra"}]"#).unwrap();

    let err = tags::load_rules(Some(dir.path())).expect_err("un regex inválido debe fallar");
    assert!(err.contains(&path.display().to_string()), "el error debe nombrar el archivo: {err}");
    assert!(err.contains("roto"), "el error debe nombrar la regla ofensora: {err}");
}

#[test]
fn tags_json_con_json_malformado_falla_nombrando_el_archivo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tags.json");
    std::fs::write(&path, "esto no es json").unwrap();

    let err = tags::load_rules(Some(dir.path())).expect_err("JSON inválido debe fallar");
    assert!(err.contains(&path.display().to_string()), "el error debe nombrar el archivo: {err}");
}

#[test]
fn tags_json_vacio_falla_en_vez_de_dejar_sin_reglas() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tags.json");
    std::fs::write(&path, "[]").unwrap();

    let err = tags::load_rules(Some(dir.path())).expect_err("un archivo sin reglas debe fallar");
    assert!(err.contains(&path.display().to_string()), "el error debe nombrar el archivo: {err}");
}
