//! Traspaso entre agentes: qué se guardó últimamente y quién lo guardó.
//!
//! Lo que se comprueba acá es que la funcionalidad sea aditiva. Una base que
//! nunca la use tiene que comportarse igual que antes, y una base anterior a
//! estas columnas tiene que seguir andando y migrarse sola.

use anathem::{config::Config, db, ingest, mcp, reciente, search};
use rusqlite::Connection;
use std::path::Path;
use std::process::Command;

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

/// Fija `creado_ts` a mano para poder ordenar y filtrar sin esperar.
fn fechar(conn: &Connection, id: i64, ts: &str) {
    conn.execute("update memories set creado_ts = ?2 where id = ?1", rusqlite::params![id, ts])
        .unwrap();
}

fn hace(segundos: i64) -> String {
    (chrono::Local::now() - chrono::Duration::seconds(segundos)).to_rfc3339()
}

// ---------------------------------------------------------------- duraciones

#[test]
fn las_ventanas_se_entienden_en_las_unidades_usuales() {
    assert_eq!(reciente::duracion_en_segundos("45s").unwrap(), 45);
    assert_eq!(reciente::duracion_en_segundos("90m").unwrap(), 5_400);
    assert_eq!(reciente::duracion_en_segundos("6h").unwrap(), 21_600);
    assert_eq!(reciente::duracion_en_segundos("2d").unwrap(), 172_800);
    // Sin sufijo son horas: es la unidad del caso que motivó esto.
    assert_eq!(reciente::duracion_en_segundos("12").unwrap(), 43_200);
}

#[test]
fn una_ventana_que_no_se_entiende_falla_y_lo_dice() {
    for malo in ["", "   ", "ayer", "-3h", "0h", "3x"] {
        let e = reciente::duracion_en_segundos(malo);
        assert!(e.is_err(), "«{malo}» debería fallar y no lo hizo");
    }
    let msg = reciente::duracion_en_segundos("ayer").unwrap_err().to_string();
    assert!(msg.contains("6h"), "el mensaje tiene que sugerir el formato: {msg}");
}

// -------------------------------------------------------------------- listar

#[test]
fn lo_mas_nuevo_va_primero() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    let viejo = ingest::guardar_explicito(&conn, "lo primero", "p", vec![]).unwrap().0;
    let nuevo = ingest::guardar_explicito(&conn, "lo segundo", "p", vec![]).unwrap().0;
    fechar(&conn, viejo, &hace(7200));
    fechar(&conn, nuevo, &hace(60));

    let r = reciente::listar(&conn, Some("p"), None, 10, 120).unwrap();
    let ids: Vec<i64> = r.iter().map(|x| x.id).collect();
    assert_eq!(ids, vec![nuevo, viejo], "tiene que venir de más nuevo a más viejo");
}

#[test]
fn la_ventana_deja_afuera_lo_anterior() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    let dentro = ingest::guardar_explicito(&conn, "de hace un rato", "p", vec![]).unwrap().0;
    let fuera = ingest::guardar_explicito(&conn, "de anteayer", "p", vec![]).unwrap().0;
    fechar(&conn, dentro, &hace(1800));
    fechar(&conn, fuera, &hace(200_000));

    let r = reciente::listar(&conn, Some("p"), Some(3600), 10, 120).unwrap();
    let ids: Vec<i64> = r.iter().map(|x| x.id).collect();
    assert_eq!(ids, vec![dentro], "la ventana de una hora tiene que dejar afuera lo de anteayer");
}

#[test]
fn solo_el_proyecto_pedido_y_tambien_todos() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    ingest::guardar_explicito(&conn, "cosa de uno", "uno", vec![]).unwrap();
    ingest::guardar_explicito(&conn, "cosa de dos", "dos", vec![]).unwrap();

    let solo = reciente::listar(&conn, Some("uno"), None, 10, 120).unwrap();
    assert_eq!(solo.len(), 1);
    assert_eq!(solo[0].project, "uno");

    let todos = reciente::listar(&conn, None, None, 10, 120).unwrap();
    assert_eq!(todos.len(), 2, "sin proyecto tiene que traer los dos");
}

