//! Clave de tema: guardar con la misma clave reemplaza (por supersession) a la
//! memoria activa anterior del mismo proyecto.

use anathem::{config::Config, db, ingest, mcp, search};
use std::path::Path;
use std::process::Command;

/// Corre `mem` contra la base de `dir`. Devuelve (éxito, stdout, stderr).
fn mem(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_mem"))
        .arg("--db")
        .arg(dir.join("mem.db"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

fn mem_ok(dir: &Path, args: &[&str]) -> String {
    let (ok, out, err) = mem(dir, args);
    assert!(ok, "mem {args:?} falló: {err}");
    out
}

fn estado(dir: &Path, id: i64) -> String {
    let conn = rusqlite::Connection::open(dir.join("mem.db")).unwrap();
    conn.query_row("select status from memories where id = ?1", [id], |r| r.get(0)).unwrap()
}

const PAGOS_VIEJO: &str = "El proveedor de pagos es Stripe";
const PAGOS_NUEVO: &str = "El proveedor de pagos es Adyen desde octubre";

#[test]
fn guardar_dos_veces_con_la_misma_clave_deja_la_primera_superseded_y_la_segunda_activa() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "pagos"]);

    assert_eq!(estado(d.path(), 1), "superseded");
    assert_eq!(estado(d.path(), 2), "active");
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let sup: i64 = conn
        .query_row("select supersedes_id from memories where id = 2", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sup, 1, "la nueva tiene que apuntar a la que reemplazó");
}

#[test]
fn la_vieja_deja_de_aparecer_en_la_busqueda_y_la_nueva_si() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "pagos"]);

    let out = mem_ok(d.path(), &["search", "proveedor pagos"]);
    assert!(out.contains("Adyen"), "la nueva tiene que aparecer: {out}");
    assert!(!out.contains("Stripe"), "la vieja sigue apareciendo: {out}");
}

#[test]
fn history_sobre_la_nueva_muestra_la_cadena_hasta_la_vieja() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", "El proveedor de pagos es Adyen", "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "pagos"]);

    let out = mem_ok(d.path(), &["history", "3"]);
    let lineas: Vec<&str> = out.lines().collect();
    assert_eq!(lineas.len(), 3, "la cadena tiene tres eslabones: {out}");
    assert!(lineas[0].contains("active") && lineas[0].contains("desde octubre"), "{out}");
    assert!(lineas[1].contains("superseded") && lineas[1].contains("Adyen"), "{out}");
    assert!(lineas[2].contains("superseded") && lineas[2].contains("Stripe"), "{out}");
}

#[test]
fn la_misma_clave_en_dos_proyectos_no_se_pisa() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", "La base de a es Postgres", "--project", "a", "--topic", "base-de-datos"]);
    mem_ok(d.path(), &["save", "La base de b es MySQL", "--project", "b", "--topic", "base-de-datos"]);

    assert_eq!(estado(d.path(), 1), "active");
    assert_eq!(estado(d.path(), 2), "active");
}

#[test]
fn sin_clave_de_tema_guardar_dos_veces_deja_las_dos_activas() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p"]);
    mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p"]);

    assert_eq!(estado(d.path(), 1), "active");
    assert_eq!(estado(d.path(), 2), "active");
    // y sin clave no queda nada que listar
    assert!(mem_ok(d.path(), &["topics"]).contains("Sin claves de tema"));
}

#[test]
fn guardar_con_una_clave_nueva_simplemente_guarda() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", "Otro hecho", "--project", "p", "--topic", "otro"]);
    let out = mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);

    assert_eq!(estado(d.path(), 1), "active", "una clave distinta no toca a las demás");
    assert_eq!(estado(d.path(), 2), "active");
    assert!(out.contains("guardada memoria 2"), "{out}");
    assert!(out.contains("pagos") && out.contains("nueva"), "tiene que decir que la clave era nueva: {out}");
}

#[test]
fn al_reemplazar_la_salida_dice_a_que_memoria_reemplazo_con_su_id() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    let out = mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "pagos"]);

    assert!(out.contains("guardada memoria 2"), "{out}");
    assert!(out.contains("reemplaza a la memoria 1"), "tiene que nombrar el id reemplazado: {out}");
    assert!(out.contains("Stripe"), "tiene que mostrar el título reemplazado: {out}");
}

