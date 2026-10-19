use anathem::{db, ingest, model::NewMemory, search};
use rusqlite::Connection;

fn seed(conn: &Connection, ext: &str, project: &str, body: &str) -> i64 {
    ingest::insert(conn, &NewMemory {
        project: project.into(),
        title: body.chars().take(60).collect(),
        body: body.into(),
        created: "2026-09-01".into(),
        event_start: None, event_end: None,
        source: "migrated".into(),
        ext_id: Some(ext.into()),
        tags: vec![], refs: vec![],
    }).unwrap()
}

fn fixture() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    seed(&conn, "a", "acme-billing", "El cache del breakdown no se justifica porque agrega staleness sin ganancia medible.");
    seed(&conn, "b", "acme-billing", "Convenciones de testing: la suite de integración cubre el status 500 con manejo de excepciones.");
    seed(&conn, "c", "docs-portal", "El design system interno define los colores de la plataforma admin.");
    (dir, conn)
}

#[test]
fn encuentra_por_termino() {
    let (_d, conn) = fixture();
    let q = search::Query { text: "cache staleness".into(), ..Default::default() };
    let hits = search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap();
    assert!(!hits.is_empty(), "sin resultados");
    assert_eq!(hits[0].ext_id.as_deref(), Some("a"));
}

#[test]
fn filtra_por_proyecto() {
    let (_d, conn) = fixture();
    let q = search::Query {
        text: "design system".into(),
        project: Some("acme-billing".into()),
        ..Default::default()
    };
    assert!(search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap().is_empty(), "no debe cruzar de proyecto cuando se filtra");
}

#[test]
fn deduplica_por_memoria() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    // cuerpo largo => varios chunks, todos con la misma palabra
    let largo = "cache ".repeat(400);
    seed(&conn, "x", "p", &largo);
    let q = search::Query { text: "cache".into(), ..Default::default() };
    let hits = search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap();
    assert_eq!(hits.len(), 1, "una memoria debe aparecer una sola vez, hubo {}", hits.len());
}

#[test]
fn excluye_las_superseded() {
    let (_d, conn) = fixture();
    conn.execute("update memories set status='superseded' where ext_id='a'", []).unwrap();
    let q = search::Query { text: "cache staleness".into(), ..Default::default() };
    let hits = search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap();
    assert!(hits.iter().all(|h| h.ext_id.as_deref() != Some("a")));
}

#[test]
fn respeta_el_presupuesto_de_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    for i in 0..20 {
        seed(&conn, &format!("m{i}"), "p", &format!("cache numero {i} {}", "x".repeat(2000)));
    }
    let q = search::Query { text: "cache".into(), budget_tokens: 300, ..Default::default() };
    let hits = search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap();
    let tokens: usize = hits.iter().map(|h| (h.title.len() + h.snippet.len()) / 4).sum();
    assert!(tokens <= 300, "se pasó del presupuesto: {tokens} tokens");
    assert!(!hits.is_empty(), "el presupuesto no debe vaciar el resultado");
}

#[test]
fn las_pinned_van_primero() {
    let (_d, conn) = fixture();
    conn.execute("update memories set pinned=1 where ext_id='b'", []).unwrap();
    let q = search::Query {
        text: "cache testing".into(),
        project: Some("acme-billing".into()),
        ..Default::default()
    };
    let hits = search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap();
    assert_eq!(hits[0].ext_id.as_deref(), Some("b"), "la pinned debe ir primero");
}

#[test]
fn el_orden_es_deterministico_ante_empates_de_bm25() {
    // Regresión del hallazgo C: `best` era un HashMap y `into_values()` no
    // garantiza orden; sin un tiebreak final tras el score, dos memorias con
    // bm25 empatado podían salir en orden distinto entre corridas. Sembramos
    // muchas memorias con cuerpo idéntico (mismo bm25 exacto) para forzar el
    // empate y comparamos el orden devuelto en corridas repetidas.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    for i in 0..40 {
        seed(&conn, &format!("tie{i}"), "p", "faro identico para forzar empate de bm25");
    }
    let q = search::Query { text: "faro".into(), project: Some("p".into()), limit: 40, ..Default::default() };

    let primera: Vec<Option<String>> = search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap().into_iter().map(|h| h.ext_id).collect();
    for _ in 0..5 {
        let siguiente: Vec<Option<String>> = search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap().into_iter().map(|h| h.ext_id).collect();
        assert_eq!(primera, siguiente, "el orden cambió entre corridas idénticas");
    }
}