#[test]
fn lo_reemplazado_y_lo_olvidado_no_aparecen() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    // Una clave de tema deja superseded a la anterior.
    ingest::guardar_con_tema(&conn, "primera version", "p", vec![], Some("estado"), None).unwrap();
    let vigente =
        ingest::guardar_con_tema(&conn, "segunda version", "p", vec![], Some("estado"), None)
            .unwrap()
            .id;
    let olvidada = ingest::guardar_explicito(&conn, "algo que se olvida", "p", vec![]).unwrap().0;
    anathem::maint::forget(&conn, olvidada).unwrap();

    let r = reciente::listar(&conn, Some("p"), None, 10, 120).unwrap();
    let ids: Vec<i64> = r.iter().map(|x| x.id).collect();
    assert_eq!(ids, vec![vigente], "solo lo activo: {r:?}");
    assert_eq!(r[0].topic.as_deref(), Some("estado"), "la clave de tema tiene que viajar");
}

#[test]
fn el_limite_recorta_pero_conserva_lo_mas_nuevo() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    let mut ids = Vec::new();
    for i in 0..5 {
        let id = ingest::guardar_explicito(&conn, &format!("memoria {i}"), "p", vec![]).unwrap().0;
        fechar(&conn, id, &hace(1000 - i * 100));
        ids.push(id);
    }
    let r = reciente::listar(&conn, Some("p"), None, 2, 120).unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].id, ids[4], "el más nuevo primero");
    assert_eq!(r[1].id, ids[3]);
}

// ------------------------------------------------------------------- agentes

#[test]
fn el_servidor_mcp_anota_quien_lo_guardo_sin_que_nadie_lo_configure() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    let ctx = search::Contexto::solo_bm25();
    let cfg = Config::default();
    let mut agente = None;

    // El handshake que el protocolo obliga a mandar.
    let init = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "opencode", "version": "1.2.3"}}
    });
    mcp::handle_con_agente(&conn, &ctx, &cfg, &init, &mut agente).unwrap().unwrap();
    assert_eq!(agente.as_deref(), Some("opencode"), "el nombre del cliente se tiene que recordar");

    let save = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "save_memory",
                   "arguments": {"text": "algo que dejó opencode", "project": "p"}}
    });
    mcp::handle_con_agente(&conn, &ctx, &cfg, &save, &mut agente).unwrap().unwrap();

    let r = reciente::listar(&conn, Some("p"), None, 10, 120).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].agente.as_deref(), Some("opencode"));
}

#[test]
fn sin_clientinfo_no_se_inventa_un_agente() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    let ctx = search::Contexto::solo_bm25();
    let cfg = Config::default();
    let mut agente = None;

    // Un cliente que no manda `clientInfo`, o lo manda vacío.
    for params in [
        serde_json::json!({"protocolVersion": "2025-06-18"}),
        serde_json::json!({"clientInfo": {"name": "   "}}),
    ] {
        let init = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": params});
        mcp::handle_con_agente(&conn, &ctx, &cfg, &init, &mut agente).unwrap().unwrap();
        assert_eq!(agente, None, "un nombre ausente o vacío no es un agente");
    }

    let save = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "save_memory", "arguments": {"text": "anónimo", "project": "p"}}
    });
    mcp::handle_con_agente(&conn, &ctx, &cfg, &save, &mut agente).unwrap().unwrap();
    let r = reciente::listar(&conn, Some("p"), None, 10, 120).unwrap();
    assert_eq!(r[0].agente, None, "mejor sin agente que con uno inventado");
}

#[test]
fn lo_guardado_desde_la_cli_no_tiene_agente() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    let id = ingest::guardar_explicito(&conn, "desde la terminal", "p", vec![]).unwrap().0;
    let a: Option<String> = conn
        .query_row("select agente from memories where id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(a, None);
}

#[test]
fn la_tool_recent_memories_existe_y_contesta() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    let ctx = search::Contexto::solo_bm25();
    let cfg = Config::default();

    let lista = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    let res = mcp::handle(&conn, &ctx, &cfg, &lista).unwrap().unwrap();
    let nombres: Vec<String> = res["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(nombres.contains(&"recent_memories".to_string()), "tools: {nombres:?}");

    ingest::guardar_explicito(&conn, "algo reciente del proyecto", "p", vec![]).unwrap();
    let call = serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "recent_memories", "arguments": {"project": "p"}}
    });
    let res = mcp::handle(&conn, &ctx, &cfg, &call).unwrap().unwrap();
    let txt = res["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("algo reciente"), "respuesta: {txt}");
}

