use anathem::tags;

#[test]
fn clasifica_perf() {
    let t = tags::classify("El endpoint tiene un N+1 y consume 215 MB por request").unwrap();
    assert!(t.contains(&"performance".to_string()), "tags: {t:?}");
}

#[test]
fn it_classifies_with_multiple_tags() {
    let t = tags::classify(
        "el revisor pidió un test que cubra el N+1 del serializer antes de aprobar el PR",
    )
    .unwrap();
    assert!(t.contains(&"code-review".to_string()), "tags: {t:?}");
    assert!(t.contains(&"testing".to_string()), "tags: {t:?}");
    assert!(t.contains(&"performance".to_string()), "tags: {t:?}");
}

#[test]
fn with_no_signal_it_assigns_no_tags() {
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
fn it_does_not_mistake_loose_numbers_for_a_sha() {
    // Use 8+ digit pure numbers that pass length gate but must fail hex-letter guard.
    // Real corpus examples: Unix timestamps and large decimal counts.
    let r = tags::extract_refs("ocurrió 1346635041 veces, exactamente 1365639216 registros, 12345678 eventos");
    assert!(r.iter().all(|(k, _)| k != "sha"), "refs: {r:?}");
}

#[test]
fn it_strips_the_save_prefix_and_returns_the_date() {
    let (body, date) = tags::strip_save_prefix("[SAVE:demo:2026-09-21]\n\nEl cache no se justifica.");
    assert_eq!(body, "El cache no se justifica.");
    assert_eq!(date.as_deref(), Some("2026-09-21"));
}

#[test]
fn with_no_prefix_it_returns_the_body_untouched() {
    let (body, date) = tags::strip_save_prefix("texto normal");
    assert_eq!(body, "texto normal");
    assert!(date.is_none());
}

#[test]
fn it_also_strips_the_save_terminator() {
    // Regression for finding D: strip_save_prefix removed the opening marker
    // but left the closing [/SAVE], which survived in 87% of migrated bodies
    // and showed up in MCP search snippets, in exported files and in the FTS
    // index.
    let (body, date) = tags::strip_save_prefix(
        "[SAVE:demo:2026-09-21]\n\nEl cache no se justifica.\n[/SAVE]",
    );
    assert_eq!(body, "El cache no se justifica.");
    assert_eq!(date.as_deref(), Some("2026-09-21"));
}

#[test]
fn it_strips_the_save_terminator_even_without_a_prefix() {
    let (body, date) = tags::strip_save_prefix("texto normal sin marcador de apertura.\n[/SAVE]");
    assert_eq!(body, "texto normal sin marcador de apertura.");
    assert!(date.is_none());
}

#[test]
fn clasifica_code_review_en_espanol() {
    // Spanish-only forms (with no "PR #") - it has to match through the
    // "revisión/revisó" patterns.
    let t = tags::classify("sobre el doble rol en la revisión del ticket PROJ-143").unwrap();
    assert!(t.contains(&"code-review".to_string()), "tags: {t:?}");
}

#[test]
fn it_classifies_code_review_with_a_reviewer() {
    // Another Spanish variant.
    let t = tags::classify("el revisor aprobó los cambios tras revisarlos").unwrap();
    assert!(t.contains(&"code-review".to_string()), "tags: {t:?}");
}
