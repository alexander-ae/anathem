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

/// Las siete reglas por defecto tienen que disparar en los dos idiomas.
///
/// La taxonomía por defecto es, a propósito, de ingeniería general: sin
/// nombres propios ni vocabulario de un stack o equipo. Pero sus patrones
/// nacieron escritos contra memorias en español, así que para alguien que
/// guarda en inglés varias quedaban mudas — y una herramienta que dice
/// clasificar sola y no clasifica nada parece rota, no neutral.
///
/// Un `tags.json` propio sigue reemplazándolas por completo; esto es solo
/// qué pasa cuando no hay ninguno.
#[test]
fn the_default_taxonomy_fires_in_both_languages() {
    // (tag esperado, texto en español, texto en inglés)
    let casos: &[(&str, &str, &str)] = &[
        ("decision",    "Se decidió usar SQLite por la concurrencia",
                        "We decided to use SQLite because of concurrency"),
        ("bug",         "Hay un bug en el importador que rompe la carga",
                        "There is a bug in the importer that breaks the load"),
        ("convention",  "La convención del equipo es correr lint antes del push",
                        "The team convention is to run lint before pushing"),
        ("performance", "La consulta tiene un N+1 y sube la latencia",
                        "The query has an N+1 and raises latency"),
        ("testing",     "Faltan specs y la cobertura bajó",
                        "Specs are missing and coverage dropped"),
        ("docs",        "Actualizar la documentación y el changelog",
                        "Update the documentation and the changelog"),
        ("code-review", "Lo levantó el reviewer en el PR #4821",
                        "The reviewer raised it in PR #4821"),
    ];
    for (tag, es, en) in casos {
        for (idioma, texto) in [("español", es), ("inglés", en)] {
            let t = tags::classify(texto).unwrap();
            assert!(
                t.contains(&tag.to_string()),
                "la regla «{tag}» no disparó en {idioma} con «{texto}» — salió {t:?}"
            );
        }
    }
}

#[test]
fn a_merge_request_counts_as_a_review_too() {
    // El proyecto vive en GitLab, donde no se llaman pull requests.
    let t = tags::classify("Lo discutimos en el merge request").unwrap();
    assert!(t.contains(&"code-review".to_string()), "tags: {t:?}");
}