#[test]
fn pinned_sobrevive_el_recorte_de_candidatos() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();

    // 210 memorias con match fuerte del término: llenan de sobra el cupo de
    // 200 candidatos que arma la consulta SQL con "order by score limit 200".
    for i in 0..210 {
        seed(&conn, &format!("c{i}"), "p", &"faro ".repeat(30));
    }

    // Memoria pinned con match débil (el término aparece una sola vez, enterrado
    // en relleno): en bm25 puro queda por debajo del corte de 200 candidatos.
    let relleno = format!("{}faro", "relleno ".repeat(50));
    seed(&conn, "pinned1", "p", &relleno);
    conn.execute("update memories set pinned = 1 where ext_id = 'pinned1'", []).unwrap();

    let q = search::Query {
        text: "faro".into(),
        project: Some("p".into()),
        limit: 5,
        ..Default::default()
    };
    let hits = search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap();
    assert_eq!(
        hits[0].ext_id.as_deref(),
        Some("pinned1"),
        "la pinned debe sobrevivir aunque su bm25 caiga fuera del top 200 de candidatos"
    );
}

mod apoyo;

#[test]
fn sin_paquete_el_resultado_es_el_de_siempre() {
    let (_d, conn) = fixture();
    let q = search::Query { text: "cache staleness".into(), ..Default::default() };
    let hits = search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap();
    assert_eq!(hits[0].ext_id.as_deref(), Some("a"));
}

#[test]
fn con_paquete_aparece_lo_que_bm25_no_encuentra() {
    // El fixture mini solo conoce "hola", "mundo" y "s". Una memoria cuyo
    // cuerpo es "mundo" no matchea la consulta "hola" por bm25, pero sí se
    // parece por vector, así que la fusión tiene que sacarla a la superficie.
    let dir = tempfile::tempdir().unwrap();
    let conn = anathem::db::open(&dir.path().join("t.db")).unwrap();
    let r = dir.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = anathem::pack::Pack::abrir(&r).unwrap();
    seed(&conn, "x", "p", "hola");
    seed(&conn, "y", "p", "mundo");
    apoyo::registrar_y_sincronizar(&conn, &p);

    let q = search::Query { text: "hola".into(), ..Default::default() };
    let solo = search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap();
    assert_eq!(solo.len(), 1, "bm25 solo encuentra la que comparte la palabra");

    let ctx = search::Contexto::de_lectura(Some(&p), 0.25);
    let con = search::run(&conn, &q, &ctx).unwrap();
    assert_eq!(con.len(), 2, "la fusión debe traer también la vectorial");
    assert_eq!(con[0].ext_id.as_deref(), Some("x"), "la exacta sigue primera");
}

#[test]
fn las_memorias_fijadas_siguen_primero_con_fusion() {
    let dir = tempfile::tempdir().unwrap();
    let conn = anathem::db::open(&dir.path().join("t.db")).unwrap();
    let r = dir.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = anathem::pack::Pack::abrir(&r).unwrap();
    seed(&conn, "x", "p", "hola");
    let fijada = seed(&conn, "y", "p", "mundo");
    anathem::maint::pin(&conn, fijada, true).unwrap();
    apoyo::registrar_y_sincronizar(&conn, &p);

    let q = search::Query { text: "hola".into(), ..Default::default() };
    let ctx = search::Contexto::de_lectura(Some(&p), 0.25);
    let hits = search::run(&conn, &q, &ctx).unwrap();
    assert_eq!(hits[0].ext_id.as_deref(), Some("y"), "la fijada va primera igual");
}

#[test]
fn consulta_sin_tokens_conocidos_no_da_resultados_espurios() {
    // Ronda de arreglos 1: el tokenizador mini solo conoce "hola" y "mundo";
    // todo lo demás cae en <unk>, cuya fila es el vector nulo. Sin el chequeo
    // de norma cero, el coseno del vector nulo contra todo es 0 y
    // `vecstore::buscar` devolvería memorias arbitrarias empatadas en 0:
    // basura con apariencia de resultado válido.
    let dir = tempfile::tempdir().unwrap();
    let conn = anathem::db::open(&dir.path().join("t.db")).unwrap();
    let r = dir.path().join("mini.anathem-pack");
    apoyo::escribir_pack_mini(&r, "mini-es");
    let p = anathem::pack::Pack::abrir(&r).unwrap();
    seed(&conn, "x", "p", "hola");
    seed(&conn, "y", "p", "mundo");
    apoyo::registrar_y_sincronizar(&conn, &p);

    let q = search::Query { text: "xyz".into(), ..Default::default() };
    let ctx = search::Contexto::de_lectura(Some(&p), 0.25);
    let hits = search::run(&conn, &q, &ctx).unwrap();
    assert!(
        hits.is_empty(),
        "una consulta sin tokens conocidos no debe devolver resultados espurios, dio {}",
        hits.len()
    );
}

/// Doce memorias con el mismo cuerpo largo (que entra en un solo chunk, ver
/// `ingest::CHUNK_SIZE`), para poder comparar cuántas entran en el mismo
/// presupuesto de tokens con y sin recorte de extracto.
fn fixture_cuerpos_largos() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    for i in 0..12 {
        let body = format!("faro {i} {}", "x".repeat(465));
        seed(&conn, &format!("faro{i}"), "p", &body);
    }
    (dir, conn)
}