#[test]
fn topics_lista_proyecto_cantidad_y_vigente() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", "La base es Postgres", "--project", "q", "--topic", "base"]);
    mem_ok(d.path(), &["save", "Sin clave, no figura", "--project", "p"]);

    let out = mem_ok(d.path(), &["topics"]);
    let fila = |clave: &str| -> Vec<String> {
        out.lines()
            .find(|l| l.split_whitespace().nth(1) == Some(clave))
            .unwrap_or_else(|| panic!("falta la clave {clave} en:\n{out}"))
            .split_whitespace()
            .map(str::to_string)
            .collect()
    };
    assert_eq!(fila("pagos"), ["p", "pagos", "2", "2"], "{out}");
    assert_eq!(fila("base"), ["q", "base", "1", "3"], "{out}");
    assert_eq!(out.lines().count(), 3, "cabecera y dos claves, nada más: {out}");
}

#[test]
fn topics_dice_que_no_hay_vigente_si_la_unica_se_olvido() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["forget", "1"]);

    let out = mem_ok(d.path(), &["topics"]);
    assert!(out.contains("pagos") && out.contains("ninguna"), "{out}");
}

#[test]
fn si_la_vigente_se_olvido_la_clave_vuelve_a_guardar_sin_reemplazar() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["forget", "1"]);
    let out = mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "pagos"]);

    assert!(!out.contains("reemplaza a"), "no había una activa que reemplazar: {out}");
    assert_eq!(estado(d.path(), 1), "forgotten", "no se revive lo olvidado");
    assert_eq!(estado(d.path(), 2), "active");
}

#[test]
fn una_clave_vacia_se_rechaza_y_no_guarda_nada() {
    let d = tempfile::tempdir().unwrap();
    let (ok, _, err) = mem(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "  "]);
    assert!(!ok, "una clave vacía tiene que fallar");
    assert!(err.contains("clave de tema"), "{err}");
    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let n: i64 = conn.query_row("select count(*) from memories", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn la_clave_se_recorta_asi_que_con_espacios_alrededor_es_el_mismo_tema() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", " pagos "]);
    assert_eq!(estado(d.path(), 1), "superseded");
}

#[test]
fn mayusculas_y_espacios_internos_no_hacen_otro_tema() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "proveedor de pagos"]);
    let out = mem_ok(d.path(), &["save", PAGOS_NUEVO, "--project", "p", "--topic", "  Proveedor   DE\tPagos "]);

    assert_eq!(estado(d.path(), 1), "superseded", "{out}");
    assert!(out.contains("«proveedor de pagos»"), "muestra la clave normalizada: {out}");
    let topics = mem_ok(d.path(), &["topics"]);
    assert!(topics.contains("proveedor de pagos") && !topics.contains("Proveedor"), "{topics}");
}

#[test]
fn una_clave_nueva_lista_las_que_ya_existen_en_el_proyecto() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", "El stack es React", "--project", "p", "--topic", "stack-del-frontend"]);
    mem_ok(d.path(), &["save", "Otra cosa", "--project", "p", "--topic", "politica-de-ramas"]);
    mem_ok(d.path(), &["save", "De otro proyecto", "--project", "q", "--topic", "solo-en-q"]);
    // `stack-frontend` no coincide con `stack-del-frontend`: sin la lista,
    // el mensaje sería idéntico al de una clave legítimamente nueva.
    let out = mem_ok(d.path(), &["save", "El stack es Svelte", "--project", "p", "--topic", "stack-frontend"]);

    assert!(out.contains("stack-del-frontend") && out.contains("politica-de-ramas"), "{out}");
    assert!(!out.contains("solo-en-q"), "solo las del mismo proyecto: {out}");
    assert_eq!(estado(d.path(), 1), "active");
}

#[test]
fn si_no_hay_otras_claves_el_mensaje_lo_dice() {
    let d = tempfile::tempdir().unwrap();
    let out = mem_ok(d.path(), &["save", "Primera", "--project", "p", "--topic", "unica"]);
    assert!(out.contains("No hay otras claves"), "{out}");
}

