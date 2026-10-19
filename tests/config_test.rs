use anathem::config;

#[test]
fn sin_archivo_usa_el_peso_por_defecto() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(config::cargar(d.path()).unwrap().peso_bm25, 1.0);
}

#[test]
fn lee_el_peso_del_archivo() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"peso_bm25": 0.25}"#).unwrap();
    assert_eq!(config::cargar(d.path()).unwrap().peso_bm25, 0.25);
}

#[test]
fn un_archivo_roto_falla_ruidosamente() {
    // Igual que tags.json: una configuración inválida no puede degradarse en
    // silencio a los valores por defecto, porque el usuario creería que su
    // ajuste está aplicado.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), "{esto no es json").unwrap();
    let e = config::cargar(d.path()).unwrap_err().to_string();
    assert!(e.contains("config.json"), "el error debe nombrar el archivo: {e}");
}

#[test]
fn un_peso_negativo_se_rechaza() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"peso_bm25": -1.0}"#).unwrap();
    assert!(config::cargar(d.path()).is_err());
}

#[test]
fn sin_archivo_usa_veinte_candidatos_y_ciento_veinte_de_extracto() {
    // Estos son los valores que hacen que `search_memory` devuelva muchos
    // candidatos breves en vez de pocos fragmentos completos.
    let d = tempfile::tempdir().unwrap();
    let cfg = config::cargar(d.path()).unwrap();
    assert_eq!(cfg.candidatos, 20);
    assert_eq!(cfg.extracto, 120);
}

#[test]
fn lee_candidatos_y_extracto_del_archivo() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("config.json"),
        r#"{"candidatos": 30, "extracto": 80}"#,
    ).unwrap();
    let cfg = config::cargar(d.path()).unwrap();
    assert_eq!(cfg.candidatos, 30);
    assert_eq!(cfg.extracto, 80);
}

#[test]
fn un_candidatos_en_cero_se_rechaza_nombrando_archivo_y_campo() {
    // Misma política que peso_bm25: un config.json inválido falla
    // ruidosamente, no se degrada en silencio a los valores por defecto.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"candidatos": 0}"#).unwrap();
    let e = config::cargar(d.path()).unwrap_err().to_string();
    assert!(e.contains("config.json"), "el error debe nombrar el archivo: {e}");
    assert!(e.contains("candidatos"), "el error debe nombrar el campo: {e}");
}

#[test]
fn un_extracto_en_cero_se_rechaza_nombrando_archivo_y_campo() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.json"), r#"{"extracto": 0}"#).unwrap();
    let e = config::cargar(d.path()).unwrap_err().to_string();
    assert!(e.contains("config.json"), "el error debe nombrar el archivo: {e}");
    assert!(e.contains("extracto"), "el error debe nombrar el campo: {e}");
}
