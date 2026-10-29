use anathem::{db, edit, export, ingest, model::NewMemory, search};

fn seed(conn: &rusqlite::Connection, body: &str) -> i64 {
    ingest::insert(conn, &NewMemory {
        project: "p".into(), title: "t".into(), body: body.into(),
        created: "2026-09-21".into(), event_start: None, event_end: None,
        source: "migrated".into(), ext_id: Some("a".into()),
        tags: vec!["performance".into()], refs: vec![],
    }).unwrap()
}

#[test]
fn replace_body_rechunkea_y_reindexa() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = seed(&conn, "texto viejo sobre zanahorias");
    edit::replace_body(&conn, id, "texto nuevo sobre berenjenas").unwrap();

    let q = search::Query { text: "zanahorias".into(), ..Default::default() };
    assert!(search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap().is_empty(), "el texto viejo sigue indexado");

    let q = search::Query { text: "berenjenas".into(), ..Default::default() };
    assert_eq!(search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap().len(), 1, "el texto nuevo no se indexó");
}

#[test]
fn reindex_reconstruye_desde_cero() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    seed(&conn, "el cache del breakdown no se justifica");
    conn.execute("delete from chunks", []).unwrap();

    let q = search::Query { text: "cache".into(), ..Default::default() };
    assert!(search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap().is_empty(), "precondición: índice vacío");

    let n = edit::reindex(&conn).unwrap();
    assert_eq!(n, 1);
    assert_eq!(search::run(&conn, &q, &search::SearchContext::bm25_only()).unwrap().len(), 1, "reindex no repobló el índice");
}

#[test]
fn export_writes_markdown_with_frontmatter() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    seed(&conn, "el cache del breakdown no se justifica");
    let out = dir.path().join("export");
    let n = export::to_markdown(&conn, &out).unwrap();
    assert_eq!(n, 1);

    let files: Vec<_> = std::fs::read_dir(out.join("p")).unwrap().flatten().collect();
    assert_eq!(files.len(), 1, "archivos: {files:?}");
    let txt = std::fs::read_to_string(files[0].path()).unwrap();
    assert!(txt.starts_with("---\n"), "falta el frontmatter:\n{txt}");
    assert!(txt.contains("project: \"p\""), "frontmatter incompleto:\n{txt}");
    assert!(txt.contains("tags: [\"performance\"]"), "frontmatter incompleto:\n{txt}");
    assert!(txt.contains("el cache del breakdown"), "falta el cuerpo:\n{txt}");
    // Claude Code's native format nests `type` under `metadata:`, not as a
    // flat key — a flat key produces a file that native memory cannot read,
    // which is precisely what the export is for.
    assert!(
        txt.contains("metadata:\n  type: project"),
        "type debe ir anidado bajo metadata, no como clave plana:\n{txt}"
    );
}

/// Extracts the YAML block between the frontmatter's `---` delimiters.
fn frontmatter_yaml(txt: &str) -> &str {
    let body = txt.strip_prefix("---\n").expect("falta el delimitador de apertura");
    let end = body.find("\n---\n").expect("falta el delimitador de cierre");
    &body[..end]
}

#[test]
fn export_frontmatter_is_valid_yaml_with_a_colon_in_the_title() {
    // Regression for finding A: a title like "PROJ-136 2026-08-11: ..." made
    // YAML read the ": " as a new nested key, breaking the parse. This is the
    // real shape of titles migrated from tickets with a date in the name.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let title = "PROJ-136 2026-08-11: el rate limit se disparó tras el deploy";
    ingest::insert(&conn, &NewMemory {
        project: "p".into(),
        title: title.into(),
        body: "cuerpo de prueba con el hallazgo completo".into(),
        created: "2026-08-11".into(),
        event_start: None,
        event_end: None,
        source: "migrated".into(),
        ext_id: Some("z".into()),
        tags: vec!["performance".into(), "code-review".into()],
        refs: vec![],
    })
    .unwrap();

    let out = dir.path().join("export");
    let n = export::to_markdown(&conn, &out).unwrap();
    assert_eq!(n, 1);

    let files: Vec<_> = std::fs::read_dir(out.join("p")).unwrap().flatten().collect();
    assert_eq!(files.len(), 1, "archivos: {files:?}");
    let txt = std::fs::read_to_string(files[0].path()).unwrap();

    let yaml_src = frontmatter_yaml(&txt);
    let value: serde_yaml::Value = serde_yaml::from_str(yaml_src).unwrap_or_else(|e| {
        panic!("el frontmatter no parsea como YAML: {e}\n---\n{yaml_src}\n---")
    });

    assert_eq!(
        value["name"].as_str(),
        Some(export::slug(title).as_str()),
        "name no sobrevivió el round trip"
    );
    assert_eq!(
        value["description"].as_str(),
        Some(title),
        "description no sobrevivió el round trip"
    );
    assert_eq!(
        value["created"].as_str(),
        Some("2026-08-11"),
        "created no sobrevivió el round trip"
    );
    assert_eq!(value["project"].as_str(), Some("p"));
}