#[test]
fn al_reemplazar_no_se_listan_otras_claves() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", "Uno", "--project", "p", "--topic", "a"]);
    mem_ok(d.path(), &["save", "Dos", "--project", "p", "--topic", "b"]);
    let out = mem_ok(d.path(), &["save", "Dos bis", "--project", "p", "--topic", "b"]);
    assert!(out.contains("reemplaza a la memoria 2"), "{out}");
    assert!(!out.contains("Claves que ya existen"), "{out}");
}

// -- la capa vectorial ------------------------------------------------------

#[test]
fn la_busqueda_vectorial_tampoco_devuelve_la_memoria_reemplazada() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let vieja = ingest::guardar_con_tema(&conn, PAGOS_VIEJO, "p", vec![], Some("pagos"), None).unwrap();
    let nueva = ingest::guardar_con_tema(&conn, PAGOS_NUEVO, "p", vec![], Some("pagos"), None).unwrap();

    // El mismo vector para los fragmentos de las dos: si alguna se filtra, es
    // por el estado y no por el parecido.
    let blob: Vec<u8> = [1.0f32, 0.0].iter().flat_map(|f| f.to_le_bytes()).collect();
    for id in [vieja.id, nueva.id] {
        conn.execute(
            "insert into chunk_vectors (chunk_id, vec)
             select id, ?2 from chunks where memory_id = ?1",
            rusqlite::params![id, blob],
        )
        .unwrap();
    }

    let q = search::Query { text: "pagos".into(), ..Default::default() };
    let hits = anathem::vecstore::buscar(&conn, &[1.0, 0.0], &q, 10).unwrap();
    let ids: Vec<i64> = hits.iter().map(|h| h.memory_id).collect();
    assert_eq!(ids, vec![nueva.id], "solo la vigente puede salir por vector: {ids:?}");
}

// -- el guardado lo hace una sola transacción --------------------------------

#[test]
fn si_el_enlace_falla_no_queda_la_memoria_nueva_a_medias() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let primera = ingest::guardar_con_tema(&conn, PAGOS_VIEJO, "p", vec![], Some("pagos"), None).unwrap();
    // Un trigger que aborta el supersede simula cualquier falla del enlace.
    conn.execute_batch(
        "create trigger no_superseder before update of status on memories
         when new.status = 'superseded' begin select raise(abort, 'falla simulada'); end;",
    )
    .unwrap();

    let err = ingest::guardar_con_tema(&conn, PAGOS_NUEVO, "p", vec![], Some("pagos"), None);
    assert!(err.is_err(), "la falla del enlace tiene que propagarse");

    let n: i64 = conn.query_row("select count(*) from memories", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "el insert se tiene que deshacer junto con el enlace");
    let st: String = conn
        .query_row("select status from memories where id = ?1", [primera.id], |r| r.get(0))
        .unwrap();
    assert_eq!(st, "active");
}

// -- varios procesos a la vez -------------------------------------------------

