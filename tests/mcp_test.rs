use anathem::{config::Config, db, ingest, mcp, model::NewMemory, search};

fn conn_with_data() -> (tempfile::TempDir, rusqlite::Connection) {
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
    let (_d, conn) = conn_with_data();
    let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
    let res = mcp::handle(&conn, &search::SearchContext::bm25_only(), &Config::default(), &req).unwrap().unwrap();
    assert_eq!(res["jsonrpc"], "2.0");
    assert_eq!(res["id"], 1);
    assert!(res["result"]["capabilities"]["tools"].is_object(), "res: {res}");
}

#[test]
fn tools_list_exposes_all_four() {
    let (_d, conn) = conn_with_data();
    let req = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
    let res = mcp::handle(&conn, &search::SearchContext::bm25_only(), &Config::default(), &req).unwrap().unwrap();
    let names: Vec<String> = res["result"]["tools"].as_array().unwrap().iter()
        .map(|t| t["name"].as_str().unwrap().to_string()).collect();
    for n in ["search_memory", "save_memory", "get_memory", "list_tags"] {
        assert!(names.contains(&n.to_string()), "falta {n} en {names:?}");
    }
}

#[test]
fn search_memory_returns_results() {
    let (_d, conn) = conn_with_data();
    let req = serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"search_memory","arguments":{"query":"cache staleness"}}});
    let res = mcp::handle(&conn, &search::SearchContext::bm25_only(), &Config::default(), &req).unwrap().unwrap();
    let text = res["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("cache"), "texto devuelto: {text}");
}

#[test]
fn save_memory_persiste() {
    let (_d, conn) = conn_with_data();
    let req = serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
        "params":{"name":"save_memory","arguments":{
            "text":"El job programado del hub sincroniza 12 servicios.","project":"acme-billing"}}});
    mcp::handle(&conn, &search::SearchContext::bm25_only(), &Config::default(), &req).unwrap().unwrap();
    let n: i64 = conn.query_row(
        "select count(*) from memories where source='explicit'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}

#[test]
fn an_unknown_method_returns_a_jsonrpc_error() {
    let (_d, conn) = conn_with_data();
    let req = serde_json::json!({"jsonrpc":"2.0","id":5,"method":"no/existe"});
    let res = mcp::handle(&conn, &search::SearchContext::bm25_only(), &Config::default(), &req).unwrap().unwrap();
    assert_eq!(res["error"]["code"], -32601);
}

#[test]
fn a_notification_produces_no_response() {
    let (_d, conn) = conn_with_data();
    let req = serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"});
    assert!(mcp::handle(&conn, &search::SearchContext::bm25_only(), &Config::default(), &req).unwrap().is_none(), "una notificación no lleva respuesta");
}

#[test]
fn the_search_is_recorded_in_the_log() {
    let (_d, conn) = conn_with_data();
    let req = serde_json::json!({"jsonrpc":"2.0","id":6,"method":"tools/call",
        "params":{"name":"search_memory","arguments":{"query":"cache"}}});
    mcp::handle(&conn, &search::SearchContext::bm25_only(), &Config::default(), &req).unwrap();
    let n: i64 = conn.query_row("select count(*) from search_log", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}

#[test]
fn search_memory_respects_the_requested_limit() {
    // With no `limit` there is no way to ask the server for more than the
    // default ten candidates, which is the parameter that has to be movable
    // in order to evaluate whether returning more and shorter ones pays
    // off.
    let (_d, conn) = fixture_with_several();
    let ctx = search::SearchContext::bm25_only();
    let ask_for = |n: u64| {
        let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"search_memory","arguments":{"query":"memoria relleno","limit":n,"budget":100000}}});
        let r = mcp::handle(&conn, &ctx, &Config::default(), &req).unwrap().unwrap();
        r["result"]["content"][0]["text"].as_str().unwrap()
            .lines().filter(|l| l.starts_with("[id:")).count()
    };
    let few = ask_for(2);
    let many = ask_for(8);
    assert_eq!(few, 2, "con limit 2 tienen que volver 2");
    assert!(many > few, "con limit 8 tienen que volver más: {many} vs {few}");
}

fn fixture_with_several() -> (tempfile::TempDir, rusqlite::Connection) {
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
fn search_memory_returns_twenty_candidates_by_default() {
    // With no explicit `limit`, the tool has to use
    // `Config::default().candidates` (20) and not the earlier 10: it is half
    // of the design change (more numerous candidates, shorter excerpt, same
    // budget).
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
    let res = mcp::handle(&conn, &search::SearchContext::bm25_only(), &Config::default(), &req).unwrap().unwrap();
    let n = res["result"]["content"][0]["text"].as_str().unwrap()
        .lines().filter(|l| l.starts_with("[id:")).count();
    assert_eq!(n, 20, "con 25 memorias que matchean, sin limit deben volver los 20 candidatos por defecto");
}

// --- B-4: what a caller may ask for has a ceiling ------------------------

#[test]
fn search_memory_clamps_an_absurd_limit_and_budget() {
    // `limit` and `budget` come straight off the request. Asking for a
    // billion of each has to come back with an answer, promptly, not with
    // an error and not with the process thinking about it.
    let d = tempfile::tempdir().unwrap();
    let conn = anathem::db::open(&d.path().join("m.db")).unwrap();
    for i in 0..5 {
        anathem::ingest::save_explicit(&conn, &format!("nota {i} sobre cache"), "p", vec![])
            .unwrap();
    }
    let cfg = anathem::config::Config::default();
    let ctx = anathem::search::SearchContext::bm25_only();

    let req = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "search_memory", "arguments": {
            "query": "cache",
            "limit": u64::MAX,
            "budget": u64::MAX
        }}
    });
    let comienzo = std::time::Instant::now();
    let res = anathem::mcp::handle(&conn, &ctx, &cfg, &req).unwrap().unwrap();
    assert!(comienzo.elapsed() < std::time::Duration::from_secs(5), "tardó demasiado");
    let texto = res["result"]["content"][0]["text"].as_str().unwrap();
    assert!(texto.contains("cache"), "tenía que devolver resultados igual: {texto}");
}

#[test]
fn a_very_long_query_does_not_grow_the_fts_expression_without_bound() {
    let largo: String = (0..5_000).map(|i| format!("palabra{i} ")).collect();
    let expr = anathem::search::to_match(&largo);
    let ramas = expr.matches(" OR ").count() + 1;
    assert!(ramas <= 50, "la expresión creció a {ramas} ramas");
}

// --- la superficie que lee el modelo está en inglés ----------------------

#[test]
fn the_topic_label_and_the_tool_description_agree() {
    // Si estas dos se separan, el modelo lee una etiqueta y se le pide
    // reusar otra, y el reemplazo silenciosamente no ocurre.
    let defs = serde_json::to_string(&anathem::mcp::tool_defs()).unwrap();
    assert!(defs.contains("[topic:"), "la descripción tiene que nombrar la etiqueta real: {defs}");
    assert!(!defs.contains("[tema:"), "no puede quedar la vieja");
}

#[test]
fn every_tool_description_is_english() {
    let defs = serde_json::to_string(&anathem::mcp::tool_defs()).unwrap();
    for palabra in ["memoria", "búsqueda", "guardar", "usuario", "clave de tema", "proyecto"] {
        assert!(!defs.contains(palabra), "queda español en las tools: «{palabra}»");
    }
}
