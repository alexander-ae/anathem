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
    assert!(search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap().is_empty(), "el texto viejo sigue indexado");

    let q = search::Query { text: "berenjenas".into(), ..Default::default() };
    assert_eq!(search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap().len(), 1, "el texto nuevo no se indexó");
}

#[test]
fn reindex_reconstruye_desde_cero() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    seed(&conn, "el cache del breakdown no se justifica");
    conn.execute("delete from chunks", []).unwrap();

    let q = search::Query { text: "cache".into(), ..Default::default() };
    assert!(search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap().is_empty(), "precondición: índice vacío");

    let n = edit::reindex(&conn).unwrap();
    assert_eq!(n, 1);
    assert_eq!(search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap().len(), 1, "reindex no repobló el índice");
}

#[test]
fn export_escribe_markdown_con_frontmatter() {
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
    // El formato nativo de Claude Code anida `type` bajo `metadata:`, no como
    // clave plana — una clave plana produce un archivo que esa memoria nativa
    // no puede leer, que es justamente el propósito del export.
    assert!(
        txt.contains("metadata:\n  type: project"),
        "type debe ir anidado bajo metadata, no como clave plana:\n{txt}"
    );
}

/// Extrae el bloque YAML entre los delimitadores `---` del frontmatter.
fn frontmatter_yaml(txt: &str) -> &str {
    let body = txt.strip_prefix("---\n").expect("falta el delimitador de apertura");
    let end = body.find("\n---\n").expect("falta el delimitador de cierre");
    &body[..end]
}

#[test]
fn export_frontmatter_es_yaml_valido_con_dos_puntos_en_el_titulo() {
    // Regresión del hallazgo A: un título con "PROJ-136 2026-08-11: ..." hacía
    // que YAML interpretara el ": " como una nueva clave anidada, rompiendo
    // el parseo. Esta es la forma real en que aparecen los títulos migrados
    // desde tickets con fecha en el nombre.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let titulo = "PROJ-136 2026-08-11: el rate limit se disparó tras el deploy";
    ingest::insert(&conn, &NewMemory {
        project: "p".into(),
        title: titulo.into(),
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
        Some(export::slug(titulo).as_str()),
        "name no sobrevivió el round trip"
    );
    assert_eq!(
        value["description"].as_str(),
        Some(titulo),
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
    // Regresión del hallazgo B: `project` llega desde afuera (CLI, y en
    // producción desde argumentos MCP que un agente completa a partir de
    // conversación) y no debe poder escribir fuera del directorio de salida.
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

    // Nada debe haberse escrito fuera de `out`: ni en `dir` directamente, ni
    // en un directorio hermano llamado ESCAPADO.
    assert!(
        !dir.path().join("ESCAPADO").exists(),
        "escapó del directorio de salida hacia {}",
        dir.path().join("ESCAPADO").display()
    );
    assert!(
        !dir.path().join("a").join("ESCAPADO").exists(),
        "escapó un nivel del directorio de salida"
    );

    // Todo lo escrito debe quedar contenido dentro de `out`.
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
fn export_desambigua_titulos_que_colisionan_en_las_primeras_ocho_palabras() {
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
fn run_explained_expone_el_score() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    seed(&conn, "el cache del breakdown no se justifica");
    let q = search::Query { text: "cache".into(), ..Default::default() };
    let ex = search::run_explained(&conn, &q, &search::Contexto::solo_bm25()).unwrap();
    assert_eq!(ex.len(), 1);
    assert!(ex[0].bm25.unwrap() < 0.0, "bm25() de SQLite es negativo, fue {:?}", ex[0].bm25);
}

#[test]
fn editar_el_cuerpo_recalcula_el_titulo() {
    // El título se deriva del cuerpo. Si al editar no se recalcula, la memoria
    // queda anunciándose con un texto que ya no contiene — que es exactamente
    // lo que pasó al reimportar una memoria cuyo encabezado se quitó después.
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
fn el_export_preserva_todo_lo_necesario_para_migrar() {
    // El export es la salida de emergencia del proyecto: si no alcanza para
    // reconstruir la base en otra herramienta, encierra al usuario. Antes
    // perdía refs, pinned, la cadena de supersession y el id externo.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let vieja = ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: "la vieja".into(), body: "cuerpo viejo".into(),
        created: "2026-09-01".into(), event_start: None, event_end: None,
        source: "migrated".into(), ext_id: Some("EXT-123".into()),
        tags: vec!["testing".into()],
        refs: vec![("ticket".into(), "ABC-112".into()), ("pr".into(), "1234".into())],
    }).unwrap();
    let nueva = ingest::insert(&conn, &NewMemory {
        project: "p".into(), title: "la nueva".into(), body: "cuerpo nuevo".into(),
        created: "2026-09-02".into(), event_start: Some("2026-08-01".into()), event_end: None,
        source: "explicit".into(), ext_id: None, tags: vec![], refs: vec![],
    }).unwrap();
    anathem::maint::pin(&conn, vieja, true).unwrap();
    anathem::supersede::link(&conn, nueva, vieja).unwrap();

    let salida = dir.path().join("out");
    anathem::export::to_markdown(&conn, &salida).unwrap();
    let leer = |id: i64| -> String {
        let mut encontrado = None;
        for e in walk(&salida) {
            let s = std::fs::read_to_string(&e).unwrap();
            if s.contains(&format!("\nid: {id}\n")) { encontrado = Some(s); }
        }
        encontrado.unwrap_or_else(|| panic!("no se exportó la memoria {id}"))
    };

    let v = leer(vieja);
    assert!(v.contains("ext_id: \"EXT-123\""), "falta ext_id:\n{v}");
    assert!(v.contains("pinned: true"), "falta pinned:\n{v}");
    assert!(v.contains("kind: \"ticket\"") && v.contains("value: \"ABC-112\""), "faltan refs:\n{v}");
    assert!(v.contains("kind: \"pr\"") && v.contains("value: \"1234\""), "falta el ref de pr:\n{v}");
    assert!(v.contains("source: \"migrated\""), "falta source:\n{v}");
    assert!(v.contains("status: \"superseded\""), "la vieja quedó superseded:\n{v}");

    let n = leer(nueva);
    assert!(n.contains(&format!("supersedes: {vieja}")), "falta la cadena:\n{n}");
    assert!(n.contains("event_start: \"2026-08-01\""), "falta event_start:\n{n}");
    assert!(!n.contains("ext_id:"), "no debe emitir ext_id vacío:\n{n}");
    assert!(!n.contains("event_end:"), "no debe emitir event_end vacío:\n{n}");
}

#[test]
fn el_export_incluye_las_memorias_olvidadas() {
    // `forget` oculta, no borra. Dejarlas afuera del export convertía cada
    // migración en una pérdida de datos silenciosa.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = seed(&conn, "algo que después se olvida");
    anathem::maint::forget(&conn, id).unwrap();
    let salida = dir.path().join("out");
    let n = anathem::export::to_markdown(&conn, &salida).unwrap();
    assert_eq!(n, 1, "la olvidada tiene que exportarse igual");
    let s = walk(&salida).into_iter().map(|p| std::fs::read_to_string(p).unwrap()).collect::<String>();
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