#[test]
fn varios_save_con_la_misma_clave_a_la_vez_tienen_exito_y_dejan_una_sola_activa() {
    // Con una transacción diferida, cada proceso lee la vigente con un lock
    // de lectura y después intenta subirlo a escritura: SQLite no espera, falla
    // al instante con `database is locked` (los que pierden la carrera salen
    // con código 1 y su memoria se pierde). Con `begin immediate` el lock de
    // escritura se toma antes de leer y el segundo espera su turno.
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["init"]);

    const N: usize = 12;
    let hijos: Vec<_> = (0..N)
        .map(|i| {
            Command::new(env!("CARGO_BIN_EXE_mem"))
                .arg("--db")
                .arg(d.path().join("mem.db"))
                .args(["save", &format!("El proveedor de pagos, versión {i}")])
                .args(["--project", "p", "--topic", "pagos"])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for (i, h) in hijos.into_iter().enumerate() {
        let o = h.wait_with_output().unwrap();
        assert!(
            o.status.success(),
            "el guardado {i} falló: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }

    let conn = rusqlite::Connection::open(d.path().join("mem.db")).unwrap();
    let total: i64 = conn.query_row("select count(*) from memories", [], |r| r.get(0)).unwrap();
    assert_eq!(total, N as i64, "se perdió algún guardado");
    let activas: i64 = conn
        .query_row(
            "select count(*) from memories where project='p' and topic='pagos' and status='active'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(activas, 1, "tiene que quedar exactamente una activa por clave");
}

// -- MCP ---------------------------------------------------------------------

fn llamar(conn: &rusqlite::Connection, tool: &str, args: serde_json::Value) -> String {
    let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":tool,"arguments":args}});
    let res = mcp::handle(conn, &search::Contexto::solo_bm25(), &Config::default(), &req)
        .unwrap()
        .unwrap();
    res["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[test]
fn save_memory_con_topic_reemplaza_e_informa_a_cual() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    llamar(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_VIEJO, "project": "p", "topic": "pagos"}));
    let out = llamar(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "topic": "pagos"}));

    assert!(out.contains("reemplaza a la memoria 1"), "{out}");
    let st: String = conn.query_row("select status from memories where id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(st, "superseded");
}

#[test]
fn save_memory_sin_topic_o_con_topic_vacio_no_reemplaza_nada() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    llamar(&conn, "save_memory", serde_json::json!({"text": PAGOS_VIEJO, "project": "p"}));
    llamar(&conn, "save_memory", serde_json::json!({"text": PAGOS_NUEVO, "project": "p"}));
    llamar(&conn, "save_memory", serde_json::json!({
        "text": "Otro dato sobre pagos", "project": "p", "topic": ""}));

    let activas: i64 = conn
        .query_row("select count(*) from memories where status = 'active'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(activas, 3);
}

#[test]
fn la_descripcion_de_save_memory_explica_cuando_usar_topic() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let req = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
    let res = mcp::handle(&conn, &search::Contexto::solo_bm25(), &Config::default(), &req)
        .unwrap()
        .unwrap();
    let tools = res["result"]["tools"].as_array().unwrap();
    let save = tools.iter().find(|t| t["name"] == "save_memory").unwrap();

    assert!(save["inputSchema"]["properties"]["topic"].is_object(), "falta el parámetro topic");
    let desc = save["description"].as_str().unwrap().to_lowercase();
    for clave in ["topic", "corrección", "reemplaza", "versión actualizada"] {
        assert!(desc.contains(clave), "la descripción no menciona {clave:?}: {desc}");
    }
}

#[test]
fn search_y_get_memory_muestran_la_clave_para_poder_reutilizarla() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    llamar(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_VIEJO, "project": "p", "topic": "pagos"}));

    let busqueda = llamar(&conn, "search_memory", serde_json::json!({"query": "proveedor pagos"}));
    assert!(busqueda.contains("[tema: pagos]"), "{busqueda}");
    let cuerpo = llamar(&conn, "get_memory", serde_json::json!({"id": 1}));
    assert!(cuerpo.contains("[tema: pagos]"), "{cuerpo}");
}

// -- supersedes: corregir una memoria concreta ya guardada ------------------------

fn total(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("select count(*) from memories", [], |r| r.get(0)).unwrap()
}

fn status(conn: &rusqlite::Connection, id: i64) -> String {
    conn.query_row("select status from memories where id = ?1", [id], |r| r.get(0)).unwrap()
}

#[test]
fn supersedes_corrige_una_memoria_guardada_sin_clave() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    llamar(&conn, "save_memory", serde_json::json!({"text": PAGOS_VIEJO, "project": "p"}));
    let out = llamar(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "supersedes": 1}));

    assert!(out.contains("Reemplaza a la memoria 1"), "{out}");
    assert_eq!(status(&conn, 1), "superseded");
    assert_eq!(status(&conn, 2), "active");
    let sup: i64 = conn.query_row("select supersedes_id from memories where id = 2", [], |r| r.get(0)).unwrap();
    assert_eq!(sup, 1);
    let hits = llamar(&conn, "search_memory", serde_json::json!({"query": "proveedor pagos"}));
    assert!(hits.contains("Adyen") && !hits.contains("Stripe"), "{hits}");
}

#[test]
fn supersedes_acepta_el_id_como_texto() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    llamar(&conn, "save_memory", serde_json::json!({"text": PAGOS_VIEJO, "project": "p"}));
    llamar(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "supersedes": " 1 "}));
    assert_eq!(status(&conn, 1), "superseded");
}

