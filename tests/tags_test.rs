use anathem::tags;

#[test]
fn clasifica_perf() {
    let t = tags::classify("El endpoint tiene un N+1 y consume 215 MB por request").unwrap();
    assert!(t.contains(&"performance".to_string()), "tags: {t:?}");
}

#[test]
fn clasifica_multietiqueta() {
    let t = tags::classify(
        "el revisor pidió un test que cubra el N+1 del serializer antes de aprobar el PR",
    )
    .unwrap();
    assert!(t.contains(&"code-review".to_string()), "tags: {t:?}");
    assert!(t.contains(&"testing".to_string()), "tags: {t:?}");
    assert!(t.contains(&"performance".to_string()), "tags: {t:?}");
}

#[test]
fn sin_senal_no_asigna_tags() {
    assert!(tags::classify("una nota cualquiera sin nada relevante").unwrap().is_empty());
}

#[test]
fn extrae_referencias() {
    let r = tags::extract_refs("PROJ-482 en el PR #4821, commit c7eb6749be");
    assert!(r.contains(&("ticket".into(), "PROJ-482".into())), "refs: {r:?}");
    assert!(r.contains(&("pr".into(), "4821".into())), "refs: {r:?}");
    assert!(r.contains(&("sha".into(), "c7eb6749be".into())), "refs: {r:?}");
}

#[test]
fn no_confunde_numeros_sueltos_con_sha() {
    // Use 8+ digit pure numbers that pass length gate but must fail hex-letter guard.
    // Real corpus examples: Unix timestamps and large decimal counts.
    let r = tags::extract_refs("ocurrió 1346635041 veces, exactamente 1365639216 registros, 12345678 eventos");
    assert!(r.iter().all(|(k, _)| k != "sha"), "refs: {r:?}");
}

#[test]
fn quita_el_prefijo_save_y_devuelve_la_fecha() {
    let (body, date) = tags::strip_save_prefix("[SAVE:demo:2026-09-21]\n\nEl cache no se justifica.");
    assert_eq!(body, "El cache no se justifica.");
    assert_eq!(date.as_deref(), Some("2026-09-21"));
}

#[test]
fn sin_prefijo_devuelve_el_cuerpo_intacto() {
    let (body, date) = tags::strip_save_prefix("texto normal");
    assert_eq!(body, "texto normal");
    assert!(date.is_none());
}

#[test]
fn quita_tambien_el_terminador_save() {
    // Regresión del hallazgo D: strip_save_prefix sacaba el marcador de
    // apertura pero dejaba el [/SAVE] de cierre, que sobrevivía en el 87% de
    // los cuerpos migrados y aparecía en snippets de búsqueda del MCP, en los
    // archivos exportados y en el índice FTS.
    let (body, date) = tags::strip_save_prefix(
        "[SAVE:demo:2026-09-21]\n\nEl cache no se justifica.\n[/SAVE]",
    );
    assert_eq!(body, "El cache no se justifica.");
    assert_eq!(date.as_deref(), Some("2026-09-21"));
}

#[test]
fn quita_el_terminador_save_aunque_no_haya_prefijo() {
    let (body, date) = tags::strip_save_prefix("texto normal sin marcador de apertura.\n[/SAVE]");
    assert_eq!(body, "texto normal sin marcador de apertura.");
    assert!(date.is_none());
}

#[test]
fn clasifica_code_review_en_espanol() {
    // Formas solo en español (sin "PR #") - debe matchear vía los patrones
    // de "revisión/revisó".
    let t = tags::classify("sobre el doble rol en la revisión del ticket PROJ-143").unwrap();
    assert!(t.contains(&"code-review".to_string()), "tags: {t:?}");
}

#[test]
fn clasifica_code_review_con_revisor() {
    // Otra variante en español.
    let t = tags::classify("el revisor aprobó los cambios tras revisarlos").unwrap();
    assert!(t.contains(&"code-review".to_string()), "tags: {t:?}");
}
