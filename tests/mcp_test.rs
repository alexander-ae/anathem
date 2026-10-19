use anathem::{config::Config, db, ingest, mcp, model::NewMemory, search};

fn conn_con_datos() -> (tempfile::TempDir, rusqlite::Connection) {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    ingest::insert(&conn, &NewMemory {
        project: "acme-billing".into(),
        title: "cache rechazado".into(),
        body: "El cache del breakdown no se justifica por staleness.".into(),
        created: "2026-09-21".into(), event_start: None, event_end: None,
        source: "migrated".into(), ext_id: Some("a".into()),
        tags: vec!["performance".into()], refs: vec![],
    }).unwrap();
    (dir, conn)
}

#[test]
fn initialize_devuelve_capabilities() {
    let (_d, conn) = conn_con_datos();
    let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
    let res = mcp::handle(&conn, &search::Contexto::solo_bm25(), &Config::default(), &req).unwrap().unwrap();
    assert_eq!(res["jsonrpc"], "2.0");
    assert_eq!(res["id"], 1);
    assert!(res["result"]["capabilities"]["tools"].is_object(), "res: {res}");
}

#[test]
fn tools_list_expone_las_cuatro() {
    let (_d, conn) = conn_con_datos();
    let req = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
    let res = mcp::handle(&conn, &search::Contexto::solo_bm25(), &Config::default(), &req).unwrap().unwrap();
    let names: Vec<String> = res["result"]["tools"].as_array().unwrap().iter()
        .map(|t| t["name"].as_str().unwrap().to_string()).collect();
    for n in ["search_memory", "save_memory", "get_memory", "list_tags"] {
        assert!(names.contains(&n.to_string()), "falta {n} en {names:?}");
    }
}

#[test]
fn search_memory_devuelve_resultados() {
    let (_d, conn) = conn_con_datos();
    let req = serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"search_memory","arguments":{"query":"cache staleness"}}});
    let res = mcp::handle(&conn, &search::Contexto::solo_bm25(), &Config::default(), &req).unwrap().unwrap();
    let text = res["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("cache"), "texto devuelto: {text}");
}

#[test]
fn save_memory_persiste() {
    let (_d, conn) = conn_con_datos();
    let req = serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
        "params":{"name":"save_memory","arguments":{
            "text":"El job programado del hub sincroniza 12 servicios.","project":"acme-billing"}}});
    mcp::handle(&conn, &search::Contexto::solo_bm25(), &Config::default(), &req).unwrap().unwrap();
    let n: i64 = conn.query_row(
        "select count(*) from memories where source='explicit'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}

#[test]
fn metodo_desconocido_devuelve_error_jsonrpc() {
    let (_d, conn) = conn_con_datos();
    let req = serde_json::json!({"jsonrpc":"2.0","id":5,"method":"no/existe"});
    let res = mcp::handle(&conn, &search::Contexto::solo_bm25(), &Config::default(), &req).unwrap().unwrap();
    assert_eq!(res["error"]["code"], -32601);
}

#[test]
fn notificacion_no_produce_respuesta() {
    let (_d, conn) = conn_con_datos();
    let req = serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"});
    assert!(mcp::handle(&conn, &search::Contexto::solo_bm25(), &Config::default(), &req).unwrap().is_none(), "una notificación no lleva respuesta");
}

#[test]
fn la_busqueda_queda_en_el_log() {
    let (_d, conn) = conn_con_datos();
    let req = serde_json::json!({"jsonrpc":"2.0","id":6,"method":"tools/call",
        "params":{"name":"search_memory","arguments":{"query":"cache"}}});
    mcp::handle(&conn, &search::Contexto::solo_bm25(), &Config::default(), &req).unwrap();
    let n: i64 = conn.query_row("select count(*) from search_log", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}

#[test]
fn search_memory_respeta_el_limite_pedido() {
    // Sin `limit` no hay forma de pedirle al servidor más candidatos de los
    // diez por defecto, que es el parámetro que hace falta poder mover para
    // evaluar si conviene devolver más y más cortos.
    let (_d, conn) = fixture_con_varias();
    let ctx = search::Contexto::solo_bm25();
    let pedir = |n: u64| {
        let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"search_memory","arguments":{"query":"memoria relleno","limit":n,"budget":100000}}});
        let r = mcp::handle(&conn, &ctx, &Config::default(), &req).unwrap().unwrap();
        r["result"]["content"][0]["text"].as_str().unwrap()
            .lines().filter(|l| l.starts_with("[id:")).count()
    };
    let pocos = pedir(2);
    let muchos = pedir(8);
    assert_eq!(pocos, 2, "con limit 2 tienen que volver 2");
    assert!(muchos > pocos, "con limit 8 tienen que volver más: {muchos} vs {pocos}");
}

fn fixture_con_varias() -> (tempfile::TempDir, rusqlite::Connection) {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    for i in 0..8 {
        anathem::ingest::insert(&conn, &anathem::model::NewMemory {
            project: "p".into(),
            title: format!("memoria numero {i}"),
            body: format!("memoria numero {i} con mucho relleno {}", "texto ".repeat(80)),
            created: "2026-09-01".into(), event_start: None, event_end: None,
            source: "explicit".into(), ext_id: None, tags: vec![], refs: vec![],
        }).unwrap();
    }
    (dir, conn)
}

#[test]
fn search_memory_devuelve_veinte_candidatos_por_defecto() {
    // Sin `limit` explícito, la tool tiene que usar `Config::default().candidatos`
    // (20) y no los 10 de antes: es la mitad del cambio de diseño (candidatos
    // más numerosos, extracto más corto, mismo presupuesto).
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    for i in 0..25 {
        anathem::ingest::insert(&conn, &anathem::model::NewMemory {
            project: "p".into(),
            title: format!("memoria numero {i}"),
            body: format!("memoria numero {i} con mucho relleno {}", "texto ".repeat(80)),
            created: "2026-09-01".into(), event_start: None, event_end: None,
            source: "explicit".into(), ext_id: None, tags: vec![], refs: vec![],
        }).unwrap();
    }
    let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"search_memory","arguments":{"query":"memoria relleno"}}});
    let res = mcp::handle(&conn, &search::Contexto::solo_bm25(), &Config::default(), &req).unwrap().unwrap();
    let n = res["result"]["content"][0]["text"].as_str().unwrap()
        .lines().filter(|l| l.starts_with("[id:")).count();
    assert_eq!(n, 20, "con 25 memorias que matchean, sin limit deben volver los 20 candidatos por defecto");
}