#[test]
fn export_rechaza_path_traversal_via_project() {
    // Regression for finding B: `project` arrives from outside (the CLI,
    // and in production from MCP arguments an agent fills in from
    // conversation) and must not be able to write outside the output
    // directory.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    ingest::insert(&conn, &NewMemory {
        project: "../../ESCAPADO".into(),
        title: "intento de escape".into(),
        body: "cuerpo".into(),
        created: "2026-09-22".into(),
        event_start: None,
        event_end: None,
        source: "explicit".into(),
        ext_id: None,
        tags: vec![],
        refs: vec![],
    })
    .unwrap();

    let out = dir.path().join("a").join("b");
    std::fs::create_dir_all(&out).unwrap();
    export::to_markdown(&conn, &out).unwrap();

    // Nothing may have been written outside `out`: not into `dir` directly,
    // nor into a sibling directory named ESCAPADO.
    assert!(
        !dir.path().join("ESCAPADO").exists(),
        "escapó del directorio de salida hacia {}",
        dir.path().join("ESCAPADO").display()
    );
    assert!(
        !dir.path().join("a").join("ESCAPADO").exists(),
        "escapó un nivel del directorio de salida"
    );

    // Everything written has to stay contained inside `out`.
    for entry in walkdir(&out) {
        assert!(
            entry.starts_with(&out),
            "archivo escrito fuera del directorio de salida: {}",
            entry.display()
        );
    }
}

fn walkdir(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(walkdir(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn export_disambiguates_titles_colliding_on_the_first_eight_words() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let base = "PROJ-482 / PR #4821 — ronda de hallazgos del bot";

    ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: format!("{base}, primera parte del hallazgo"),
        body: "cuerpo de la memoria a".into(),
        created: "2026-09-10".into(), event_start: None, event_end: None,
        source: "migrated".into(), ext_id: Some("a".into()),
        tags: vec![], refs: vec![],
    }).unwrap();
    ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: format!("{base}, segunda parte, reemplaza a la anterior"),
        body: "cuerpo de la memoria b".into(),
        created: "2026-09-10".into(), event_start: None, event_end: None,
        source: "migrated".into(), ext_id: Some("b".into()),
        tags: vec![], refs: vec![],
    }).unwrap();

    let out = dir.path().join("export");
    let n = export::to_markdown(&conn, &out).unwrap();
    assert_eq!(n, 2, "el conteo reportado debe igualar los archivos realmente escritos");

    let files: Vec<_> = std::fs::read_dir(out.join("p")).unwrap().flatten().collect();
    assert_eq!(
        files.len(),
        2,
        "dos memorias con los mismos primeros 8 palabras deben producir dos archivos, no uno pisando al otro: {files:?}"
    );

    let bodies: Vec<String> = files
        .iter()
        .map(|f| std::fs::read_to_string(f.path()).unwrap())
        .collect();
    assert!(bodies.iter().any(|b| b.contains("cuerpo de la memoria a")), "se perdió la memoria a");
    assert!(bodies.iter().any(|b| b.contains("cuerpo de la memoria b")), "se perdió la memoria b");
}

#[test]
fn run_explained_exposes_the_score() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    seed(&conn, "el cache del breakdown no se justifica");
    let q = search::Query { text: "cache".into(), ..Default::default() };
    let ex = search::run_explained(&conn, &q, &search::SearchContext::bm25_only()).unwrap();
    assert_eq!(ex.len(), 1);
    assert!(ex[0].bm25.unwrap() < 0.0, "bm25() de SQLite es negativo, fue {:?}", ex[0].bm25);
}

