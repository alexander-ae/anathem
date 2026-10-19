use anathem::{db, ingest, model::NewMemory, search, supersede};

fn seed(conn: &rusqlite::Connection, ext: &str, body: &str) -> i64 {
    ingest::insert(conn, &NewMemory {
        project: "p".into(), title: body.chars().take(40).collect(), body: body.into(),
        created: "2026-09-21".into(), event_start: None, event_end: None,
        source: "explicit".into(), ext_id: Some(ext.into()),
        tags: vec![], refs: vec![],
    }).unwrap()
}

#[test]
fn propone_candidatos_parecidos() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    seed(&conn, "vieja", "El cache del breakdown se justifica por rendimiento");
    seed(&conn, "otra", "El design system define los colores de la plataforma");

    let c = supersede::candidates(&conn, "El cache del breakdown NO se justifica", "p", 5).unwrap();
    assert!(!c.is_empty(), "no propuso candidatos");
    assert_eq!(c[0].ext_id.as_deref(), Some("vieja"));
}

#[test]
fn link_marca_la_vieja_y_la_saca_de_la_busqueda() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let vieja = seed(&conn, "vieja", "El cache del breakdown se justifica");
    let nueva = seed(&conn, "nueva", "El cache del breakdown no se justifica, medido");
    supersede::link(&conn, nueva, vieja).unwrap();

    let st: String = conn.query_row("select status from memories where id=?1", [vieja], |r| r.get(0)).unwrap();
    assert_eq!(st, "superseded");
    let sup: i64 = conn.query_row("select supersedes_id from memories where id=?1", [nueva], |r| r.get(0)).unwrap();
    assert_eq!(sup, vieja);

    let q = search::Query { text: "cache breakdown justifica".into(), ..Default::default() };
    let hits = search::run(&conn, &q, &search::Contexto::solo_bm25()).unwrap();
    assert!(hits.iter().all(|h| h.ext_id.as_deref() != Some("vieja")), "la superseded sigue apareciendo");
}

#[test]
fn no_se_propone_a_si_misma() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let id = seed(&conn, "unica", "El cache del breakdown no se justifica");
    let c = supersede::candidates(&conn, "El cache del breakdown no se justifica", "p", 5).unwrap();
    assert!(c.iter().all(|h| h.memory_id != id) || c.is_empty(),
            "se propuso a sí misma: {c:?}");
}

// -- I1: `link` con `nueva` inexistente ------------------------------------

#[test]
fn link_con_nueva_inexistente_falla_y_no_toca_vieja() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let vieja = seed(&conn, "vieja", "El cache del breakdown se justifica");

    let err = supersede::link(&conn, 999_999, vieja)
        .expect_err("link con nueva inexistente debe fallar, no reportar éxito");
    assert!(err.to_string().contains("999999"), "mensaje inesperado: {err}");

    // vieja no debe quedar huérfana: ni marcada superseded ni con
    // supersedes_id, porque la llamada entera debe abortar antes de tocar
    // la base.
    let (status, supersedes): (String, Option<i64>) = conn.query_row(
        "select status, supersedes_id from memories where id = ?1",
        [vieja],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();
    assert_eq!(status, "active", "vieja quedó superseded apuntando a nada");
    assert_eq!(supersedes, None);
}

// -- I2: `link` no debe permitir armar un ciclo -----------------------------

#[test]
fn link_rechaza_cerrar_un_ciclo_de_dos() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let siete = seed(&conn, "siete", "El endpoint de comites ignora el filtro de estado");
    let ocho = seed(&conn, "ocho", "El endpoint de comites ignora el filtro de estado activo");

    supersede::link(&conn, siete, ocho).unwrap();
    let err = supersede::link(&conn, ocho, siete)
        .expect_err("cerrar el ciclo debe fallar, no dejar ambas superseded");

    assert!(
        err.to_string().to_lowercase().contains("ciclo")
            || err.to_string().to_lowercase().contains("activ"),
        "mensaje inesperado: {err}"
    );

    // debe sobrevivir exactamente una versión activa
    let activos: i64 = conn.query_row(
        "select count(*) from memories where id in (?1, ?2) and status = 'active'",
        [siete, ocho],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(activos, 1, "el ciclo dejó sin ninguna versión activa (o dos)");
}

// -- I3: candidates() por id, no por contenido, con un caso que distingue --

#[test]
fn candidates_no_descarta_duplicados_preexistentes_del_mismo_proyecto() {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    let texto = "El endpoint de boletas duplica el cobro de cuota mensual";

    // dos memorias activas preexistentes, mismo proyecto, cuerpo idéntico
    // entre sí -- el caso real es 17/18, pero esas están en proyectos
    // distintos; acá lo reproducimos en un mismo proyecto a propósito, que
    // es el caso que la regla por id debe cubrir aunque hoy no esté en el
    // corpus migrado.
    let dup_a = seed(&conn, "dup_a", texto);
    let dup_b = seed(&conn, "dup_b", texto);
    // una tercera, recién insertada, con el mismo cuerpo -- es la que
    // candidates() debe tratar como "self" y excluir.
    let recien_insertada = seed(&conn, "nueva", texto);

    let c = supersede::candidates(&conn, texto, "p", 5).unwrap();

    // la regla anterior (excluir por cuerpo idéntico al query) habría
    // descartado dup_a, dup_b Y recien_insertada -- las tres tienen el
    // mismo cuerpo que el query -- dejando `c` sin ninguno de los
    // duplicados reales. La regla por id solo descarta la de mayor id.
    assert!(c.iter().any(|h| h.memory_id == dup_a), "no propuso dup_a: {c:?}");
    assert!(c.iter().any(|h| h.memory_id == dup_b), "no propuso dup_b: {c:?}");
    assert!(c.iter().all(|h| h.memory_id != recien_insertada),
            "se propuso a sí misma: {c:?}");
}