#[test]
fn supersedes_que_no_es_un_id_no_guarda_nada() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let out = llamar(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "supersedes": "la anterior"}));
    assert!(out.starts_with("No se guardó"), "{out}");
    assert_eq!(total(&conn), 0);
}

#[test]
fn supersedes_de_una_memoria_inexistente_se_rechaza_con_texto_y_no_guarda_nada() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let out = llamar(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "supersedes": 999999}));
    assert!(out.starts_with("No se guardó") && out.contains("999999"), "{out}");
    assert_eq!(total(&conn), 0, "el rechazo no puede dejar la memoria nueva guardada");
}

#[test]
fn supersedes_de_una_memoria_que_no_esta_activa_se_rechaza() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    llamar(&conn, "save_memory", serde_json::json!({"text": PAGOS_VIEJO, "project": "p"}));
    anathem::maint::forget(&conn, 1).unwrap();
    let out = llamar(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "supersedes": 1}));

    assert!(out.starts_with("No se guardó") && out.contains("forgotten"), "{out}");
    assert_eq!(total(&conn), 1);
    assert_eq!(status(&conn, 1), "forgotten", "no se revive lo olvidado");
}

#[test]
fn supersedes_y_topic_apuntando_a_la_misma_memoria_reemplazan_una_sola_vez() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    llamar(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_VIEJO, "project": "p", "topic": "pagos"}));
    let out = llamar(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "topic": "pagos", "supersedes": 1}));

    assert!(out.contains("Tema «pagos»: reemplaza a la memoria 1"), "{out}");
    assert_eq!(status(&conn, 1), "superseded");
    assert_eq!(status(&conn, 2), "active");
}

#[test]
fn supersedes_y_topic_que_apuntan_a_memorias_distintas_se_rechazan() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    llamar(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_VIEJO, "project": "p", "topic": "pagos"}));
    llamar(&conn, "save_memory", serde_json::json!({"text": "Otra cosa", "project": "p"}));
    let out = llamar(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "topic": "pagos", "supersedes": 2}));

    assert!(out.starts_with("No se guardó"), "{out}");
    assert!(out.contains("id:1") && out.contains("id:2"), "nombra las dos: {out}");
    assert_eq!(total(&conn), 2);
    assert_eq!(status(&conn, 1), "active");
    assert_eq!(status(&conn, 2), "active");
}

#[test]
fn supersedes_con_una_clave_nueva_deja_la_clave_lista_para_la_proxima() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    llamar(&conn, "save_memory", serde_json::json!({"text": PAGOS_VIEJO, "project": "p"}));
    let out = llamar(&conn, "save_memory", serde_json::json!({
        "text": PAGOS_NUEVO, "project": "p", "topic": "pagos", "supersedes": 1}));
    assert!(out.contains("Reemplaza a la memoria 1") && out.contains("clave nueva"), "{out}");

    // La siguiente con la clave ya reemplaza a la que corrigió la anterior.
    llamar(&conn, "save_memory", serde_json::json!({
        "text": "El proveedor de pagos es Mercado Pago", "project": "p", "topic": "pagos"}));
    assert_eq!(status(&conn, 2), "superseded");
    assert_eq!(status(&conn, 3), "active");
}

#[test]
fn la_descripcion_distingue_cuando_usar_supersedes_y_cuando_topic() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let req = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
    let res = mcp::handle(&conn, &search::Contexto::solo_bm25(), &Config::default(), &req)
        .unwrap()
        .unwrap();
    let save = res["result"]["tools"].as_array().unwrap().iter()
        .find(|t| t["name"] == "save_memory").unwrap().clone();

    assert_eq!(save["inputSchema"]["properties"]["supersedes"]["type"], "integer");
    let desc = save["description"].as_str().unwrap();
    assert!(desc.contains("`supersedes`") && desc.contains("`topic`"), "{desc}");
    assert!(desc.contains("SEGUIR EVOLUCIONANDO"), "cuándo va topic: {desc}");
    assert!(desc.contains("MEMORIA CONCRETA"), "cuándo va supersedes: {desc}");
}

// -- export ------------------------------------------------------------------