#[test]
fn el_extracto_deja_entrar_mas_candidatos_en_el_mismo_presupuesto() {
    // Éste es el test que prueba que el recorte ocurre ANTES de contar el
    // costo contra el presupuesto, dentro de `run_explained`, y no después:
    // si se recortara al final (p.ej. en `mcp.rs`), el presupuesto ya habría
    // cortado la lista al tamaño que daban los fragmentos completos y
    // devolver más candidatos breves no cambiaría nada.
    let (_d, conn) = fixture_cuerpos_largos();
    let ctx = search::Contexto::solo_bm25();

    let sin_recorte = search::Query {
        text: "faro".into(),
        limit: 12,
        budget_tokens: 300,
        extracto: None,
        ..Default::default()
    };
    let con_recorte = search::Query { extracto: Some(20), ..sin_recorte.clone() };

    let hits_sin = search::run_explained(&conn, &sin_recorte, &ctx).unwrap();
    let hits_con = search::run_explained(&conn, &con_recorte, &ctx).unwrap();

    assert!(
        hits_con.len() > hits_sin.len(),
        "con extracto recortado tienen que entrar más candidatos en el mismo presupuesto: \
         sin recorte {} candidatos, con recorte {}",
        hits_sin.len(),
        hits_con.len()
    );
    assert_eq!(hits_con.len(), 12, "con extractos de 20 caracteres las 12 memorias caben en el presupuesto");
}

#[test]
fn el_extracto_recorta_el_fragmento_a_n_caracteres() {
    let (_d, conn) = fixture_cuerpos_largos();
    let ctx = search::Contexto::solo_bm25();
    let q = search::Query {
        text: "faro".into(),
        limit: 12,
        budget_tokens: usize::MAX,
        extracto: Some(20),
        ..Default::default()
    };
    let hits = search::run_explained(&conn, &q, &ctx).unwrap();
    assert!(!hits.is_empty(), "sin resultados");
    for e in &hits {
        assert_eq!(
            e.hit.snippet.chars().count(),
            20,
            "el fragmento debe quedar recortado a 20 caracteres, salió {:?}",
            e.hit.snippet
        );
    }
}

#[test]
fn el_extracto_no_parte_un_caracter_multibyte() {
    // El corpus es en español y tiene acentos: un recorte por bytes cortaría
    // un carácter UTF-8 a la mitad (p.ej. partir "ñ" o "á" en dos bytes que ya
    // no forman un carácter válido). `String` en Rust no puede contener bytes
    // inválidos, así que un recorte incorrecto no produciría un string raro:
    // haría panic al construirlo. Este test recorta justo en medio de una
    // racha de acentos para forzar ese caso.
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    // Cada carácter de relleno ocupa más de un byte en UTF-8 (2 bytes cada
    // uno), así que cortar por bytes en un índice impar partiría uno. El
    // espacio entre repeticiones es necesario para que FTS5 las tokenice
    // como palabras separadas en vez de una sola palabra de 300 caracteres.
    let body = "áéíóúñ ".repeat(50);
    seed(&conn, "acentos", "p", &body);

    for n in [1usize, 3, 5, 7, 41] {
        let q = search::Query {
            text: "áéíóúñ".into(),
            limit: 1,
            budget_tokens: usize::MAX,
            extracto: Some(n),
            ..Default::default()
        };
        let hits = search::run_explained(&conn, &q, &search::Contexto::solo_bm25()).unwrap();
        assert_eq!(hits.len(), 1, "n={n}: se esperaba 1 resultado");
        assert_eq!(
            hits[0].hit.snippet.chars().count(),
            n,
            "n={n}: el fragmento debe tener exactamente n caracteres"
        );
    }
}

#[test]
fn extracto_none_deja_el_fragmento_completo_como_antes() {
    let (_d, conn) = fixture();
    let q_viejo = search::Query { text: "cache staleness".into(), ..Default::default() };
    let q_explicito = search::Query { text: "cache staleness".into(), extracto: None, ..Default::default() };
    let ctx = search::Contexto::solo_bm25();

    let hits_viejo = search::run_explained(&conn, &q_viejo, &ctx).unwrap();
    let hits_explicito = search::run_explained(&conn, &q_explicito, &ctx).unwrap();

    assert_eq!(hits_viejo.len(), hits_explicito.len());
    for (a, b) in hits_viejo.iter().zip(hits_explicito.iter()) {
        assert_eq!(a.hit.snippet, b.hit.snippet, "extracto: None no debe alterar el fragmento");
    }
    // Y el fragmento sigue siendo el cuerpo completo, no uno recortado.
    assert!(
        hits_viejo[0].hit.snippet.chars().count() > 20,
        "sin extracto el fragmento debe seguir siendo el completo"
    );
}