#[test]
fn editing_the_body_recomputes_the_title() {
    // The title is derived from the body. If editing does not recompute it,
    // the memory goes on announcing itself with text it no longer contains —
    // which is exactly what happened on reimporting a memory whose heading
    // was later removed.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = seed(&conn, "encabezado viejo\n\ncuerpo original");
    let antes: String = conn
        .query_row("select title from memories where id=?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(antes, "t", "el helper siembra un título arbitrario, a propósito");

    anathem::edit::replace_body(&conn, id, "encabezado nuevo\n\notro cuerpo").unwrap();

    let despues: String = conn
        .query_row("select title from memories where id=?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(despues, "encabezado nuevo", "el título quedó desactualizado");
}

#[test]
fn export_preserves_everything_needed_to_migrate() {
    // The export is the project's emergency exit: if it is not enough to
    // rebuild the database in another tool, it locks the user in. It used to
    // lose refs, pinned, the supersession chain and the external id.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let older = ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: "la vieja".into(), body: "cuerpo viejo".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "migrated".into(), ext_id: Some("EXT-123".into()),
        tags: vec!["testing".into()],
        refs: vec![("ticket".into(), "ABC-112".into()), ("pr".into(), "1234".into())],
    }).unwrap();
    let newer = ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: "la nueva".into(), body: "cuerpo nuevo".into(),
        created: "2026-09-02".into(), event_start: Some("2026-08-01".into()), event_end: None,
        source: "explicit".into(), ext_id: None, tags: vec![], refs: vec![],
    }).unwrap();
    anathem::maint::pin(&conn, older, true).unwrap();
    anathem::supersede::link(&conn, newer, older).unwrap();

    let out_text = dir.path().join("out");
    anathem::export::to_markdown(&conn, &out_text).unwrap();
    let parse = |id: i64| -> String {
        let mut encontrado = None;
        for e in walk(&out_text) {
            let s = std::fs::read_to_string(&e).unwrap();
            if s.contains(&format!("\nid: {id}\n")) { encontrado = Some(s); }
        }
        encontrado.unwrap_or_else(|| panic!("no se exportó la memoria {id}"))
    };

    let v = parse(older);
    assert!(v.contains("ext_id: \"EXT-123\""), "falta ext_id:\n{v}");
    assert!(v.contains("pinned: true"), "falta pinned:\n{v}");
    assert!(v.contains("kind: \"ticket\"") && v.contains("value: \"ABC-112\""), "faltan refs:\n{v}");
    assert!(v.contains("kind: \"pr\"") && v.contains("value: \"1234\""), "falta el ref de pr:\n{v}");
    assert!(v.contains("source: \"migrated\""), "falta source:\n{v}");
    assert!(v.contains("status: \"superseded\""), "la vieja quedó superseded:\n{v}");

    let n = parse(newer);
    assert!(n.contains(&format!("supersedes: {older}")), "falta la cadena:\n{n}");
    assert!(n.contains("event_start: \"2026-08-01\""), "falta event_start:\n{n}");
    assert!(!n.contains("ext_id:"), "no debe emitir ext_id vacío:\n{n}");
    assert!(!n.contains("event_end:"), "no debe emitir event_end vacío:\n{n}");
}

#[test]
fn export_includes_forgotten_memories() {
    // `forget` hides, it does not delete. Leaving them out of the export
    // turned every migration into a silent loss of data.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = seed(&conn, "algo que después se olvida");
    anathem::maint::forget(&conn, id).unwrap();
    let out_text = dir.path().join("out");
    let n = anathem::export::to_markdown(&conn, &out_text).unwrap();
    assert_eq!(n, 1, "la olvidada tiene que exportarse igual");
    let s = walk(&out_text).into_iter().map(|p| std::fs::read_to_string(p).unwrap()).collect::<String>();
    assert!(s.contains("status: \"forgotten\""), "y marcada como olvidada:\n{s}");
}

fn walk(d: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(d) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() { v.extend(walk(&p)); } else { v.push(p); }
        }
    }
    v
}