#[test]
fn el_export_conserva_la_clave_de_tema() {
    let d = tempfile::tempdir().unwrap();
    mem_ok(d.path(), &["save", PAGOS_VIEJO, "--project", "p", "--topic", "pagos"]);
    mem_ok(d.path(), &["save", "Sin clave", "--project", "p"]);
    let salida = d.path().join("out");
    mem_ok(d.path(), &["export", "--out", salida.to_str().unwrap()]);

    let mut con_tema = 0;
    for f in std::fs::read_dir(salida.join("p")).unwrap() {
        let txt = std::fs::read_to_string(f.unwrap().path()).unwrap();
        if txt.contains("topic: \"pagos\"") {
            con_tema += 1;
        } else {
            assert!(!txt.contains("topic:"), "sin clave no se emite el campo: {txt}");
        }
    }
    assert_eq!(con_tema, 1);
}

// -- una base anterior a este cambio ------------------------------------------

/// El esquema tal cual era antes de la clave de tema: sin `memories.topic`.
const ESQUEMA_ANTERIOR: &str = r#"
pragma journal_mode = wal;
pragma foreign_keys = on;

create table if not exists memories (
  id            integer primary key,
  project       text not null,
  title         text not null,
  body          text not null,
  created       text not null,
  updated       text not null,
  event_start   text,
  event_end     text,
  source        text not null check (source in ('explicit','suggested','migrated')),
  status        text not null default 'active'
                check (status in ('active','superseded','draft','forgotten')),
  pinned        integer not null default 0,
  last_accessed text,
  access_count  integer not null default 0,
  supersedes_id integer references memories(id),
  ext_id        text unique
);

create table if not exists memory_tags (
  memory_id integer not null references memories(id) on delete cascade,
  tag       text not null,
  primary key (memory_id, tag)
);

create table if not exists memory_refs (
  memory_id integer not null references memories(id) on delete cascade,
  kind      text not null check (kind in ('ticket','pr','sha')),
  value     text not null,
  primary key (memory_id, kind, value)
);

create table if not exists chunks (
  id        integer primary key,
  memory_id integer not null references memories(id) on delete cascade,
  ord       integer not null,
  body      text not null
);

create virtual table if not exists chunks_fts using fts5(
  body,
  content='chunks',
  content_rowid='id',
  tokenize="unicode61 remove_diacritics 2"
);

create table if not exists chunk_vectors (
  chunk_id integer primary key references chunks(id) on delete cascade,
  vec      blob not null   -- f32 little endian x dims, ya normalizado
);

create table if not exists meta (
  clave text primary key,
  valor text not null
);

create table if not exists search_log (
  id           integer primary key,
  ts           text not null,
  query        text not null,
  project      text,
  returned_ids text not null,
  budget_used  integer not null,
  followed_up  integer not null default 0
);

create index if not exists idx_mem_project_status on memories(project, status);
create index if not exists idx_chunks_memory      on chunks(memory_id);
create index if not exists idx_tags_tag           on memory_tags(tag);
create index if not exists idx_refs_value         on memory_refs(value);

create trigger if not exists chunks_ai after insert on chunks begin
  insert into chunks_fts(rowid, body) values (new.id, new.body);
end;
create trigger if not exists chunks_ad after delete on chunks begin
  insert into chunks_fts(chunks_fts, rowid, body) values ('delete', old.id, old.body);
end;
create trigger if not exists chunks_au after update on chunks begin
  insert into chunks_fts(chunks_fts, rowid, body) values ('delete', old.id, old.body);
  insert into chunks_fts(rowid, body) values (new.id, new.body);
end;
"#;