#[test]
fn una_ventana_invalida_en_la_tool_se_explica_y_no_rompe() {
    let d = tempfile::tempdir().unwrap();
    let conn = db::open(&d.path().join("mem.db")).unwrap();
    let call = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "recent_memories", "arguments": {"since": "ayer"}}
    });
    let res = mcp::handle(&conn, &search::Contexto::solo_bm25(), &Config::default(), &call)
        .unwrap()
        .unwrap();
    assert!(res["error"].is_null(), "no tiene que ser un error de protocolo: {res}");
    let txt = res["result"]["content"][0]["text"].as_str().unwrap();
    assert!(txt.contains("No se pudo listar"), "respuesta: {txt}");
}

// ------------------------------------------------------------------ migración

#[test]
fn una_base_anterior_a_estas_columnas_sigue_andando_y_se_migra_sola() {
    let d = tempfile::tempdir().unwrap();
    let db_path = d.path().join("mem.db");
    {
        let conn = db::open(&db_path).unwrap();
        ingest::guardar_explicito(&conn, "memoria de antes", "p", vec![]).unwrap();
        // Se simula la base vieja. El índice referencia `creado_ts`, así que
        // sale primero: SQLite no deja tirar una columna que un índice usa.
        conn.execute("drop index if exists idx_mem_project_creado", []).unwrap();
        for col in ["agente", "creado_ts"] {
            conn.execute(&format!("alter table memories drop column {col}"), []).unwrap();
        }
        let n: i64 = conn
            .query_row(
                "select count(*) from pragma_table_info('memories') where name in ('agente','creado_ts')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "el escenario exige que las columnas no estén");
    }

    // Abrir la base vieja la migra.
    let conn = db::open(&db_path).unwrap();
    let n: i64 = conn
        .query_row(
            "select count(*) from pragma_table_info('memories') where name in ('agente','creado_ts')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 2, "abrir la base tiene que agregar las dos columnas");

    // La memoria vieja aparece igual, con lo único que de ella se sabe.
    let r = reciente::listar(&conn, Some("p"), None, 10, 120).unwrap();
    assert_eq!(r.len(), 1, "la memoria anterior tiene que seguir apareciendo");
    assert_eq!(r[0].agente, None);
    assert!(!r[0].cuando.is_empty(), "sin creado_ts se cae a la fecha de created");
}

// ------------------------------------------------------------------------ CLI

#[test]
fn el_comando_recent_imprime_lo_ultimo() {
    let d = tempfile::tempdir().unwrap();
    {
        let conn = db::open(&d.path().join("mem.db")).unwrap();
        ingest::guardar_explicito(&conn, "una nota del proyecto equis", "equis", vec![]).unwrap();
    }
    let (ok, out, err) = mem(d.path(), &["recent", "--project", "equis"]);
    assert!(ok, "falló: {err}");
    assert!(out.contains("una nota del proyecto equis"), "salida: {out}");

    // Una ventana que no abarca nada lo dice en vez de mentir con una lista.
    // La memoria se envejece a mano: recién guardada cae dentro de cualquier
    // ventana, y el test no probaría nada.
    {
        let conn = Connection::open(d.path().join("mem.db")).unwrap();
        conn.execute("update memories set creado_ts = ?1", [hace(200_000)]).unwrap();
    }
    let (ok, out, _) = mem(d.path(), &["recent", "--project", "equis", "--desde", "1h"]);
    assert!(ok);
    assert!(out.contains("sin memorias"), "salida: {out}");

    // `--desde todo` no acota.
    let (ok, out, _) = mem(d.path(), &["recent", "--project", "equis", "--desde", "todo"]);
    assert!(ok);
    assert!(out.contains("una nota del proyecto equis"), "salida: {out}");

    // Una ventana ilegible falla y lo explica.
    let (ok, _, err) = mem(d.path(), &["recent", "--desde", "ayer"]);
    assert!(!ok, "una ventana ilegible no puede pasar por buena");
    assert!(err.contains("ayer"), "stderr: {err}");
}