fn base_anterior(dir: &Path) {
    let conn = rusqlite::Connection::open(dir.join("mem.db")).unwrap();
    conn.execute_batch(ESQUEMA_ANTERIOR).unwrap();
    conn.execute(
        "insert into memories (project, title, body, created, updated, source)
         values ('p', 'La base es Postgres', 'La base es Postgres', '2026-01-01', '2026-01-01', 'explicit')",
        [],
    )
    .unwrap();
    conn.execute("insert into chunks (memory_id, ord, body) values (1, 0, 'La base es Postgres')", [])
        .unwrap();
    let tiene: i64 = conn
        .query_row("select count(*) from pragma_table_info('memories') where name='topic'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(tiene, 0, "la base de partida no debería tener la columna topic");
}

#[test]
fn una_base_anterior_sigue_abriendose_y_conserva_sus_memorias() {
    let d = tempfile::tempdir().unwrap();
    base_anterior(d.path());

    let out = mem_ok(d.path(), &["search", "postgres"]);
    assert!(out.contains("La base es Postgres"), "{out}");
    // abrirla dos veces no puede fallar por la columna ya agregada
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    db::open(&d.path().join("mem.db")).unwrap();
    let n: i64 = conn.query_row("select count(*) from memories where topic is null", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "la memoria vieja queda sin clave");
}

#[test]
fn una_base_anterior_acepta_guardar_con_clave_y_listar_temas() {
    let d = tempfile::tempdir().unwrap();
    base_anterior(d.path());

    mem_ok(d.path(), &["save", "La base es MySQL", "--project", "p", "--topic", "base"]);
    let out = mem_ok(d.path(), &["save", "La base es Oracle", "--project", "p", "--topic", "base"]);
    assert!(out.contains("reemplaza a la memoria 2"), "{out}");
    assert_eq!(estado(d.path(), 1), "active", "la anterior sin clave no se toca");
    assert_eq!(estado(d.path(), 2), "superseded");
    assert!(mem_ok(d.path(), &["topics"]).contains("base"));
}

#[test]
fn un_indice_sobre_proyecto_y_clave_existe() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let cols: Vec<String> = conn
        .prepare("select name from pragma_index_info('idx_mem_project_topic') order by seqno")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(cols, ["project", "topic"]);
}

// -- invariante estructural: una activa por (proyecto, clave) ---------------------

fn fila_con_clave(conn: &rusqlite::Connection, project: &str, topic: Option<&str>, status: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "insert into memories (project, title, body, created, updated, source, status, topic)
         values (?1, 't', 'b', '2026-01-01', '2026-01-01', 'explicit', ?3, ?2)",
        rusqlite::params![project, topic, status],
    )
}

#[test]
fn la_base_impide_dos_activas_con_la_misma_clave_en_el_mismo_proyecto() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    fila_con_clave(&conn, "p", Some("pagos"), "active").unwrap();
    let err = fila_con_clave(&conn, "p", Some("pagos"), "active")
        .expect_err("una segunda activa con la misma clave tiene que rechazarse");
    assert!(err.to_string().contains("UNIQUE"), "{err}");
}

#[test]
fn el_indice_unico_no_molesta_a_lo_que_no_es_la_vigente() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    fila_con_clave(&conn, "p", Some("pagos"), "active").unwrap();
    // otro proyecto, misma clave
    fila_con_clave(&conn, "q", Some("pagos"), "active").unwrap();
    // la historia de la clave: superseded y forgotten pueden ser varias
    for st in ["superseded", "superseded", "forgotten"] {
        fila_con_clave(&conn, "p", Some("pagos"), st).unwrap();
    }
    // sin clave no hay tope
    for _ in 0..3 {
        fila_con_clave(&conn, "p", None, "active").unwrap();
    }
}

#[test]
fn revivir_una_olvidada_cuando_ya_hay_vigente_falla_en_vez_de_dejar_dos_activas() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("t.db")).unwrap();
    let vieja = ingest::guardar_con_tema(&conn, PAGOS_VIEJO, "p", vec![], Some("pagos"), None).unwrap();
    anathem::maint::forget(&conn, vieja.id).unwrap();
    ingest::guardar_con_tema(&conn, PAGOS_NUEVO, "p", vec![], Some("pagos"), None).unwrap();

    let r = conn.execute("update memories set status = 'active' where id = ?1", [vieja.id]);
    assert!(r.is_err(), "revivir la olvidada habría dejado dos activas para la clave");
}

#[test]
fn una_base_anterior_tambien_recibe_el_indice_unico() {
    let d = tempfile::tempdir().unwrap();
    base_anterior(d.path());
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    fila_con_clave(&conn, "p", Some("k"), "active").unwrap();
    assert!(fila_con_clave(&conn, "p", Some("k"), "active").is_err());
}
